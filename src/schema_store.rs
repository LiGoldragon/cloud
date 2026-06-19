//! Durable state plane for the schema-engine `SchemaRuntime`.
//!
//! The two schema-emitted SEMA tables — `AccountPolicyTable` (keyed by
//! provider + account) and `PlanTable` (a 1:N keyed collection of `StoredPlan`,
//! keyed by plan identifier) — plus the monotonic commit sequence, held behind
//! one `Mutex` so a single write commits across both tables. The `SemaEngine`
//! and `NexusEngine` impls on `SchemaRuntime` read and write through this state.
//!
//! In-memory for this slice, matching the lojix `Store` template and the
//! `cloud` legacy `Store` shape. Report 77's interim workaround: the
//! `PlanTable` is the 1:N collection keyed by a composite identifier, so no
//! `sema-engine` identified-multi-key (`ox7e`) primitive is required. Durable
//! `sema-engine` backing is the noted follow-on; the old `signal-core`
//! dependency blocker is gone, so the remaining work is adopting engine-owned
//! database operations for these tables.

use std::sync::{Mutex, MutexGuard};

use signal_cloud::schema::lib::{Provider, ProviderAccount};

use crate::schema::sema::{
    AccountBinding, CommitSequence, CredentialHandle, StateDigest, StateMarker, StoredPlan,
};
use crate::{Error, Result};

/// Projects the meta-contract provider into the ordinary-contract provider that
/// keys the `AccountPolicyTable`. The two contracts declare structurally
/// identical `Provider` enums, but they are distinct Rust types; the durable
/// `AccountBinding` is keyed by the ordinary `Provider`, so a meta-contract
/// registration's provider is mapped through this projection on the way in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderProjection {
    provider: meta_signal_cloud::schema::lib::Provider,
}

impl ProviderProjection {
    pub fn new(provider: meta_signal_cloud::schema::lib::Provider) -> Self {
        Self { provider }
    }

    pub fn into_ordinary(self) -> Provider {
        match self.provider {
            meta_signal_cloud::schema::lib::Provider::Cloudflare => Provider::Cloudflare,
            meta_signal_cloud::schema::lib::Provider::GoogleCloud => Provider::GoogleCloud,
            meta_signal_cloud::schema::lib::Provider::Hetzner => Provider::Hetzner,
            meta_signal_cloud::schema::lib::Provider::DigitalOcean => Provider::DigitalOcean,
        }
    }
}

/// The two SEMA tables plus the monotonic commit sequence, held under one lock
/// so a single write commits atomically across the tables. The state digest is
/// modeled as the commit sequence for this in-memory slice.
#[derive(Debug, Default)]
pub struct SchemaStoreState {
    account_policy: Vec<AccountBinding>,
    plans: Vec<StoredPlan>,
    commit_sequence: u64,
}

impl SchemaStoreState {
    /// Advance the commit sequence and return the new value.
    pub fn next_commit_sequence(&mut self) -> u64 {
        self.commit_sequence += 1;
        self.commit_sequence
    }

    pub fn commit_sequence(&self) -> u64 {
        self.commit_sequence
    }

    /// The current state marker (commit sequence doubling as the digest).
    pub fn marker(&self) -> StateMarker {
        Self::marker_for(self.commit_sequence)
    }

    pub fn accounts(&self) -> &[AccountBinding] {
        &self.account_policy
    }

    pub fn plans(&self) -> &[StoredPlan] {
        &self.plans
    }

    /// Insert or replace the account binding keyed by provider + account. The
    /// `AccountPolicyTable` is keyed by the (provider, account) pair, so a
    /// repeat registration replaces the existing binding rather than appending.
    /// Returns the post-commit marker.
    pub fn put_account(&mut self, binding: AccountBinding) -> StateMarker {
        let commit_sequence = self.next_commit_sequence();
        match self
            .account_policy
            .iter_mut()
            .find(|existing| Self::same_account(existing, &binding))
        {
            Some(existing) => *existing = binding,
            None => self.account_policy.push(binding),
        }
        Self::marker_for(commit_sequence)
    }

    /// Rotate the credential handle on the account keyed by provider + account.
    /// Returns the post-commit marker when the account exists, `None` when it is
    /// unknown.
    pub fn rotate_credential(
        &mut self,
        provider: &Provider,
        provider_account: &ProviderAccount,
        credential_handle: CredentialHandle,
    ) -> Option<StateMarker> {
        let commit_sequence = self.next_commit_sequence();
        let binding = self.account_policy.iter_mut().find(|existing| {
            &existing.provider == provider && &existing.provider_account == provider_account
        })?;
        binding.credential_handle = credential_handle;
        Some(Self::marker_for(commit_sequence))
    }

    /// Retire the account keyed by provider + account. Returns the post-commit
    /// marker when an account was removed, `None` when none matched.
    pub fn retire_account(
        &mut self,
        provider: &Provider,
        provider_account: &ProviderAccount,
    ) -> Option<StateMarker> {
        let commit_sequence = self.next_commit_sequence();
        let before = self.account_policy.len();
        self.account_policy.retain(|existing| {
            !(&existing.provider == provider && &existing.provider_account == provider_account)
        });
        (self.account_policy.len() != before).then(|| Self::marker_for(commit_sequence))
    }

    /// Insert or replace a stored plan keyed by its plan identifier. The
    /// `PlanTable` is the 1:N keyed collection (report 77 interim): the plan
    /// identifier is the composite key, so re-preparing a plan with the same
    /// identifier replaces it.
    pub fn put_plan(&mut self, plan: StoredPlan) -> StateMarker {
        let commit_sequence = self.next_commit_sequence();
        match self
            .plans
            .iter_mut()
            .find(|existing| existing.plan.identifier == plan.plan.identifier)
        {
            Some(existing) => *existing = plan,
            None => self.plans.push(plan),
        }
        Self::marker_for(commit_sequence)
    }

    pub fn plan(&self, identifier: &str) -> Option<&StoredPlan> {
        self.plans
            .iter()
            .find(|stored| stored.plan.identifier.payload() == identifier)
    }

    /// Mark the plan with the given identifier approved. Returns the post-commit
    /// marker when the plan exists, `None` when it is unknown.
    pub fn approve_plan(&mut self, identifier: &str) -> Option<StateMarker> {
        let commit_sequence = self.next_commit_sequence();
        let stored = self
            .plans
            .iter_mut()
            .find(|stored| stored.plan.identifier.payload() == identifier)?;
        stored.approved = true;
        Some(Self::marker_for(commit_sequence))
    }

    fn same_account(left: &AccountBinding, right: &AccountBinding) -> bool {
        left.provider == right.provider && left.provider_account == right.provider_account
    }

    fn marker_for(commit_sequence: u64) -> StateMarker {
        StateMarker {
            commit_sequence: CommitSequence::new(commit_sequence),
            state_digest: StateDigest::new(commit_sequence),
        }
    }
}

/// Durable schema-engine state, behind one `Mutex`. The daemon constructs one
/// shared handle; each request is served by its own `SchemaRuntime` over a clone
/// of this `Arc`, so concurrent requests share the durable tables. In-memory for
/// this slice (lojix template); sema-engine / redb backing is the follow-on.
#[derive(Debug, Default)]
pub struct SchemaStore {
    state: Mutex<SchemaStoreState>,
}

impl SchemaStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lock the durable state. Returns `StorePoisoned` if a prior holder
    /// panicked while the lock was held.
    pub fn lock(&self) -> Result<MutexGuard<'_, SchemaStoreState>> {
        self.state.lock().map_err(|_| Error::StorePoisoned)
    }

    pub fn commit_sequence(&self) -> Result<u64> {
        Ok(self.lock()?.commit_sequence())
    }
}
