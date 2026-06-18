//! Hand-implemented `SchemaRuntime` noun — the single data-bearing type that
//! implements both schema-engine traits (`nexus::NexusEngine` +
//! `sema::SemaEngine`) over a durable [`SchemaStore`].
//!
//! `decide` is the routing brain: ordinary observation/validation requests route
//! to `SemaRead`, meta registration/policy/plan mutations route to `SemaWrite`,
//! and the SEMA completions turn back into ordinary / meta Signal replies. The
//! account and plan state lives in the shared `Arc<SchemaStore>` (the two
//! schema-emitted SEMA tables), so each request is served by its own
//! `SchemaRuntime` over a clone of that handle while the durable tables are
//! shared across connections. This engine is still the pure schema/Nexus/SEMA
//! experiment; the live `cloud-daemon` currently uses the actor-native listener
//! spine with the provider `Store` behind a schema bridge until provider effects
//! move fully into the schema effect plane.

use std::sync::Arc;

use meta_signal_cloud::schema::lib as meta;
use signal_cloud::schema::lib as ordinary;

use crate::schema::{nexus, sema};
use crate::schema_store::{ProviderProjection, SchemaStore};

/// The cloud schema-engine noun. Carries the durable `Store` (the two SEMA
/// tables) plus the static capability matrix and the policy snapshot the SEMA
/// `SetPolicy` write reports counts from. Implements both engine traits; the
/// generated `NexusEngine::execute` drives the `Runner` over it.
#[derive(Debug)]
pub struct SchemaRuntime {
    store: Arc<SchemaStore>,
    capabilities: Vec<ordinary::CapabilityObservation>,
    policy: Option<meta::Policy>,
}

impl Default for SchemaRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaRuntime {
    pub fn new() -> Self {
        Self::with_store(Arc::new(SchemaStore::new()))
    }

    /// Build an engine over a SHARED `Store`. The daemon constructs one per
    /// request from a single shared `Arc<SchemaStore>`, so concurrent requests
    /// share the durable tables while each owns its policy snapshot.
    pub fn with_store(store: Arc<SchemaStore>) -> Self {
        Self {
            store,
            capabilities: Self::default_capabilities(),
            policy: None,
        }
    }

    pub fn with_capabilities(capabilities: Vec<ordinary::CapabilityObservation>) -> Self {
        Self {
            store: Arc::new(SchemaStore::new()),
            capabilities,
            policy: None,
        }
    }

    /// Drive one arriving signal to its reply `SignalOutput` over a SHARED
    /// `Store`. The daemon builds one fresh `SchemaRuntime` per request from the
    /// shared `Arc<SchemaStore>` (the per-request engine model, intent 2alg),
    /// runs the Nexus continuation to its terminal `ReplyToSignal`, and returns
    /// the reply. A non-reply terminal action is a runtime-invariant violation;
    /// it surfaces as an ordinary `PlanExpired` rejection to the caller. This is
    /// the per-request execute the emitted daemon's `handle_working_input` /
    /// `handle_meta_connection` hooks call — it lives on the engine noun, not
    /// the ZST daemon marker.
    pub fn reply_to_signal(
        store: Arc<SchemaStore>,
        signal_input: nexus::SignalInput,
    ) -> nexus::SignalOutput {
        let mut engine = Self::with_store(store);
        let action = engine.decide_signal_arrival(signal_input);
        engine.resolve_action(action)
    }

    pub fn store(&self) -> &SchemaStore {
        self.store.as_ref()
    }

    /// The registered account bindings, read from the durable account-policy
    /// table.
    pub fn accounts(&self) -> Vec<sema::AccountBinding> {
        match self.store.lock() {
            Ok(state) => state.accounts().to_vec(),
            Err(_) => Vec::new(),
        }
    }

    pub fn policy(&self) -> Option<&meta::Policy> {
        self.policy.as_ref()
    }

    fn commit_sequence(&self) -> u64 {
        self.store.commit_sequence().unwrap_or(0)
    }

    fn default_capabilities() -> Vec<ordinary::CapabilityObservation> {
        vec![
            ordinary::CapabilityObservation {
                provider: ordinary::Provider::Cloudflare,
                capability: ordinary::Capability::DomainNameSystemRecords,
                capability_state: ordinary::CapabilityState::Compiled,
            },
            ordinary::CapabilityObservation {
                provider: ordinary::Provider::Cloudflare,
                capability: ordinary::Capability::RedirectRules,
                capability_state: ordinary::CapabilityState::NotBuilt,
            },
            ordinary::CapabilityObservation {
                provider: ordinary::Provider::GoogleCloud,
                capability: ordinary::Capability::DomainNameSystemRecords,
                capability_state: ordinary::CapabilityState::NotBuilt,
            },
            ordinary::CapabilityObservation {
                provider: ordinary::Provider::Hetzner,
                capability: ordinary::Capability::DomainNameSystemRecords,
                capability_state: ordinary::CapabilityState::NotBuilt,
            },
        ]
    }

    // ---- decide: signal arrival routing ---------------------------------

    fn decide_signal_arrival(&self, input: nexus::SignalInput) -> nexus::NexusAction {
        match input {
            nexus::SignalInput::OrdinaryInput(input) => self.decide_ordinary_input(input),
            nexus::SignalInput::MetaInput(input) => self.decide_meta_input(input),
        }
    }

    fn decide_ordinary_input(&self, input: ordinary::Input) -> nexus::NexusAction {
        match input {
            ordinary::Input::Observe(observation) => {
                nexus::NexusAction::command_sema_read(sema::SemaReadInput::observe(observation))
            }
            ordinary::Input::Validate(validation) => {
                nexus::NexusAction::command_sema_read(sema::SemaReadInput::validate(validation))
            }
        }
    }

    fn decide_meta_input(&self, input: meta::Input) -> nexus::NexusAction {
        let command = match input {
            meta::Input::RegisterAccount(payload) => {
                sema::SemaWriteInput::register_account(payload)
            }
            meta::Input::RotateCredential(payload) => {
                sema::SemaWriteInput::rotate_credential(payload)
            }
            meta::Input::SetPolicy(payload) => sema::SemaWriteInput::set_policy(payload),
            meta::Input::PreparePlan(payload) => sema::SemaWriteInput::prepare_plan(payload),
            // Host plan preparation and host destruction are not carried by the
            // experimental schema engine in Phase 1 — the live provider `Store`
            // owns host plans. Reply with the honest rejection rather than
            // fabricating a host plan on this plane.
            meta::Input::PrepareHostPlan(_) | meta::Input::PrepareHostDestruction(_) => {
                return nexus::NexusAction::reply_to_signal(nexus::SignalOutput::meta_output(
                    meta::Output::request_rejected(
                        self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
                    ),
                ));
            }
            meta::Input::PrepareProjection(payload) => {
                sema::SemaWriteInput::prepare_projection(payload)
            }
            meta::Input::ApprovePlan(payload) => sema::SemaWriteInput::approve_plan(payload),
            meta::Input::ApplyPlan(payload) => sema::SemaWriteInput::apply_plan(payload),
            meta::Input::RetireAccount(payload) => sema::SemaWriteInput::retire_account(payload),
        };
        nexus::NexusAction::command_sema_write(command)
    }

    fn decide_read_completion(&self, output: sema::SemaReadOutput) -> nexus::NexusAction {
        let output = match output {
            sema::SemaReadOutput::Observed(payload) => {
                ordinary::Output::observed(payload.into_payload())
            }
            sema::SemaReadOutput::Validated(payload) => {
                ordinary::Output::validated(payload.into_payload().into_payload())
            }
            sema::SemaReadOutput::PlanObserved(payload) => ordinary::Output::observed(
                ordinary::ObservationResult::plan_result(payload.into_payload().into_payload()),
            ),
            sema::SemaReadOutput::Missed(_) => Self::ordinary_plan_expired_rejection(),
        };
        nexus::NexusAction::reply_to_signal(nexus::SignalOutput::ordinary_output(output))
    }

    fn decide_write_completion(&self, output: sema::SemaWriteOutput) -> nexus::NexusAction {
        let output = match output {
            sema::SemaWriteOutput::AccountRegistered(payload) => {
                meta::Output::account_registered(payload)
            }
            sema::SemaWriteOutput::CredentialRotated(payload) => {
                meta::Output::credential_rotated(payload)
            }
            sema::SemaWriteOutput::PolicySet(payload) => meta::Output::policy_set(payload),
            sema::SemaWriteOutput::PlanPrepared(payload) => {
                meta::Output::plan_prepared(payload.into_payload())
            }
            sema::SemaWriteOutput::PlanApproved(payload) => {
                meta::Output::plan_approved(payload.into_payload())
            }
            sema::SemaWriteOutput::PlanApplied(payload) => {
                meta::Output::plan_applied(payload.into_payload())
            }
            sema::SemaWriteOutput::AccountRetired(payload) => {
                meta::Output::account_retired(payload)
            }
            sema::SemaWriteOutput::RequestRejected(payload) => {
                meta::Output::request_rejected(payload)
            }
        };
        nexus::NexusAction::reply_to_signal(nexus::SignalOutput::meta_output(output))
    }

    fn resolve_action(&mut self, action: nexus::NexusAction) -> nexus::SignalOutput {
        match action {
            nexus::NexusAction::CommandSemaRead(command) => {
                let output = self.observe_sema(command.into_payload());
                self.resolve_action(self.decide_read_completion(output))
            }
            nexus::NexusAction::CommandSemaWrite(command) => {
                let output = self.apply_sema(command.into_payload());
                self.resolve_action(self.decide_write_completion(output))
            }
            nexus::NexusAction::CommandEffect(command) => {
                let output = self.run_effect(command);
                self.resolve_action(nexus::NexusAction::r#continue(
                    nexus::NexusWork::effect_completed(output),
                ))
            }
            nexus::NexusAction::ReplyToSignal(output) => output.into_payload(),
            nexus::NexusAction::Continue(_) => Self::ordinary_rejected_signal_output(),
        }
    }

    fn ordinary_rejected_signal_output() -> nexus::SignalOutput {
        nexus::SignalOutput::ordinary_output(Self::ordinary_plan_expired_rejection())
    }

    fn ordinary_plan_expired_rejection() -> ordinary::Output {
        ordinary::Output::request_rejected(ordinary::RejectionReason::PlanExpired)
    }

    // ---- sema observe (the read path over the durable tables) -----------

    fn observe_sema(&self, input: sema::SemaReadInput) -> sema::SemaReadOutput {
        match input {
            sema::SemaReadInput::Observe(observation) => {
                self.observe_ordinary(observation.into_payload())
            }
            sema::SemaReadInput::ObservePlan(query) => self.observe_plan(query.into_payload()),
            sema::SemaReadInput::Validate(validation) => {
                self.validate_ordinary(validation.into_payload())
            }
        }
    }

    fn observe_ordinary(&self, observation: ordinary::Observation) -> sema::SemaReadOutput {
        let result = match observation {
            ordinary::Observation::Capabilities(query) => {
                ordinary::ObservationResult::Capabilities(
                    self.capability_report(query.into_payload()),
                )
            }
            ordinary::Observation::Zones(_) => {
                ordinary::ObservationResult::Zones(ordinary::ZoneListing::new(Vec::new()))
            }
            ordinary::Observation::Records(_) => {
                ordinary::ObservationResult::Records(ordinary::RecordListing::new(Vec::new()))
            }
            ordinary::Observation::Redirects(_) => {
                ordinary::ObservationResult::Redirects(ordinary::RedirectListing::new(Vec::new()))
            }
            ordinary::Observation::ObserveServers(_) => {
                ordinary::ObservationResult::Servers(ordinary::CloudHostListing::new(Vec::new()))
            }
            ordinary::Observation::ObservePlan(query) => {
                return self.observe_plan(query.into_payload());
            }
        };
        sema::SemaReadOutput::observed(ordinary::Observed::new(result))
    }

    fn observe_plan(&self, query: ordinary::PlanQuery) -> sema::SemaReadOutput {
        // The `PlanTable` is consulted by plan identifier (the composite 1:N
        // key). Plan generation is not yet on this pure schema engine path —
        // PreparePlan still returns the honest rejection here, while the live
        // daemon reaches diff-aware planning through the provider `Store`
        // bridge — so the table is empty and the durable lookup misses. The
        // read still routes through the store, demonstrating the keyed lookup;
        // projecting a stored meta `Plan` back into the ordinary `Plan` reply
        // lands with engine-side plan generation.
        let _ = self
            .store
            .lock()
            .map(|state| state.plan(query.payload().payload()).cloned());
        sema::SemaReadOutput::missed(self.rejection_report(sema::RejectionReason::PlanUnknown))
    }

    fn validate_ordinary(&self, _validation: ordinary::Validation) -> sema::SemaReadOutput {
        sema::SemaReadOutput::validated(ordinary::Validated::new(ordinary::ValidationReport::new(
            Vec::new(),
        )))
    }

    // ---- sema apply (the write path over the durable tables) ------------

    fn apply_sema(&mut self, input: sema::SemaWriteInput) -> sema::SemaWriteOutput {
        match input {
            sema::SemaWriteInput::RegisterAccount(registration) => {
                self.register_account(registration.into_payload())
            }
            sema::SemaWriteInput::RotateCredential(rotation) => {
                self.rotate_credential(rotation.into_payload())
            }
            sema::SemaWriteInput::SetPolicy(policy) => self.set_policy(policy.into_payload()),
            sema::SemaWriteInput::PreparePlan(_) => sema::SemaWriteOutput::request_rejected(
                self.meta_rejection(meta::RejectionReason::PlanGenerationFailed),
            ),
            sema::SemaWriteInput::PrepareProjection(_) => sema::SemaWriteOutput::request_rejected(
                self.meta_rejection(meta::RejectionReason::PlanGenerationFailed),
            ),
            sema::SemaWriteInput::ApprovePlan(approval) => {
                self.approve_plan(approval.into_payload())
            }
            sema::SemaWriteInput::ApplyPlan(application) => {
                self.apply_plan(application.into_payload())
            }
            sema::SemaWriteInput::RetireAccount(retirement) => {
                self.retire_account(retirement.into_payload())
            }
        }
    }

    fn register_account(&mut self, registration: meta::Registration) -> sema::SemaWriteOutput {
        let binding = sema::AccountBinding {
            provider: ProviderProjection::new(registration.provider).into_ordinary(),
            provider_account: ordinary::ProviderAccount::new(
                registration.provider_account.payload().clone(),
            ),
            credential_handle: registration.credential_handle.clone(),
        };
        match self.store.lock() {
            Ok(mut state) => {
                state.put_account(binding);
                sema::SemaWriteOutput::account_registered(meta::AccountRegistered {
                    provider: registration.provider,
                    provider_account: registration.provider_account,
                })
            }
            Err(_) => sema::SemaWriteOutput::request_rejected(
                self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
            ),
        }
    }

    fn rotate_credential(&mut self, rotation: meta::Rotation) -> sema::SemaWriteOutput {
        let provider = ProviderProjection::new(rotation.provider).into_ordinary();
        let provider_account =
            ordinary::ProviderAccount::new(rotation.provider_account.payload().clone());
        let rotated = match self.store.lock() {
            Ok(mut state) => state.rotate_credential(
                &provider,
                &provider_account,
                rotation.credential_handle.clone(),
            ),
            Err(_) => {
                return sema::SemaWriteOutput::request_rejected(
                    self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
                );
            }
        };
        match rotated {
            Some(_) => sema::SemaWriteOutput::credential_rotated(meta::CredentialRotated {
                provider: rotation.provider,
                provider_account: rotation.provider_account,
            }),
            None => sema::SemaWriteOutput::request_rejected(
                self.meta_rejection(meta::RejectionReason::AccountUnknown),
            ),
        }
    }

    fn set_policy(&mut self, policy: meta::Policy) -> sema::SemaWriteOutput {
        let capability_policy_count = policy.capabilities.len() as u64;
        let zone_policy_count = policy.zones.len() as u64;
        if let Ok(mut state) = self.store.lock() {
            state.next_commit_sequence();
        }
        self.policy = Some(policy);
        sema::SemaWriteOutput::policy_set(meta::PolicySet {
            capability_policy_count,
            zone_policy_count,
        })
    }

    fn approve_plan(&mut self, approval: meta::Approval) -> sema::SemaWriteOutput {
        let plan = approval.into_payload();
        let approved = match self.store.lock() {
            Ok(mut state) => state.approve_plan(plan.payload()),
            Err(_) => {
                return sema::SemaWriteOutput::request_rejected(
                    self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
                );
            }
        };
        match approved {
            Some(_) => sema::SemaWriteOutput::plan_approved(meta::PlanApproved::new(plan)),
            None => sema::SemaWriteOutput::request_rejected(
                self.meta_rejection(meta::RejectionReason::PlanUnknown),
            ),
        }
    }

    fn apply_plan(&mut self, application: meta::Application) -> sema::SemaWriteOutput {
        let plan = application.into_payload();
        let approved = match self.store.lock() {
            Ok(state) => state.plan(plan.payload()).map(|stored| stored.approved),
            Err(_) => {
                return sema::SemaWriteOutput::request_rejected(
                    self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
                );
            }
        };
        match approved {
            Some(true) => sema::SemaWriteOutput::plan_applied(meta::PlanApplied::new(plan)),
            Some(false) => sema::SemaWriteOutput::request_rejected(
                self.meta_rejection(meta::RejectionReason::PlanNotApproved),
            ),
            None => sema::SemaWriteOutput::request_rejected(
                self.meta_rejection(meta::RejectionReason::PlanUnknown),
            ),
        }
    }

    fn retire_account(&mut self, retirement: meta::Retirement) -> sema::SemaWriteOutput {
        let provider = ProviderProjection::new(retirement.provider).into_ordinary();
        let provider_account =
            ordinary::ProviderAccount::new(retirement.provider_account.payload().clone());
        let retired = match self.store.lock() {
            Ok(mut state) => state.retire_account(&provider, &provider_account),
            Err(_) => {
                return sema::SemaWriteOutput::request_rejected(
                    self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
                );
            }
        };
        match retired {
            Some(_) => sema::SemaWriteOutput::account_retired(meta::AccountRetired {
                provider: retirement.provider,
                provider_account: retirement.provider_account,
            }),
            None => sema::SemaWriteOutput::request_rejected(
                self.meta_rejection(meta::RejectionReason::AccountUnknown),
            ),
        }
    }

    fn capability_report(&self, query: ordinary::CapabilityQuery) -> ordinary::CapabilityReport {
        ordinary::CapabilityReport::new(
            self.capabilities
                .iter()
                .filter(|observation| {
                    query
                        .provider
                        .as_ref()
                        .is_none_or(|provider| provider == &observation.provider)
                })
                .filter(|observation| {
                    query
                        .capability
                        .as_ref()
                        .is_none_or(|capability| capability == &observation.capability)
                })
                .cloned()
                .collect(),
        )
    }

    fn rejection_report(&self, reason: sema::RejectionReason) -> sema::RejectionReport {
        let commit_sequence = self.commit_sequence();
        sema::RejectionReport {
            reason,
            marker: sema::StateMarker {
                commit_sequence: sema::CommitSequence::new(commit_sequence),
                state_digest: sema::StateDigest::new(commit_sequence),
            },
        }
    }

    fn meta_rejection(&self, reason: meta::RejectionReason) -> meta::RequestRejected {
        let commit_sequence = self.commit_sequence();
        meta::RequestRejected {
            rejection_reason: reason,
            database_marker: meta::DatabaseMarker {
                commit_sequence: meta::CommitSequence::new(commit_sequence),
                state_digest: meta::StateDigest::new(commit_sequence),
            },
        }
    }

    fn run_effect(&mut self, input: nexus::CommandEffect) -> nexus::EffectResult {
        match input.into_payload() {
            nexus::EffectCommand::CloudflareObserveZones(_) => {
                nexus::EffectResult::zones_observed(ordinary::ZoneListing::new(Vec::new()))
            }
            nexus::EffectCommand::CloudflareObserveRecords(_) => {
                nexus::EffectResult::records_observed(ordinary::RecordListing::new(Vec::new()))
            }
            nexus::EffectCommand::CloudflareApplyPlan(identifier) => {
                nexus::EffectResult::plan_applied(meta::PlanApplied::new(identifier.into_payload()))
            }
        }
    }
}

impl nexus::NexusEngine for SchemaRuntime {
    fn decide(
        &mut self,
        input: nexus::nexus::Nexus<nexus::nexus::Work>,
    ) -> nexus::nexus::Nexus<nexus::nexus::Action> {
        let origin_route = input.origin_route();
        let action = match input.into_root() {
            nexus::NexusWork::SignalArrived(input) => {
                self.decide_signal_arrival(input.into_payload())
            }
            nexus::NexusWork::SemaReadCompleted(output) => {
                self.decide_read_completion(output.into_payload())
            }
            nexus::NexusWork::SemaWriteCompleted(output) => {
                self.decide_write_completion(output.into_payload())
            }
            nexus::NexusWork::EffectCompleted(result) => nexus::NexusAction::r#continue(
                nexus::NexusWork::effect_completed(result.into_payload()),
            ),
        };
        action.with_origin_route(origin_route)
    }
}

impl sema::SemaEngine for SchemaRuntime {
    fn apply_inner(
        &mut self,
        input: sema::sema::Sema<sema::sema::WriteInput>,
    ) -> sema::sema::Sema<sema::sema::WriteOutput> {
        let origin_route = input.origin_route();
        self.apply_sema(input.into_root())
            .with_origin_route(origin_route)
    }

    fn observe_inner(
        &self,
        input: sema::sema::Sema<sema::sema::ReadInput>,
    ) -> sema::sema::Sema<sema::sema::ReadOutput> {
        let origin_route = input.origin_route();
        self.observe_sema(input.into_root())
            .with_origin_route(origin_route)
    }
}
