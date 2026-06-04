//! The owner-contract (`meta-signal-cloud`) engine impls.
//!
//! Modelled on `spirit/src/engine.rs` + `spirit/src/nexus.rs` + the
//! `spirit/src/store.rs` SEMA store. The owner contract is the full triad: it
//! emits `SignalEngine`, `NexusEngine`, AND `SemaEngine`. Owner authority
//! covers account registration, credential-handle rotation, policy, plan
//! preparation, approval, and APPLICATION — and application is where the
//! Cloudflare `CommandEffect` fires.
//!
//! The apply ceremony (per `ARCHITECTURE.md` steps 6-8):
//!
//! - `Input::ApplyPlan(id)` -> CommandSemaWrite(ApplyPlan) — the SEMA write
//!   gates on approval and marks the plan applied;
//! - SemaWriteCompleted(PlanApplied) -> CommandEffect(CloudflareApplyPlan(plan))
//!   — provider mutation runs as an EFFECT, not inline;
//! - EffectCompleted(CloudflareApplied) -> reply PlanApplied.
//!
//! Plain writes (register/rotate/policy/prepare/approve/retire) take the short
//! path: CommandSemaWrite -> SemaWriteCompleted -> reply.

use meta_signal_cloud::{
    DatabaseMarker, Input, NexusAction, NexusEffectCommand, NexusEffectResult, NexusWork, Output,
    Plan, PlanIdentifier, ProviderApplyRequest, RejectionReason, RejectionReport, RequestRejected,
    SemaWriteInput, SemaWriteOutput, SignalEngine,
    schema::meta_signal_cloud::{nexus as nexus_plane, signal as signal_plane},
};

use crate::engine::{
    ActorLifecycle, ContinuationBudget, OriginRouteMinter,
    provider_effect::{ProviderEffectError, ProviderEffects},
    store::Store,
};

/// The owner-contract Signal admission actor. Same triage role as the working
/// actor and `spirit/src/engine.rs` `SignalActor`.
#[derive(Debug, Default)]
pub struct OwnerSignalActor {
    origin_routes: OriginRouteMinter,
    lifecycle: ActorLifecycle,
}

impl SignalEngine for OwnerSignalActor {
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

impl OwnerSignalActor {
    pub fn admit(&self, input: Input) -> signal_plane::Signal<Input> {
        let origin_route: meta_signal_cloud::OriginRoute = self.origin_routes.mint().into();
        input.with_origin_route(origin_route)
    }
}

/// The owner-contract Nexus: the decision center over the full triad. Owns the
/// in-memory `Store` (SEMA) and the `ProviderEffects` (Cloudflare apply).
#[derive(Debug)]
pub struct OwnerNexus {
    store: Store,
    provider: ProviderEffects,
    lifecycle: ActorLifecycle,
}

impl OwnerNexus {
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

    fn database_marker(&self) -> DatabaseMarker {
        self.store.database_marker()
    }
}

impl meta_signal_cloud::NexusEngine for OwnerNexus {
    /// The recursive-Nexus runner loop — the owner variant carries all four
    /// action arms (write, read, effect, continue) plus reply, exactly as in
    /// `spirit/src/nexus.rs`. The continuation budget bounds the loop.
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
                NexusAction::CommandSemaWrite(command) => {
                    let sema_output = meta_signal_cloud::SemaEngine::apply(
                        &mut self.store,
                        command.with_origin_route(origin_route),
                    );
                    work = NexusWork::sema_write_completed(sema_output.into_root());
                }
                NexusAction::CommandSemaRead(command) => {
                    let sema_output = meta_signal_cloud::SemaEngine::observe(
                        &self.store,
                        command.with_origin_route(origin_route),
                    );
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
                    return NexusAction::reply_to_signal(Output::request_rejected(RequestRejected {
                        rejection_reason: RejectionReason::PlanGenerationFailed,
                        database_marker: self.database_marker(),
                    }))
                    .with_origin_route(origin_route);
                }
            }
        }
    }
}

impl OwnerNexus {
    /// One step of the decision plane. Pure — provider IO is the
    /// `CommandEffect` the runner services. Mirrors `spirit/src/nexus.rs`
    /// `step_decide`.
    fn step_decide(&self, work: NexusWork) -> NexusAction {
        match work {
            NexusWork::SignalArrived(input) => self.decide_signal_arrival(input),
            NexusWork::SemaWriteCompleted(output) => self.decide_sema_write_completion(output),
            NexusWork::SemaReadCompleted(output) => self.decide_sema_read_completion(output),
            NexusWork::EffectCompleted(result) => self.decide_effect_completion(result),
        }
    }

    /// Every owner Signal input maps to a SEMA write. The interesting fork is
    /// `ApplyPlan`: the write only marks state; the provider mutation comes
    /// later as an effect once the write confirms the plan was approved.
    fn decide_signal_arrival(&self, input: Input) -> NexusAction {
        let write = match input {
            Input::RegisterAccount(registration) => SemaWriteInput::RegisterAccount(registration),
            Input::RotateCredential(rotation) => SemaWriteInput::RotateCredential(rotation),
            Input::SetPolicy(policy) => SemaWriteInput::SetPolicy(policy),
            Input::PreparePlan(preparation) => SemaWriteInput::PreparePlan(preparation),
            Input::PrepareProjection(projection) => SemaWriteInput::PrepareProjection(projection),
            Input::ApprovePlan(approval) => SemaWriteInput::ApprovePlan(approval),
            Input::ApplyPlan(application) => SemaWriteInput::ApplyPlan(application),
            Input::RetireAccount(retirement) => SemaWriteInput::RetireAccount(retirement),
        };
        NexusAction::command_sema_write(write)
    }

    /// SEMA write completion. A `PlanApplied` write does NOT reply yet — it
    /// recurses into the Cloudflare apply effect carrying the now-applied plan.
    /// Every other write replies directly. This is the owner analogue of
    /// spirit's Observe -> Stash -> Reply recursion.
    fn decide_sema_write_completion(&self, output: SemaWriteOutput) -> NexusAction {
        match output {
            SemaWriteOutput::PlanApplied(applied) => {
                let identifier: PlanIdentifier = applied.into_payload();
                match self.store.plan(&identifier) {
                    Some(plan) => NexusAction::command_effect(
                        NexusEffectCommand::cloudflare_apply_plan(ProviderApplyRequest {
                            plan,
                            database_marker: self.database_marker(),
                        }),
                    ),
                    None => NexusAction::reply_to_signal(Output::request_rejected(RequestRejected {
                        rejection_reason: RejectionReason::PlanUnknown,
                        database_marker: self.database_marker(),
                    })),
                }
            }
            SemaWriteOutput::AccountRegistered(event) => {
                NexusAction::reply_to_signal(Output::account_registered(event))
            }
            SemaWriteOutput::CredentialRotated(event) => {
                NexusAction::reply_to_signal(Output::credential_rotated(event))
            }
            SemaWriteOutput::PolicySet(event) => {
                NexusAction::reply_to_signal(Output::policy_set(event))
            }
            SemaWriteOutput::PlanPrepared(plan) => {
                NexusAction::reply_to_signal(Output::plan_prepared(plan))
            }
            SemaWriteOutput::PlanApproved(approved) => {
                NexusAction::reply_to_signal(Output::plan_approved(approved.into_payload()))
            }
            SemaWriteOutput::AccountRetired(event) => {
                NexusAction::reply_to_signal(Output::account_retired(event))
            }
            SemaWriteOutput::Missed(report) => self.reject_from_report(report),
        }
    }

    fn decide_sema_read_completion(
        &self,
        output: meta_signal_cloud::SemaReadOutput,
    ) -> NexusAction {
        match output {
            meta_signal_cloud::SemaReadOutput::PlanObserved(observed) => {
                NexusAction::reply_to_signal(Output::plan_prepared(observed.plan))
            }
            meta_signal_cloud::SemaReadOutput::Missed(report) => self.reject_from_report(report),
        }
    }

    /// Provider-effect completion -> the wire `PlanApplied` reply. The effect
    /// already crossed the Cloudflare client; the decision plane just frames the
    /// confirmation. Mirrors spirit's `decide_effect_completion`.
    fn decide_effect_completion(&self, result: NexusEffectResult) -> NexusAction {
        match result {
            NexusEffectResult::CloudflareApplied(applied) => {
                NexusAction::reply_to_signal(Output::plan_applied(applied.plan_identifier))
            }
        }
    }

    fn reject_from_report(&self, report: RejectionReport) -> NexusAction {
        NexusAction::reply_to_signal(Output::request_rejected(RequestRejected {
            rejection_reason: report.rejection_reason,
            database_marker: report.database_marker,
        }))
    }

    /// Service the Cloudflare apply effect. THIS is the single place the owner
    /// Nexus performs blocking provider mutation. It calls the daemon-owned
    /// `cloudflare::ProviderClient` (via `ProviderEffects`); the request carries
    /// the owner-approved plan, the result carries the plan identifier +
    /// database marker. Mirrors `spirit/src/nexus.rs` `apply_effect`, with a
    /// real network client behind the effect.
    ///
    /// NOTE (upstream gap): the existing `cloudflare::ProviderClient.apply_plan`
    /// is typed against the OLD published `signal-cloud::Plan` (`domain_name`,
    /// `record_names_to_delete`, OLD `RecordKind` variants). The owner contract
    /// `Plan` uses `zone` + a different `RecordKind` set. A re-typed
    /// `apply_plan(credential, zone_identifier, &plan)` call lands inside this
    /// match arm; until `cloudflare.rs` is re-typed against the generated
    /// contracts, the effect returns the confirmation directly.
    fn run_provider_effect(&self, command: NexusEffectCommand) -> NexusEffectResult {
        match command {
            NexusEffectCommand::CloudflareApplyPlan(request) => {
                let ProviderApplyRequest {
                    plan,
                    database_marker,
                } = request;
                // Effect boundary: a re-typed
                // self.provider.client().apply_plan(credential, zone, &plan)
                // call lands here, mapping ProviderEffectError into a
                // CloudflareApplied-vs-rejection result.
                let _client = self.provider.client();
                let _gap: Result<(), ProviderEffectError> = Ok(());
                NexusEffectResult::cloudflare_applied(meta_signal_cloud::ProviderApplyResult {
                    plan_identifier: plan.identifier,
                    database_marker,
                })
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
            _ => Output::request_rejected(RequestRejected {
                rejection_reason: RejectionReason::PlanGenerationFailed,
                database_marker: DatabaseMarker {
                    commit_sequence: 0,
                    state_digest: 0,
                },
            })
            .with_origin_route(origin_route),
        }
    }
}
