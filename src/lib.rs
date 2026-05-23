//! Cloud provider API runtime.
//!
//! The daemon owns provider policy and plan state. The CLI is only a
//! text-to-Signal adapter for this daemon.

use std::path::Path;
use std::sync::Mutex;

use nota_codec::NotaRecord;
use owner_signal_cloud::{
    AccountRegistered, AccountRetired, Application, Approval, CredentialRotated,
    Operation as OwnerOperation, PlanApproved, PolicySet, Registration, Reply as OwnerReply,
    RequestRejected as OwnerRequestRejected, Retirement, Rotation,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_cloud::{
    Capability, CapabilityObservation, CapabilityQuery, CapabilityReport, CapabilityState,
    DesiredState, Observation, ObservationResult, Operation as CloudOperation, Plan,
    PlanIdentifier, PlanQuery, PlanRequest, Provider, RecordListing, RedirectListing,
    Reply as CloudReply, RequestRejected, RequestUnsupported, UnsupportedReason, ValidationReport,
    Zone, ZoneListing,
};
use signal_frame::{NonEmpty, Reply as FrameReply, SubReply};

pub mod client;
pub mod daemon;
pub mod frame_io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("signal frame error: {0}")]
    Frame(#[from] signal_frame::FrameError),

    #[error("command-line route error: {0}")]
    CommandLineRoute(#[from] signal_frame::CommandLineRouteError),

    #[error("NOTA decode error: {0}")]
    Nota(#[from] nota_codec::Error),

    #[error("configuration decode error: {0}")]
    Configuration(#[from] nota_config::Error),

    #[error("expected exactly one argument")]
    ExpectedSingleArgument,

    #[error("flag-style arguments are not part of component binaries: {0}")]
    FlagArgument(String),

    #[error("unexpected signal frame for this socket")]
    UnexpectedFrame,

    #[error("trailing input after single NOTA request")]
    TrailingInput,

    #[error("connection closed before a complete frame arrived")]
    ConnectionClosed,

    #[error("signal handshake was rejected")]
    HandshakeRejected,

    #[error("signal request was rejected before execution")]
    SignalRequestRejected,

    #[error("signal request failed during execution")]
    SignalRequestFailed,

    #[error("cloud store mutex was poisoned")]
    StorePoisoned,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn command_line_route(error: signal_frame::CommandLineRouteError) -> Self {
        Self::CommandLineRoute(error)
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfiguration {
    pub ordinary_socket_path: String,
    pub ordinary_socket_mode: u32,
    pub owner_socket_path: String,
    pub owner_socket_mode: u32,
}

nota_config::impl_rkyv_configuration!(DaemonConfiguration);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBinding {
    provider: Provider,
    account: owner_signal_cloud::ProviderAccount,
    credential: owner_signal_cloud::CredentialHandle,
}

#[derive(Debug)]
pub struct Store {
    accounts: Mutex<Vec<AccountBinding>>,
    policy: Mutex<owner_signal_cloud::Policy>,
    plans: Mutex<Vec<Plan>>,
    approved_plans: Mutex<Vec<PlanIdentifier>>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            accounts: Mutex::new(Vec::new()),
            policy: Mutex::new(owner_signal_cloud::Policy {
                zones: Vec::new(),
                capabilities: Vec::new(),
            }),
            plans: Mutex::new(Vec::new()),
            approved_plans: Mutex::new(Vec::new()),
        }
    }

    pub fn open(_path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::new())
    }

    pub fn handle_ordinary_request(
        &self,
        request: signal_cloud::ChannelRequest,
    ) -> signal_cloud::ChannelReply {
        let replies = request
            .payloads
            .into_iter()
            .map(|operation| SubReply::Ok(self.handle_ordinary_operation(operation)))
            .collect::<Vec<_>>();
        FrameReply::committed(
            NonEmpty::try_from_vec(replies).expect("signal request is guaranteed non-empty"),
        )
    }

    pub fn handle_owner_request(
        &self,
        request: owner_signal_cloud::ChannelRequest,
    ) -> owner_signal_cloud::ChannelReply {
        let replies = request
            .payloads
            .into_iter()
            .map(|operation| SubReply::Ok(self.handle_owner_operation(operation)))
            .collect::<Vec<_>>();
        FrameReply::committed(
            NonEmpty::try_from_vec(replies).expect("signal request is guaranteed non-empty"),
        )
    }

    fn handle_ordinary_operation(&self, operation: CloudOperation) -> CloudReply {
        match operation {
            CloudOperation::Observe(observation) => self.observe(observation),
            CloudOperation::Validate(validation) => self.validate(validation.desired_state),
            CloudOperation::Plan(request) => self.plan(request),
        }
    }

    fn observe(&self, observation: Observation) -> CloudReply {
        match observation {
            Observation::Capabilities(query) => {
                CloudReply::Observed(ObservationResult::Capabilities(self.capabilities(query)))
            }
            Observation::Zones(query) => {
                if let Some(provider) = query.provider {
                    if !self.provider_is_configured(provider) {
                        return CloudReply::RequestUnsupported(RequestUnsupported {
                            operation: signal_cloud::OperationKind::Observe,
                            provider: Some(provider),
                            capability: Some(Capability::DomainNameSystemRecords),
                            reason: UnsupportedReason::ProviderNotConfigured,
                        });
                    }
                }
                CloudReply::Observed(ObservationResult::Zones(self.zones()))
            }
            Observation::Records(query) => {
                if !self.provider_is_configured(query.provider) {
                    return CloudReply::RequestUnsupported(RequestUnsupported {
                        operation: signal_cloud::OperationKind::Observe,
                        provider: Some(query.provider),
                        capability: Some(Capability::DomainNameSystemRecords),
                        reason: UnsupportedReason::ProviderNotConfigured,
                    });
                }
                CloudReply::Observed(ObservationResult::Records(RecordListing {
                    records: vec![],
                }))
            }
            Observation::Redirects(query) => {
                if !self.provider_is_configured(query.provider) {
                    return CloudReply::RequestUnsupported(RequestUnsupported {
                        operation: signal_cloud::OperationKind::Observe,
                        provider: Some(query.provider),
                        capability: Some(Capability::RedirectRules),
                        reason: UnsupportedReason::ProviderNotConfigured,
                    });
                }
                CloudReply::Observed(ObservationResult::Redirects(RedirectListing {
                    rules: vec![],
                }))
            }
            Observation::Plan(query) => self.observe_plan(query),
        }
    }

    fn capabilities(&self, query: CapabilityQuery) -> CapabilityReport {
        let configured = |provider| self.provider_is_configured(provider);
        let capabilities = [
            (Provider::Cloudflare, Capability::DomainNameSystemRecords),
            (Provider::Cloudflare, Capability::RedirectRules),
            (Provider::GoogleCloud, Capability::DomainNameSystemRecords),
            (Provider::Hetzner, Capability::CloudHosts),
            (Provider::Hetzner, Capability::Networks),
            (Provider::Hetzner, Capability::Firewalls),
            (Provider::Hetzner, Capability::LoadBalancers),
        ]
        .into_iter()
        .filter(|(provider, capability)| {
            query
                .provider
                .is_none_or(|requested| requested == *provider)
                && query
                    .capability
                    .is_none_or(|requested| requested == *capability)
        })
        .map(|(provider, capability)| CapabilityObservation {
            provider,
            capability,
            state: if configured(provider) {
                CapabilityState::Configured
            } else {
                CapabilityState::Compiled
            },
        })
        .collect();
        CapabilityReport { capabilities }
    }

    fn zones(&self) -> ZoneListing {
        let accounts = self.accounts.lock().expect("accounts mutex");
        let policy = self.policy.lock().expect("policy mutex");
        let mut zones = Vec::new();
        for zone_policy in &policy.zones {
            if !accounts.iter().any(|account| {
                account.provider == zone_policy.provider && account.account == zone_policy.account
            }) {
                continue;
            }
            for domain in &zone_policy.allowed_zones {
                zones.push(Zone {
                    provider: zone_policy.provider,
                    account: zone_policy.account.clone(),
                    identifier: signal_cloud::ZoneIdentifier::new(format!(
                        "{}:{}",
                        zone_policy.account.as_str(),
                        domain.as_str()
                    )),
                    name: domain.clone(),
                });
            }
        }
        ZoneListing { zones }
    }

    fn validate(&self, desired_state: DesiredState) -> CloudReply {
        if !self.provider_is_configured(desired_state.provider) {
            return CloudReply::RequestUnsupported(RequestUnsupported {
                operation: signal_cloud::OperationKind::Validate,
                provider: Some(desired_state.provider),
                capability: None,
                reason: UnsupportedReason::ProviderNotConfigured,
            });
        }
        CloudReply::Validated(ValidationReport { findings: vec![] })
    }

    fn plan(&self, request: PlanRequest) -> CloudReply {
        let DesiredState {
            provider,
            zone,
            records,
            redirects,
        } = request.desired_state;
        if !self.provider_is_configured(provider) {
            return CloudReply::RequestUnsupported(RequestUnsupported {
                operation: signal_cloud::OperationKind::Plan,
                provider: Some(provider),
                capability: None,
                reason: UnsupportedReason::ProviderNotConfigured,
            });
        }
        let plan = Plan {
            identifier: PlanIdentifier::new(format!("{}-{:?}-plan", zone.as_str(), provider)),
            provider,
            zone,
            records_to_create: records,
            records_to_update: vec![],
            record_names_to_delete: vec![],
            redirects_to_create: redirects,
            redirects_to_update: vec![],
            redirect_sources_to_delete: vec![],
        };
        self.plans.lock().expect("plans mutex").push(plan.clone());
        CloudReply::PlanPrepared(plan)
    }

    fn observe_plan(&self, query: PlanQuery) -> CloudReply {
        let plans = self.plans.lock().expect("plans mutex");
        match plans
            .iter()
            .find(|plan| plan.identifier == query.identifier)
            .cloned()
        {
            Some(plan) => CloudReply::Observed(ObservationResult::Plan(plan)),
            None => CloudReply::RequestRejected(RequestRejected {
                operation: signal_cloud::OperationKind::Observe,
                reason: signal_cloud::RejectionReason::PlanExpired,
            }),
        }
    }

    fn handle_owner_operation(&self, operation: OwnerOperation) -> OwnerReply {
        match operation {
            OwnerOperation::RegisterAccount(registration) => self.register_account(registration),
            OwnerOperation::RotateCredential(rotation) => self.rotate_credential(rotation),
            OwnerOperation::SetPolicy(policy) => self.set_policy(policy),
            OwnerOperation::ApprovePlan(approval) => self.approve_plan(approval),
            OwnerOperation::ApplyPlan(application) => self.apply_plan(application),
            OwnerOperation::RetireAccount(retirement) => self.retire_account(retirement),
        }
    }

    fn register_account(&self, registration: Registration) -> OwnerReply {
        let binding = AccountBinding {
            provider: registration.provider,
            account: registration.account.clone(),
            credential: registration.credential,
        };
        let mut accounts = self.accounts.lock().expect("accounts mutex");
        if let Some(existing) = accounts.iter_mut().find(|account| {
            account.provider == binding.provider && account.account == binding.account
        }) {
            *existing = binding;
        } else {
            accounts.push(binding);
        }
        OwnerReply::AccountRegistered(AccountRegistered {
            provider: registration.provider,
            account: registration.account,
        })
    }

    fn rotate_credential(&self, rotation: Rotation) -> OwnerReply {
        let mut accounts = self.accounts.lock().expect("accounts mutex");
        if let Some(existing) = accounts.iter_mut().find(|account| {
            account.provider == rotation.provider && account.account == rotation.account
        }) {
            existing.credential = rotation.credential;
            OwnerReply::CredentialRotated(CredentialRotated {
                provider: rotation.provider,
                account: rotation.account,
            })
        } else {
            OwnerReply::RequestRejected(OwnerRequestRejected {
                operation: owner_signal_cloud::OperationKind::RotateCredential,
                reason: owner_signal_cloud::RejectionReason::AccountUnknown,
            })
        }
    }

    fn set_policy(&self, policy: owner_signal_cloud::Policy) -> OwnerReply {
        let capability_policy_count = policy.capabilities.len() as u64;
        let zone_policy_count = policy.zones.len() as u64;
        *self.policy.lock().expect("policy mutex") = policy;
        OwnerReply::PolicySet(PolicySet {
            capability_policy_count,
            zone_policy_count,
        })
    }

    fn approve_plan(&self, approval: Approval) -> OwnerReply {
        if !self.plan_exists(&approval.plan) {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                operation: owner_signal_cloud::OperationKind::ApprovePlan,
                reason: owner_signal_cloud::RejectionReason::PlanUnknown,
            });
        }
        self.approved_plans
            .lock()
            .expect("approved plans mutex")
            .push(approval.plan.clone());
        OwnerReply::PlanApproved(PlanApproved {
            plan: approval.plan,
        })
    }

    fn apply_plan(&self, application: Application) -> OwnerReply {
        if !self.plan_exists(&application.plan) {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                operation: owner_signal_cloud::OperationKind::ApplyPlan,
                reason: owner_signal_cloud::RejectionReason::PlanUnknown,
            });
        }
        if !self
            .approved_plans
            .lock()
            .expect("approved plans mutex")
            .iter()
            .any(|plan| plan == &application.plan)
        {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                operation: owner_signal_cloud::OperationKind::ApplyPlan,
                reason: owner_signal_cloud::RejectionReason::PlanNotApproved,
            });
        }
        OwnerReply::RequestRejected(OwnerRequestRejected {
            operation: owner_signal_cloud::OperationKind::ApplyPlan,
            reason: owner_signal_cloud::RejectionReason::CapabilityUnauthorized,
        })
    }

    fn retire_account(&self, retirement: Retirement) -> OwnerReply {
        let mut accounts = self.accounts.lock().expect("accounts mutex");
        accounts.retain(|account| {
            !(account.provider == retirement.provider && account.account == retirement.account)
        });
        OwnerReply::AccountRetired(AccountRetired {
            provider: retirement.provider,
            account: retirement.account,
        })
    }

    fn provider_is_configured(&self, provider: Provider) -> bool {
        self.accounts
            .lock()
            .expect("accounts mutex")
            .iter()
            .any(|account| account.provider == provider)
    }

    fn plan_exists(&self, identifier: &PlanIdentifier) -> bool {
        self.plans
            .lock()
            .expect("plans mutex")
            .iter()
            .any(|plan| &plan.identifier == identifier)
    }
}
