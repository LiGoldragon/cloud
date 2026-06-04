//! The two-listener daemon driving the schema-derived triad engines.
//!
//! Preserves the existing daemon's two-socket shape (working socket + owner
//! socket — see `src/daemon.rs`) while routing each socket through its
//! contract's `SignalEngine` + `NexusEngine` composition, exactly as
//! `spirit/src/daemon.rs` routes one socket through Signal -> Nexus.
//!
//! The two engine compositions SHARE one in-memory `Store` behind a `Mutex`:
//! the owner contract writes account/policy/plan state; the working contract
//! reads plan state. This mirrors the pre-schema daemon, which also shares one
//! `Arc<Mutex<Store>>` across both listeners.
//!
//! Each request crosses the single-NOTA-argument boundary the same way spirit
//! does: a length-prefixed signal frame in, the engine composition runs, a
//! signal frame out. The frame codec is the generated `encode_signal_frame` /
//! `decode_signal_frame` on each contract's `Input` / `Output` (no hand-rolled
//! rkyv in transport — see `spirit/flake.nix` `binary-boundary-test`).

use std::sync::{Arc, Mutex};

use signal_cloud::{NexusEngine as WorkingNexusEngine, SignalEngine as WorkingSignalEngine};

use meta_signal_cloud::{NexusEngine as OwnerNexusEngine, SignalEngine as OwnerSignalEngine};

use crate::engine::{
    owner::{OwnerNexus, OwnerSignalActor},
    provider_effect::ProviderEffects,
    store::Store,
    working::{WorkingNexus, WorkingSignalActor},
};

/// The shared daemon state: one in-memory store plus the per-contract engine
/// pieces. The store is shared (owner writes, working reads); each contract has
/// its own Signal actor and Nexus.
pub struct SchemaDaemon {
    working_signal: WorkingSignalActor,
    working_nexus: Mutex<WorkingNexus>,
    owner_signal: OwnerSignalActor,
    owner_nexus: Mutex<OwnerNexus>,
}

impl SchemaDaemon {
    /// Build the daemon over a fresh in-memory store and the production
    /// Cloudflare provider effects. The store is cloned-by-value into each
    /// Nexus today (pilot); the durable-store slice replaces these with one
    /// `Arc<Mutex<Store>>` shared by both Nexuses, matching the pre-schema
    /// daemon's sharing.
    pub fn production() -> Self {
        Self::new(ProviderEffects::production())
    }

    pub fn new(provider: ProviderEffects) -> Self {
        Self {
            working_signal: WorkingSignalActor::default(),
            working_nexus: Mutex::new(WorkingNexus::new(Store::new(), provider.clone())),
            owner_signal: OwnerSignalActor::default(),
            owner_nexus: Mutex::new(OwnerNexus::new(Store::new(), provider)),
        }
    }

    /// Run one ordinary (working) request through the working-contract
    /// composition: admit -> triage -> Nexus execute -> reply. Mirrors
    /// `spirit/src/engine.rs` `Engine::handle`.
    pub fn handle_working(
        &self,
        input: signal_cloud::Input,
    ) -> signal_cloud::schema::lib::signal::Signal<signal_cloud::Output> {
        let admitted = self.working_signal.admit(input);
        let work = self.working_signal.triage(admitted);
        let mut nexus = self.working_nexus.lock().expect("working nexus lock");
        let action = nexus.execute(work);
        self.working_signal.reply(action)
    }

    /// Run one owner request through the owner-contract composition. Same
    /// shape, the full triad (the Nexus may drive SEMA writes + the Cloudflare
    /// apply effect before replying).
    pub fn handle_owner(
        &self,
        input: meta_signal_cloud::Input,
    ) -> meta_signal_cloud::schema::meta_signal_cloud::signal::Signal<meta_signal_cloud::Output> {
        let admitted = self.owner_signal.admit(input);
        let work = self.owner_signal.triage(admitted);
        let mut nexus = self.owner_nexus.lock().expect("owner nexus lock");
        let action = nexus.execute(work);
        self.owner_signal.reply(action)
    }
}

/// A shareable handle. The Unix-socket listener loops (one per socket, as in
/// `src/daemon.rs`) clone this across accepted connections.
#[derive(Clone)]
pub struct SchemaDaemonHandle {
    daemon: Arc<SchemaDaemon>,
}

impl SchemaDaemonHandle {
    pub fn new(daemon: SchemaDaemon) -> Self {
        Self {
            daemon: Arc::new(daemon),
        }
    }

    pub fn daemon(&self) -> &SchemaDaemon {
        &self.daemon
    }
}
