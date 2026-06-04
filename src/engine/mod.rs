//! Schema-derived triad engines for the cloud daemon.
//!
//! This module is the schema-derived port modelled on the spirit pilot
//! (`spirit/src/engine.rs` / `nexus.rs` / `store.rs`). It implements the three
//! generated engine traits — `SignalEngine`, `NexusEngine`, `SemaEngine` — for
//! BOTH cloud contracts:
//!
//! - working `signal-cloud` (read-only): `WorkingSignalActor` + `WorkingNexus`.
//!   No `SemaEngine` is emitted for the working contract (no write leg).
//! - owner `meta-signal-cloud` (full triad): `OwnerSignalActor` + `OwnerNexus`
//!   + the `SemaEngine` impl on the shared in-memory `Store`.
//!
//! THE KEY SHAPE: provider IO is a Nexus `CommandEffect`, not inline handler
//! code. See `provider_effect.rs` and the `run_provider_effect` methods on each
//! Nexus. The decide loops only choose WHICH effect to run; the effect handler
//! is the one place blocking Cloudflare calls happen.
//!
//! The `Store` is intentionally IN-MEMORY: durable `sema-engine` persistence is
//! deferred because the current production engine still pulls the deprecated
//! `signal-core` dependency (per `ARCHITECTURE.md`). The SEMA boundary is kept
//! small so a durable store (modelled on `spirit/src/store.rs`) drops in later
//! without touching the decide loops.

pub mod owner;
pub mod provider_effect;
pub mod store;
pub mod working;

/// The continuation budget per the spirit pilot (`spirit/src/nexus.rs`): a
/// runner policy bounding how many times a Nexus may recurse before the runner
/// declares the loop unsound. A loop that never reaches `ReplyToSignal` is a
/// runtime error, not valid behaviour. The budget travels with the runner so
/// every contract shares the same bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContinuationBudget(u32);

impl ContinuationBudget {
    /// 32 iterations is ample for the deepest cloud flow
    /// (apply: write -> effect -> reply is 3) plus headroom.
    pub fn default_for_pilot() -> Self {
        Self(32)
    }

    pub fn from_iteration_count(count: u32) -> Self {
        Self(count)
    }

    pub fn remaining(self) -> u32 {
        self.0
    }

    pub fn spend_one(self) -> Option<Self> {
        if self.0 == 0 {
            None
        } else {
            Some(Self(self.0 - 1))
        }
    }
}

/// Mints monotone origin routes for admitted signals, one per Signal actor.
/// The origin route threads a request through the Signal -> Nexus -> SEMA
/// planes so the reply frames back to the right caller (see the generated
/// `with_origin_route` on every plane root).
#[derive(Debug, Default)]
pub struct OriginRouteMinter {
    next: std::sync::Mutex<u64>,
}

const ORIGIN_ROUTE_BASE: u64 = 1_000_000;

impl OriginRouteMinter {
    pub fn mint(&self) -> OriginRoute {
        let mut next = self.next.lock().expect("origin route lock");
        *next += 1;
        OriginRoute(ORIGIN_ROUTE_BASE + *next)
    }
}

/// The origin-route handle. Both generated contracts have their OWN
/// `OriginRoute` newtype; this minter-local handle converts into either via the
/// `From` impls below so one minter serves both Signal actors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OriginRoute(pub u64);

impl From<OriginRoute> for signal_cloud::OriginRoute {
    fn from(route: OriginRoute) -> Self {
        signal_cloud::OriginRoute(route.0)
    }
}

impl From<OriginRoute> for meta_signal_cloud::OriginRoute {
    fn from(route: OriginRoute) -> Self {
        meta_signal_cloud::OriginRoute(route.0)
    }
}

/// Tracks an actor's start/stop lifecycle bit. The generated `on_start` /
/// `on_stop` engine hooks flip this so the daemon can assert all actors are
/// running before it accepts traffic. A data-bearing replacement for the
/// trace-only lifecycle in the spirit pilot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActorLifecycle {
    started: bool,
}

impl ActorLifecycle {
    pub fn mark_started(&mut self) {
        self.started = true;
    }

    pub fn mark_stopped(&mut self) {
        self.started = false;
    }

    pub fn is_started(self) -> bool {
        self.started
    }
}
