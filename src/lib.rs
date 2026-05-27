//! Cloud provider API runtime.
//!
//! The daemon owns provider policy and plan state. The CLI is only a
//! text-to-Signal adapter for this daemon.

use std::path::Path;
use std::sync::Mutex;

use nota_codec::NotaRecord;
use owner_signal_cloud::{
    AccountRegistered, AccountRetired, Application, Approval, CredentialRotated,
    Operation as OwnerOperation, PlanApproved, PlanPreparation, PolicySet, Registration,
    Reply as OwnerReply, RequestRejected as OwnerRequestRejected, Retirement, Rotation,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_cloud::{
    Capability, CapabilityObservation, CapabilityQuery, CapabilityReport, CapabilityState,
    DesiredState, DomainName, Observation, ObservationResult, Operation as CloudOperation, Plan,
    PlanIdentifier, PlanQuery, Provider, RecordListing, RecordQuery, RedirectListing,
    Reply as CloudReply, RequestRejected, RequestUnsupported, UnsupportedReason, ValidationReport,
    Zone, ZoneListing, ZoneQuery,
};
use signal_frame::{NonEmpty, Reply as FrameReply, SubReply};

pub mod client;
#[cfg(feature = "cloudflare")]
pub mod cloudflare;
#[cfg(feature = "cloudflare")]
pub mod cloudflare_cli;
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

    #[cfg(feature = "cloudflare")]
    #[error("Cloudflare provider error: {0}")]
    Cloudflare(#[from] cloudflare::Error),
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
    pub(crate) provider: Provider,
    pub(crate) account: owner_signal_cloud::ProviderAccount,
    pub(crate) credential: owner_signal_cloud::CredentialHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedRecordListing {
    provider: Provider,
    zone: DomainName,
    listing: RecordListing,
}

#[derive(Debug)]
pub struct Store {
    accounts: Mutex<Vec<AccountBinding>>,
    policy: Mutex<owner_signal_cloud::Policy>,
    plans: Mutex<Vec<Plan>>,
    approved_plans: Mutex<Vec<PlanIdentifier>>,
    last_known_zones: Mutex<Vec<Zone>>,
    last_known_records: Mutex<Vec<CachedRecordListing>>,
    #[cfg(feature = "cloudflare")]
    cloudflare: cloudflare::ProviderClient,
}

impl Store {
    pub fn new() -> Self {
        #[cfg(feature = "cloudflare")]
        let cloudflare = cloudflare::ProviderClient::production();
        Self::with_parts(
            Vec::new(),
            owner_signal_cloud::Policy {
                zones: Vec::new(),
                capabilities: Vec::new(),
            },
            #[cfg(feature = "cloudflare")]
            cloudflare,
        )
    }

    #[cfg(feature = "cloudflare")]
    pub fn with_cloudflare_provider(cloudflare: cloudflare::ProviderClient) -> Self {
        Self::with_parts(
            Vec::new(),
            owner_signal_cloud::Policy {
                zones: Vec::new(),
                capabilities: Vec::new(),
            },
            cloudflare,
        )
    }

    #[cfg(feature = "cloudflare")]
    fn with_parts(
        accounts: Vec<AccountBinding>,
        policy: owner_signal_cloud::Policy,
        cloudflare: cloudflare::ProviderClient,
    ) -> Self {
        Self {
            accounts: Mutex::new(accounts),
            policy: Mutex::new(policy),
            plans: Mutex::new(Vec::new()),
            approved_plans: Mutex::new(Vec::new()),
            last_known_zones: Mutex::new(Vec::new()),
            last_known_records: Mutex::new(Vec::new()),
            cloudflare,
        }
    }

    #[cfg(not(feature = "cloudflare"))]
    fn with_parts(accounts: Vec<AccountBinding>, policy: owner_signal_cloud::Policy) -> Self {
        Self {
            accounts: Mutex::new(accounts),
            policy: Mutex::new(policy),
            plans: Mutex::new(Vec::new()),
            approved_plans: Mutex::new(Vec::new()),
            last_known_zones: Mutex::new(Vec::new()),
            last_known_records: Mutex::new(Vec::new()),
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
        }
    }

    fn observe(&self, observation: Observation) -> CloudReply {
        match observation {
            Observation::Capabilities(query) => {
                CloudReply::Observed(ObservationResult::Capabilities(self.capabilities(query)))
            }
            Observation::Zones(query) => self.observe_zones(query),
            Observation::Records(query) => self.observe_records(query),
            Observation::Redirects(query) => {
                if let Some(reply) =
                    self.unsupported_provider_reply(query.provider, Some(Capability::RedirectRules))
                {
                    return reply;
                }
                if !Self::provider_supports_capability(query.provider, Capability::RedirectRules) {
                    return CloudReply::RequestUnsupported(RequestUnsupported {
                        provider: Some(query.provider),
                        capability: Some(Capability::RedirectRules),
                        reason: UnsupportedReason::CapabilityNotCompiled,
                    });
                }
                if !self.provider_is_configured(query.provider) {
                    return CloudReply::RequestUnsupported(RequestUnsupported {
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
            state: self.capability_state(provider),
        })
        .collect();
        CapabilityReport { capabilities }
    }

    fn observe_zones(&self, query: ZoneQuery) -> CloudReply {
        if let Some(provider) = query.provider {
            if let Some(reply) =
                self.unsupported_provider_reply(provider, Some(Capability::DomainNameSystemRecords))
            {
                return reply;
            }
            if !self.provider_is_configured(provider) {
                return CloudReply::RequestUnsupported(RequestUnsupported {
                    provider: Some(provider),
                    capability: Some(Capability::DomainNameSystemRecords),
                    reason: UnsupportedReason::ProviderNotConfigured,
                });
            }
            #[cfg(feature = "cloudflare")]
            if provider == Provider::Cloudflare {
                return self.observe_cloudflare_zones(query.account);
            }
        }
        CloudReply::Observed(ObservationResult::Zones(self.zones()))
    }

    fn observe_records(&self, query: RecordQuery) -> CloudReply {
        if let Some(reply) = self
            .unsupported_provider_reply(query.provider, Some(Capability::DomainNameSystemRecords))
        {
            return reply;
        }
        if !Self::provider_supports_capability(query.provider, Capability::DomainNameSystemRecords)
        {
            return CloudReply::RequestUnsupported(RequestUnsupported {
                provider: Some(query.provider),
                capability: Some(Capability::DomainNameSystemRecords),
                reason: UnsupportedReason::CapabilityNotCompiled,
            });
        }
        if !self.provider_is_configured(query.provider) {
            return CloudReply::RequestUnsupported(RequestUnsupported {
                provider: Some(query.provider),
                capability: Some(Capability::DomainNameSystemRecords),
                reason: UnsupportedReason::ProviderNotConfigured,
            });
        }
        #[cfg(feature = "cloudflare")]
        if query.provider == Provider::Cloudflare {
            return self.observe_cloudflare_records(query);
        }
        CloudReply::Observed(ObservationResult::Records(RecordListing {
            records: vec![],
        }))
    }

    fn zones(&self) -> ZoneListing {
        let accounts = self.accounts.lock().expect("accounts mutex");
        let policy = self.policy.lock().expect("policy mutex");
        let mut zones = Vec::new();
        for zone_policy in &policy.zones {
            if !Self::provider_is_built(zone_policy.provider) {
                continue;
            }
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

    pub fn last_known_records(
        &self,
        provider: Provider,
        zone: &DomainName,
    ) -> Option<RecordListing> {
        self.last_known_records
            .lock()
            .expect("last known records mutex")
            .iter()
            .find(|listing| listing.provider == provider && &listing.zone == zone)
            .map(|listing| listing.listing.clone())
    }

    pub fn last_known_zones(&self) -> ZoneListing {
        ZoneListing {
            zones: self
                .last_known_zones
                .lock()
                .expect("last known zones mutex")
                .clone(),
        }
    }

    #[cfg(feature = "cloudflare")]
    fn observe_cloudflare_zones(
        &self,
        account: Option<owner_signal_cloud::ProviderAccount>,
    ) -> CloudReply {
        match self.cloudflare_zone_listing(account) {
            Ok(listing) => CloudReply::Observed(ObservationResult::Zones(listing)),
            Err(cloudflare::Error::CredentialUnavailable(_)) => {
                CloudReply::RequestUnsupported(RequestUnsupported {
                    provider: Some(Provider::Cloudflare),
                    capability: Some(Capability::DomainNameSystemRecords),
                    reason: UnsupportedReason::ProviderNotConfigured,
                })
            }
            Err(_) => CloudReply::RequestRejected(RequestRejected {
                reason: signal_cloud::RejectionReason::ProviderUnavailable,
            }),
        }
    }

    #[cfg(feature = "cloudflare")]
    fn observe_cloudflare_records(&self, query: RecordQuery) -> CloudReply {
        match self.cloudflare_record_listing(&query.zone) {
            Ok(listing) => CloudReply::Observed(ObservationResult::Records(listing)),
            Err(cloudflare::Error::CredentialUnavailable(_)) => {
                CloudReply::RequestUnsupported(RequestUnsupported {
                    provider: Some(Provider::Cloudflare),
                    capability: Some(Capability::DomainNameSystemRecords),
                    reason: UnsupportedReason::ProviderNotConfigured,
                })
            }
            Err(_) => CloudReply::RequestRejected(RequestRejected {
                reason: signal_cloud::RejectionReason::ProviderUnavailable,
            }),
        }
    }

    #[cfg(feature = "cloudflare")]
    fn cloudflare_zone_listing(
        &self,
        account: Option<owner_signal_cloud::ProviderAccount>,
    ) -> cloudflare::Result<ZoneListing> {
        let bindings = self.account_bindings(Provider::Cloudflare, account);
        let mut zones = Vec::new();
        for binding in bindings {
            let zone_names = self.allowed_zone_names(&binding.account);
            zones.extend(self.cloudflare.zones(
                &binding.account,
                &binding.credential,
                &zone_names,
            )?);
        }
        self.replace_last_known_zones(zones.clone());
        Ok(ZoneListing { zones })
    }

    #[cfg(feature = "cloudflare")]
    fn cloudflare_record_listing(&self, zone: &DomainName) -> cloudflare::Result<RecordListing> {
        let binding = self
            .account_binding_for_zone(Provider::Cloudflare, zone)
            .ok_or_else(|| cloudflare::Error::ZoneNotFound(zone.as_str().to_owned()))?;
        let zone_identifier = self.cloudflare_zone_identifier(&binding, zone)?;
        let listing = self
            .cloudflare
            .records(&binding.credential, &zone_identifier)?;
        self.replace_last_known_records(Provider::Cloudflare, zone.clone(), listing.clone());
        Ok(listing)
    }

    #[cfg(feature = "cloudflare")]
    fn cloudflare_zone_identifier(
        &self,
        binding: &AccountBinding,
        zone: &DomainName,
    ) -> cloudflare::Result<signal_cloud::ZoneIdentifier> {
        self.cloudflare
            .zones(
                &binding.account,
                &binding.credential,
                std::slice::from_ref(zone),
            )?
            .into_iter()
            .find(|candidate| candidate.name == *zone)
            .map(|candidate| candidate.identifier)
            .ok_or_else(|| cloudflare::Error::ZoneNotFound(zone.as_str().to_owned()))
    }

    #[cfg(feature = "cloudflare")]
    fn account_bindings(
        &self,
        provider: Provider,
        account: Option<owner_signal_cloud::ProviderAccount>,
    ) -> Vec<AccountBinding> {
        self.accounts
            .lock()
            .expect("accounts mutex")
            .iter()
            .filter(|binding| {
                binding.provider == provider
                    && account
                        .as_ref()
                        .is_none_or(|requested| &binding.account == requested)
            })
            .cloned()
            .collect()
    }

    #[cfg(feature = "cloudflare")]
    fn account_binding_for_zone(
        &self,
        provider: Provider,
        zone: &DomainName,
    ) -> Option<AccountBinding> {
        let accounts = self.accounts.lock().expect("accounts mutex");
        let policy = self.policy.lock().expect("policy mutex");
        policy
            .zones
            .iter()
            .find(|policy| {
                policy.provider == provider
                    && policy.allowed_zones.iter().any(|allowed| allowed == zone)
            })
            .and_then(|policy| {
                accounts
                    .iter()
                    .find(|binding| {
                        binding.provider == provider && binding.account == policy.account
                    })
                    .cloned()
            })
    }

    #[cfg(feature = "cloudflare")]
    fn allowed_zone_names(&self, account: &owner_signal_cloud::ProviderAccount) -> Vec<DomainName> {
        self.policy
            .lock()
            .expect("policy mutex")
            .zones
            .iter()
            .filter(|policy| policy.provider == Provider::Cloudflare && &policy.account == account)
            .flat_map(|policy| policy.allowed_zones.clone())
            .collect()
    }

    #[cfg(feature = "cloudflare")]
    fn replace_last_known_zones(&self, zones: Vec<Zone>) {
        *self
            .last_known_zones
            .lock()
            .expect("last known zones mutex") = zones;
    }

    #[cfg(feature = "cloudflare")]
    fn replace_last_known_records(
        &self,
        provider: Provider,
        zone: DomainName,
        listing: RecordListing,
    ) {
        let mut records = self
            .last_known_records
            .lock()
            .expect("last known records mutex");
        if let Some(existing) = records
            .iter_mut()
            .find(|listing| listing.provider == provider && listing.zone == zone)
        {
            existing.listing = listing;
        } else {
            records.push(CachedRecordListing {
                provider,
                zone,
                listing,
            });
        }
    }

    fn validate(&self, desired_state: DesiredState) -> CloudReply {
        if let Some(reply) = self.unsupported_provider_reply(desired_state.provider, None) {
            return reply;
        }
        if !desired_state.records.is_empty()
            && !Self::provider_supports_capability(
                desired_state.provider,
                Capability::DomainNameSystemRecords,
            )
        {
            return CloudReply::RequestUnsupported(RequestUnsupported {
                provider: Some(desired_state.provider),
                capability: Some(Capability::DomainNameSystemRecords),
                reason: UnsupportedReason::CapabilityNotCompiled,
            });
        }
        if !desired_state.redirects.is_empty()
            && !Self::provider_supports_capability(
                desired_state.provider,
                Capability::RedirectRules,
            )
        {
            return CloudReply::RequestUnsupported(RequestUnsupported {
                provider: Some(desired_state.provider),
                capability: Some(Capability::RedirectRules),
                reason: UnsupportedReason::CapabilityNotCompiled,
            });
        }
        if !self.provider_is_configured(desired_state.provider) {
            return CloudReply::RequestUnsupported(RequestUnsupported {
                provider: Some(desired_state.provider),
                capability: None,
                reason: UnsupportedReason::ProviderNotConfigured,
            });
        }
        CloudReply::Validated(ValidationReport { findings: vec![] })
    }

    fn prepare_plan(&self, preparation: PlanPreparation) -> OwnerReply {
        let DesiredState {
            provider,
            zone,
            records,
            redirects,
        } = preparation.desired_state;
        if !Self::provider_is_built(provider) {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                reason: owner_signal_cloud::RejectionReason::ProviderNotConfigured,
            });
        }
        if !self.provider_is_configured(provider) {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                reason: owner_signal_cloud::RejectionReason::ProviderNotConfigured,
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
        OwnerReply::PlanPrepared(plan)
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
                reason: signal_cloud::RejectionReason::PlanExpired,
            }),
        }
    }

    fn handle_owner_operation(&self, operation: OwnerOperation) -> OwnerReply {
        match operation {
            OwnerOperation::RegisterAccount(registration) => self.register_account(registration),
            OwnerOperation::RotateCredential(rotation) => self.rotate_credential(rotation),
            OwnerOperation::SetPolicy(policy) => self.set_policy(policy),
            OwnerOperation::PreparePlan(preparation) => self.prepare_plan(preparation),
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
        let Some(plan) = self.plan_for_identifier(&application.plan) else {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                reason: owner_signal_cloud::RejectionReason::PlanUnknown,
            });
        };
        if !self
            .approved_plans
            .lock()
            .expect("approved plans mutex")
            .iter()
            .any(|plan| plan == &application.plan)
        {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                reason: owner_signal_cloud::RejectionReason::PlanNotApproved,
            });
        }
        if Self::plan_includes_redirect_changes(&plan) {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                reason: owner_signal_cloud::RejectionReason::CapabilityUnauthorized,
            });
        }
        match plan.provider {
            Provider::Cloudflare => self.apply_cloudflare_plan(plan),
            _ => OwnerReply::RequestRejected(OwnerRequestRejected {
                reason: owner_signal_cloud::RejectionReason::ProviderNotConfigured,
            }),
        }
    }

    #[cfg(feature = "cloudflare")]
    fn apply_cloudflare_plan(&self, plan: Plan) -> OwnerReply {
        let Some(binding) = self.account_binding_for_zone(Provider::Cloudflare, &plan.zone) else {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                reason: owner_signal_cloud::RejectionReason::ProviderNotConfigured,
            });
        };
        let zone_identifier = match self.cloudflare_zone_identifier(&binding, &plan.zone) {
            Ok(identifier) => identifier,
            Err(error) => return Self::owner_reply_for_cloudflare_error(error),
        };
        let listing = match self
            .cloudflare
            .apply_plan(&binding.credential, &zone_identifier, &plan)
        {
            Ok(listing) => listing,
            Err(error) => return Self::owner_reply_for_cloudflare_error(error),
        };
        self.replace_last_known_records(Provider::Cloudflare, plan.zone.clone(), listing);
        OwnerReply::PlanApplied(owner_signal_cloud::PlanApplied {
            plan: plan.identifier,
        })
    }

    #[cfg(not(feature = "cloudflare"))]
    fn apply_cloudflare_plan(&self, _plan: Plan) -> OwnerReply {
        OwnerReply::RequestRejected(OwnerRequestRejected {
            reason: owner_signal_cloud::RejectionReason::ProviderNotConfigured,
        })
    }

    #[cfg(feature = "cloudflare")]
    fn owner_reply_for_cloudflare_error(error: cloudflare::Error) -> OwnerReply {
        let reason = match error {
            cloudflare::Error::CredentialUnavailable(_) => {
                owner_signal_cloud::RejectionReason::CredentialHandleUnknown
            }
            cloudflare::Error::ZoneNotFound(_) => {
                owner_signal_cloud::RejectionReason::ProviderNotConfigured
            }
            cloudflare::Error::RequestFailed(_)
            | cloudflare::Error::RequestRejected(_)
            | cloudflare::Error::UnsupportedRecordKind(_) => {
                owner_signal_cloud::RejectionReason::PlanGenerationFailed
            }
        };
        OwnerReply::RequestRejected(OwnerRequestRejected { reason })
    }

    fn plan_includes_redirect_changes(plan: &Plan) -> bool {
        !plan.redirects_to_create.is_empty()
            || !plan.redirects_to_update.is_empty()
            || !plan.redirect_sources_to_delete.is_empty()
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
        if !Self::provider_is_built(provider) {
            return false;
        }
        self.accounts
            .lock()
            .expect("accounts mutex")
            .iter()
            .any(|account| account.provider == provider)
    }

    fn capability_state(&self, provider: Provider) -> CapabilityState {
        if !Self::provider_is_built(provider) {
            return CapabilityState::NotBuilt;
        }
        if self.provider_is_configured(provider) {
            CapabilityState::Configured
        } else {
            CapabilityState::Compiled
        }
    }

    fn unsupported_provider_reply(
        &self,
        provider: Provider,
        capability: Option<Capability>,
    ) -> Option<CloudReply> {
        if !Self::provider_is_built(provider) {
            return Some(CloudReply::RequestUnsupported(RequestUnsupported {
                provider: Some(provider),
                capability,
                reason: UnsupportedReason::ProviderNotBuilt,
            }));
        }
        None
    }

    fn provider_is_built(provider: Provider) -> bool {
        match provider {
            Provider::Cloudflare => cfg!(feature = "cloudflare"),
            Provider::GoogleCloud => cfg!(feature = "google-cloud"),
            Provider::Hetzner => cfg!(feature = "hetzner"),
        }
    }

    fn provider_supports_capability(provider: Provider, capability: Capability) -> bool {
        matches!(
            (provider, capability),
            (Provider::Cloudflare, Capability::DomainNameSystemRecords)
                | (Provider::Cloudflare, Capability::RedirectRules)
                | (Provider::GoogleCloud, Capability::DomainNameSystemRecords)
                | (Provider::Hetzner, Capability::CloudHosts)
                | (Provider::Hetzner, Capability::Networks)
                | (Provider::Hetzner, Capability::Firewalls)
                | (Provider::Hetzner, Capability::LoadBalancers)
        )
    }

    fn plan_exists(&self, identifier: &PlanIdentifier) -> bool {
        self.plan_for_identifier(identifier).is_some()
    }

    fn plan_for_identifier(&self, identifier: &PlanIdentifier) -> Option<Plan> {
        self.plans
            .lock()
            .expect("plans mutex")
            .iter()
            .find(|plan| &plan.identifier == identifier)
            .cloned()
    }
}
