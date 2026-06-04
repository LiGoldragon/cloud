//! In-memory SEMA store for the cloud daemon — the owner-contract
//! `SemaEngine` impl plus the read-only working-contract plan reads.
//!
//! Per `ARCHITECTURE.md` §"Current Implementation Slice": `sema-engine`
//! persistence is intentionally deferred because the current production
//! engine still pulls the deprecated `signal-core` dependency. This store
//! keeps the SEMA boundary small and entirely in memory so a durable
//! `sema-engine`-backed `Store` (modelled on `spirit/src/store.rs`) can be
//! swapped in once that dependency is removed without touching the Nexus
//! decide loops.
//!
//! The store maps the owner contract's SEMA roots onto an in-memory account /
//! policy / plan table. It owns durable identifier allocation, the commit
//! sequence, and the content digest behind `DatabaseMarker`. Query predicate
//! and plan-generation semantics stay here because they are cloud-specific
//! SEMA behaviour, not generic daemon plumbing.

use std::collections::HashMap;

use meta_signal_cloud::{
    AccountRegistered, AccountRetired, CommitSequence, CredentialHandle, CredentialRotated,
    DatabaseMarker, DomainName, ObservedPlan, Plan, PlanIdentifier, Policy, PolicySet, Provider,
    ProviderAccount, RejectionReason, RejectionReport, SemaEngine, SemaReadInput, SemaReadOutput,
    SemaWriteInput, SemaWriteOutput, StateDigest, schema::meta_signal_cloud::sema as sema_plane,
};
use signal_cloud::schema::lib::sema as working_sema;

use crate::engine::ActorLifecycle;

/// A registered provider account and the policy attached to it. Credentials
/// cross owner policy by handle only — no secret bytes ever land here.
#[derive(Clone, Debug)]
struct AccountRecord {
    provider: Provider,
    account: ProviderAccount,
    credential_handle: CredentialHandle,
    policy: Option<Policy>,
}

/// A prepared plan and its approval / application lifecycle bit.
#[derive(Clone, Debug)]
struct PlanRecord {
    plan: Plan,
    approved: bool,
    applied: bool,
}

/// The in-memory owner store. `commit_sequence` advances on every accepted
/// write so `DatabaseMarker` is monotone, mirroring the durable store's
/// contract without the `sema-engine` dependency.
#[derive(Debug, Default)]
pub struct Store {
    accounts: HashMap<(Provider, ProviderAccount), AccountRecord>,
    plans: HashMap<PlanIdentifier, PlanRecord>,
    next_plan_ordinal: u64,
    commit_sequence: CommitSequence,
    lifecycle: ActorLifecycle,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// The SEMA commit marker. A real `sema-engine` store digests committed
    /// records with blake3 (see `spirit/src/store.rs`); the in-memory pilot
    /// derives a cheap stable digest from the live table sizes so the marker
    /// still changes shape as state grows.
    pub fn database_marker(&self) -> DatabaseMarker {
        DatabaseMarker {
            commit_sequence: self.commit_sequence,
            state_digest: self.state_digest(),
        }
    }

    fn state_digest(&self) -> StateDigest {
        let accounts = self.accounts.len() as u64;
        let plans = self.plans.len() as u64;
        accounts.wrapping_mul(1_000_003).wrapping_add(plans)
    }

    fn advance_commit(&mut self) {
        self.commit_sequence = self.commit_sequence.wrapping_add(1);
    }

    fn require_account(
        &self,
        provider: &Provider,
        account: &ProviderAccount,
    ) -> Result<&AccountRecord, RejectionReason> {
        self.accounts
            .get(&(provider.clone(), account.clone()))
            .ok_or(RejectionReason::AccountUnknown)
    }

    fn mint_plan_identifier(&mut self) -> PlanIdentifier {
        self.next_plan_ordinal = self.next_plan_ordinal.wrapping_add(1);
        format!("plan-{}", self.next_plan_ordinal)
    }

    fn rejection(&self, reason: RejectionReason) -> RejectionReport {
        RejectionReport {
            rejection_reason: reason,
            database_marker: self.database_marker(),
        }
    }

    /// Read a plan by identifier without crossing the SEMA write path. The
    /// working contract reuses this for its read-only `ObservePlan` flow.
    pub fn plan(&self, identifier: &PlanIdentifier) -> Option<Plan> {
        self.plans.get(identifier).map(|record| record.plan.clone())
    }

    /// Whether a plan is approved — the apply ceremony gate.
    pub fn plan_is_approved(&self, identifier: &PlanIdentifier) -> bool {
        self.plans
            .get(identifier)
            .map(|record| record.approved)
            .unwrap_or(false)
    }

    /// The working-contract read path. The working `signal-cloud` contract has
    /// its OWN `SemaReadInput` / `SemaReadOutput` types, distinct from the owner
    /// contract's. The working Nexus reads plans through this method, which
    /// crosses the contract boundary by projecting the owner-held `Plan` into
    /// the working contract's `Plan`.
    ///
    /// NOTE (cross-contract gap): owner `Plan` uses `identifier` + `zone`;
    /// working `Plan` uses `plan_identifier` + `domain_name`, and the two
    /// `RecordKind` / `DomainNameSystemRecord` shapes differ. A faithful
    /// projection needs an `impl From<owner Plan> for working Plan` once the two
    /// record shapes are reconciled (see report — both contracts should share a
    /// single record schema via signal reuse). Until then this read path
    /// reports `Missed` rather than emit a lossy projection, keeping the gap
    /// visible at the wire.
    pub fn observe_read(
        &self,
        query: working_sema::Sema<working_sema::ReadInput>,
    ) -> working_sema::Sema<working_sema::ReadOutput> {
        let origin_route = query.origin_route();
        let output = match query.into_root() {
            signal_cloud::SemaReadInput::ObservePlan(_plan_query) => {
                signal_cloud::SemaReadOutput::missed(signal_cloud::ErrorReport::new(
                    String::from("cross-contract plan projection not yet wired"),
                ))
            }
            signal_cloud::SemaReadInput::Validate(validation) => {
                signal_cloud::SemaReadOutput::validated(self.validate(validation))
            }
            // The working Nexus routes Zones/Records observations to provider
            // effects, never to this SEMA read; an Observe arriving here means
            // an unrouted observation, reported as Missed.
            signal_cloud::SemaReadInput::Observe(_observation) => {
                signal_cloud::SemaReadOutput::missed(signal_cloud::ErrorReport::new(
                    String::from("observation has no SEMA read path"),
                ))
            }
        };
        output.with_origin_route(origin_route)
    }

    /// Validate a desired state into a working-contract `ValidationReport`. A
    /// later slice ports the IPv4/IPv6/redirect-target checks from the
    /// pre-schema `lib.rs` (per `ARCHITECTURE.md` step 10); the pilot emits an
    /// empty (all-clear) report to prove the validate -> reply path.
    fn validate(&self, _validation: signal_cloud::Validation) -> signal_cloud::ValidationReport {
        signal_cloud::ValidationReport(Vec::new())
    }
}

impl SemaEngine for Store {
    fn apply_inner(
        &mut self,
        command: sema_plane::Sema<sema_plane::WriteInput>,
    ) -> sema_plane::Sema<sema_plane::WriteOutput> {
        let origin_route = command.origin_route();
        let output = self.apply_write(command.into_root());
        output.with_origin_route(origin_route)
    }

    fn observe_inner(
        &self,
        query: sema_plane::Sema<sema_plane::ReadInput>,
    ) -> sema_plane::Sema<sema_plane::ReadOutput> {
        let origin_route = query.origin_route();
        let output = match query.into_root() {
            SemaReadInput::ObservePlan(identifier) => match self.plan(&identifier) {
                Some(plan) => SemaReadOutput::plan_observed(ObservedPlan {
                    plan,
                    database_marker: self.database_marker(),
                }),
                None => SemaReadOutput::missed(self.rejection(RejectionReason::PlanUnknown)),
            },
        };
        output.with_origin_route(origin_route)
    }
}

impl Store {
    /// The owner write plane: register/retire accounts, set policy, prepare
    /// plans, and record approval / application transitions. Each accepted
    /// write advances the commit sequence so the reply marker is fresh.
    fn apply_write(&mut self, input: SemaWriteInput) -> SemaWriteOutput {
        match input {
            SemaWriteInput::RegisterAccount(registration) => {
                let key = (registration.provider.clone(), registration.provider_account.clone());
                self.accounts.insert(
                    key,
                    AccountRecord {
                        provider: registration.provider.clone(),
                        account: registration.provider_account.clone(),
                        credential_handle: registration.credential_handle,
                        policy: None,
                    },
                );
                self.advance_commit();
                SemaWriteOutput::account_registered(AccountRegistered {
                    provider: registration.provider,
                    provider_account: registration.provider_account,
                })
            }
            SemaWriteInput::RotateCredential(rotation) => {
                match self
                    .accounts
                    .get_mut(&(rotation.provider.clone(), rotation.provider_account.clone()))
                {
                    Some(record) => {
                        record.credential_handle = rotation.credential_handle;
                        self.advance_commit();
                        SemaWriteOutput::credential_rotated(CredentialRotated {
                            provider: rotation.provider,
                            provider_account: rotation.provider_account,
                        })
                    }
                    None => SemaWriteOutput::Missed(self.rejection(RejectionReason::AccountUnknown)),
                }
            }
            SemaWriteInput::SetPolicy(policy) => {
                // Pilot: a global policy table keyed by the first registered
                // account. A later slice keys policy per (provider, account).
                let capability_policy_count = policy.capabilities.len() as u64;
                let zone_policy_count = policy.zones.len() as u64;
                if let Some(record) = self.accounts.values_mut().next() {
                    record.policy = Some(policy);
                }
                self.advance_commit();
                SemaWriteOutput::policy_set(PolicySet {
                    capability_policy_count,
                    zone_policy_count,
                })
            }
            SemaWriteInput::PreparePlan(preparation) => {
                let desired = preparation.into_payload();
                let plan = self.generate_plan(desired);
                self.plans.insert(
                    plan.identifier.clone(),
                    PlanRecord {
                        plan: plan.clone(),
                        approved: false,
                        applied: false,
                    },
                );
                self.advance_commit();
                SemaWriteOutput::plan_prepared(plan)
            }
            SemaWriteInput::PrepareProjection(projection) => {
                // domain-criome projections lower into a plan exactly like a
                // DesiredState; the pilot folds the projected records into a
                // fresh plan on the projection's provider/domain.
                let plan = self.generate_plan_from_projection(projection);
                self.plans.insert(
                    plan.identifier.clone(),
                    PlanRecord {
                        plan: plan.clone(),
                        approved: false,
                        applied: false,
                    },
                );
                self.advance_commit();
                SemaWriteOutput::plan_prepared(plan)
            }
            SemaWriteInput::ApprovePlan(approval) => {
                let identifier = approval.into_payload();
                match self.plans.get_mut(&identifier) {
                    Some(record) => {
                        record.approved = true;
                        self.advance_commit();
                        SemaWriteOutput::plan_approved(identifier)
                    }
                    None => SemaWriteOutput::Missed(self.rejection(RejectionReason::PlanUnknown)),
                }
            }
            SemaWriteInput::ApplyPlan(application) => {
                let identifier = application.into_payload();
                match self.plans.get(&identifier) {
                    Some(record) if !record.approved => {
                        SemaWriteOutput::Missed(self.rejection(RejectionReason::PlanNotApproved))
                    }
                    Some(_) => {
                        // The actual provider IO is a CommandEffect, not a
                        // SEMA write — Nexus emits CloudflareApplyPlan after
                        // this write marks the plan applied. See
                        // `provider_effect.rs`.
                        if let Some(record) = self.plans.get_mut(&identifier) {
                            record.applied = true;
                        }
                        self.advance_commit();
                        SemaWriteOutput::plan_applied(identifier)
                    }
                    None => SemaWriteOutput::Missed(self.rejection(RejectionReason::PlanUnknown)),
                }
            }
            SemaWriteInput::RetireAccount(retirement) => {
                self.accounts
                    .remove(&(retirement.provider.clone(), retirement.provider_account.clone()));
                self.advance_commit();
                SemaWriteOutput::account_retired(AccountRetired {
                    provider: retirement.provider,
                    provider_account: retirement.provider_account,
                })
            }
        }
    }

    /// Generate a create-everything plan from a desired state. A later slice
    /// diffs against last-known provider reads (see `ARCHITECTURE.md` step 11);
    /// the pilot proves the prepare → approve → apply ceremony with a plan that
    /// creates every desired record.
    fn generate_plan(&mut self, desired: meta_signal_cloud::DesiredState) -> Plan {
        let identifier = self.mint_plan_identifier();
        Plan {
            identifier,
            provider: desired.provider,
            zone: desired.zone,
            records_to_create: desired.records,
            records_to_update: Vec::new(),
            record_names_to_delete: Vec::new(),
            redirects_to_create: desired.redirects,
            redirects_to_update: Vec::new(),
            redirect_sources_to_delete: Vec::new(),
        }
    }

    fn generate_plan_from_projection(
        &mut self,
        projection: meta_signal_cloud::ProjectionPreparation,
    ) -> Plan {
        let identifier = self.mint_plan_identifier();
        let zone: DomainName = projection.projection.projection_query.domain.clone();
        Plan {
            identifier,
            provider: projection.provider,
            zone,
            records_to_create: projection.projection.records,
            records_to_update: Vec::new(),
            record_names_to_delete: Vec::new(),
            redirects_to_create: projection.projection.redirects,
            redirects_to_update: Vec::new(),
            redirect_sources_to_delete: Vec::new(),
        }
    }
}
