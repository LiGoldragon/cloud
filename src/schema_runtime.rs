use meta_signal_cloud::schema::lib as meta;
use signal_cloud::schema::lib as ordinary;

use crate::schema::{nexus, sema};

#[derive(Debug, Clone)]
pub struct SchemaRuntime {
    capabilities: Vec<ordinary::CapabilityObservation>,
    accounts: Vec<meta::Registration>,
    policy: Option<meta::Policy>,
    commit_sequence: u64,
}

impl Default for SchemaRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaRuntime {
    pub fn new() -> Self {
        Self {
            capabilities: Self::default_capabilities(),
            accounts: Vec::new(),
            policy: None,
            commit_sequence: 0,
        }
    }

    pub fn with_capabilities(capabilities: Vec<ordinary::CapabilityObservation>) -> Self {
        Self {
            capabilities,
            accounts: Vec::new(),
            policy: None,
            commit_sequence: 0,
        }
    }

    pub fn accounts(&self) -> &[meta::Registration] {
        &self.accounts
    }

    pub fn policy(&self) -> Option<&meta::Policy> {
        self.policy.as_ref()
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

    fn observe_plan(&self, _query: ordinary::PlanQuery) -> sema::SemaReadOutput {
        sema::SemaReadOutput::Missed(self.rejection_report(sema::RejectionReason::PlanUnknown))
    }

    fn validate_ordinary(&self, _validation: ordinary::Validation) -> sema::SemaReadOutput {
        sema::SemaReadOutput::Validated(ordinary::ValidationReport::new(Vec::new()))
    }

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
            sema::SemaWriteInput::ApprovePlan(_) => sema::SemaWriteOutput::RequestRejected(
                self.meta_rejection(meta::RejectionReason::PlanUnknown),
            ),
            sema::SemaWriteInput::ApplyPlan(_) => sema::SemaWriteOutput::RequestRejected(
                self.meta_rejection(meta::RejectionReason::PlanUnknown),
            ),
            sema::SemaWriteInput::RetireAccount(retirement) => self.retire_account(retirement),
        }
    }

    fn register_account(&mut self, registration: meta::Registration) -> sema::SemaWriteOutput {
        self.commit_sequence += 1;
        self.accounts.push(registration.clone());
        sema::SemaWriteOutput::AccountRegistered(meta::AccountRegistered {
            provider: registration.provider,
            provider_account: registration.provider_account,
        })
    }

    fn rotate_credential(&mut self, rotation: meta::Rotation) -> sema::SemaWriteOutput {
        self.commit_sequence += 1;
        if let Some(account) = self.accounts.iter_mut().find(|account| {
            account.provider == rotation.provider
                && account.provider_account == rotation.provider_account
        }) {
            account.credential_handle = rotation.credential_handle;
            return sema::SemaWriteOutput::CredentialRotated(meta::CredentialRotated {
                provider: rotation.provider,
                provider_account: rotation.provider_account,
            });
        }
        sema::SemaWriteOutput::RequestRejected(
            self.meta_rejection(meta::RejectionReason::AccountUnknown),
        )
    }

    fn set_policy(&mut self, policy: meta::Policy) -> sema::SemaWriteOutput {
        self.commit_sequence += 1;
        let capability_policy_count = policy.capabilities.len() as u64;
        let zone_policy_count = policy.zones.len() as u64;
        self.policy = Some(policy);
        sema::SemaWriteOutput::PolicySet(meta::PolicySet {
            capability_policy_count,
            zone_policy_count,
        })
    }

    fn retire_account(&mut self, retirement: meta::Retirement) -> sema::SemaWriteOutput {
        self.commit_sequence += 1;
        let before = self.accounts.len();
        self.accounts.retain(|account| {
            account.provider != retirement.provider
                || account.provider_account != retirement.provider_account
        });
        if self.accounts.len() == before {
            return sema::SemaWriteOutput::RequestRejected(
                self.meta_rejection(meta::RejectionReason::AccountUnknown),
            );
        }
        sema::SemaWriteOutput::AccountRetired(meta::AccountRetired {
            provider: retirement.provider,
            provider_account: retirement.provider_account,
        })
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
        sema::RejectionReport {
            reason,
            marker: sema::StateMarker {
                commit_sequence: self.commit_sequence,
                state_digest: self.commit_sequence,
            },
        }
    }

    fn meta_rejection(&self, reason: meta::RejectionReason) -> meta::RequestRejected {
        meta::RequestRejected {
            rejection_reason: reason,
            database_marker: meta::DatabaseMarker {
                commit_sequence: self.commit_sequence,
                state_digest: self.commit_sequence,
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
