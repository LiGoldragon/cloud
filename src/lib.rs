//! Cloud provider API runtime.
//!
//! The daemon owns provider policy and plan state. The CLI is only a
//! text-to-Signal adapter for this daemon.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use meta_signal_cloud::{
    AccountRegistered, AccountRetired, Application, Approval, CredentialRotated,
    Operation as MetaOperation, PlanApproved, PlanPreparation, PolicySet, ProjectionPreparation,
    Registration, Reply as MetaReply, RequestRejected as MetaRequestRejected, Retirement, Rotation,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_cloud::{
    Capability, CapabilityObservation, CapabilityQuery, CapabilityReport, CapabilityState,
    DesiredState, DomainName, DomainNameSystemRecord, FindingSeverity, Observation,
    ObservationResult, Operation as CloudOperation, Plan, PlanIdentifier, PlanQuery, Provider,
    RecordKind, RecordListing, RecordQuery, Reply as CloudReply, RequestRejected,
    RequestUnsupported, UnsupportedReason, ValidationFinding, ValidationReport, Zone, ZoneListing,
    ZoneQuery,
};
use signal_frame::{NonEmpty, Reply as FrameReply, SubReply};

pub mod client;
#[cfg(feature = "cloudflare")]
pub mod cloudflare;
#[cfg(feature = "cloudflare")]
pub mod cloudflare_cli;
pub mod daemon;
pub mod daemon_command;
pub mod frame_io;
pub mod schema;
pub mod schema_daemon;
pub mod schema_runtime;
pub mod schema_store;

pub use daemon_command::{CloudDaemonCommand, CloudDaemonConfigurationFile};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("signal frame error: {0}")]
    Frame(#[from] signal_frame::FrameError),

    #[error("length-prefixed frame error: {0}")]
    LengthPrefixedFrame(#[from] triad_runtime::FrameError),

    #[error("listener error: {0}")]
    Listener(#[from] triad_runtime::ListenerError),

    #[error("ordinary signal frame error: {0}")]
    OrdinaryFrame(signal_cloud::schema::lib::SignalFrameError),

    #[error("meta signal frame error: {0}")]
    MetaFrame(meta_signal_cloud::schema::lib::SignalFrameError),

    #[error("command-line route error: {0}")]
    CommandLineRoute(#[from] signal_frame::CommandLineRouteError),

    #[error("NOTA decode error: {0}")]
    Nota(#[from] nota_next::NotaDecodeError),

    #[error("command-line error: {0}")]
    CommandLine(#[from] signal_frame::CommandLineError),

    #[error("configuration archive decode failed")]
    ConfigurationArchiveDecode,

    #[error("configuration archive encode failed")]
    ConfigurationArchiveEncode,

    #[error("configuration read failed at {path}: {source}")]
    ConfigurationRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("configuration write failed at {path}: {source}")]
    ConfigurationWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("argument: {0}")]
    Argument(#[from] triad_runtime::ArgumentError),

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

impl From<signal_cloud::schema::lib::SignalFrameError> for Error {
    fn from(error: signal_cloud::schema::lib::SignalFrameError) -> Self {
        Self::OrdinaryFrame(error)
    }
}

impl From<meta_signal_cloud::schema::lib::SignalFrameError> for Error {
    fn from(error: meta_signal_cloud::schema::lib::SignalFrameError) -> Self {
        Self::MetaFrame(error)
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfiguration {
    pub ordinary_socket_path: String,
    pub ordinary_socket_mode: u32,
    pub meta_socket_path: String,
    pub meta_socket_mode: u32,
}

impl DaemonConfiguration {
    pub fn from_rkyv_bytes(bytes: &[u8]) -> Result<Self> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
            .map_err(|_| Error::ConfigurationArchiveDecode)
    }

    pub fn to_rkyv_bytes(&self) -> Result<Vec<u8>> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map_err(|_| Error::ConfigurationArchiveEncode)?;
        Ok(bytes.into_vec())
    }
}

impl triad_runtime::DaemonConfiguration for DaemonConfiguration {
    fn socket_path(&self) -> &Path {
        Path::new(&self.ordinary_socket_path)
    }

    fn meta_socket_path(&self) -> Option<&Path> {
        Some(Path::new(&self.meta_socket_path))
    }

    /// cloud's schema engine holds its account / plan tables in an in-memory
    /// `SchemaStore` for this slice (durable redb backing is the noted
    /// follow-on), so no durable database path is opened — `build_runtime`
    /// ignores it. The trait requires the method, so it returns an empty
    /// placeholder path until the durable store lands.
    fn database_path(&self) -> &Path {
        Path::new("")
    }

    fn meta_socket_mode(&self) -> Option<triad_runtime::SocketMode> {
        Some(triad_runtime::SocketMode::new(self.meta_socket_mode))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBinding {
    pub(crate) provider: Provider,
    pub(crate) account: meta_signal_cloud::ProviderAccount,
    pub(crate) credential: meta_signal_cloud::CredentialHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedRecordListing {
    provider: Provider,
    zone: DomainName,
    listing: RecordListing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainProjection {
    provider: Provider,
    projection: signal_domain_criome::Projection,
}

impl DomainProjection {
    pub fn from_preparation(preparation: ProjectionPreparation) -> Self {
        Self {
            provider: preparation.provider,
            projection: preparation.projection,
        }
    }

    pub fn into_desired_state(self) -> DesiredState {
        DesiredState {
            provider: self.provider,
            zone: DomainName::new(self.projection.query.domain.as_str()),
            records: self
                .projection
                .records
                .into_iter()
                .map(ProjectedRecord::new)
                .map(DomainNameSystemRecord::from)
                .collect(),
            redirects: self
                .projection
                .redirects
                .into_iter()
                .map(ProjectedRedirect::new)
                .map(signal_cloud::RedirectRule::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedRecord {
    record: signal_domain_criome::DomainNameSystemRecord,
}

impl ProjectedRecord {
    pub fn new(record: signal_domain_criome::DomainNameSystemRecord) -> Self {
        Self { record }
    }
}

impl From<ProjectedRecord> for DomainNameSystemRecord {
    fn from(record: ProjectedRecord) -> Self {
        Self {
            name: DomainName::new(record.record.name.as_str()),
            kind: RecordKindProjection::new(record.record.kind).into_record_kind(),
            value: signal_cloud::RecordValue::new(record.record.value.as_str()),
            proxy_mode: signal_cloud::ProxyMode::Direct,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordKindProjection {
    kind: signal_domain_criome::RecordKind,
}

impl RecordKindProjection {
    pub fn new(kind: signal_domain_criome::RecordKind) -> Self {
        Self { kind }
    }

    pub fn into_record_kind(self) -> signal_cloud::RecordKind {
        match self.kind {
            signal_domain_criome::RecordKind::AddressV4 => signal_cloud::RecordKind::AddressV4,
            signal_domain_criome::RecordKind::AddressV6 => signal_cloud::RecordKind::AddressV6,
            signal_domain_criome::RecordKind::CanonicalName => {
                signal_cloud::RecordKind::CanonicalName
            }
            signal_domain_criome::RecordKind::Text => signal_cloud::RecordKind::Text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedRedirect {
    redirect: signal_domain_criome::RedirectRule,
}

impl ProjectedRedirect {
    pub fn new(redirect: signal_domain_criome::RedirectRule) -> Self {
        Self { redirect }
    }
}

impl From<ProjectedRedirect> for signal_cloud::RedirectRule {
    fn from(redirect: ProjectedRedirect) -> Self {
        Self {
            source: DomainName::new(redirect.redirect.source.as_str()),
            target: signal_cloud::UniformResourceLocator::new(redirect.redirect.target.as_str()),
            status: RedirectStatusProjection::new(redirect.redirect.status).into_redirect_status(),
            path_treatment: PathTreatmentProjection::new(redirect.redirect.path_treatment)
                .into_path_treatment(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedirectStatusProjection {
    status: signal_domain_criome::RedirectStatus,
}

impl RedirectStatusProjection {
    pub fn new(status: signal_domain_criome::RedirectStatus) -> Self {
        Self { status }
    }

    pub fn into_redirect_status(self) -> signal_cloud::RedirectStatus {
        match self.status {
            signal_domain_criome::RedirectStatus::Permanent => {
                signal_cloud::RedirectStatus::Permanent
            }
            signal_domain_criome::RedirectStatus::Temporary => {
                signal_cloud::RedirectStatus::Temporary
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathTreatmentProjection {
    treatment: signal_domain_criome::PathTreatment,
}

impl PathTreatmentProjection {
    pub fn new(treatment: signal_domain_criome::PathTreatment) -> Self {
        Self { treatment }
    }

    pub fn into_path_treatment(self) -> signal_cloud::PathTreatment {
        match self.treatment {
            signal_domain_criome::PathTreatment::Preserve => signal_cloud::PathTreatment::Preserve,
            signal_domain_criome::PathTreatment::Replace => signal_cloud::PathTreatment::Replace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredStateValidation {
    desired_state: DesiredState,
}

impl DesiredStateValidation {
    pub fn new(desired_state: DesiredState) -> Self {
        Self { desired_state }
    }

    pub fn findings(&self) -> Vec<ValidationFinding> {
        self.desired_state
            .records
            .iter()
            .filter_map(|record| RecordValueValidation::new(record).finding())
            .chain(
                self.desired_state
                    .redirects
                    .iter()
                    .filter_map(|redirect| RedirectValidation::new(redirect).finding()),
            )
            .collect()
    }
}

pub struct RecordValueValidation<'record> {
    record: &'record DomainNameSystemRecord,
}

impl<'record> RecordValueValidation<'record> {
    pub fn new(record: &'record DomainNameSystemRecord) -> Self {
        Self { record }
    }

    pub fn finding(&self) -> Option<ValidationFinding> {
        match self.record.kind {
            RecordKind::AddressV4
                if self
                    .record
                    .value
                    .as_str()
                    .parse::<std::net::Ipv4Addr>()
                    .is_err() =>
            {
                Some(self.finding_with_message("A record value must be an IPv4 address"))
            }
            RecordKind::AddressV6
                if self
                    .record
                    .value
                    .as_str()
                    .parse::<std::net::Ipv6Addr>()
                    .is_err() =>
            {
                Some(self.finding_with_message("AAAA record value must be an IPv6 address"))
            }
            RecordKind::CanonicalName
            | RecordKind::MailExchange
            | RecordKind::NameServer
            | RecordKind::Pointer
                if self.record.value.as_str().trim().is_empty() =>
            {
                Some(self.finding_with_message("record target must not be empty"))
            }
            _ => None,
        }
    }

    fn finding_with_message(&self, message: &str) -> ValidationFinding {
        ValidationFinding {
            severity: FindingSeverity::Error,
            message: format!(
                "{} {:?}: {}",
                self.record.name.as_str(),
                self.record.kind,
                message
            ),
        }
    }
}

pub struct RedirectValidation<'redirect> {
    redirect: &'redirect signal_cloud::RedirectRule,
}

impl<'redirect> RedirectValidation<'redirect> {
    pub fn new(redirect: &'redirect signal_cloud::RedirectRule) -> Self {
        Self { redirect }
    }

    pub fn finding(&self) -> Option<ValidationFinding> {
        let target = self.redirect.target.as_str();
        if target.starts_with("http://") || target.starts_with("https://") {
            None
        } else {
            Some(ValidationFinding {
                severity: FindingSeverity::Error,
                message: format!(
                    "{} redirect target must start with http:// or https://",
                    self.redirect.source.as_str()
                ),
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordPlan {
    current: Vec<DomainNameSystemRecord>,
    desired: Vec<DomainNameSystemRecord>,
}

impl RecordPlan {
    pub fn new(current: Vec<DomainNameSystemRecord>, desired: Vec<DomainNameSystemRecord>) -> Self {
        Self { current, desired }
    }

    pub fn into_parts(self) -> RecordPlanParts {
        let records_to_create = self
            .desired
            .iter()
            .filter(|desired| {
                !self
                    .current
                    .iter()
                    .any(|current| self.same_identity(current, desired))
            })
            .cloned()
            .collect();
        let records_to_update = self
            .desired
            .iter()
            .filter(|desired| {
                self.current
                    .iter()
                    .any(|current| self.same_identity(current, desired) && current != *desired)
            })
            .cloned()
            .collect();
        let mut record_names_to_delete = Vec::new();
        for current in self.current.iter().filter(|current| {
            !self
                .desired
                .iter()
                .any(|desired| self.same_identity(current, desired))
        }) {
            if !record_names_to_delete.contains(&current.name) {
                record_names_to_delete.push(current.name.clone());
            }
        }
        RecordPlanParts {
            records_to_create,
            records_to_update,
            record_names_to_delete,
        }
    }

    fn same_identity(&self, left: &DomainNameSystemRecord, right: &DomainNameSystemRecord) -> bool {
        left.name == right.name && left.kind == right.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordPlanParts {
    pub records_to_create: Vec<DomainNameSystemRecord>,
    pub records_to_update: Vec<DomainNameSystemRecord>,
    pub record_names_to_delete: Vec<DomainName>,
}

#[derive(Debug)]
struct MetaReplyError {
    reply: Box<MetaReply>,
}

impl MetaReplyError {
    fn new(reply: MetaReply) -> Self {
        Self {
            reply: Box::new(reply),
        }
    }

    fn into_reply(self) -> MetaReply {
        *self.reply
    }
}

#[derive(Debug)]
pub struct Store {
    accounts: Mutex<Vec<AccountBinding>>,
    policy: Mutex<meta_signal_cloud::Policy>,
    plans: Mutex<Vec<Plan>>,
    approved_plans: Mutex<Vec<PlanIdentifier>>,
    last_known_zones: Mutex<Vec<Zone>>,
    last_known_records: Mutex<Vec<CachedRecordListing>>,
    #[cfg(feature = "cloudflare")]
    cloudflare: cloudflare::ProviderClient,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        #[cfg(feature = "cloudflare")]
        let cloudflare = cloudflare::ProviderClient::production();
        Self::with_parts(
            Vec::new(),
            meta_signal_cloud::Policy {
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
            meta_signal_cloud::Policy {
                zones: Vec::new(),
                capabilities: Vec::new(),
            },
            cloudflare,
        )
    }

    #[cfg(feature = "cloudflare")]
    fn with_parts(
        accounts: Vec<AccountBinding>,
        policy: meta_signal_cloud::Policy,
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
    fn with_parts(accounts: Vec<AccountBinding>, policy: meta_signal_cloud::Policy) -> Self {
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

    pub fn handle_meta_request(
        &self,
        request: meta_signal_cloud::ChannelRequest,
    ) -> meta_signal_cloud::ChannelReply {
        let replies = request
            .payloads
            .into_iter()
            .map(|operation| SubReply::Ok(self.handle_meta_operation(operation)))
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
                CloudReply::RequestUnsupported(RequestUnsupported {
                    provider: Some(query.provider),
                    capability: Some(Capability::RedirectRules),
                    reason: UnsupportedReason::CapabilityNotCompiled,
                })
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
            state: self.capability_state(provider, capability),
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
        account: Option<meta_signal_cloud::ProviderAccount>,
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
        account: Option<meta_signal_cloud::ProviderAccount>,
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
        account: Option<meta_signal_cloud::ProviderAccount>,
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
    fn allowed_zone_names(&self, account: &meta_signal_cloud::ProviderAccount) -> Vec<DomainName> {
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
        CloudReply::Validated(ValidationReport {
            findings: DesiredStateValidation::new(desired_state).findings(),
        })
    }

    fn prepare_projection(&self, preparation: ProjectionPreparation) -> MetaReply {
        self.prepare_plan(PlanPreparation {
            desired_state: DomainProjection::from_preparation(preparation).into_desired_state(),
        })
    }

    fn prepare_plan(&self, preparation: PlanPreparation) -> MetaReply {
        let DesiredState {
            provider,
            zone,
            records,
            redirects,
        } = preparation.desired_state;
        if !Self::provider_is_built(provider) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::ProviderNotConfigured,
            });
        }
        if !self.provider_is_configured(provider) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::ProviderNotConfigured,
            });
        }
        if !redirects.is_empty()
            && !Self::provider_supports_capability(provider, Capability::RedirectRules)
        {
            return MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::CapabilityUnauthorized,
            });
        }
        let record_plan = match self.record_plan_for(provider, &zone, records) {
            Ok(plan) => plan,
            Err(reply) => return reply.into_reply(),
        };
        let plan = Plan {
            identifier: PlanIdentifier::new(format!("{}-{:?}-plan", zone.as_str(), provider)),
            provider,
            zone,
            records_to_create: record_plan.records_to_create,
            records_to_update: record_plan.records_to_update,
            record_names_to_delete: record_plan.record_names_to_delete,
            redirects_to_create: redirects,
            redirects_to_update: vec![],
            redirect_sources_to_delete: vec![],
        };
        self.plans.lock().expect("plans mutex").push(plan.clone());
        MetaReply::PlanPrepared(plan)
    }

    fn record_plan_for(
        &self,
        provider: Provider,
        zone: &DomainName,
        records: Vec<DomainNameSystemRecord>,
    ) -> std::result::Result<RecordPlanParts, MetaReplyError> {
        let current = self.current_records_for_plan(provider, zone)?;
        Ok(RecordPlan::new(current.records, records).into_parts())
    }

    fn current_records_for_plan(
        &self,
        provider: Provider,
        zone: &DomainName,
    ) -> std::result::Result<RecordListing, MetaReplyError> {
        #[cfg(feature = "cloudflare")]
        if provider == Provider::Cloudflare {
            return self.cloudflare_record_listing(zone).map_err(|error| {
                MetaReplyError::new(Self::meta_reply_for_cloudflare_error(error))
            });
        }
        Ok(RecordListing { records: vec![] })
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

    fn handle_meta_operation(&self, operation: MetaOperation) -> MetaReply {
        match operation {
            MetaOperation::RegisterAccount(registration) => self.register_account(registration),
            MetaOperation::RotateCredential(rotation) => self.rotate_credential(rotation),
            MetaOperation::SetPolicy(policy) => self.set_policy(policy),
            MetaOperation::PreparePlan(preparation) => self.prepare_plan(preparation),
            MetaOperation::PrepareProjection(preparation) => self.prepare_projection(preparation),
            MetaOperation::ApprovePlan(approval) => self.approve_plan(approval),
            MetaOperation::ApplyPlan(application) => self.apply_plan(application),
            MetaOperation::RetireAccount(retirement) => self.retire_account(retirement),
        }
    }

    fn register_account(&self, registration: Registration) -> MetaReply {
        if !Self::provider_is_built(registration.provider) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::ProviderNotConfigured,
            });
        }
        #[cfg(feature = "cloudflare")]
        if registration.provider == Provider::Cloudflare
            && let Err(error) = self.cloudflare.verify_credential(&registration.credential)
        {
            return Self::meta_reply_for_cloudflare_error(error);
        }
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
        MetaReply::AccountRegistered(AccountRegistered {
            provider: registration.provider,
            account: registration.account,
        })
    }

    fn rotate_credential(&self, rotation: Rotation) -> MetaReply {
        let mut accounts = self.accounts.lock().expect("accounts mutex");
        if let Some(existing) = accounts.iter_mut().find(|account| {
            account.provider == rotation.provider && account.account == rotation.account
        }) {
            existing.credential = rotation.credential;
            MetaReply::CredentialRotated(CredentialRotated {
                provider: rotation.provider,
                account: rotation.account,
            })
        } else {
            MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::AccountUnknown,
            })
        }
    }

    fn set_policy(&self, policy: meta_signal_cloud::Policy) -> MetaReply {
        let capability_policy_count = policy.capabilities.len() as u64;
        let zone_policy_count = policy.zones.len() as u64;
        *self.policy.lock().expect("policy mutex") = policy;
        MetaReply::PolicySet(PolicySet {
            capability_policy_count,
            zone_policy_count,
        })
    }

    fn approve_plan(&self, approval: Approval) -> MetaReply {
        if !self.plan_exists(&approval.plan) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::PlanUnknown,
            });
        }
        self.approved_plans
            .lock()
            .expect("approved plans mutex")
            .push(approval.plan.clone());
        MetaReply::PlanApproved(PlanApproved {
            plan: approval.plan,
        })
    }

    fn apply_plan(&self, application: Application) -> MetaReply {
        let Some(plan) = self.plan_for_identifier(&application.plan) else {
            return MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::PlanUnknown,
            });
        };
        if !self
            .approved_plans
            .lock()
            .expect("approved plans mutex")
            .iter()
            .any(|plan| plan == &application.plan)
        {
            return MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::PlanNotApproved,
            });
        }
        if Self::plan_includes_redirect_changes(&plan) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::CapabilityUnauthorized,
            });
        }
        match plan.provider {
            Provider::Cloudflare => self.apply_cloudflare_plan(plan),
            _ => MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::ProviderNotConfigured,
            }),
        }
    }

    #[cfg(feature = "cloudflare")]
    fn apply_cloudflare_plan(&self, plan: Plan) -> MetaReply {
        let Some(binding) = self.account_binding_for_zone(Provider::Cloudflare, &plan.zone) else {
            return MetaReply::RequestRejected(MetaRequestRejected {
                reason: meta_signal_cloud::RejectionReason::ProviderNotConfigured,
            });
        };
        let zone_identifier = match self.cloudflare_zone_identifier(&binding, &plan.zone) {
            Ok(identifier) => identifier,
            Err(error) => return Self::meta_reply_for_cloudflare_error(error),
        };
        let listing = match self
            .cloudflare
            .apply_plan(&binding.credential, &zone_identifier, &plan)
        {
            Ok(listing) => listing,
            Err(error) => return Self::meta_reply_for_cloudflare_error(error),
        };
        self.replace_last_known_records(Provider::Cloudflare, plan.zone.clone(), listing);
        MetaReply::PlanApplied(meta_signal_cloud::PlanApplied {
            plan: plan.identifier,
        })
    }

    #[cfg(not(feature = "cloudflare"))]
    fn apply_cloudflare_plan(&self, _plan: Plan) -> MetaReply {
        MetaReply::RequestRejected(MetaRequestRejected {
            reason: meta_signal_cloud::RejectionReason::ProviderNotConfigured,
        })
    }

    #[cfg(feature = "cloudflare")]
    fn meta_reply_for_cloudflare_error(error: cloudflare::Error) -> MetaReply {
        let reason = match error {
            cloudflare::Error::CredentialUnavailable(_) => {
                meta_signal_cloud::RejectionReason::CredentialHandleUnknown
            }
            cloudflare::Error::ZoneNotFound(_) => {
                meta_signal_cloud::RejectionReason::ProviderNotConfigured
            }
            cloudflare::Error::RequestFailed(_)
            | cloudflare::Error::RequestRejected(_)
            | cloudflare::Error::UnsupportedRecordKind(_) => {
                meta_signal_cloud::RejectionReason::PlanGenerationFailed
            }
        };
        MetaReply::RequestRejected(MetaRequestRejected { reason })
    }

    fn plan_includes_redirect_changes(plan: &Plan) -> bool {
        !plan.redirects_to_create.is_empty()
            || !plan.redirects_to_update.is_empty()
            || !plan.redirect_sources_to_delete.is_empty()
    }

    fn retire_account(&self, retirement: Retirement) -> MetaReply {
        let mut accounts = self.accounts.lock().expect("accounts mutex");
        accounts.retain(|account| {
            !(account.provider == retirement.provider && account.account == retirement.account)
        });
        MetaReply::AccountRetired(AccountRetired {
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

    fn capability_state(&self, provider: Provider, capability: Capability) -> CapabilityState {
        if !Self::provider_is_built(provider) {
            return CapabilityState::NotBuilt;
        }
        if !Self::provider_supports_capability(provider, capability) {
            return CapabilityState::Unsupported;
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
