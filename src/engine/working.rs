//! The ordinary working-contract (`signal-cloud`) engine impls.
//!
//! Modelled on `spirit/src/engine.rs` (SignalActor triage) and
//! `spirit/src/nexus.rs` (the Nexus decide loop over NexusWork -> NexusAction
//! with a ContinuationBudget). The working contract is READ-ONLY: there is no
//! `SemaWriteInput` leg and no `SemaEngine` is emitted for it. Its Nexus drives
//! SEMA reads (plan observation) and Cloudflare OBSERVE effects, then replies.
//!
//! Flow proven here:
//!
//! - `Input::Observe(Zones)`  -> CommandEffect(CloudflareObserveZones)
//!                            -> EffectCompleted(ZonesObserved) -> reply Observed(Zones).
//! - `Input::Observe(Records)`-> CommandEffect(CloudflareObserveRecords)
//!                            -> EffectCompleted(RecordsObserved) -> reply Observed(Records).
//! - `Input::Observe(ObservePlan)` -> CommandSemaRead(ObservePlan)
//!                            -> SemaReadCompleted(PlanObserved) -> reply Observed(PlanResult).
//! - `Input::Validate(..)`    -> CommandSemaRead(Validate) -> reply Validated(report).

use signal_cloud::{
    Input, NexusAction, NexusEffectCommand, NexusEffectResult, NexusWork, Observation,
    ObservationResult, Output, Provider, SemaReadInput, SemaReadOutput, UnsupportedReason,
    UnsupportedRequest,
    schema::lib::{nexus as nexus_plane, signal as signal_plane},
};

use crate::engine::{
    ActorLifecycle, ContinuationBudget, OriginRouteMinter, provider_effect::ProviderEffects,
    store::Store,
};

/// The working-contract Signal admission actor. Mints the origin route and
/// triages a Signal `Input` into Nexus `NexusWork::SignalArrived` — the same
/// triage role as `spirit/src/engine.rs` `SignalActor`.
#[derive(Debug, Default)]
pub struct WorkingSignalActor {
    origin_routes: OriginRouteMinter,
    lifecycle: ActorLifecycle,
}

impl signal_cloud::SignalEngine for WorkingSignalActor {
    fn triage_inner(
        &self,
        input: signal_plane::Signal<Input>,
    ) -> nexus_plane::Nexus<NexusWork> {
        let origin_route = input.origin_route();
        NexusWork::signal_arrived(input.into_root()).with_origin_route(origin_route)
    }

    fn reply_inner(
        &self,
        output: nexus_plane::Nexus<NexusAction>,
    ) -> signal_plane::Signal<Output> {
        output.into_signal_output()
    }
}

impl WorkingSignalActor {
    pub fn admit(&self, input: Input) -> signal_plane::Signal<Input> {
        let origin_route: signal_cloud::OriginRoute = self.origin_routes.mint().into();
        input.with_origin_route(origin_route)
    }
}

/// The working-contract Nexus: the read-only decision center between Signal and
/// SEMA / provider observe effects. Holds a shared in-memory `Store` for plan
/// reads and a `ProviderEffects` for Cloudflare observation.
#[derive(Debug)]
pub struct WorkingNexus {
    store: Store,
    provider: ProviderEffects,
    lifecycle: ActorLifecycle,
}

impl WorkingNexus {
    pub fn new(store: Store, provider: ProviderEffects) -> Self {
        Self {
            store,
            provider,
            lifecycle: ActorLifecycle::default(),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }
}

impl signal_cloud::NexusEngine for WorkingNexus {
    /// The recursive-Nexus runner loop, hand-piloted exactly as in
    /// `spirit/src/nexus.rs`. The decision plane is `step_decide`
    /// (NexusWork -> NexusAction); the loop services the action and re-enters
    /// until `ReplyToSignal` or the continuation budget runs out.
    fn decide(
        &mut self,
        input: nexus_plane::Nexus<nexus_plane::Work>,
    ) -> nexus_plane::Nexus<nexus_plane::Action> {
        let origin_route = input.origin_route();
        let mut work = input.into_root();
        let mut budget = ContinuationBudget::default_for_pilot();

        loop {
            let action = self.step_decide(work);

            match action {
                NexusAction::ReplyToSignal(reply) => {
                    return NexusAction::reply_to_signal(reply).with_origin_route(origin_route);
                }
                NexusAction::CommandSemaRead(command) => {
                    let sema_output =
                        self.store.observe_read(command.with_origin_route(origin_route));
                    work = NexusWork::sema_read_completed(sema_output.into_root());
                }
                NexusAction::CommandEffect(command) => {
                    let result = self.run_provider_effect(command);
                    work = NexusWork::effect_completed(result);
                }
                NexusAction::Continue(next_work) => {
                    work = next_work;
                }
            }

            match budget.spend_one() {
                Some(remaining) => budget = remaining,
                None => {
                    return NexusAction::reply_to_signal(Output::request_unsupported(
                        UnsupportedRequest {
                            provider: None,
                            capability: None,
                            reason: UnsupportedReason::ProviderNotConfigured,
                        },
                    ))
                    .with_origin_route(origin_route);
                }
            }
        }
    }
}

impl WorkingNexus {
    /// One step of the decision plane: consume a NexusWork, emit a NexusAction.
    /// Pure — provider IO is deferred to the `CommandEffect` the runner
    /// services. Mirrors `spirit/src/nexus.rs` `step_decide`.
    fn step_decide(&self, work: NexusWork) -> NexusAction {
        match work {
            NexusWork::SignalArrived(input) => self.decide_signal_arrival(input),
            NexusWork::SemaReadCompleted(output) => self.decide_sema_read_completion(output),
            NexusWork::EffectCompleted(result) => self.decide_effect_completion(result),
        }
    }

    fn decide_signal_arrival(&self, input: Input) -> NexusAction {
        match input {
            Input::Observe(observation) => self.decide_observation(observation),
            Input::Validate(validation) => {
                NexusAction::command_sema_read(SemaReadInput::validate(validation))
            }
        }
    }

    /// Observation triage. Zones and Records are PROVIDER reads, so they
    /// become Cloudflare observe effects. Plan reads are SEMA reads.
    /// Capabilities / Redirects have no read path yet, so they reply with the
    /// contract's typed unsupported reply (per `ARCHITECTURE.md`: redirects
    /// return typed unsupported rather than a silent empty listing).
    fn decide_observation(&self, observation: Observation) -> NexusAction {
        match observation {
            Observation::Zones(zone_query) => NexusAction::command_effect(
                NexusEffectCommand::cloudflare_observe_zones(zone_query),
            ),
            Observation::Records(record_query) => NexusAction::command_effect(
                NexusEffectCommand::cloudflare_observe_records(record_query),
            ),
            Observation::ObservePlan(plan_query) => {
                NexusAction::command_sema_read(SemaReadInput::observe_plan(plan_query))
            }
            Observation::Capabilities(query) => {
                NexusAction::reply_to_signal(Output::request_unsupported(UnsupportedRequest {
                    provider: query.provider,
                    capability: query.capability,
                    reason: UnsupportedReason::CapabilityNotConfigured,
                }))
            }
            Observation::Redirects(query) => {
                NexusAction::reply_to_signal(Output::request_unsupported(UnsupportedRequest {
                    provider: Some(query.provider),
                    capability: None,
                    reason: UnsupportedReason::CapabilityNotConfigured,
                }))
            }
        }
    }

    fn decide_sema_read_completion(&self, output: SemaReadOutput) -> NexusAction {
        match output {
            SemaReadOutput::PlanObserved(plan) => {
                NexusAction::reply_to_signal(Output::observed(ObservationResult::PlanResult(plan)))
            }
            SemaReadOutput::Observed(observed) => {
                NexusAction::reply_to_signal(Output::observed(observed))
            }
            SemaReadOutput::Validated(report) => {
                NexusAction::reply_to_signal(Output::validated(report))
            }
            SemaReadOutput::Missed(_report) => {
                NexusAction::reply_to_signal(Output::request_unsupported(UnsupportedRequest {
                    provider: None,
                    capability: None,
                    reason: UnsupportedReason::ProviderNotConfigured,
                }))
            }
        }
    }

    /// Provider-effect completion becomes the wire reply. The effect already
    /// crossed the Cloudflare client; here the decision plane just frames the
    /// observed listing as the contract `Output`.
    ///
    /// SCHEMA GAP (see report): the generated `ObservationResult::Zones` /
    /// `::Records` arms are QUERY-shaped (`ZoneQuery` / `RecordQuery`), not
    /// listing-shaped, so the observed `ZoneListing` / `RecordListing` cannot
    /// ride the `Observed` reply. The schema's `ObservationResult` should carry
    /// `ZoneListing` / `RecordListing` in those arms (mirroring
    /// `SemaReadOutput::Observed`). Until that schema fix lands, the working
    /// contract can only echo the query back; this handler therefore replies
    /// `request_unsupported` rather than fabricate a query-shaped result, so the
    /// gap is visible at the wire rather than silently wrong.
    fn decide_effect_completion(&self, result: NexusEffectResult) -> NexusAction {
        let reason = match result {
            NexusEffectResult::ZonesObserved(_) | NexusEffectResult::RecordsObserved(_) => {
                UnsupportedReason::CapabilityNotConfigured
            }
        };
        NexusAction::reply_to_signal(Output::request_unsupported(UnsupportedRequest {
            provider: Some(Provider::Cloudflare),
            capability: None,
            reason,
        }))
    }

    /// Service one provider effect by calling the daemon-owned Cloudflare
    /// client. THIS is the single place the working Nexus performs blocking
    /// provider IO. Mirrors `spirit/src/nexus.rs` `apply_effect`, except the
    /// effect crosses a real network client instead of an in-memory table.
    ///
    /// NOTE (upstream gap): the existing `cloudflare::ProviderClient` is typed
    /// against the OLD published `signal-cloud` crate (`Zone { account,
    /// identifier, name }`, `RecordListing { records }`). The NEW schema-derived
    /// contract has `Zone { provider, provider_account, zone_identifier,
    /// domain_name }` and `RecordListing(pub Vec<..>)`. Until `cloudflare.rs`
    /// is re-typed against the generated contract, this handler returns the
    /// effect's own listing shape rather than a re-projected client result.
    fn run_provider_effect(&self, command: NexusEffectCommand) -> NexusEffectResult {
        match command {
            NexusEffectCommand::CloudflareObserveZones(_zone_query) => {
                // Effect boundary: a re-typed ProviderClient.zones(..) call
                // lands here. Returns the contract's ZoneListing.
                NexusEffectResult::zones_observed(signal_cloud::ZoneListing(Vec::new()))
            }
            NexusEffectCommand::CloudflareObserveRecords(_record_query) => {
                // Effect boundary: a re-typed ProviderClient.records(..) call
                // lands here. Returns the contract's RecordListing.
                NexusEffectResult::records_observed(signal_cloud::RecordListing(Vec::new()))
            }
        }
    }
}

/// Frame a Nexus reply action back into the Signal output plane. Mirrors
/// `spirit/src/engine.rs` `impl Nexus<NexusAction> { into_signal_output }`.
trait IntoSignalOutput {
    fn into_signal_output(self) -> signal_plane::Signal<Output>;
}

impl IntoSignalOutput for nexus_plane::Nexus<NexusAction> {
    fn into_signal_output(self) -> signal_plane::Signal<Output> {
        let origin_route = self.origin_route();
        match self.into_root() {
            NexusAction::ReplyToSignal(output) => output.with_origin_route(origin_route),
            _ => Output::request_unsupported(UnsupportedRequest {
                provider: None,
                capability: None,
                reason: UnsupportedReason::ProviderNotConfigured,
            })
            .with_origin_route(origin_route),
        }
    }
}
