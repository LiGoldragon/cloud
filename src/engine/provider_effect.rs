//! The Cloudflare provider path as a Nexus `CommandEffect` handler.
//!
//! THIS IS THE KEY SHAPE of the schema-derived port. In the pre-schema daemon,
//! provider IO ran inline inside the request handlers in `lib.rs`. Under the
//! triad-engine shape (modelled on `spirit/src/nexus.rs` §`apply_effect`),
//! provider IO becomes a typed effect:
//!
//! - the Nexus decide loop emits `NexusAction::CommandEffect(...)`;
//! - the runner calls this handler;
//! - this handler calls the existing `cloudflare::ProviderClient`;
//! - the result re-enters the loop as `NexusWork::EffectCompleted(...)`.
//!
//! Provider IO is therefore NEVER inline in a decide step. The decision plane
//! (`Nexus::step_decide`) is pure: it only chooses WHICH effect to run. The
//! effect handler is the single place blocking provider HTTP/`flarectl` calls
//! happen, which is what keeps the listeners free per `ARCHITECTURE.md`
//! §"Actor Shape" ("Provider calls must not block the … listener").
//!
//! Two effect command sets exist, one per contract:
//!
//! - working `signal-cloud`: `CloudflareObserveZones` / `CloudflareObserveRecords`
//!   (read-only observation effects);
//! - owner `meta-signal-cloud`: `CloudflareApplyPlan` (owner-approved mutation).

use std::sync::Arc;

use crate::cloudflare::{self, ProviderClient};

/// The provider-effect executor: the daemon-owned Cloudflare client wrapped so
/// both the working Nexus and the owner Nexus can run their typed effects
/// through one place. A later slice puts this behind a `CloudflareProvider`
/// actor with a timeout (see `ARCHITECTURE.md` §"Actor Shape"); the pilot keeps
/// it synchronous to prove the effect boundary first.
#[derive(Clone, Debug)]
pub struct ProviderEffects {
    client: Arc<ProviderClient>,
}

impl ProviderEffects {
    pub fn new(client: ProviderClient) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    pub fn production() -> Self {
        Self::new(ProviderClient::production())
    }

    pub fn client(&self) -> &ProviderClient {
        &self.client
    }
}

/// The error a provider effect can surface, mapped from the `cloudflare`
/// adapter error. The Nexus decide loop converts this into the contract's
/// typed rejection / error reply — provider failure never panics the runner.
#[derive(Debug, thiserror::Error)]
pub enum ProviderEffectError {
    #[error("cloudflare provider effect failed: {0}")]
    Cloudflare(#[from] cloudflare::Error),

    #[error("no provider account is registered for the requested effect")]
    NoRegisteredAccount,
}
