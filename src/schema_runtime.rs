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

use crate::schema::nexus::NexusEngine;
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
        let work =
            nexus::NexusWork::SignalArrived(signal_input).with_origin_route(nexus::OriginRoute(0));
        match engine.execute(work).into_root() {
            nexus::NexusAction::ReplyToSignal(output) => output,
            _ => nexus::SignalOutput::OrdinaryOutput(ordinary::Output::RequestRejected(
                ordinary::RejectedRequest(ordinary::RejectionReason::PlanExpired),
            )),
        }
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
                nexus::NexusAction::CommandSemaRead(sema::SemaReadInput::Observe(observation))
            }
            ordinary::Input::Validate(validation) => {
                nexus::NexusAction::CommandSemaRead(sema::SemaReadInput::Validate(validation))
            }
        }
    }

    fn decide_meta_input(&self, input: meta::Input) -> nexus::NexusAction {
        let command = match input {
            meta::Input::RegisterAccount(payload) => sema::SemaWriteInput::RegisterAccount(payload),
            meta::Input::RotateCredential(payload) => {
                sema::SemaWriteInput::RotateCredential(payload)
            }
            meta::Input::SetPolicy(payload) => sema::SemaWriteInput::SetPolicy(payload),
            meta::Input::PreparePlan(payload) => sema::SemaWriteInput::PreparePlan(payload),
            meta::Input::PrepareProjection(payload) => {
                sema::SemaWriteInput::PrepareProjection(payload)
            }
            meta::Input::ApprovePlan(payload) => sema::SemaWriteInput::ApprovePlan(payload),
            meta::Input::ApplyPlan(payload) => sema::SemaWriteInput::ApplyPlan(payload),
            meta::Input::RetireAccount(payload) => sema::SemaWriteInput::RetireAccount(payload),
        };
        nexus::NexusAction::CommandSemaWrite(command)
    }

    fn decide_read_completion(&self, output: sema::SemaReadOutput) -> nexus::NexusAction {
        let output = match output {
            sema::SemaReadOutput::Observed(payload) => ordinary::Output::Observed(payload),
            sema::SemaReadOutput::Validated(payload) => ordinary::Output::Validated(payload),
            sema::SemaReadOutput::PlanObserved(payload) => {
                ordinary::Output::Observed(ordinary::ObservationResult::PlanResult(payload))
            }
            sema::SemaReadOutput::Missed(_) => ordinary::Output::RequestRejected(
                ordinary::RejectedRequest(ordinary::RejectionReason::PlanExpired),
            ),
        };
        nexus::NexusAction::ReplyToSignal(nexus::SignalOutput::OrdinaryOutput(output))
    }

    fn decide_write_completion(&self, output: sema::SemaWriteOutput) -> nexus::NexusAction {
        let output = match output {
            sema::SemaWriteOutput::AccountRegistered(payload) => {
                meta::Output::AccountRegistered(payload)
            }
            sema::SemaWriteOutput::CredentialRotated(payload) => {
                meta::Output::CredentialRotated(payload)
            }
            sema::SemaWriteOutput::PolicySet(payload) => meta::Output::PolicySet(payload),
            sema::SemaWriteOutput::PlanPrepared(payload) => meta::Output::PlanPrepared(payload),
            sema::SemaWriteOutput::PlanApproved(payload) => meta::Output::PlanApproved(payload),
            sema::SemaWriteOutput::PlanApplied(payload) => meta::Output::PlanApplied(payload),
            sema::SemaWriteOutput::AccountRetired(payload) => meta::Output::AccountRetired(payload),
            sema::SemaWriteOutput::RequestRejected(payload) => {
                meta::Output::RequestRejected(payload)
            }
        };
        nexus::NexusAction::ReplyToSignal(nexus::SignalOutput::MetaOutput(output))
    }

    // ---- sema observe (the read path over the durable tables) -----------

    fn observe_sema(&self, input: sema::SemaReadInput) -> sema::SemaReadOutput {
        match input {
            sema::SemaReadInput::Observe(observation) => self.observe_ordinary(observation),
            sema::SemaReadInput::ObservePlan(query) => self.observe_plan(query),
            sema::SemaReadInput::Validate(validation) => self.validate_ordinary(validation),
        }
    }

    fn observe_ordinary(&self, observation: ordinary::Observation) -> sema::SemaReadOutput {
        let result = match observation {
            ordinary::Observation::Capabilities(query) => {
                ordinary::ObservationResult::Capabilities(self.capability_report(query))
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
            ordinary::Observation::ObservePlan(query) => {
                return self.observe_plan(query);
            }
        };
        sema::SemaReadOutput::Observed(result)
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
            .map(|state| state.plan(query.0.as_str()).cloned());
        sema::SemaReadOutput::Missed(self.rejection_report(sema::RejectionReason::PlanUnknown))
    }

    fn validate_ordinary(&self, _validation: ordinary::Validation) -> sema::SemaReadOutput {
        sema::SemaReadOutput::Validated(ordinary::ValidationReport::new(Vec::new()))
    }

    // ---- sema apply (the write path over the durable tables) ------------

    fn apply_sema(&mut self, input: sema::SemaWriteInput) -> sema::SemaWriteOutput {
        match input {
            sema::SemaWriteInput::RegisterAccount(registration) => {
                self.register_account(registration)
            }
            sema::SemaWriteInput::RotateCredential(rotation) => self.rotate_credential(rotation),
            sema::SemaWriteInput::SetPolicy(policy) => self.set_policy(policy),
            sema::SemaWriteInput::PreparePlan(_) => sema::SemaWriteOutput::RequestRejected(
                self.meta_rejection(meta::RejectionReason::PlanGenerationFailed),
            ),
            sema::SemaWriteInput::PrepareProjection(_) => sema::SemaWriteOutput::RequestRejected(
                self.meta_rejection(meta::RejectionReason::PlanGenerationFailed),
            ),
            sema::SemaWriteInput::ApprovePlan(approval) => self.approve_plan(approval),
            sema::SemaWriteInput::ApplyPlan(application) => self.apply_plan(application),
            sema::SemaWriteInput::RetireAccount(retirement) => self.retire_account(retirement),
        }
    }

    fn register_account(&mut self, registration: meta::Registration) -> sema::SemaWriteOutput {
        let binding = sema::AccountBinding {
            provider: ProviderProjection::new(registration.provider.clone()).into_ordinary(),
            provider_account: registration.provider_account.clone(),
            credential_handle: registration.credential_handle.clone(),
        };
        match self.store.lock() {
            Ok(mut state) => {
                state.put_account(binding);
                sema::SemaWriteOutput::AccountRegistered(meta::AccountRegistered {
                    provider: registration.provider,
                    provider_account: registration.provider_account,
                })
            }
            Err(_) => sema::SemaWriteOutput::RequestRejected(
                self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
            ),
        }
    }

    fn rotate_credential(&mut self, rotation: meta::Rotation) -> sema::SemaWriteOutput {
        let provider = ProviderProjection::new(rotation.provider.clone()).into_ordinary();
        let rotated = match self.store.lock() {
            Ok(mut state) => state.rotate_credential(
                &provider,
                &rotation.provider_account,
                rotation.credential_handle.clone(),
            ),
            Err(_) => {
                return sema::SemaWriteOutput::RequestRejected(
                    self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
                );
            }
        };
        match rotated {
            Some(_) => sema::SemaWriteOutput::CredentialRotated(meta::CredentialRotated {
                provider: rotation.provider,
                provider_account: rotation.provider_account,
            }),
            None => sema::SemaWriteOutput::RequestRejected(
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
        sema::SemaWriteOutput::PolicySet(meta::PolicySet {
            capability_policy_count,
            zone_policy_count,
        })
    }

    fn approve_plan(&mut self, approval: meta::Approval) -> sema::SemaWriteOutput {
        let approved = match self.store.lock() {
            Ok(mut state) => state.approve_plan(approval.0.as_str()),
            Err(_) => {
                return sema::SemaWriteOutput::RequestRejected(
                    self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
                );
            }
        };
        match approved {
            Some(_) => sema::SemaWriteOutput::PlanApproved(meta::PlanApproved(approval.0)),
            None => sema::SemaWriteOutput::RequestRejected(
                self.meta_rejection(meta::RejectionReason::PlanUnknown),
            ),
        }
    }

    fn apply_plan(&mut self, application: meta::Application) -> sema::SemaWriteOutput {
        let approved = match self.store.lock() {
            Ok(state) => state
                .plan(application.0.as_str())
                .map(|stored| stored.approved),
            Err(_) => {
                return sema::SemaWriteOutput::RequestRejected(
                    self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
                );
            }
        };
        match approved {
            Some(true) => sema::SemaWriteOutput::PlanApplied(meta::PlanApplied(application.0)),
            Some(false) => sema::SemaWriteOutput::RequestRejected(
                self.meta_rejection(meta::RejectionReason::PlanNotApproved),
            ),
            None => sema::SemaWriteOutput::RequestRejected(
                self.meta_rejection(meta::RejectionReason::PlanUnknown),
            ),
        }
    }

    fn retire_account(&mut self, retirement: meta::Retirement) -> sema::SemaWriteOutput {
        let provider = ProviderProjection::new(retirement.provider.clone()).into_ordinary();
        let retired = match self.store.lock() {
            Ok(mut state) => state.retire_account(&provider, &retirement.provider_account),
            Err(_) => {
                return sema::SemaWriteOutput::RequestRejected(
                    self.meta_rejection(meta::RejectionReason::ProviderNotConfigured),
                );
            }
        };
        match retired {
            Some(_) => sema::SemaWriteOutput::AccountRetired(meta::AccountRetired {
                provider: retirement.provider,
                provider_account: retirement.provider_account,
            }),
            None => sema::SemaWriteOutput::RequestRejected(
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
                commit_sequence,
                state_digest: commit_sequence,
            },
        }
    }

    fn meta_rejection(&self, reason: meta::RejectionReason) -> meta::RequestRejected {
        let commit_sequence = self.commit_sequence();
        meta::RequestRejected {
            rejection_reason: reason,
            database_marker: meta::DatabaseMarker {
                commit_sequence,
                state_digest: commit_sequence,
            },
        }
    }
}

impl nexus::NexusEngine for SchemaRuntime {
    fn apply_sema_write(
        &mut self,
        _origin_route: nexus::OriginRoute,
        input: nexus::CommandSemaWrite,
    ) -> nexus::SemaWriteCompleted {
        self.apply_sema(input)
    }

    fn observe_sema_read(
        &self,
        _origin_route: nexus::OriginRoute,
        input: nexus::CommandSemaRead,
    ) -> nexus::SemaReadCompleted {
        self.observe_sema(input)
    }

    fn run_effect(&mut self, input: nexus::CommandEffect) -> nexus::EffectCompleted {
        match input {
            nexus::CommandEffect::CloudflareObserveZones(_) => {
                nexus::EffectResult::ZonesObserved(ordinary::ZoneListing::new(Vec::new()))
            }
            nexus::CommandEffect::CloudflareObserveRecords(_) => {
                nexus::EffectResult::RecordsObserved(ordinary::RecordListing::new(Vec::new()))
            }
            nexus::CommandEffect::CloudflareApplyPlan(identifier) => {
                nexus::EffectResult::PlanApplied(meta::PlanApplied::new(identifier))
            }
        }
    }

    fn budget_exhausted_reply(
        &self,
        _exhausted: triad_runtime::ContinuationExhausted,
    ) -> nexus::ReplyToSignal {
        nexus::SignalOutput::OrdinaryOutput(ordinary::Output::RequestRejected(
            ordinary::RejectedRequest(ordinary::RejectionReason::PlanExpired),
        ))
    }

    fn decide(
        &mut self,
        input: nexus::nexus::Nexus<nexus::nexus::Work>,
    ) -> nexus::nexus::Nexus<nexus::nexus::Action> {
        let origin_route = input.origin_route();
        let action = match input.into_root() {
            nexus::NexusWork::SignalArrived(input) => self.decide_signal_arrival(input),
            nexus::NexusWork::SemaReadCompleted(output) => self.decide_read_completion(output),
            nexus::NexusWork::SemaWriteCompleted(output) => self.decide_write_completion(output),
            nexus::NexusWork::EffectCompleted(result) => {
                nexus::NexusAction::Continue(nexus::NexusWork::EffectCompleted(result))
            }
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
