use meta_signal_cloud::schema::lib as meta;
use signal_cloud::schema::lib as ordinary;

pub(crate) struct SchemaCloudInput {
    input: ordinary::Input,
}

impl SchemaCloudInput {
    pub fn new(input: ordinary::Input) -> Self {
        Self { input }
    }

    pub fn from_operation(operation: signal_cloud::Operation) -> Self {
        let input = match operation {
            signal_cloud::Operation::Observe(observation) => {
                ordinary::Input::observe(LegacyObservation::new(observation).into_schema())
            }
            signal_cloud::Operation::Validate(validation) => ordinary::Input::validate(
                LegacyValidation::new(validation)
                    .into_schema()
                    .into_payload(),
            ),
        };
        Self { input }
    }

    pub fn into_input(self) -> ordinary::Input {
        self.input
    }

    pub fn into_operation(self) -> signal_cloud::Operation {
        match self.input {
            ordinary::Input::Observe(observation) => {
                signal_cloud::Operation::Observe(SchemaObservation::new(observation).into_legacy())
            }
            ordinary::Input::Validate(validation) => {
                signal_cloud::Operation::Validate(SchemaValidation::new(validation).into_legacy())
            }
        }
    }
}

pub(crate) struct SchemaCloudOutput {
    output: ordinary::Output,
}

impl SchemaCloudOutput {
    pub fn new(output: ordinary::Output) -> Self {
        Self { output }
    }

    pub fn from_reply(reply: signal_cloud::Reply) -> Self {
        let output = match reply {
            signal_cloud::Reply::Observed(result) => {
                ordinary::Output::observed(LegacyObservationResult::new(result).into_schema())
            }
            signal_cloud::Reply::Validated(report) => ordinary::Output::validated(
                LegacyValidationReport::new(report)
                    .into_schema()
                    .into_payload(),
            ),
            signal_cloud::Reply::RequestUnsupported(unsupported) => {
                ordinary::Output::request_unsupported(
                    LegacyUnsupportedRequest::new(unsupported).into_schema(),
                )
            }
            signal_cloud::Reply::RequestRejected(rejected) => ordinary::Output::request_rejected(
                LegacyRejectedRequest::new(rejected)
                    .into_schema()
                    .into_payload(),
            ),
        };
        Self { output }
    }

    pub fn into_output(self) -> ordinary::Output {
        self.output
    }

    pub fn into_reply(self) -> signal_cloud::Reply {
        match self.output {
            ordinary::Output::Observed(result) => {
                signal_cloud::Reply::Observed(SchemaObservationResult::new(result).into_legacy())
            }
            ordinary::Output::Validated(report) => {
                signal_cloud::Reply::Validated(SchemaValidationReport::new(report).into_legacy())
            }
            ordinary::Output::RequestUnsupported(unsupported) => {
                signal_cloud::Reply::RequestUnsupported(
                    SchemaUnsupportedRequest::new(unsupported).into_legacy(),
                )
            }
            ordinary::Output::RequestRejected(rejected) => signal_cloud::Reply::RequestRejected(
                SchemaRejectedRequest::new(rejected).into_legacy(),
            ),
        }
    }
}

pub(crate) struct SchemaMetaInput {
    input: meta::Input,
}

impl SchemaMetaInput {
    pub fn new(input: meta::Input) -> Self {
        Self { input }
    }

    pub fn from_operation(operation: meta_signal_cloud::Operation) -> Self {
        let input = match operation {
            meta_signal_cloud::Operation::RegisterAccount(registration) => {
                meta::Input::register_account(LegacyRegistration::new(registration).into_schema())
            }
            meta_signal_cloud::Operation::RotateCredential(rotation) => {
                meta::Input::rotate_credential(LegacyRotation::new(rotation).into_schema())
            }
            meta_signal_cloud::Operation::SetPolicy(policy) => {
                meta::Input::set_policy(LegacyPolicy::new(policy).into_schema())
            }
            meta_signal_cloud::Operation::PreparePlan(preparation) => meta::Input::prepare_plan(
                LegacyPlanPreparation::new(preparation)
                    .into_schema()
                    .into_payload(),
            ),
            meta_signal_cloud::Operation::PrepareHostPlan(preparation) => {
                meta::Input::prepare_host_plan(
                    LegacyHostPlanPreparation::new(preparation)
                        .into_schema()
                        .into_payload(),
                )
            }
            meta_signal_cloud::Operation::PrepareHostDestruction(destruction) => {
                meta::Input::prepare_host_destruction(
                    LegacyHostDestruction::new(destruction).into_schema(),
                )
            }
            meta_signal_cloud::Operation::PrepareProjection(preparation) => {
                meta::Input::prepare_projection(
                    LegacyProjectionPreparation::new(preparation).into_schema(),
                )
            }
            meta_signal_cloud::Operation::ApprovePlan(approval) => meta::Input::approve_plan(
                meta::PlanIdentifier::new(approval.plan.as_str().to_owned()),
            ),
            meta_signal_cloud::Operation::ApplyPlan(application) => meta::Input::apply_plan(
                meta::PlanIdentifier::new(application.plan.as_str().to_owned()),
            ),
            meta_signal_cloud::Operation::RetireAccount(retirement) => {
                meta::Input::retire_account(LegacyRetirement::new(retirement).into_schema())
            }
        };
        Self { input }
    }

    pub fn into_input(self) -> meta::Input {
        self.input
    }

    pub fn into_operation(self) -> meta_signal_cloud::Operation {
        match self.input {
            meta::Input::RegisterAccount(registration) => {
                meta_signal_cloud::Operation::RegisterAccount(
                    SchemaRegistration::new(registration).into_legacy(),
                )
            }
            meta::Input::RotateCredential(rotation) => {
                meta_signal_cloud::Operation::RotateCredential(
                    SchemaRotation::new(rotation).into_legacy(),
                )
            }
            meta::Input::SetPolicy(policy) => {
                meta_signal_cloud::Operation::SetPolicy(SchemaPolicy::new(policy).into_legacy())
            }
            meta::Input::PreparePlan(preparation) => meta_signal_cloud::Operation::PreparePlan(
                SchemaPlanPreparation::new(preparation).into_legacy(),
            ),
            meta::Input::PrepareHostPlan(preparation) => {
                meta_signal_cloud::Operation::PrepareHostPlan(
                    SchemaHostPlanPreparation::new(preparation).into_legacy(),
                )
            }
            meta::Input::PrepareHostDestruction(destruction) => {
                meta_signal_cloud::Operation::PrepareHostDestruction(
                    SchemaHostDestruction::new(destruction).into_legacy(),
                )
            }
            meta::Input::PrepareProjection(preparation) => {
                meta_signal_cloud::Operation::PrepareProjection(
                    SchemaProjectionPreparation::new(preparation).into_legacy(),
                )
            }
            meta::Input::ApprovePlan(approval) => {
                meta_signal_cloud::Operation::ApprovePlan(meta_signal_cloud::Approval {
                    plan: signal_cloud::PlanIdentifier::new(approval.into_payload().into_payload()),
                })
            }
            meta::Input::ApplyPlan(application) => {
                meta_signal_cloud::Operation::ApplyPlan(meta_signal_cloud::Application {
                    plan: signal_cloud::PlanIdentifier::new(
                        application.into_payload().into_payload(),
                    ),
                })
            }
            meta::Input::RetireAccount(retirement) => meta_signal_cloud::Operation::RetireAccount(
                SchemaRetirement::new(retirement).into_legacy(),
            ),
        }
    }
}

pub(crate) struct SchemaMetaOutput {
    output: meta::Output,
}

impl SchemaMetaOutput {
    pub fn new(output: meta::Output) -> Self {
        Self { output }
    }

    pub fn from_reply(reply: meta_signal_cloud::Reply) -> Self {
        let output = match reply {
            meta_signal_cloud::Reply::AccountRegistered(registered) => {
                meta::Output::account_registered(
                    LegacyAccountRegistered::new(registered).into_schema(),
                )
            }
            meta_signal_cloud::Reply::CredentialRotated(rotated) => {
                meta::Output::credential_rotated(
                    LegacyCredentialRotated::new(rotated).into_schema(),
                )
            }
            meta_signal_cloud::Reply::PolicySet(policy) => {
                meta::Output::policy_set(meta::PolicySet {
                    capability_policy_count: policy.capability_policy_count,
                    zone_policy_count: policy.zone_policy_count,
                })
            }
            meta_signal_cloud::Reply::PlanPrepared(plan) => {
                meta::Output::plan_prepared(LegacyPlan::new(plan).into_meta_schema())
            }
            meta_signal_cloud::Reply::HostPlanPrepared(plan) => {
                meta::Output::host_plan_prepared(LegacyHostPlan::new(plan).into_meta_schema())
            }
            meta_signal_cloud::Reply::PlanApproved(approved) => meta::Output::plan_approved(
                meta::PlanIdentifier::new(approved.plan.as_str().to_owned()),
            ),
            meta_signal_cloud::Reply::PlanApplied(applied) => meta::Output::plan_applied(
                meta::PlanIdentifier::new(applied.plan.as_str().to_owned()),
            ),
            meta_signal_cloud::Reply::AccountRetired(retired) => {
                meta::Output::account_retired(LegacyAccountRetired::new(retired).into_schema())
            }
            meta_signal_cloud::Reply::RequestRejected(rejected) => meta::Output::request_rejected(
                LegacyMetaRejectedRequest::new(rejected).into_schema(),
            ),
        };
        Self { output }
    }

    pub fn into_output(self) -> meta::Output {
        self.output
    }

    pub fn into_reply(self) -> meta_signal_cloud::Reply {
        match self.output {
            meta::Output::AccountRegistered(registered) => {
                meta_signal_cloud::Reply::AccountRegistered(
                    SchemaAccountRegistered::new(registered).into_legacy(),
                )
            }
            meta::Output::CredentialRotated(rotated) => {
                meta_signal_cloud::Reply::CredentialRotated(
                    SchemaCredentialRotated::new(rotated).into_legacy(),
                )
            }
            meta::Output::PolicySet(policy) => {
                meta_signal_cloud::Reply::PolicySet(meta_signal_cloud::PolicySet {
                    capability_policy_count: policy.capability_policy_count,
                    zone_policy_count: policy.zone_policy_count,
                })
            }
            meta::Output::PlanPrepared(plan) => meta_signal_cloud::Reply::PlanPrepared(
                SchemaMetaPlan::new(plan.into_payload()).into_legacy(),
            ),
            meta::Output::HostPlanPrepared(plan) => meta_signal_cloud::Reply::HostPlanPrepared(
                SchemaMetaHostPlan::new(plan.into_payload()).into_legacy(),
            ),
            meta::Output::PlanApproved(approved) => {
                meta_signal_cloud::Reply::PlanApproved(meta_signal_cloud::PlanApproved {
                    plan: signal_cloud::PlanIdentifier::new(approved.into_payload().into_payload()),
                })
            }
            meta::Output::PlanApplied(applied) => {
                meta_signal_cloud::Reply::PlanApplied(meta_signal_cloud::PlanApplied {
                    plan: signal_cloud::PlanIdentifier::new(applied.into_payload().into_payload()),
                })
            }
            meta::Output::AccountRetired(retired) => meta_signal_cloud::Reply::AccountRetired(
                SchemaAccountRetired::new(retired).into_legacy(),
            ),
            meta::Output::RequestRejected(rejected) => meta_signal_cloud::Reply::RequestRejected(
                SchemaMetaRejectedRequest::new(rejected).into_legacy(),
            ),
        }
    }
}

struct SchemaProvider {
    provider: ordinary::Provider,
}

impl SchemaProvider {
    pub fn new(provider: ordinary::Provider) -> Self {
        Self { provider }
    }

    pub fn into_legacy(self) -> signal_cloud::Provider {
        match self.provider {
            ordinary::Provider::Cloudflare => signal_cloud::Provider::Cloudflare,
            ordinary::Provider::GoogleCloud => signal_cloud::Provider::GoogleCloud,
            ordinary::Provider::Hetzner => signal_cloud::Provider::Hetzner,
            ordinary::Provider::DigitalOcean => signal_cloud::Provider::DigitalOcean,
        }
    }
}

struct LegacyProvider {
    provider: signal_cloud::Provider,
}

impl LegacyProvider {
    pub fn new(provider: signal_cloud::Provider) -> Self {
        Self { provider }
    }

    pub fn into_schema(self) -> ordinary::Provider {
        match self.provider {
            signal_cloud::Provider::Cloudflare => ordinary::Provider::Cloudflare,
            signal_cloud::Provider::GoogleCloud => ordinary::Provider::GoogleCloud,
            signal_cloud::Provider::Hetzner => ordinary::Provider::Hetzner,
            signal_cloud::Provider::DigitalOcean => ordinary::Provider::DigitalOcean,
        }
    }

    pub fn into_meta_schema(self) -> meta::Provider {
        match self.provider {
            signal_cloud::Provider::Cloudflare => meta::Provider::Cloudflare,
            signal_cloud::Provider::GoogleCloud => meta::Provider::GoogleCloud,
            signal_cloud::Provider::Hetzner => meta::Provider::Hetzner,
            signal_cloud::Provider::DigitalOcean => meta::Provider::DigitalOcean,
        }
    }
}

struct MetaSchemaProvider {
    provider: meta::Provider,
}

impl MetaSchemaProvider {
    pub fn new(provider: meta::Provider) -> Self {
        Self { provider }
    }

    pub fn into_legacy(self) -> signal_cloud::Provider {
        match self.provider {
            meta::Provider::Cloudflare => signal_cloud::Provider::Cloudflare,
            meta::Provider::GoogleCloud => signal_cloud::Provider::GoogleCloud,
            meta::Provider::Hetzner => signal_cloud::Provider::Hetzner,
            meta::Provider::DigitalOcean => signal_cloud::Provider::DigitalOcean,
        }
    }
}

struct SchemaCapability {
    capability: ordinary::Capability,
}

impl SchemaCapability {
    pub fn new(capability: ordinary::Capability) -> Self {
        Self { capability }
    }

    pub fn into_legacy(self) -> signal_cloud::Capability {
        match self.capability {
            ordinary::Capability::DomainNameSystemRecords => {
                signal_cloud::Capability::DomainNameSystemRecords
            }
            ordinary::Capability::RedirectRules => signal_cloud::Capability::RedirectRules,
            ordinary::Capability::CloudHosts => signal_cloud::Capability::CloudHosts,
            ordinary::Capability::Networks => signal_cloud::Capability::Networks,
            ordinary::Capability::Firewalls => signal_cloud::Capability::Firewalls,
            ordinary::Capability::LoadBalancers => signal_cloud::Capability::LoadBalancers,
        }
    }
}

struct LegacyCapability {
    capability: signal_cloud::Capability,
}

impl LegacyCapability {
    pub fn new(capability: signal_cloud::Capability) -> Self {
        Self { capability }
    }

    pub fn into_schema(self) -> ordinary::Capability {
        match self.capability {
            signal_cloud::Capability::DomainNameSystemRecords => {
                ordinary::Capability::DomainNameSystemRecords
            }
            signal_cloud::Capability::RedirectRules => ordinary::Capability::RedirectRules,
            signal_cloud::Capability::CloudHosts => ordinary::Capability::CloudHosts,
            signal_cloud::Capability::Networks => ordinary::Capability::Networks,
            signal_cloud::Capability::Firewalls => ordinary::Capability::Firewalls,
            signal_cloud::Capability::LoadBalancers => ordinary::Capability::LoadBalancers,
        }
    }

    pub fn into_meta_schema(self) -> meta::Capability {
        match self.capability {
            signal_cloud::Capability::DomainNameSystemRecords => {
                meta::Capability::DomainNameSystemRecords
            }
            signal_cloud::Capability::RedirectRules => meta::Capability::RedirectRules,
            signal_cloud::Capability::CloudHosts => meta::Capability::CloudHosts,
            signal_cloud::Capability::Networks => meta::Capability::Networks,
            signal_cloud::Capability::Firewalls => meta::Capability::Firewalls,
            signal_cloud::Capability::LoadBalancers => meta::Capability::LoadBalancers,
        }
    }
}

struct MetaSchemaCapability {
    capability: meta::Capability,
}

impl MetaSchemaCapability {
    pub fn new(capability: meta::Capability) -> Self {
        Self { capability }
    }

    pub fn into_legacy(self) -> signal_cloud::Capability {
        match self.capability {
            meta::Capability::DomainNameSystemRecords => {
                signal_cloud::Capability::DomainNameSystemRecords
            }
            meta::Capability::RedirectRules => signal_cloud::Capability::RedirectRules,
            meta::Capability::CloudHosts => signal_cloud::Capability::CloudHosts,
            meta::Capability::Networks => signal_cloud::Capability::Networks,
            meta::Capability::Firewalls => signal_cloud::Capability::Firewalls,
            meta::Capability::LoadBalancers => signal_cloud::Capability::LoadBalancers,
        }
    }
}

struct LegacyCapabilityState {
    state: signal_cloud::CapabilityState,
}

impl LegacyCapabilityState {
    pub fn new(state: signal_cloud::CapabilityState) -> Self {
        Self { state }
    }

    pub fn into_schema(self) -> ordinary::CapabilityState {
        match self.state {
            signal_cloud::CapabilityState::NotBuilt => ordinary::CapabilityState::NotBuilt,
            signal_cloud::CapabilityState::Compiled => ordinary::CapabilityState::Compiled,
            signal_cloud::CapabilityState::Configured => ordinary::CapabilityState::Configured,
            signal_cloud::CapabilityState::Authorized => ordinary::CapabilityState::Authorized,
            signal_cloud::CapabilityState::Unsupported => ordinary::CapabilityState::Unsupported,
            signal_cloud::CapabilityState::Unauthorized => ordinary::CapabilityState::Unauthorized,
        }
    }
}

struct SchemaCapabilityState {
    state: ordinary::CapabilityState,
}

impl SchemaCapabilityState {
    pub fn new(state: ordinary::CapabilityState) -> Self {
        Self { state }
    }

    pub fn into_legacy(self) -> signal_cloud::CapabilityState {
        match self.state {
            ordinary::CapabilityState::NotBuilt => signal_cloud::CapabilityState::NotBuilt,
            ordinary::CapabilityState::Compiled => signal_cloud::CapabilityState::Compiled,
            ordinary::CapabilityState::Configured => signal_cloud::CapabilityState::Configured,
            ordinary::CapabilityState::Authorized => signal_cloud::CapabilityState::Authorized,
            ordinary::CapabilityState::Unsupported => signal_cloud::CapabilityState::Unsupported,
            ordinary::CapabilityState::Unauthorized => signal_cloud::CapabilityState::Unauthorized,
        }
    }
}

struct SchemaObservation {
    observation: ordinary::Observation,
}

impl SchemaObservation {
    pub fn new(observation: ordinary::Observation) -> Self {
        Self { observation }
    }

    pub fn into_legacy(self) -> signal_cloud::Observation {
        match self.observation {
            ordinary::Observation::Capabilities(query) => signal_cloud::Observation::Capabilities(
                SchemaCapabilityQuery::new(query.into_payload()).into_legacy(),
            ),
            ordinary::Observation::Zones(query) => signal_cloud::Observation::Zones(
                SchemaZoneQuery::new(query.into_payload()).into_legacy(),
            ),
            ordinary::Observation::Records(query) => signal_cloud::Observation::Records(
                SchemaRecordQuery::new(query.into_payload()).into_legacy(),
            ),
            ordinary::Observation::Redirects(query) => signal_cloud::Observation::Redirects(
                SchemaRedirectQuery::new(query.into_payload()).into_legacy(),
            ),
            ordinary::Observation::ObserveServers(query) => signal_cloud::Observation::Servers(
                SchemaHostQuery::new(query.into_payload()).into_legacy(),
            ),
            ordinary::Observation::ObservePlan(query) => {
                signal_cloud::Observation::Plan(signal_cloud::PlanQuery {
                    identifier: signal_cloud::PlanIdentifier::new(
                        query.into_payload().into_payload().into_payload(),
                    ),
                })
            }
        }
    }
}

struct LegacyObservation {
    observation: signal_cloud::Observation,
}

impl LegacyObservation {
    pub fn new(observation: signal_cloud::Observation) -> Self {
        Self { observation }
    }

    pub fn into_schema(self) -> ordinary::Observation {
        match self.observation {
            signal_cloud::Observation::Capabilities(query) => {
                ordinary::Observation::capabilities(LegacyCapabilityQuery::new(query).into_schema())
            }
            signal_cloud::Observation::Zones(query) => {
                ordinary::Observation::zones(LegacyZoneQuery::new(query).into_schema())
            }
            signal_cloud::Observation::Records(query) => {
                ordinary::Observation::records(LegacyRecordQuery::new(query).into_schema())
            }
            signal_cloud::Observation::Redirects(query) => {
                ordinary::Observation::redirects(LegacyRedirectQuery::new(query).into_schema())
            }
            signal_cloud::Observation::Servers(query) => {
                ordinary::Observation::observe_servers(LegacyHostQuery::new(query).into_schema())
            }
            signal_cloud::Observation::Plan(query) => {
                ordinary::Observation::observe_plan(ordinary::PlanQuery::new(
                    ordinary::PlanIdentifier::new(query.identifier.as_str().to_owned()),
                ))
            }
        }
    }
}

struct SchemaCapabilityQuery {
    query: ordinary::CapabilityQuery,
}

impl SchemaCapabilityQuery {
    pub fn new(query: ordinary::CapabilityQuery) -> Self {
        Self { query }
    }

    pub fn into_legacy(self) -> signal_cloud::CapabilityQuery {
        signal_cloud::CapabilityQuery {
            provider: self
                .query
                .provider
                .map(|provider| SchemaProvider::new(provider).into_legacy()),
            capability: self
                .query
                .capability
                .map(|capability| SchemaCapability::new(capability).into_legacy()),
        }
    }
}

struct LegacyCapabilityQuery {
    query: signal_cloud::CapabilityQuery,
}

impl LegacyCapabilityQuery {
    pub fn new(query: signal_cloud::CapabilityQuery) -> Self {
        Self { query }
    }

    pub fn into_schema(self) -> ordinary::CapabilityQuery {
        ordinary::CapabilityQuery {
            provider: self
                .query
                .provider
                .map(|provider| LegacyProvider::new(provider).into_schema()),
            capability: self
                .query
                .capability
                .map(|capability| LegacyCapability::new(capability).into_schema()),
        }
    }
}

struct SchemaZoneQuery {
    query: ordinary::ZoneQuery,
}

impl SchemaZoneQuery {
    pub fn new(query: ordinary::ZoneQuery) -> Self {
        Self { query }
    }

    pub fn into_legacy(self) -> signal_cloud::ZoneQuery {
        signal_cloud::ZoneQuery {
            provider: self
                .query
                .provider
                .map(|provider| SchemaProvider::new(provider).into_legacy()),
            account: self
                .query
                .account
                .map(|account| signal_cloud::ProviderAccount::new(account.into_payload())),
        }
    }
}

struct LegacyZoneQuery {
    query: signal_cloud::ZoneQuery,
}

impl LegacyZoneQuery {
    pub fn new(query: signal_cloud::ZoneQuery) -> Self {
        Self { query }
    }

    pub fn into_schema(self) -> ordinary::ZoneQuery {
        ordinary::ZoneQuery {
            provider: self
                .query
                .provider
                .map(|provider| LegacyProvider::new(provider).into_schema()),
            account: self
                .query
                .account
                .map(|account| ordinary::ProviderAccount::new(account.as_str().to_owned())),
        }
    }
}

struct SchemaRecordQuery {
    query: ordinary::RecordQuery,
}

impl SchemaRecordQuery {
    pub fn new(query: ordinary::RecordQuery) -> Self {
        Self { query }
    }

    pub fn into_legacy(self) -> signal_cloud::RecordQuery {
        signal_cloud::RecordQuery {
            provider: SchemaProvider::new(self.query.provider).into_legacy(),
            zone: signal_cloud::DomainName::new(self.query.domain_name.into_payload()),
        }
    }
}

struct LegacyRecordQuery {
    query: signal_cloud::RecordQuery,
}

impl LegacyRecordQuery {
    pub fn new(query: signal_cloud::RecordQuery) -> Self {
        Self { query }
    }

    pub fn into_schema(self) -> ordinary::RecordQuery {
        ordinary::RecordQuery {
            provider: LegacyProvider::new(self.query.provider).into_schema(),
            domain_name: ordinary::DomainName::new(self.query.zone.as_str().to_owned()),
        }
    }
}

struct SchemaRedirectQuery {
    query: ordinary::RedirectQuery,
}

impl SchemaRedirectQuery {
    pub fn new(query: ordinary::RedirectQuery) -> Self {
        Self { query }
    }

    pub fn into_legacy(self) -> signal_cloud::RedirectQuery {
        signal_cloud::RedirectQuery {
            provider: SchemaProvider::new(self.query.provider).into_legacy(),
            zone: signal_cloud::DomainName::new(self.query.domain_name.into_payload()),
        }
    }
}

struct LegacyRedirectQuery {
    query: signal_cloud::RedirectQuery,
}

impl LegacyRedirectQuery {
    pub fn new(query: signal_cloud::RedirectQuery) -> Self {
        Self { query }
    }

    pub fn into_schema(self) -> ordinary::RedirectQuery {
        ordinary::RedirectQuery {
            provider: LegacyProvider::new(self.query.provider).into_schema(),
            domain_name: ordinary::DomainName::new(self.query.zone.as_str().to_owned()),
        }
    }
}

struct SchemaValidation {
    validation: ordinary::Validation,
}

impl SchemaValidation {
    pub fn new(validation: ordinary::Validation) -> Self {
        Self { validation }
    }

    pub fn into_legacy(self) -> signal_cloud::Validation {
        signal_cloud::Validation {
            desired_state: SchemaDesiredState::new(self.validation.into_payload()).into_legacy(),
        }
    }
}

struct LegacyValidation {
    validation: signal_cloud::Validation,
}

impl LegacyValidation {
    pub fn new(validation: signal_cloud::Validation) -> Self {
        Self { validation }
    }

    pub fn into_schema(self) -> ordinary::Validation {
        ordinary::Validation::new(
            LegacyDesiredState::new(self.validation.desired_state).into_schema(),
        )
    }
}

struct SchemaDesiredState {
    desired: ordinary::DesiredState,
}

impl SchemaDesiredState {
    pub fn new(desired: ordinary::DesiredState) -> Self {
        Self { desired }
    }

    pub fn into_legacy(self) -> signal_cloud::DesiredState {
        signal_cloud::DesiredState {
            provider: SchemaProvider::new(self.desired.provider).into_legacy(),
            zone: signal_cloud::DomainName::new(self.desired.domain_name.into_payload()),
            records: self
                .desired
                .records
                .into_iter()
                .map(|record| SchemaRecord::new(record).into_legacy())
                .collect(),
            redirects: self
                .desired
                .redirects
                .into_iter()
                .map(|redirect| SchemaRedirect::new(redirect).into_legacy())
                .collect(),
        }
    }
}

struct LegacyDesiredState {
    desired: signal_cloud::DesiredState,
}

impl LegacyDesiredState {
    pub fn new(desired: signal_cloud::DesiredState) -> Self {
        Self { desired }
    }

    pub fn into_schema(self) -> ordinary::DesiredState {
        ordinary::DesiredState {
            provider: LegacyProvider::new(self.desired.provider).into_schema(),
            domain_name: ordinary::DomainName::new(self.desired.zone.as_str().to_owned()),
            records: self
                .desired
                .records
                .into_iter()
                .map(|record| LegacyRecord::new(record).into_schema())
                .collect(),
            redirects: self
                .desired
                .redirects
                .into_iter()
                .map(|redirect| LegacyRedirect::new(redirect).into_schema())
                .collect(),
        }
    }
}

struct SchemaObservationResult {
    result: ordinary::ObservationResult,
}

impl SchemaObservationResult {
    pub fn new(result: ordinary::ObservationResult) -> Self {
        Self { result }
    }

    pub fn into_legacy(self) -> signal_cloud::ObservationResult {
        match self.result {
            ordinary::ObservationResult::Capabilities(report) => {
                signal_cloud::ObservationResult::Capabilities(
                    SchemaCapabilityReport::new(report).into_legacy(),
                )
            }
            ordinary::ObservationResult::Zones(listing) => signal_cloud::ObservationResult::Zones(
                SchemaZoneListing::new(listing).into_legacy(),
            ),
            ordinary::ObservationResult::Records(listing) => {
                signal_cloud::ObservationResult::Records(
                    SchemaRecordListing::new(listing).into_legacy(),
                )
            }
            ordinary::ObservationResult::Redirects(listing) => {
                signal_cloud::ObservationResult::Redirects(
                    SchemaRedirectListing::new(listing).into_legacy(),
                )
            }
            ordinary::ObservationResult::Servers(listing) => {
                signal_cloud::ObservationResult::Servers(
                    SchemaCloudHostListing::new(listing).into_legacy(),
                )
            }
            ordinary::ObservationResult::PlanResult(plan) => signal_cloud::ObservationResult::Plan(
                SchemaPlan::new(plan.into_payload()).into_legacy(),
            ),
        }
    }
}

struct LegacyObservationResult {
    result: signal_cloud::ObservationResult,
}

impl LegacyObservationResult {
    pub fn new(result: signal_cloud::ObservationResult) -> Self {
        Self { result }
    }

    pub fn into_schema(self) -> ordinary::ObservationResult {
        match self.result {
            signal_cloud::ObservationResult::Capabilities(report) => {
                ordinary::ObservationResult::Capabilities(
                    LegacyCapabilityReport::new(report).into_schema(),
                )
            }
            signal_cloud::ObservationResult::Zones(listing) => {
                ordinary::ObservationResult::Zones(LegacyZoneListing::new(listing).into_schema())
            }
            signal_cloud::ObservationResult::Records(listing) => {
                ordinary::ObservationResult::Records(
                    LegacyRecordListing::new(listing).into_schema(),
                )
            }
            signal_cloud::ObservationResult::Redirects(listing) => {
                ordinary::ObservationResult::Redirects(
                    LegacyRedirectListing::new(listing).into_schema(),
                )
            }
            signal_cloud::ObservationResult::Servers(listing) => {
                ordinary::ObservationResult::Servers(
                    LegacyCloudHostListing::new(listing).into_schema(),
                )
            }
            signal_cloud::ObservationResult::Plan(plan) => {
                ordinary::ObservationResult::plan_result(LegacyPlan::new(plan).into_schema())
            }
        }
    }
}

struct SchemaCapabilityReport {
    report: ordinary::CapabilityReport,
}

impl SchemaCapabilityReport {
    pub fn new(report: ordinary::CapabilityReport) -> Self {
        Self { report }
    }

    pub fn into_legacy(self) -> signal_cloud::CapabilityReport {
        signal_cloud::CapabilityReport {
            capabilities: self
                .report
                .into_payload()
                .into_iter()
                .map(|observation| signal_cloud::CapabilityObservation {
                    provider: SchemaProvider::new(observation.provider).into_legacy(),
                    capability: SchemaCapability::new(observation.capability).into_legacy(),
                    state: SchemaCapabilityState::new(observation.capability_state).into_legacy(),
                })
                .collect(),
        }
    }
}

struct LegacyCapabilityReport {
    report: signal_cloud::CapabilityReport,
}

impl LegacyCapabilityReport {
    pub fn new(report: signal_cloud::CapabilityReport) -> Self {
        Self { report }
    }

    pub fn into_schema(self) -> ordinary::CapabilityReport {
        ordinary::CapabilityReport::new(
            self.report
                .capabilities
                .into_iter()
                .map(|observation| ordinary::CapabilityObservation {
                    provider: LegacyProvider::new(observation.provider).into_schema(),
                    capability: LegacyCapability::new(observation.capability).into_schema(),
                    capability_state: LegacyCapabilityState::new(observation.state).into_schema(),
                })
                .collect(),
        )
    }
}

struct SchemaZoneListing {
    listing: ordinary::ZoneListing,
}

impl SchemaZoneListing {
    pub fn new(listing: ordinary::ZoneListing) -> Self {
        Self { listing }
    }

    pub fn into_legacy(self) -> signal_cloud::ZoneListing {
        signal_cloud::ZoneListing {
            zones: self
                .listing
                .into_payload()
                .into_iter()
                .map(|zone| signal_cloud::Zone {
                    provider: SchemaProvider::new(zone.provider).into_legacy(),
                    account: signal_cloud::ProviderAccount::new(
                        zone.provider_account.into_payload(),
                    ),
                    identifier: signal_cloud::ZoneIdentifier::new(
                        zone.zone_identifier.into_payload(),
                    ),
                    name: signal_cloud::DomainName::new(zone.domain_name.into_payload()),
                })
                .collect(),
        }
    }
}

struct LegacyZoneListing {
    listing: signal_cloud::ZoneListing,
}

impl LegacyZoneListing {
    pub fn new(listing: signal_cloud::ZoneListing) -> Self {
        Self { listing }
    }

    pub fn into_schema(self) -> ordinary::ZoneListing {
        ordinary::ZoneListing::new(
            self.listing
                .zones
                .into_iter()
                .map(|zone| ordinary::Zone {
                    provider: LegacyProvider::new(zone.provider).into_schema(),
                    provider_account: ordinary::ProviderAccount::new(
                        zone.account.as_str().to_owned(),
                    ),
                    zone_identifier: ordinary::ZoneIdentifier::new(
                        zone.identifier.as_str().to_owned(),
                    ),
                    domain_name: ordinary::DomainName::new(zone.name.as_str().to_owned()),
                })
                .collect(),
        )
    }
}

struct SchemaRecordListing {
    listing: ordinary::RecordListing,
}

impl SchemaRecordListing {
    pub fn new(listing: ordinary::RecordListing) -> Self {
        Self { listing }
    }

    pub fn into_legacy(self) -> signal_cloud::RecordListing {
        signal_cloud::RecordListing {
            records: self
                .listing
                .into_payload()
                .into_iter()
                .map(|record| SchemaRecord::new(record).into_legacy())
                .collect(),
        }
    }
}

struct LegacyRecordListing {
    listing: signal_cloud::RecordListing,
}

impl LegacyRecordListing {
    pub fn new(listing: signal_cloud::RecordListing) -> Self {
        Self { listing }
    }

    pub fn into_schema(self) -> ordinary::RecordListing {
        ordinary::RecordListing::new(
            self.listing
                .records
                .into_iter()
                .map(|record| LegacyRecord::new(record).into_schema())
                .collect(),
        )
    }
}

struct SchemaRedirectListing {
    listing: ordinary::RedirectListing,
}

impl SchemaRedirectListing {
    pub fn new(listing: ordinary::RedirectListing) -> Self {
        Self { listing }
    }

    pub fn into_legacy(self) -> signal_cloud::RedirectListing {
        signal_cloud::RedirectListing {
            rules: self
                .listing
                .into_payload()
                .into_iter()
                .map(|redirect| SchemaRedirect::new(redirect).into_legacy())
                .collect(),
        }
    }
}

struct LegacyRedirectListing {
    listing: signal_cloud::RedirectListing,
}

impl LegacyRedirectListing {
    pub fn new(listing: signal_cloud::RedirectListing) -> Self {
        Self { listing }
    }

    pub fn into_schema(self) -> ordinary::RedirectListing {
        ordinary::RedirectListing::new(
            self.listing
                .rules
                .into_iter()
                .map(|redirect| LegacyRedirect::new(redirect).into_schema())
                .collect(),
        )
    }
}

struct SchemaRecord {
    record: ordinary::DomainNameSystemRecord,
}

impl SchemaRecord {
    pub fn new(record: ordinary::DomainNameSystemRecord) -> Self {
        Self { record }
    }

    pub fn into_legacy(self) -> signal_cloud::DomainNameSystemRecord {
        signal_cloud::DomainNameSystemRecord {
            name: signal_cloud::DomainName::new(self.record.domain_name.into_payload()),
            kind: SchemaRecordKind::new(self.record.record_kind).into_legacy(),
            value: signal_cloud::RecordValue::new(self.record.record_value.into_payload()),
            proxy_mode: SchemaProxyMode::new(self.record.proxy_mode).into_legacy(),
        }
    }
}

struct LegacyRecord {
    record: signal_cloud::DomainNameSystemRecord,
}

impl LegacyRecord {
    pub fn new(record: signal_cloud::DomainNameSystemRecord) -> Self {
        Self { record }
    }

    pub fn into_schema(self) -> ordinary::DomainNameSystemRecord {
        ordinary::DomainNameSystemRecord {
            domain_name: ordinary::DomainName::new(self.record.name.as_str().to_owned()),
            record_kind: LegacyRecordKind::new(self.record.kind).into_schema(),
            record_value: ordinary::RecordValue::new(self.record.value.as_str().to_owned()),
            proxy_mode: LegacyProxyMode::new(self.record.proxy_mode).into_schema(),
        }
    }

    pub fn into_meta_schema(self) -> meta::DomainNameSystemRecord {
        meta::DomainNameSystemRecord {
            name: meta::DomainName::new(self.record.name.as_str().to_owned()),
            record_kind: LegacyRecordKind::new(self.record.kind).into_meta_schema(),
            content: meta::RecordContent::new(self.record.value.as_str().to_owned()),
        }
    }
}

struct SchemaRecordKind {
    kind: ordinary::RecordKind,
}

impl SchemaRecordKind {
    pub fn new(kind: ordinary::RecordKind) -> Self {
        Self { kind }
    }

    pub fn into_legacy(self) -> signal_cloud::RecordKind {
        match self.kind {
            ordinary::RecordKind::AddressV4 => signal_cloud::RecordKind::AddressV4,
            ordinary::RecordKind::AddressV6 => signal_cloud::RecordKind::AddressV6,
            ordinary::RecordKind::CanonicalName => signal_cloud::RecordKind::CanonicalName,
            ordinary::RecordKind::Text => signal_cloud::RecordKind::Text,
            ordinary::RecordKind::MailExchange => signal_cloud::RecordKind::MailExchange,
            ordinary::RecordKind::NameServer => signal_cloud::RecordKind::NameServer,
            ordinary::RecordKind::Pointer => signal_cloud::RecordKind::Pointer,
            ordinary::RecordKind::Service => signal_cloud::RecordKind::Service,
            ordinary::RecordKind::CertificateAuthorityAuthorization => {
                signal_cloud::RecordKind::CertificateAuthorityAuthorization
            }
            ordinary::RecordKind::SecureShellFingerprint => {
                signal_cloud::RecordKind::SecureShellFingerprint
            }
            ordinary::RecordKind::TransportLayerSecurityAuthentication => {
                signal_cloud::RecordKind::TransportLayerSecurityAuthentication
            }
            ordinary::RecordKind::UniformResourceIdentifier => {
                signal_cloud::RecordKind::UniformResourceIdentifier
            }
            ordinary::RecordKind::ServiceBinding => signal_cloud::RecordKind::ServiceBinding,
            ordinary::RecordKind::HttpsBinding => signal_cloud::RecordKind::HttpsBinding,
            ordinary::RecordKind::Location => signal_cloud::RecordKind::Location,
        }
    }
}

struct LegacyRecordKind {
    kind: signal_cloud::RecordKind,
}

impl LegacyRecordKind {
    pub fn new(kind: signal_cloud::RecordKind) -> Self {
        Self { kind }
    }

    pub fn into_schema(self) -> ordinary::RecordKind {
        match self.kind {
            signal_cloud::RecordKind::AddressV4 => ordinary::RecordKind::AddressV4,
            signal_cloud::RecordKind::AddressV6 => ordinary::RecordKind::AddressV6,
            signal_cloud::RecordKind::CanonicalName => ordinary::RecordKind::CanonicalName,
            signal_cloud::RecordKind::Text => ordinary::RecordKind::Text,
            signal_cloud::RecordKind::MailExchange => ordinary::RecordKind::MailExchange,
            signal_cloud::RecordKind::NameServer => ordinary::RecordKind::NameServer,
            signal_cloud::RecordKind::Pointer => ordinary::RecordKind::Pointer,
            signal_cloud::RecordKind::Service => ordinary::RecordKind::Service,
            signal_cloud::RecordKind::CertificateAuthorityAuthorization => {
                ordinary::RecordKind::CertificateAuthorityAuthorization
            }
            signal_cloud::RecordKind::SecureShellFingerprint => {
                ordinary::RecordKind::SecureShellFingerprint
            }
            signal_cloud::RecordKind::TransportLayerSecurityAuthentication => {
                ordinary::RecordKind::TransportLayerSecurityAuthentication
            }
            signal_cloud::RecordKind::UniformResourceIdentifier => {
                ordinary::RecordKind::UniformResourceIdentifier
            }
            signal_cloud::RecordKind::ServiceBinding => ordinary::RecordKind::ServiceBinding,
            signal_cloud::RecordKind::HttpsBinding => ordinary::RecordKind::HttpsBinding,
            signal_cloud::RecordKind::Location => ordinary::RecordKind::Location,
        }
    }

    pub fn into_meta_schema(self) -> meta::RecordKind {
        match self.kind {
            signal_cloud::RecordKind::AddressV4 => meta::RecordKind::Address,
            signal_cloud::RecordKind::AddressV6 => meta::RecordKind::AddressSix,
            signal_cloud::RecordKind::CanonicalName => meta::RecordKind::CanonicalName,
            signal_cloud::RecordKind::MailExchange => meta::RecordKind::MailExchange,
            signal_cloud::RecordKind::Text => meta::RecordKind::Text,
            signal_cloud::RecordKind::Service => meta::RecordKind::Service,
            _ => meta::RecordKind::Text,
        }
    }
}

struct SchemaMetaRecordKind {
    kind: meta::RecordKind,
}

impl SchemaMetaRecordKind {
    pub fn new(kind: meta::RecordKind) -> Self {
        Self { kind }
    }

    pub fn into_legacy(self) -> signal_cloud::RecordKind {
        match self.kind {
            meta::RecordKind::Address => signal_cloud::RecordKind::AddressV4,
            meta::RecordKind::AddressSix => signal_cloud::RecordKind::AddressV6,
            meta::RecordKind::CanonicalName => signal_cloud::RecordKind::CanonicalName,
            meta::RecordKind::MailExchange => signal_cloud::RecordKind::MailExchange,
            meta::RecordKind::Text => signal_cloud::RecordKind::Text,
            meta::RecordKind::Service => signal_cloud::RecordKind::Service,
        }
    }
}

struct SchemaProxyMode {
    mode: ordinary::ProxyMode,
}

impl SchemaProxyMode {
    pub fn new(mode: ordinary::ProxyMode) -> Self {
        Self { mode }
    }

    pub fn into_legacy(self) -> signal_cloud::ProxyMode {
        match self.mode {
            ordinary::ProxyMode::Direct => signal_cloud::ProxyMode::Direct,
            ordinary::ProxyMode::ProviderProxy => signal_cloud::ProxyMode::ProviderProxy,
        }
    }
}

struct LegacyProxyMode {
    mode: signal_cloud::ProxyMode,
}

impl LegacyProxyMode {
    pub fn new(mode: signal_cloud::ProxyMode) -> Self {
        Self { mode }
    }

    pub fn into_schema(self) -> ordinary::ProxyMode {
        match self.mode {
            signal_cloud::ProxyMode::Direct => ordinary::ProxyMode::Direct,
            signal_cloud::ProxyMode::ProviderProxy => ordinary::ProxyMode::ProviderProxy,
        }
    }
}

struct SchemaRedirect {
    redirect: ordinary::RedirectRule,
}

impl SchemaRedirect {
    pub fn new(redirect: ordinary::RedirectRule) -> Self {
        Self { redirect }
    }

    pub fn into_legacy(self) -> signal_cloud::RedirectRule {
        signal_cloud::RedirectRule {
            source: signal_cloud::DomainName::new(self.redirect.domain_name.into_payload()),
            target: signal_cloud::UniformResourceLocator::new(
                self.redirect.uniform_resource_locator.into_payload(),
            ),
            status: SchemaRedirectStatus::new(self.redirect.redirect_status).into_legacy(),
            path_treatment: SchemaPathTreatment::new(self.redirect.path_treatment).into_legacy(),
        }
    }
}

struct LegacyRedirect {
    redirect: signal_cloud::RedirectRule,
}

impl LegacyRedirect {
    pub fn new(redirect: signal_cloud::RedirectRule) -> Self {
        Self { redirect }
    }

    pub fn into_schema(self) -> ordinary::RedirectRule {
        ordinary::RedirectRule {
            domain_name: ordinary::DomainName::new(self.redirect.source.as_str().to_owned()),
            uniform_resource_locator: ordinary::UniformResourceLocator::new(
                self.redirect.target.as_str().to_owned(),
            ),
            redirect_status: LegacyRedirectStatus::new(self.redirect.status).into_schema(),
            path_treatment: LegacyPathTreatment::new(self.redirect.path_treatment).into_schema(),
        }
    }

    pub fn into_meta_schema(self) -> meta::RedirectRule {
        meta::RedirectRule {
            source: meta::DomainName::new(self.redirect.source.as_str().to_owned()),
            target: meta::DomainName::new(self.redirect.target.as_str().to_owned()),
            redirect_status: LegacyRedirectStatus::new(self.redirect.status).into_meta_schema(),
        }
    }
}

struct SchemaRedirectStatus {
    status: ordinary::RedirectStatus,
}

impl SchemaRedirectStatus {
    pub fn new(status: ordinary::RedirectStatus) -> Self {
        Self { status }
    }

    pub fn into_legacy(self) -> signal_cloud::RedirectStatus {
        match self.status {
            ordinary::RedirectStatus::Permanent => signal_cloud::RedirectStatus::Permanent,
            ordinary::RedirectStatus::Temporary => signal_cloud::RedirectStatus::Temporary,
        }
    }
}

struct LegacyRedirectStatus {
    status: signal_cloud::RedirectStatus,
}

impl LegacyRedirectStatus {
    pub fn new(status: signal_cloud::RedirectStatus) -> Self {
        Self { status }
    }

    pub fn into_schema(self) -> ordinary::RedirectStatus {
        match self.status {
            signal_cloud::RedirectStatus::Permanent => ordinary::RedirectStatus::Permanent,
            signal_cloud::RedirectStatus::Temporary => ordinary::RedirectStatus::Temporary,
        }
    }

    pub fn into_meta_schema(self) -> meta::RedirectStatus {
        match self.status {
            signal_cloud::RedirectStatus::Permanent => meta::RedirectStatus::Permanent,
            signal_cloud::RedirectStatus::Temporary => meta::RedirectStatus::Temporary,
        }
    }
}

struct SchemaPathTreatment {
    treatment: ordinary::PathTreatment,
}

impl SchemaPathTreatment {
    pub fn new(treatment: ordinary::PathTreatment) -> Self {
        Self { treatment }
    }

    pub fn into_legacy(self) -> signal_cloud::PathTreatment {
        match self.treatment {
            ordinary::PathTreatment::Preserve => signal_cloud::PathTreatment::Preserve,
            ordinary::PathTreatment::Replace => signal_cloud::PathTreatment::Replace,
        }
    }
}

struct LegacyPathTreatment {
    treatment: signal_cloud::PathTreatment,
}

impl LegacyPathTreatment {
    pub fn new(treatment: signal_cloud::PathTreatment) -> Self {
        Self { treatment }
    }

    pub fn into_schema(self) -> ordinary::PathTreatment {
        match self.treatment {
            signal_cloud::PathTreatment::Preserve => ordinary::PathTreatment::Preserve,
            signal_cloud::PathTreatment::Replace => ordinary::PathTreatment::Replace,
        }
    }
}

struct SchemaValidationReport {
    report: ordinary::ValidationReport,
}

impl SchemaValidationReport {
    pub fn new(report: ordinary::ValidationReport) -> Self {
        Self { report }
    }

    pub fn into_legacy(self) -> signal_cloud::ValidationReport {
        signal_cloud::ValidationReport {
            findings: self
                .report
                .into_payload()
                .into_iter()
                .map(|finding| signal_cloud::ValidationFinding {
                    severity: SchemaFindingSeverity::new(finding.finding_severity).into_legacy(),
                    message: finding.message.into_payload(),
                })
                .collect(),
        }
    }
}

struct LegacyValidationReport {
    report: signal_cloud::ValidationReport,
}

impl LegacyValidationReport {
    pub fn new(report: signal_cloud::ValidationReport) -> Self {
        Self { report }
    }

    pub fn into_schema(self) -> ordinary::ValidationReport {
        ordinary::ValidationReport::new(
            self.report
                .findings
                .into_iter()
                .map(|finding| ordinary::ValidationFinding {
                    finding_severity: LegacyFindingSeverity::new(finding.severity).into_schema(),
                    message: ordinary::Message::new(finding.message),
                })
                .collect(),
        )
    }
}

struct SchemaFindingSeverity {
    severity: ordinary::FindingSeverity,
}

impl SchemaFindingSeverity {
    pub fn new(severity: ordinary::FindingSeverity) -> Self {
        Self { severity }
    }

    pub fn into_legacy(self) -> signal_cloud::FindingSeverity {
        match self.severity {
            ordinary::FindingSeverity::Notice => signal_cloud::FindingSeverity::Notice,
            ordinary::FindingSeverity::Warning => signal_cloud::FindingSeverity::Warning,
            ordinary::FindingSeverity::Error => signal_cloud::FindingSeverity::Error,
        }
    }
}

struct LegacyFindingSeverity {
    severity: signal_cloud::FindingSeverity,
}

impl LegacyFindingSeverity {
    pub fn new(severity: signal_cloud::FindingSeverity) -> Self {
        Self { severity }
    }

    pub fn into_schema(self) -> ordinary::FindingSeverity {
        match self.severity {
            signal_cloud::FindingSeverity::Notice => ordinary::FindingSeverity::Notice,
            signal_cloud::FindingSeverity::Warning => ordinary::FindingSeverity::Warning,
            signal_cloud::FindingSeverity::Error => ordinary::FindingSeverity::Error,
        }
    }
}

struct SchemaUnsupportedRequest {
    unsupported: ordinary::UnsupportedRequest,
}

impl SchemaUnsupportedRequest {
    pub fn new(unsupported: ordinary::UnsupportedRequest) -> Self {
        Self { unsupported }
    }

    pub fn into_legacy(self) -> signal_cloud::RequestUnsupported {
        signal_cloud::RequestUnsupported {
            provider: self
                .unsupported
                .provider
                .map(|provider| SchemaProvider::new(provider).into_legacy()),
            capability: self
                .unsupported
                .capability
                .map(|capability| SchemaCapability::new(capability).into_legacy()),
            reason: SchemaUnsupportedReason::new(self.unsupported.reason).into_legacy(),
        }
    }
}

struct LegacyUnsupportedRequest {
    unsupported: signal_cloud::RequestUnsupported,
}

impl LegacyUnsupportedRequest {
    pub fn new(unsupported: signal_cloud::RequestUnsupported) -> Self {
        Self { unsupported }
    }

    pub fn into_schema(self) -> ordinary::UnsupportedRequest {
        ordinary::UnsupportedRequest {
            provider: self
                .unsupported
                .provider
                .map(|provider| LegacyProvider::new(provider).into_schema()),
            capability: self
                .unsupported
                .capability
                .map(|capability| LegacyCapability::new(capability).into_schema()),
            reason: LegacyUnsupportedReason::new(self.unsupported.reason).into_schema(),
        }
    }
}

struct SchemaUnsupportedReason {
    reason: ordinary::UnsupportedReason,
}

impl SchemaUnsupportedReason {
    pub fn new(reason: ordinary::UnsupportedReason) -> Self {
        Self { reason }
    }

    pub fn into_legacy(self) -> signal_cloud::UnsupportedReason {
        match self.reason {
            ordinary::UnsupportedReason::ProviderNotBuilt => {
                signal_cloud::UnsupportedReason::ProviderNotBuilt
            }
            ordinary::UnsupportedReason::ProviderNotCompiled => {
                signal_cloud::UnsupportedReason::ProviderNotCompiled
            }
            ordinary::UnsupportedReason::ProviderNotConfigured => {
                signal_cloud::UnsupportedReason::ProviderNotConfigured
            }
            ordinary::UnsupportedReason::CapabilityNotCompiled => {
                signal_cloud::UnsupportedReason::CapabilityNotCompiled
            }
            ordinary::UnsupportedReason::CapabilityNotConfigured => {
                signal_cloud::UnsupportedReason::CapabilityNotConfigured
            }
            ordinary::UnsupportedReason::CapabilityUnauthorized => {
                signal_cloud::UnsupportedReason::CapabilityUnauthorized
            }
        }
    }
}

struct LegacyUnsupportedReason {
    reason: signal_cloud::UnsupportedReason,
}

impl LegacyUnsupportedReason {
    pub fn new(reason: signal_cloud::UnsupportedReason) -> Self {
        Self { reason }
    }

    pub fn into_schema(self) -> ordinary::UnsupportedReason {
        match self.reason {
            signal_cloud::UnsupportedReason::ProviderNotBuilt => {
                ordinary::UnsupportedReason::ProviderNotBuilt
            }
            signal_cloud::UnsupportedReason::ProviderNotCompiled => {
                ordinary::UnsupportedReason::ProviderNotCompiled
            }
            signal_cloud::UnsupportedReason::ProviderNotConfigured => {
                ordinary::UnsupportedReason::ProviderNotConfigured
            }
            signal_cloud::UnsupportedReason::CapabilityNotCompiled => {
                ordinary::UnsupportedReason::CapabilityNotCompiled
            }
            signal_cloud::UnsupportedReason::CapabilityNotConfigured => {
                ordinary::UnsupportedReason::CapabilityNotConfigured
            }
            signal_cloud::UnsupportedReason::CapabilityUnauthorized => {
                ordinary::UnsupportedReason::CapabilityUnauthorized
            }
        }
    }
}

struct SchemaRejectedRequest {
    rejected: ordinary::RejectedRequest,
}

impl SchemaRejectedRequest {
    pub fn new(rejected: ordinary::RejectedRequest) -> Self {
        Self { rejected }
    }

    pub fn into_legacy(self) -> signal_cloud::RequestRejected {
        signal_cloud::RequestRejected {
            reason: SchemaRejectionReason::new(self.rejected.into_payload()).into_legacy(),
        }
    }
}

struct LegacyRejectedRequest {
    rejected: signal_cloud::RequestRejected,
}

impl LegacyRejectedRequest {
    pub fn new(rejected: signal_cloud::RequestRejected) -> Self {
        Self { rejected }
    }

    pub fn into_schema(self) -> ordinary::RejectedRequest {
        ordinary::RejectedRequest::new(
            LegacyRejectionReason::new(self.rejected.reason).into_schema(),
        )
    }
}

struct SchemaRejectionReason {
    reason: ordinary::RejectionReason,
}

impl SchemaRejectionReason {
    pub fn new(reason: ordinary::RejectionReason) -> Self {
        Self { reason }
    }

    pub fn into_legacy(self) -> signal_cloud::RejectionReason {
        match self.reason {
            ordinary::RejectionReason::InvalidDesiredState => {
                signal_cloud::RejectionReason::InvalidDesiredState
            }
            ordinary::RejectionReason::PlanExpired => signal_cloud::RejectionReason::PlanExpired,
            ordinary::RejectionReason::ProviderRateLimited => {
                signal_cloud::RejectionReason::ProviderRateLimited
            }
            ordinary::RejectionReason::ProviderUnavailable => {
                signal_cloud::RejectionReason::ProviderUnavailable
            }
        }
    }
}

struct LegacyRejectionReason {
    reason: signal_cloud::RejectionReason,
}

impl LegacyRejectionReason {
    pub fn new(reason: signal_cloud::RejectionReason) -> Self {
        Self { reason }
    }

    pub fn into_schema(self) -> ordinary::RejectionReason {
        match self.reason {
            signal_cloud::RejectionReason::InvalidDesiredState => {
                ordinary::RejectionReason::InvalidDesiredState
            }
            signal_cloud::RejectionReason::PlanExpired => ordinary::RejectionReason::PlanExpired,
            signal_cloud::RejectionReason::ProviderRateLimited => {
                ordinary::RejectionReason::ProviderRateLimited
            }
            signal_cloud::RejectionReason::ProviderUnavailable => {
                ordinary::RejectionReason::ProviderUnavailable
            }
        }
    }
}

struct LegacyRegistration {
    registration: meta_signal_cloud::Registration,
}

impl LegacyRegistration {
    pub fn new(registration: meta_signal_cloud::Registration) -> Self {
        Self { registration }
    }

    pub fn into_schema(self) -> meta::Registration {
        meta::Registration {
            provider: LegacyProvider::new(self.registration.provider).into_meta_schema(),
            provider_account: meta::ProviderAccount::new(
                self.registration.account.as_str().to_owned(),
            ),
            credential_handle: meta::CredentialHandle::new(
                self.registration.credential.as_str().to_owned(),
            ),
        }
    }
}

struct SchemaRegistration {
    registration: meta::Registration,
}

impl SchemaRegistration {
    pub fn new(registration: meta::Registration) -> Self {
        Self { registration }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::Registration {
        meta_signal_cloud::Registration {
            provider: MetaSchemaProvider::new(self.registration.provider).into_legacy(),
            account: signal_cloud::ProviderAccount::new(
                self.registration.provider_account.into_payload(),
            ),
            credential: meta_signal_cloud::CredentialHandle::new(
                self.registration.credential_handle.into_payload(),
            ),
        }
    }
}

struct LegacyRotation {
    rotation: meta_signal_cloud::Rotation,
}

impl LegacyRotation {
    pub fn new(rotation: meta_signal_cloud::Rotation) -> Self {
        Self { rotation }
    }

    pub fn into_schema(self) -> meta::Rotation {
        meta::Rotation {
            provider: LegacyProvider::new(self.rotation.provider).into_meta_schema(),
            provider_account: meta::ProviderAccount::new(self.rotation.account.as_str().to_owned()),
            credential_handle: meta::CredentialHandle::new(
                self.rotation.credential.as_str().to_owned(),
            ),
        }
    }
}

struct SchemaRotation {
    rotation: meta::Rotation,
}

impl SchemaRotation {
    pub fn new(rotation: meta::Rotation) -> Self {
        Self { rotation }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::Rotation {
        meta_signal_cloud::Rotation {
            provider: MetaSchemaProvider::new(self.rotation.provider).into_legacy(),
            account: signal_cloud::ProviderAccount::new(
                self.rotation.provider_account.into_payload(),
            ),
            credential: meta_signal_cloud::CredentialHandle::new(
                self.rotation.credential_handle.into_payload(),
            ),
        }
    }
}

struct LegacyPolicy {
    policy: meta_signal_cloud::Policy,
}

impl LegacyPolicy {
    pub fn new(policy: meta_signal_cloud::Policy) -> Self {
        Self { policy }
    }

    pub fn into_schema(self) -> meta::Policy {
        meta::Policy {
            zones: self
                .policy
                .zones
                .into_iter()
                .map(|policy| meta::ZonePolicy {
                    provider: LegacyProvider::new(policy.provider).into_meta_schema(),
                    provider_account: meta::ProviderAccount::new(
                        policy.account.as_str().to_owned(),
                    ),
                    allowed_zones: policy
                        .allowed_zones
                        .into_iter()
                        .map(|zone| meta::DomainName::new(zone.as_str().to_owned()))
                        .collect(),
                })
                .collect(),
            capabilities: self
                .policy
                .capabilities
                .into_iter()
                .map(|policy| meta::CapabilityPolicy {
                    provider: LegacyProvider::new(policy.provider).into_meta_schema(),
                    provider_account: meta::ProviderAccount::new(
                        policy.account.as_str().to_owned(),
                    ),
                    capability: LegacyCapability::new(policy.capability).into_meta_schema(),
                    capability_directive: LegacyCapabilityDirective::new(policy.directive)
                        .into_schema(),
                })
                .collect(),
        }
    }
}

struct SchemaPolicy {
    policy: meta::Policy,
}

impl SchemaPolicy {
    pub fn new(policy: meta::Policy) -> Self {
        Self { policy }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::Policy {
        meta_signal_cloud::Policy {
            zones: self
                .policy
                .zones
                .into_iter()
                .map(|policy| meta_signal_cloud::ZonePolicy {
                    provider: MetaSchemaProvider::new(policy.provider).into_legacy(),
                    account: signal_cloud::ProviderAccount::new(
                        policy.provider_account.into_payload(),
                    ),
                    allowed_zones: policy
                        .allowed_zones
                        .into_iter()
                        .map(|zone| signal_cloud::DomainName::new(zone.into_payload()))
                        .collect(),
                })
                .collect(),
            capabilities: self
                .policy
                .capabilities
                .into_iter()
                .map(|policy| meta_signal_cloud::CapabilityPolicy {
                    provider: MetaSchemaProvider::new(policy.provider).into_legacy(),
                    account: signal_cloud::ProviderAccount::new(
                        policy.provider_account.into_payload(),
                    ),
                    capability: MetaSchemaCapability::new(policy.capability).into_legacy(),
                    directive: SchemaCapabilityDirective::new(policy.capability_directive)
                        .into_legacy(),
                })
                .collect(),
        }
    }
}

struct LegacyCapabilityDirective {
    directive: meta_signal_cloud::CapabilityDirective,
}

impl LegacyCapabilityDirective {
    pub fn new(directive: meta_signal_cloud::CapabilityDirective) -> Self {
        Self { directive }
    }

    pub fn into_schema(self) -> meta::CapabilityDirective {
        match self.directive {
            meta_signal_cloud::CapabilityDirective::Enable => meta::CapabilityDirective::Enable,
            meta_signal_cloud::CapabilityDirective::Disable => meta::CapabilityDirective::Disable,
        }
    }
}

struct SchemaCapabilityDirective {
    directive: meta::CapabilityDirective,
}

impl SchemaCapabilityDirective {
    pub fn new(directive: meta::CapabilityDirective) -> Self {
        Self { directive }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::CapabilityDirective {
        match self.directive {
            meta::CapabilityDirective::Enable => meta_signal_cloud::CapabilityDirective::Enable,
            meta::CapabilityDirective::Disable => meta_signal_cloud::CapabilityDirective::Disable,
        }
    }
}

struct LegacyPlanPreparation {
    preparation: meta_signal_cloud::PlanPreparation,
}

impl LegacyPlanPreparation {
    pub fn new(preparation: meta_signal_cloud::PlanPreparation) -> Self {
        Self { preparation }
    }

    pub fn into_schema(self) -> meta::PlanPreparation {
        meta::PlanPreparation::new(
            LegacyDesiredState::new(self.preparation.desired_state).into_meta_schema(),
        )
    }
}

struct SchemaPlanPreparation {
    preparation: meta::PlanPreparation,
}

impl SchemaPlanPreparation {
    pub fn new(preparation: meta::PlanPreparation) -> Self {
        Self { preparation }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::PlanPreparation {
        meta_signal_cloud::PlanPreparation {
            desired_state: SchemaMetaDesiredState::new(self.preparation.into_payload())
                .into_legacy(),
        }
    }
}

struct LegacyProjectionPreparation {
    preparation: meta_signal_cloud::ProjectionPreparation,
}

impl LegacyProjectionPreparation {
    pub fn new(preparation: meta_signal_cloud::ProjectionPreparation) -> Self {
        Self { preparation }
    }

    pub fn into_schema(self) -> meta::ProjectionPreparation {
        meta::ProjectionPreparation {
            provider: LegacyProvider::new(self.preparation.provider).into_meta_schema(),
            projection: LegacyDomainProjection::new(self.preparation.projection).into_schema(),
        }
    }
}

struct SchemaProjectionPreparation {
    preparation: meta::ProjectionPreparation,
}

impl SchemaProjectionPreparation {
    pub fn new(preparation: meta::ProjectionPreparation) -> Self {
        Self { preparation }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::ProjectionPreparation {
        meta_signal_cloud::ProjectionPreparation {
            provider: MetaSchemaProvider::new(self.preparation.provider).into_legacy(),
            projection: SchemaDomainProjection::new(self.preparation.projection).into_legacy(),
        }
    }
}

struct SchemaMetaDesiredState {
    desired: meta::DesiredState,
}

impl SchemaMetaDesiredState {
    pub fn new(desired: meta::DesiredState) -> Self {
        Self { desired }
    }

    pub fn into_legacy(self) -> signal_cloud::DesiredState {
        signal_cloud::DesiredState {
            provider: MetaSchemaProvider::new(self.desired.provider).into_legacy(),
            zone: signal_cloud::DomainName::new(self.desired.zone.into_payload()),
            records: self
                .desired
                .records
                .into_iter()
                .map(|record| SchemaMetaRecord::new(record).into_legacy())
                .collect(),
            redirects: self
                .desired
                .redirects
                .into_iter()
                .map(|redirect| SchemaMetaRedirect::new(redirect).into_legacy())
                .collect(),
        }
    }
}

impl LegacyDesiredState {
    pub fn into_meta_schema(self) -> meta::DesiredState {
        meta::DesiredState {
            provider: LegacyProvider::new(self.desired.provider).into_meta_schema(),
            zone: meta::DomainName::new(self.desired.zone.as_str().to_owned()),
            records: self
                .desired
                .records
                .into_iter()
                .map(|record| LegacyRecord::new(record).into_meta_schema())
                .collect(),
            redirects: self
                .desired
                .redirects
                .into_iter()
                .map(|redirect| LegacyRedirect::new(redirect).into_meta_schema())
                .collect(),
        }
    }
}

struct SchemaMetaRecord {
    record: meta::DomainNameSystemRecord,
}

impl SchemaMetaRecord {
    pub fn new(record: meta::DomainNameSystemRecord) -> Self {
        Self { record }
    }

    pub fn into_legacy(self) -> signal_cloud::DomainNameSystemRecord {
        signal_cloud::DomainNameSystemRecord {
            name: signal_cloud::DomainName::new(self.record.name.into_payload()),
            kind: SchemaMetaRecordKind::new(self.record.record_kind).into_legacy(),
            value: signal_cloud::RecordValue::new(self.record.content.into_payload()),
            proxy_mode: signal_cloud::ProxyMode::Direct,
        }
    }
}

struct SchemaMetaRedirect {
    redirect: meta::RedirectRule,
}

impl SchemaMetaRedirect {
    pub fn new(redirect: meta::RedirectRule) -> Self {
        Self { redirect }
    }

    pub fn into_legacy(self) -> signal_cloud::RedirectRule {
        signal_cloud::RedirectRule {
            source: signal_cloud::DomainName::new(self.redirect.source.into_payload()),
            target: signal_cloud::UniformResourceLocator::new(self.redirect.target.into_payload()),
            status: SchemaMetaRedirectStatus::new(self.redirect.redirect_status).into_legacy(),
            path_treatment: signal_cloud::PathTreatment::Preserve,
        }
    }
}

struct SchemaMetaRedirectStatus {
    status: meta::RedirectStatus,
}

impl SchemaMetaRedirectStatus {
    pub fn new(status: meta::RedirectStatus) -> Self {
        Self { status }
    }

    pub fn into_legacy(self) -> signal_cloud::RedirectStatus {
        match self.status {
            meta::RedirectStatus::Permanent => signal_cloud::RedirectStatus::Permanent,
            meta::RedirectStatus::Temporary => signal_cloud::RedirectStatus::Temporary,
        }
    }
}

struct LegacyDomainProjection {
    projection: signal_domain_criome::Projection,
}

impl LegacyDomainProjection {
    pub fn new(projection: signal_domain_criome::Projection) -> Self {
        Self { projection }
    }

    pub fn into_schema(self) -> meta::Projection {
        meta::Projection {
            projection_query: meta::ProjectionQuery {
                domain: meta::DomainName::new(self.projection.query.domain.as_str().to_owned()),
                projection_scope: LegacyDomainProjectionScope::new(self.projection.query.scope)
                    .into_schema(),
            },
            records: self
                .projection
                .records
                .into_iter()
                .map(|record| LegacyDomainRecord::new(record).into_schema())
                .collect(),
            redirects: self
                .projection
                .redirects
                .into_iter()
                .map(|redirect| LegacyDomainRedirect::new(redirect).into_schema())
                .collect(),
        }
    }
}

struct SchemaDomainProjection {
    projection: meta::Projection,
}

impl SchemaDomainProjection {
    pub fn new(projection: meta::Projection) -> Self {
        Self { projection }
    }

    pub fn into_legacy(self) -> signal_domain_criome::Projection {
        signal_domain_criome::Projection {
            query: signal_domain_criome::ProjectionQuery {
                domain: signal_domain_criome::DomainName::new(
                    self.projection.projection_query.domain.into_payload(),
                ),
                scope: SchemaDomainProjectionScope::new(
                    self.projection.projection_query.projection_scope,
                )
                .into_legacy(),
            },
            records: self
                .projection
                .records
                .into_iter()
                .map(|record| SchemaDomainRecord::new(record).into_legacy())
                .collect(),
            redirects: self
                .projection
                .redirects
                .into_iter()
                .map(|redirect| SchemaDomainRedirect::new(redirect).into_legacy())
                .collect(),
        }
    }
}

struct LegacyDomainProjectionScope {
    scope: signal_domain_criome::ProjectionScope,
}

impl LegacyDomainProjectionScope {
    pub fn new(scope: signal_domain_criome::ProjectionScope) -> Self {
        Self { scope }
    }

    pub fn into_schema(self) -> meta::ProjectionScope {
        match self.scope {
            signal_domain_criome::ProjectionScope::PublicRecords => {
                meta::ProjectionScope::PublicRecords
            }
            signal_domain_criome::ProjectionScope::RedirectRules
            | signal_domain_criome::ProjectionScope::Everything => {
                meta::ProjectionScope::AllRecords
            }
        }
    }
}

struct SchemaDomainProjectionScope {
    scope: meta::ProjectionScope,
}

impl SchemaDomainProjectionScope {
    pub fn new(scope: meta::ProjectionScope) -> Self {
        Self { scope }
    }

    pub fn into_legacy(self) -> signal_domain_criome::ProjectionScope {
        match self.scope {
            meta::ProjectionScope::PublicRecords => {
                signal_domain_criome::ProjectionScope::PublicRecords
            }
            meta::ProjectionScope::AllRecords => signal_domain_criome::ProjectionScope::Everything,
        }
    }
}

struct LegacyDomainRecord {
    record: signal_domain_criome::DomainNameSystemRecord,
}

impl LegacyDomainRecord {
    pub fn new(record: signal_domain_criome::DomainNameSystemRecord) -> Self {
        Self { record }
    }

    pub fn into_schema(self) -> meta::DomainNameSystemRecord {
        meta::DomainNameSystemRecord {
            name: meta::DomainName::new(self.record.name.as_str().to_owned()),
            record_kind: LegacyDomainRecordKind::new(self.record.kind).into_schema(),
            content: meta::RecordContent::new(self.record.value.as_str().to_owned()),
        }
    }
}

struct SchemaDomainRecord {
    record: meta::DomainNameSystemRecord,
}

impl SchemaDomainRecord {
    pub fn new(record: meta::DomainNameSystemRecord) -> Self {
        Self { record }
    }

    pub fn into_legacy(self) -> signal_domain_criome::DomainNameSystemRecord {
        signal_domain_criome::DomainNameSystemRecord {
            name: signal_domain_criome::DomainName::new(self.record.name.into_payload()),
            kind: SchemaDomainRecordKind::new(self.record.record_kind).into_legacy(),
            value: signal_domain_criome::RecordValue::new(self.record.content.into_payload()),
        }
    }
}

struct LegacyDomainRecordKind {
    kind: signal_domain_criome::RecordKind,
}

impl LegacyDomainRecordKind {
    pub fn new(kind: signal_domain_criome::RecordKind) -> Self {
        Self { kind }
    }

    pub fn into_schema(self) -> meta::RecordKind {
        match self.kind {
            signal_domain_criome::RecordKind::AddressV4 => meta::RecordKind::Address,
            signal_domain_criome::RecordKind::AddressV6 => meta::RecordKind::AddressSix,
            signal_domain_criome::RecordKind::CanonicalName => meta::RecordKind::CanonicalName,
            signal_domain_criome::RecordKind::Text => meta::RecordKind::Text,
        }
    }
}

struct SchemaDomainRecordKind {
    kind: meta::RecordKind,
}

impl SchemaDomainRecordKind {
    pub fn new(kind: meta::RecordKind) -> Self {
        Self { kind }
    }

    pub fn into_legacy(self) -> signal_domain_criome::RecordKind {
        match self.kind {
            meta::RecordKind::Address => signal_domain_criome::RecordKind::AddressV4,
            meta::RecordKind::AddressSix => signal_domain_criome::RecordKind::AddressV6,
            meta::RecordKind::CanonicalName => signal_domain_criome::RecordKind::CanonicalName,
            meta::RecordKind::MailExchange | meta::RecordKind::Text | meta::RecordKind::Service => {
                signal_domain_criome::RecordKind::Text
            }
        }
    }
}

struct LegacyDomainRedirect {
    redirect: signal_domain_criome::RedirectRule,
}

impl LegacyDomainRedirect {
    pub fn new(redirect: signal_domain_criome::RedirectRule) -> Self {
        Self { redirect }
    }

    pub fn into_schema(self) -> meta::RedirectRule {
        meta::RedirectRule {
            source: meta::DomainName::new(self.redirect.source.as_str().to_owned()),
            target: meta::DomainName::new(self.redirect.target.as_str().to_owned()),
            redirect_status: LegacyDomainRedirectStatus::new(self.redirect.status).into_schema(),
        }
    }
}

struct SchemaDomainRedirect {
    redirect: meta::RedirectRule,
}

impl SchemaDomainRedirect {
    pub fn new(redirect: meta::RedirectRule) -> Self {
        Self { redirect }
    }

    pub fn into_legacy(self) -> signal_domain_criome::RedirectRule {
        signal_domain_criome::RedirectRule {
            source: signal_domain_criome::DomainName::new(self.redirect.source.into_payload()),
            target: signal_domain_criome::UniformResourceLocator::new(
                self.redirect.target.into_payload(),
            ),
            status: SchemaDomainRedirectStatus::new(self.redirect.redirect_status).into_legacy(),
            path_treatment: signal_domain_criome::PathTreatment::Preserve,
        }
    }
}

struct LegacyDomainRedirectStatus {
    status: signal_domain_criome::RedirectStatus,
}

impl LegacyDomainRedirectStatus {
    pub fn new(status: signal_domain_criome::RedirectStatus) -> Self {
        Self { status }
    }

    pub fn into_schema(self) -> meta::RedirectStatus {
        match self.status {
            signal_domain_criome::RedirectStatus::Permanent => meta::RedirectStatus::Permanent,
            signal_domain_criome::RedirectStatus::Temporary => meta::RedirectStatus::Temporary,
        }
    }
}

struct SchemaDomainRedirectStatus {
    status: meta::RedirectStatus,
}

impl SchemaDomainRedirectStatus {
    pub fn new(status: meta::RedirectStatus) -> Self {
        Self { status }
    }

    pub fn into_legacy(self) -> signal_domain_criome::RedirectStatus {
        match self.status {
            meta::RedirectStatus::Permanent => signal_domain_criome::RedirectStatus::Permanent,
            meta::RedirectStatus::Temporary => signal_domain_criome::RedirectStatus::Temporary,
        }
    }
}

struct LegacyPlan {
    plan: signal_cloud::Plan,
}

impl LegacyPlan {
    pub fn new(plan: signal_cloud::Plan) -> Self {
        Self { plan }
    }

    pub fn into_schema(self) -> ordinary::Plan {
        ordinary::Plan {
            plan_identifier: ordinary::PlanIdentifier::new(
                self.plan.identifier.as_str().to_owned(),
            ),
            provider: LegacyProvider::new(self.plan.provider).into_schema(),
            domain_name: ordinary::DomainName::new(self.plan.zone.as_str().to_owned()),
            records_to_create: self
                .plan
                .records_to_create
                .into_iter()
                .map(|record| LegacyRecord::new(record).into_schema())
                .collect(),
            records_to_update: self
                .plan
                .records_to_update
                .into_iter()
                .map(|record| LegacyRecord::new(record).into_schema())
                .collect(),
            record_names_to_delete: self
                .plan
                .record_names_to_delete
                .into_iter()
                .map(|name| ordinary::DomainName::new(name.as_str().to_owned()))
                .collect(),
            redirects_to_create: self
                .plan
                .redirects_to_create
                .into_iter()
                .map(|redirect| LegacyRedirect::new(redirect).into_schema())
                .collect(),
            redirects_to_update: self
                .plan
                .redirects_to_update
                .into_iter()
                .map(|redirect| LegacyRedirect::new(redirect).into_schema())
                .collect(),
            redirect_sources_to_delete: self
                .plan
                .redirect_sources_to_delete
                .into_iter()
                .map(|name| ordinary::DomainName::new(name.as_str().to_owned()))
                .collect(),
        }
    }

    pub fn into_meta_schema(self) -> meta::Plan {
        meta::Plan {
            identifier: meta::PlanIdentifier::new(self.plan.identifier.as_str().to_owned()),
            provider: LegacyProvider::new(self.plan.provider).into_meta_schema(),
            zone: meta::DomainName::new(self.plan.zone.as_str().to_owned()),
            records_to_create: self
                .plan
                .records_to_create
                .into_iter()
                .map(|record| LegacyRecord::new(record).into_meta_schema())
                .collect(),
            records_to_update: self
                .plan
                .records_to_update
                .into_iter()
                .map(|record| LegacyRecord::new(record).into_meta_schema())
                .collect(),
            record_names_to_delete: self
                .plan
                .record_names_to_delete
                .into_iter()
                .map(|name| meta::DomainName::new(name.as_str().to_owned()))
                .collect(),
            redirects_to_create: self
                .plan
                .redirects_to_create
                .into_iter()
                .map(|redirect| LegacyRedirect::new(redirect).into_meta_schema())
                .collect(),
            redirects_to_update: self
                .plan
                .redirects_to_update
                .into_iter()
                .map(|redirect| LegacyRedirect::new(redirect).into_meta_schema())
                .collect(),
            redirect_sources_to_delete: self
                .plan
                .redirect_sources_to_delete
                .into_iter()
                .map(|name| meta::DomainName::new(name.as_str().to_owned()))
                .collect(),
        }
    }
}

struct SchemaPlan {
    plan: ordinary::Plan,
}

impl SchemaPlan {
    pub fn new(plan: ordinary::Plan) -> Self {
        Self { plan }
    }

    pub fn into_legacy(self) -> signal_cloud::Plan {
        signal_cloud::Plan {
            identifier: signal_cloud::PlanIdentifier::new(self.plan.plan_identifier.into_payload()),
            provider: SchemaProvider::new(self.plan.provider).into_legacy(),
            zone: signal_cloud::DomainName::new(self.plan.domain_name.into_payload()),
            records_to_create: self
                .plan
                .records_to_create
                .into_iter()
                .map(|record| SchemaRecord::new(record).into_legacy())
                .collect(),
            records_to_update: self
                .plan
                .records_to_update
                .into_iter()
                .map(|record| SchemaRecord::new(record).into_legacy())
                .collect(),
            record_names_to_delete: self
                .plan
                .record_names_to_delete
                .into_iter()
                .map(|name| signal_cloud::DomainName::new(name.into_payload()))
                .collect(),
            redirects_to_create: self
                .plan
                .redirects_to_create
                .into_iter()
                .map(|redirect| SchemaRedirect::new(redirect).into_legacy())
                .collect(),
            redirects_to_update: self
                .plan
                .redirects_to_update
                .into_iter()
                .map(|redirect| SchemaRedirect::new(redirect).into_legacy())
                .collect(),
            redirect_sources_to_delete: self
                .plan
                .redirect_sources_to_delete
                .into_iter()
                .map(|name| signal_cloud::DomainName::new(name.into_payload()))
                .collect(),
        }
    }
}

struct SchemaMetaPlan {
    plan: meta::Plan,
}

impl SchemaMetaPlan {
    pub fn new(plan: meta::Plan) -> Self {
        Self { plan }
    }

    pub fn into_legacy(self) -> signal_cloud::Plan {
        signal_cloud::Plan {
            identifier: signal_cloud::PlanIdentifier::new(self.plan.identifier.into_payload()),
            provider: MetaSchemaProvider::new(self.plan.provider).into_legacy(),
            zone: signal_cloud::DomainName::new(self.plan.zone.into_payload()),
            records_to_create: self
                .plan
                .records_to_create
                .into_iter()
                .map(|record| SchemaMetaRecord::new(record).into_legacy())
                .collect(),
            records_to_update: self
                .plan
                .records_to_update
                .into_iter()
                .map(|record| SchemaMetaRecord::new(record).into_legacy())
                .collect(),
            record_names_to_delete: self
                .plan
                .record_names_to_delete
                .into_iter()
                .map(|name| signal_cloud::DomainName::new(name.into_payload()))
                .collect(),
            redirects_to_create: self
                .plan
                .redirects_to_create
                .into_iter()
                .map(|redirect| SchemaMetaRedirect::new(redirect).into_legacy())
                .collect(),
            redirects_to_update: self
                .plan
                .redirects_to_update
                .into_iter()
                .map(|redirect| SchemaMetaRedirect::new(redirect).into_legacy())
                .collect(),
            redirect_sources_to_delete: self
                .plan
                .redirect_sources_to_delete
                .into_iter()
                .map(|name| signal_cloud::DomainName::new(name.into_payload()))
                .collect(),
        }
    }
}

struct LegacyRetirement {
    retirement: meta_signal_cloud::Retirement,
}

impl LegacyRetirement {
    pub fn new(retirement: meta_signal_cloud::Retirement) -> Self {
        Self { retirement }
    }

    pub fn into_schema(self) -> meta::Retirement {
        meta::Retirement {
            provider: LegacyProvider::new(self.retirement.provider).into_meta_schema(),
            provider_account: meta::ProviderAccount::new(
                self.retirement.account.as_str().to_owned(),
            ),
        }
    }
}

struct SchemaRetirement {
    retirement: meta::Retirement,
}

impl SchemaRetirement {
    pub fn new(retirement: meta::Retirement) -> Self {
        Self { retirement }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::Retirement {
        meta_signal_cloud::Retirement {
            provider: MetaSchemaProvider::new(self.retirement.provider).into_legacy(),
            account: signal_cloud::ProviderAccount::new(
                self.retirement.provider_account.into_payload(),
            ),
        }
    }
}

struct LegacyAccountRegistered {
    registered: meta_signal_cloud::AccountRegistered,
}

impl LegacyAccountRegistered {
    pub fn new(registered: meta_signal_cloud::AccountRegistered) -> Self {
        Self { registered }
    }

    pub fn into_schema(self) -> meta::AccountRegistered {
        meta::AccountRegistered {
            provider: LegacyProvider::new(self.registered.provider).into_meta_schema(),
            provider_account: meta::ProviderAccount::new(
                self.registered.account.as_str().to_owned(),
            ),
        }
    }
}

struct SchemaAccountRegistered {
    registered: meta::AccountRegistered,
}

impl SchemaAccountRegistered {
    pub fn new(registered: meta::AccountRegistered) -> Self {
        Self { registered }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::AccountRegistered {
        meta_signal_cloud::AccountRegistered {
            provider: MetaSchemaProvider::new(self.registered.provider).into_legacy(),
            account: signal_cloud::ProviderAccount::new(
                self.registered.provider_account.into_payload(),
            ),
        }
    }
}

struct LegacyCredentialRotated {
    rotated: meta_signal_cloud::CredentialRotated,
}

impl LegacyCredentialRotated {
    pub fn new(rotated: meta_signal_cloud::CredentialRotated) -> Self {
        Self { rotated }
    }

    pub fn into_schema(self) -> meta::CredentialRotated {
        meta::CredentialRotated {
            provider: LegacyProvider::new(self.rotated.provider).into_meta_schema(),
            provider_account: meta::ProviderAccount::new(self.rotated.account.as_str().to_owned()),
        }
    }
}

struct SchemaCredentialRotated {
    rotated: meta::CredentialRotated,
}

impl SchemaCredentialRotated {
    pub fn new(rotated: meta::CredentialRotated) -> Self {
        Self { rotated }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::CredentialRotated {
        meta_signal_cloud::CredentialRotated {
            provider: MetaSchemaProvider::new(self.rotated.provider).into_legacy(),
            account: signal_cloud::ProviderAccount::new(
                self.rotated.provider_account.into_payload(),
            ),
        }
    }
}

struct LegacyAccountRetired {
    retired: meta_signal_cloud::AccountRetired,
}

impl LegacyAccountRetired {
    pub fn new(retired: meta_signal_cloud::AccountRetired) -> Self {
        Self { retired }
    }

    pub fn into_schema(self) -> meta::AccountRetired {
        meta::AccountRetired {
            provider: LegacyProvider::new(self.retired.provider).into_meta_schema(),
            provider_account: meta::ProviderAccount::new(self.retired.account.as_str().to_owned()),
        }
    }
}

struct SchemaAccountRetired {
    retired: meta::AccountRetired,
}

impl SchemaAccountRetired {
    pub fn new(retired: meta::AccountRetired) -> Self {
        Self { retired }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::AccountRetired {
        meta_signal_cloud::AccountRetired {
            provider: MetaSchemaProvider::new(self.retired.provider).into_legacy(),
            account: signal_cloud::ProviderAccount::new(
                self.retired.provider_account.into_payload(),
            ),
        }
    }
}

struct LegacyMetaRejectedRequest {
    rejected: meta_signal_cloud::RequestRejected,
}

impl LegacyMetaRejectedRequest {
    pub fn new(rejected: meta_signal_cloud::RequestRejected) -> Self {
        Self { rejected }
    }

    pub fn into_schema(self) -> meta::RequestRejected {
        meta::RequestRejected {
            rejection_reason: LegacyMetaRejectionReason::new(self.rejected.reason).into_schema(),
            database_marker: meta::DatabaseMarker {
                commit_sequence: meta::CommitSequence::new(0),
                state_digest: meta::StateDigest::new(0),
            },
        }
    }
}

struct SchemaMetaRejectedRequest {
    rejected: meta::RequestRejected,
}

impl SchemaMetaRejectedRequest {
    pub fn new(rejected: meta::RequestRejected) -> Self {
        Self { rejected }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::RequestRejected {
        meta_signal_cloud::RequestRejected {
            reason: SchemaMetaRejectionReason::new(self.rejected.rejection_reason).into_legacy(),
        }
    }
}

struct LegacyMetaRejectionReason {
    reason: meta_signal_cloud::RejectionReason,
}

impl LegacyMetaRejectionReason {
    pub fn new(reason: meta_signal_cloud::RejectionReason) -> Self {
        Self { reason }
    }

    pub fn into_schema(self) -> meta::RejectionReason {
        match self.reason {
            meta_signal_cloud::RejectionReason::CredentialHandleUnknown => {
                meta::RejectionReason::CredentialHandleUnknown
            }
            meta_signal_cloud::RejectionReason::ProviderNotConfigured => {
                meta::RejectionReason::ProviderNotConfigured
            }
            meta_signal_cloud::RejectionReason::AccountUnknown => {
                meta::RejectionReason::AccountUnknown
            }
            meta_signal_cloud::RejectionReason::PlanUnknown => meta::RejectionReason::PlanUnknown,
            meta_signal_cloud::RejectionReason::PlanNotApproved => {
                meta::RejectionReason::PlanNotApproved
            }
            meta_signal_cloud::RejectionReason::PlanGenerationFailed => {
                meta::RejectionReason::PlanGenerationFailed
            }
            meta_signal_cloud::RejectionReason::CapabilityUnauthorized => {
                meta::RejectionReason::CapabilityUnauthorized
            }
        }
    }
}

struct SchemaMetaRejectionReason {
    reason: meta::RejectionReason,
}

impl SchemaMetaRejectionReason {
    pub fn new(reason: meta::RejectionReason) -> Self {
        Self { reason }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::RejectionReason {
        match self.reason {
            meta::RejectionReason::CredentialHandleUnknown => {
                meta_signal_cloud::RejectionReason::CredentialHandleUnknown
            }
            meta::RejectionReason::ProviderNotConfigured => {
                meta_signal_cloud::RejectionReason::ProviderNotConfigured
            }
            meta::RejectionReason::AccountUnknown => {
                meta_signal_cloud::RejectionReason::AccountUnknown
            }
            meta::RejectionReason::PlanUnknown => meta_signal_cloud::RejectionReason::PlanUnknown,
            meta::RejectionReason::PlanNotApproved => {
                meta_signal_cloud::RejectionReason::PlanNotApproved
            }
            meta::RejectionReason::PlanGenerationFailed => {
                meta_signal_cloud::RejectionReason::PlanGenerationFailed
            }
            meta::RejectionReason::CapabilityUnauthorized => {
                meta_signal_cloud::RejectionReason::CapabilityUnauthorized
            }
        }
    }
}

struct SchemaHostQuery {
    query: ordinary::HostQuery,
}

impl SchemaHostQuery {
    pub fn new(query: ordinary::HostQuery) -> Self {
        Self { query }
    }

    pub fn into_legacy(self) -> signal_cloud::HostQuery {
        signal_cloud::HostQuery {
            provider: SchemaProvider::new(self.query.provider).into_legacy(),
            account: self
                .query
                .provider_account
                .map(|account| signal_cloud::ProviderAccount::new(account.into_payload())),
        }
    }
}

struct LegacyHostQuery {
    query: signal_cloud::HostQuery,
}

impl LegacyHostQuery {
    pub fn new(query: signal_cloud::HostQuery) -> Self {
        Self { query }
    }

    pub fn into_schema(self) -> ordinary::HostQuery {
        ordinary::HostQuery {
            provider: LegacyProvider::new(self.query.provider).into_schema(),
            provider_account: self
                .query
                .account
                .map(|account| ordinary::ProviderAccount::new(account.as_str().to_owned())),
        }
    }
}

struct SchemaCloudHostListing {
    listing: ordinary::CloudHostListing,
}

impl SchemaCloudHostListing {
    pub fn new(listing: ordinary::CloudHostListing) -> Self {
        Self { listing }
    }

    pub fn into_legacy(self) -> signal_cloud::CloudHostListing {
        signal_cloud::CloudHostListing {
            hosts: self
                .listing
                .into_payload()
                .into_iter()
                .map(|host| SchemaCloudHost::new(host).into_legacy())
                .collect(),
        }
    }
}

struct LegacyCloudHostListing {
    listing: signal_cloud::CloudHostListing,
}

impl LegacyCloudHostListing {
    pub fn new(listing: signal_cloud::CloudHostListing) -> Self {
        Self { listing }
    }

    pub fn into_schema(self) -> ordinary::CloudHostListing {
        ordinary::CloudHostListing::new(
            self.listing
                .hosts
                .into_iter()
                .map(|host| LegacyCloudHost::new(host).into_schema())
                .collect(),
        )
    }
}

struct SchemaCloudHost {
    host: ordinary::CloudHost,
}

impl SchemaCloudHost {
    pub fn new(host: ordinary::CloudHost) -> Self {
        Self { host }
    }

    pub fn into_legacy(self) -> signal_cloud::CloudHost {
        signal_cloud::CloudHost {
            provider: SchemaProvider::new(self.host.provider).into_legacy(),
            account: signal_cloud::ProviderAccount::new(self.host.provider_account.into_payload()),
            identifier: signal_cloud::HostIdentifier::new(self.host.host_identifier.into_payload()),
            name: signal_cloud::DomainName::new(self.host.host_name.into_payload()),
            server_type: signal_cloud::ServerType::new(self.host.server_type.into_payload()),
            image: signal_cloud::ImageName::new(self.host.image_name.into_payload()),
            ipv4: signal_cloud::IpAddress::new(self.host.ipv4_address.into_payload()),
            status: SchemaHostStatus::new(self.host.host_status).into_legacy(),
        }
    }
}

struct LegacyCloudHost {
    host: signal_cloud::CloudHost,
}

impl LegacyCloudHost {
    pub fn new(host: signal_cloud::CloudHost) -> Self {
        Self { host }
    }

    pub fn into_schema(self) -> ordinary::CloudHost {
        ordinary::CloudHost {
            provider: LegacyProvider::new(self.host.provider).into_schema(),
            provider_account: ordinary::ProviderAccount::new(self.host.account.as_str().to_owned()),
            host_identifier: ordinary::HostIdentifier::new(
                self.host.identifier.as_str().to_owned(),
            ),
            host_name: ordinary::DomainName::new(self.host.name.as_str().to_owned()),
            server_type: ordinary::ServerType::new(self.host.server_type.as_str().to_owned()),
            image_name: ordinary::ImageName::new(self.host.image.as_str().to_owned()),
            ipv4_address: ordinary::IpAddress::new(self.host.ipv4.as_str().to_owned()),
            host_status: LegacyHostStatus::new(self.host.status).into_schema(),
        }
    }
}

struct SchemaHostStatus {
    status: ordinary::HostStatus,
}

impl SchemaHostStatus {
    pub fn new(status: ordinary::HostStatus) -> Self {
        Self { status }
    }

    pub fn into_legacy(self) -> signal_cloud::HostStatus {
        match self.status {
            ordinary::HostStatus::Initializing => signal_cloud::HostStatus::Initializing,
            ordinary::HostStatus::Running => signal_cloud::HostStatus::Running,
            ordinary::HostStatus::Stopped => signal_cloud::HostStatus::Stopped,
            ordinary::HostStatus::Deleting => signal_cloud::HostStatus::Deleting,
            ordinary::HostStatus::Unknown => signal_cloud::HostStatus::Unknown,
        }
    }
}

struct LegacyHostStatus {
    status: signal_cloud::HostStatus,
}

impl LegacyHostStatus {
    pub fn new(status: signal_cloud::HostStatus) -> Self {
        Self { status }
    }

    pub fn into_schema(self) -> ordinary::HostStatus {
        match self.status {
            signal_cloud::HostStatus::Initializing => ordinary::HostStatus::Initializing,
            signal_cloud::HostStatus::Running => ordinary::HostStatus::Running,
            signal_cloud::HostStatus::Stopped => ordinary::HostStatus::Stopped,
            signal_cloud::HostStatus::Deleting => ordinary::HostStatus::Deleting,
            signal_cloud::HostStatus::Unknown => ordinary::HostStatus::Unknown,
        }
    }
}

struct LegacyHostPlanPreparation {
    preparation: meta_signal_cloud::HostPlanPreparation,
}

impl LegacyHostPlanPreparation {
    pub fn new(preparation: meta_signal_cloud::HostPlanPreparation) -> Self {
        Self { preparation }
    }

    pub fn into_schema(self) -> meta::HostPlanPreparation {
        meta::HostPlanPreparation::new(
            LegacyDesiredHostState::new(self.preparation.desired_host_state).into_meta_schema(),
        )
    }
}

struct SchemaHostPlanPreparation {
    preparation: meta::HostPlanPreparation,
}

impl SchemaHostPlanPreparation {
    pub fn new(preparation: meta::HostPlanPreparation) -> Self {
        Self { preparation }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::HostPlanPreparation {
        meta_signal_cloud::HostPlanPreparation {
            desired_host_state: SchemaMetaDesiredHostState::new(self.preparation.into_payload())
                .into_legacy(),
        }
    }
}

struct LegacyDesiredHostState {
    desired: meta_signal_cloud::DesiredHostState,
}

impl LegacyDesiredHostState {
    pub fn new(desired: meta_signal_cloud::DesiredHostState) -> Self {
        Self { desired }
    }

    pub fn into_meta_schema(self) -> meta::DesiredHostState {
        meta::DesiredHostState {
            provider: LegacyProvider::new(self.desired.provider).into_meta_schema(),
            host_name: meta::DomainName::new(self.desired.host_name.as_str().to_owned()),
            server_type: meta::ServerType::new(self.desired.server_type.as_str().to_owned()),
            image_name: meta::ImageName::new(self.desired.image_name.as_str().to_owned()),
            ssh_key_name: meta::SshKeyName::new(self.desired.ssh_key_name.as_str().to_owned()),
        }
    }
}

struct SchemaMetaDesiredHostState {
    desired: meta::DesiredHostState,
}

impl SchemaMetaDesiredHostState {
    pub fn new(desired: meta::DesiredHostState) -> Self {
        Self { desired }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::DesiredHostState {
        meta_signal_cloud::DesiredHostState {
            provider: MetaSchemaProvider::new(self.desired.provider).into_legacy(),
            host_name: signal_cloud::DomainName::new(self.desired.host_name.into_payload()),
            server_type: meta_signal_cloud::ServerType::new(
                self.desired.server_type.into_payload(),
            ),
            image_name: meta_signal_cloud::ImageName::new(self.desired.image_name.into_payload()),
            ssh_key_name: meta_signal_cloud::SshKeyName::new(
                self.desired.ssh_key_name.into_payload(),
            ),
        }
    }
}

struct LegacyHostDestruction {
    destruction: meta_signal_cloud::HostDestruction,
}

impl LegacyHostDestruction {
    pub fn new(destruction: meta_signal_cloud::HostDestruction) -> Self {
        Self { destruction }
    }

    pub fn into_schema(self) -> meta::HostDestruction {
        meta::HostDestruction {
            provider: LegacyProvider::new(self.destruction.provider).into_meta_schema(),
            host_name: meta::DomainName::new(self.destruction.host_name.as_str().to_owned()),
        }
    }
}

struct SchemaHostDestruction {
    destruction: meta::HostDestruction,
}

impl SchemaHostDestruction {
    pub fn new(destruction: meta::HostDestruction) -> Self {
        Self { destruction }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::HostDestruction {
        meta_signal_cloud::HostDestruction {
            provider: MetaSchemaProvider::new(self.destruction.provider).into_legacy(),
            host_name: signal_cloud::DomainName::new(self.destruction.host_name.into_payload()),
        }
    }
}

struct LegacyHostPlan {
    plan: meta_signal_cloud::HostPlan,
}

impl LegacyHostPlan {
    pub fn new(plan: meta_signal_cloud::HostPlan) -> Self {
        Self { plan }
    }

    pub fn into_meta_schema(self) -> meta::HostPlan {
        meta::HostPlan {
            identifier: meta::PlanIdentifier::new(self.plan.identifier.as_str().to_owned()),
            provider: LegacyProvider::new(self.plan.provider).into_meta_schema(),
            host_name: meta::DomainName::new(self.plan.host_name.as_str().to_owned()),
            server_type: meta::ServerType::new(self.plan.server_type.as_str().to_owned()),
            image_name: meta::ImageName::new(self.plan.image_name.as_str().to_owned()),
            ssh_key_name: meta::SshKeyName::new(self.plan.ssh_key_name.as_str().to_owned()),
            intent: LegacyHostIntent::new(self.plan.intent).into_meta_schema(),
        }
    }
}

struct SchemaMetaHostPlan {
    plan: meta::HostPlan,
}

impl SchemaMetaHostPlan {
    pub fn new(plan: meta::HostPlan) -> Self {
        Self { plan }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::HostPlan {
        meta_signal_cloud::HostPlan {
            identifier: signal_cloud::PlanIdentifier::new(self.plan.identifier.into_payload()),
            provider: MetaSchemaProvider::new(self.plan.provider).into_legacy(),
            host_name: signal_cloud::DomainName::new(self.plan.host_name.into_payload()),
            server_type: meta_signal_cloud::ServerType::new(self.plan.server_type.into_payload()),
            image_name: meta_signal_cloud::ImageName::new(self.plan.image_name.into_payload()),
            ssh_key_name: meta_signal_cloud::SshKeyName::new(self.plan.ssh_key_name.into_payload()),
            intent: SchemaMetaHostIntent::new(self.plan.intent).into_legacy(),
        }
    }
}

struct LegacyHostIntent {
    intent: meta_signal_cloud::HostIntent,
}

impl LegacyHostIntent {
    pub fn new(intent: meta_signal_cloud::HostIntent) -> Self {
        Self { intent }
    }

    pub fn into_meta_schema(self) -> meta::HostIntent {
        match self.intent {
            meta_signal_cloud::HostIntent::Create => meta::HostIntent::Create,
            meta_signal_cloud::HostIntent::Destroy => meta::HostIntent::Destroy,
        }
    }
}

struct SchemaMetaHostIntent {
    intent: meta::HostIntent,
}

impl SchemaMetaHostIntent {
    pub fn new(intent: meta::HostIntent) -> Self {
        Self { intent }
    }

    pub fn into_legacy(self) -> meta_signal_cloud::HostIntent {
        match self.intent {
            meta::HostIntent::Create => meta_signal_cloud::HostIntent::Create,
            meta::HostIntent::Destroy => meta_signal_cloud::HostIntent::Destroy,
        }
    }
}
