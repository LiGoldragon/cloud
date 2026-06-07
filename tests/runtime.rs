use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command};
#[cfg(feature = "cloudflare")]
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cloud::client::{CliRequest, CommandLineDispatch, SchemaConnection};
#[cfg(feature = "cloudflare")]
use cloud::cloudflare::{
    Api as CloudflareApi, ApiRecord, ApiZone, CredentialSource, RecordIdentifier, Token,
};
use cloud::{CloudDaemonCommand, CloudDaemonConfigurationFile, DaemonConfiguration, Store};
use meta_signal_cloud::schema::lib as meta_schema;
#[cfg(feature = "cloudflare")]
use meta_signal_cloud::{
    CapabilityDirective, CapabilityPolicy, PlanPreparation, Policy, ProjectionPreparation,
    ZonePolicy,
};
use meta_signal_cloud::{
    CredentialHandle, Operation as MetaOperation, Registration, Reply as MetaReply,
};
use nota_next::NotaEncode;
use signal_cloud::schema::lib as ordinary_schema;
use signal_cloud::{
    Capability, DomainName, Observation, Operation as CloudOperation, Provider, ProviderAccount,
    Reply as CloudReply, RequestUnsupported, UnsupportedReason,
};
#[cfg(feature = "cloudflare")]
use signal_cloud::{
    DomainNameSystemRecord, ProxyMode, RecordKind, RecordListing, RecordValue, ZoneIdentifier,
};
use signal_domain_criome::{Projection, ProjectionQuery, ProjectionScope};
use signal_frame::{CommandLineSocket, Reply as FrameReply, RequestPayload, SubReply};

fn encode_to_text(value: &impl NotaEncode) -> String {
    value.to_nota()
}

#[cfg(feature = "cloudflare")]
#[derive(Debug)]
struct FixtureCredentialSource {
    token: Option<Token>,
}

#[cfg(feature = "cloudflare")]
impl FixtureCredentialSource {
    fn available() -> Self {
        Self {
            token: Some(Token::new("fixture-token")),
        }
    }

    fn missing() -> Self {
        Self { token: None }
    }
}

#[cfg(feature = "cloudflare")]
impl CredentialSource for FixtureCredentialSource {
    fn token(&self, handle: &CredentialHandle) -> cloud::cloudflare::Result<Token> {
        self.token.clone().ok_or_else(|| {
            cloud::cloudflare::Error::CredentialUnavailable(handle.as_str().to_owned())
        })
    }
}

#[cfg(feature = "cloudflare")]
#[derive(Debug)]
struct FixtureCloudflareApi {
    zones: Vec<ApiZone>,
    records: Mutex<Vec<ApiRecord>>,
    queried_zones: Mutex<Vec<Option<String>>>,
    queried_record_zones: Mutex<Vec<String>>,
}

#[cfg(feature = "cloudflare")]
impl FixtureCloudflareApi {
    fn new() -> Self {
        Self {
            zones: vec![ApiZone {
                identifier: ZoneIdentifier::new("zone-one"),
                name: DomainName::new("goldragon.criome"),
            }],
            records: Mutex::new(vec![ApiRecord {
                identifier: RecordIdentifier::new("record-one"),
                name: DomainName::new("goldragon.criome"),
                kind: RecordKind::AddressV4,
                value: RecordValue::new("203.0.113.7"),
                proxy_mode: ProxyMode::ProviderProxy,
            }]),
            queried_zones: Mutex::new(Vec::new()),
            queried_record_zones: Mutex::new(Vec::new()),
        }
    }

    fn queried_record_zones(&self) -> Vec<String> {
        self.queried_record_zones
            .lock()
            .expect("queried record zones")
            .clone()
    }
}

#[cfg(feature = "cloudflare")]
impl CloudflareApi for FixtureCloudflareApi {
    fn zones(
        &self,
        _token: &Token,
        name: Option<&DomainName>,
    ) -> cloud::cloudflare::Result<Vec<ApiZone>> {
        self.queried_zones
            .lock()
            .expect("queried zones")
            .push(name.map(|name| name.as_str().to_owned()));
        Ok(self
            .zones
            .iter()
            .filter(|zone| name.is_none_or(|name| zone.name == *name))
            .cloned()
            .collect())
    }

    fn records(
        &self,
        _token: &Token,
        zone: &ZoneIdentifier,
    ) -> cloud::cloudflare::Result<Vec<ApiRecord>> {
        self.queried_record_zones
            .lock()
            .expect("queried record zones")
            .push(zone.as_str().to_owned());
        Ok(self.records.lock().expect("records").clone())
    }

    fn create_record(
        &self,
        _token: &Token,
        _zone: &ZoneIdentifier,
        record: &DomainNameSystemRecord,
    ) -> cloud::cloudflare::Result<ApiRecord> {
        let mut records = self.records.lock().expect("records");
        let record = ApiRecord {
            identifier: RecordIdentifier::new(format!("record-{}", records.len() + 1)),
            name: record.name.clone(),
            kind: record.kind,
            value: record.value.clone(),
            proxy_mode: record.proxy_mode,
        };
        records.push(record.clone());
        Ok(record)
    }

    fn update_record(
        &self,
        _token: &Token,
        _zone: &ZoneIdentifier,
        identifier: &RecordIdentifier,
        record: &DomainNameSystemRecord,
    ) -> cloud::cloudflare::Result<ApiRecord> {
        let mut records = self.records.lock().expect("records");
        let Some(existing) = records
            .iter_mut()
            .find(|record| record.identifier == *identifier)
        else {
            return Err(cloud::cloudflare::Error::RequestRejected(format!(
                "record {} not found",
                identifier.as_str()
            )));
        };
        existing.name = record.name.clone();
        existing.kind = record.kind;
        existing.value = record.value.clone();
        existing.proxy_mode = record.proxy_mode;
        Ok(existing.clone())
    }

    fn delete_record(
        &self,
        _token: &Token,
        _zone: &ZoneIdentifier,
        identifier: &RecordIdentifier,
    ) -> cloud::cloudflare::Result<()> {
        self.records
            .lock()
            .expect("records")
            .retain(|record| record.identifier != *identifier);
        Ok(())
    }
}

#[cfg(feature = "cloudflare")]
fn cloudflare_fixture_store(
    credentials: FixtureCredentialSource,
) -> (Store, Arc<FixtureCloudflareApi>) {
    let api = Arc::new(FixtureCloudflareApi::new());
    let cloudflare = cloud::cloudflare::ProviderClient::new(api.clone(), Arc::new(credentials));
    (Store::with_cloudflare_provider(cloudflare), api)
}

#[cfg(feature = "cloudflare")]
fn configure_cloudflare_account(store: &Store, credential: &str) {
    let registration = MetaOperation::RegisterAccount(Registration {
        provider: Provider::Cloudflare,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new(credential),
    })
    .into_request();
    assert!(matches!(
        store.handle_meta_request(registration),
        FrameReply::Accepted { .. }
    ));

    let policy = MetaOperation::SetPolicy(Policy {
        zones: vec![ZonePolicy {
            provider: Provider::Cloudflare,
            account: ProviderAccount::new("primary"),
            allowed_zones: vec![DomainName::new("goldragon.criome")],
        }],
        capabilities: vec![CapabilityPolicy {
            provider: Provider::Cloudflare,
            account: ProviderAccount::new("primary"),
            capability: Capability::DomainNameSystemRecords,
            directive: CapabilityDirective::Enable,
        }],
    })
    .into_request();
    assert!(matches!(
        store.handle_meta_request(policy),
        FrameReply::Accepted { .. }
    ));
}

fn capability_report_from_schema_stream(
    client_stream: &mut UnixStream,
    provider: ordinary_schema::Provider,
    capability: ordinary_schema::Capability,
) -> ordinary_schema::CapabilityReport {
    let output = SchemaConnection::new(client_stream)
        .exchange_working(ordinary_schema::Input::Observe(
            ordinary_schema::Observation::Capabilities(ordinary_schema::CapabilityQuery {
                provider: Some(provider),
                capability: Some(capability),
            }),
        ))
        .expect("schema request");
    match output {
        ordinary_schema::Output::Observed(ordinary_schema::ObservationResult::Capabilities(
            report,
        )) => report,
        other => panic!("unexpected schema output {other:?}"),
    }
}

#[test]
fn daemon_configuration_accepts_binary_file_argument() {
    let directory = tempfile::tempdir().expect("temp dir");
    let configuration_path = directory.path().join("cloud-daemon.rkyv");
    let configuration = daemon_configuration(directory.path());

    CloudDaemonConfigurationFile::new(&configuration_path)
        .write_configuration(&configuration)
        .expect("write cloud daemon configuration");

    let decoded = CloudDaemonCommand::from_arguments([configuration_path.display().to_string()])
        .configuration()
        .expect("read cloud daemon configuration");

    assert_eq!(decoded, configuration);
}

#[test]
fn daemon_configuration_rejects_nota_arguments() {
    let directory = tempfile::tempdir().expect("temp dir");
    let nota_path = directory.path().join("cloud-daemon.nota");
    std::fs::write(&nota_path, "(DaemonConfiguration)").expect("write nota fixture");

    let inline = CloudDaemonCommand::from_arguments(["(DaemonConfiguration)"])
        .configuration()
        .expect_err("inline NOTA is rejected");
    let file = CloudDaemonCommand::from_arguments([nota_path.display().to_string()])
        .configuration()
        .expect_err(".nota file is rejected");

    assert!(matches!(inline, cloud::Error::Argument(_)));
    assert!(matches!(file, cloud::Error::Argument(_)));
}

#[test]
fn daemon_process_starts_from_binary_configuration_and_answers_working_request() {
    let directory = tempfile::tempdir().expect("temp dir");
    let configuration_path = directory.path().join("cloud-daemon.rkyv");
    let configuration = daemon_configuration(directory.path());

    CloudDaemonConfigurationFile::new(&configuration_path)
        .write_configuration(&configuration)
        .expect("write cloud daemon configuration");

    let mut child = Command::new(env!("CARGO_BIN_EXE_cloud-daemon"))
        .arg(&configuration_path)
        .spawn()
        .expect("cloud-daemon starts");

    let ordinary_socket = directory.path().join("cloud.sock");
    wait_for_socket(&ordinary_socket);
    let mut stream = UnixStream::connect(&ordinary_socket).expect("client connects");
    let report = capability_report_from_schema_stream(
        &mut stream,
        ordinary_schema::Provider::Cloudflare,
        ordinary_schema::Capability::DomainNameSystemRecords,
    );
    let expected_state = if cfg!(feature = "cloudflare") {
        ordinary_schema::CapabilityState::Compiled
    } else {
        ordinary_schema::CapabilityState::NotBuilt
    };
    assert_eq!(report.payload().len(), 1);
    assert_eq!(report.payload()[0].capability_state, expected_state);

    stop_child(&mut child);
}

#[test]
fn command_line_dispatch_routes_working_and_meta_heads() {
    let dispatch = CommandLineDispatch::new();

    assert_eq!(
        dispatch.route_head("Observe").expect("working head"),
        CommandLineSocket::Working
    );
    assert_eq!(
        dispatch.route_head("RegisterAccount").expect("meta head"),
        CommandLineSocket::Meta
    );
    assert_eq!(
        dispatch.route_head("PreparePlan").expect("meta head"),
        CommandLineSocket::Meta
    );
    assert_eq!(
        dispatch.route_head("PrepareProjection").expect("meta head"),
        CommandLineSocket::Meta
    );
}

#[test]
fn command_line_request_rejects_flags_and_extra_arguments() {
    assert!(matches!(
        CliRequest::from_arguments(["--help"]),
        Err(cloud::Error::FlagArgument(_))
    ));
    assert!(matches!(
        CliRequest::from_arguments(["(Observe (Capabilities None None))", "extra"]),
        Err(cloud::Error::ExpectedSingleArgument)
    ));
}

#[test]
fn command_line_request_decodes_meta_contract_by_head() {
    let operation = MetaOperation::RegisterAccount(Registration {
        provider: Provider::Cloudflare,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new("cloudflare-dns-token"),
    });
    let text = encode_to_text(&operation);

    match CliRequest::from_nota(&text).expect("meta request") {
        CliRequest::Meta(meta_schema::Input::RegisterAccount(decoded)) => {
            assert_eq!(decoded.provider, meta_schema::Provider::Cloudflare);
            assert_eq!(decoded.provider_account, "primary");
            assert_eq!(decoded.credential_handle, "cloudflare-dns-token");
        }
        other => panic!("expected meta request, got {other:?}"),
    }
}

#[test]
fn store_answers_schema_capability_observation_through_provider_logic() {
    let output = Store::new().handle_schema_ordinary_input(ordinary_schema::Input::Observe(
        ordinary_schema::Observation::Capabilities(ordinary_schema::CapabilityQuery {
            provider: Some(ordinary_schema::Provider::Cloudflare),
            capability: Some(ordinary_schema::Capability::DomainNameSystemRecords),
        }),
    ));
    let ordinary_schema::Output::Observed(ordinary_schema::ObservationResult::Capabilities(report)) =
        output
    else {
        panic!("unexpected schema output {output:?}");
    };
    let expected_state = if cfg!(feature = "cloudflare") {
        ordinary_schema::CapabilityState::Compiled
    } else {
        ordinary_schema::CapabilityState::NotBuilt
    };

    assert_eq!(report.payload().len(), 1);
    assert_eq!(report.payload()[0].capability_state, expected_state);
}

#[test]
#[cfg(not(feature = "google-cloud"))]
fn store_reports_not_built_provider_over_schema_input() {
    let output = Store::new().handle_schema_ordinary_input(ordinary_schema::Input::Observe(
        ordinary_schema::Observation::Capabilities(ordinary_schema::CapabilityQuery {
            provider: Some(ordinary_schema::Provider::GoogleCloud),
            capability: Some(ordinary_schema::Capability::DomainNameSystemRecords),
        }),
    ));
    let ordinary_schema::Output::Observed(ordinary_schema::ObservationResult::Capabilities(report)) =
        output
    else {
        panic!("unexpected schema output {output:?}");
    };

    assert_eq!(report.payload().len(), 1);
    assert_eq!(
        report.payload()[0].capability_state,
        ordinary_schema::CapabilityState::NotBuilt
    );
}

#[test]
#[cfg(not(feature = "google-cloud"))]
fn not_built_provider_dispatch_is_never_configured() {
    let store = Store::new();
    let registration = MetaOperation::RegisterAccount(Registration {
        provider: Provider::GoogleCloud,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new("google-cloud-dns-token"),
    })
    .into_request();
    assert!(matches!(
        store.handle_meta_request(registration),
        FrameReply::Accepted { .. } | FrameReply::Rejected { .. }
    ));

    let request = CloudOperation::Observe(Observation::Records(signal_cloud::RecordQuery {
        provider: Provider::GoogleCloud,
        zone: DomainName::new("goldragon.criome"),
    }))
    .into_request();

    match store.handle_ordinary_request(request) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(CloudReply::RequestUnsupported(RequestUnsupported {
                provider,
                capability,
                reason,
                ..
            })) => {
                assert_eq!(provider, Some(Provider::GoogleCloud));
                assert_eq!(capability, Some(Capability::DomainNameSystemRecords));
                assert_eq!(reason, UnsupportedReason::ProviderNotBuilt);
            }
            other => panic!("unexpected observe reply {other:?}"),
        },
        other => panic!("unexpected frame reply {other:?}"),
    }
}

#[test]
#[cfg(feature = "cloudflare")]
fn cloudflare_record_observation_uses_provider_actor_and_caches_last_known_state() {
    let (store, api) = cloudflare_fixture_store(FixtureCredentialSource::available());
    configure_cloudflare_account(&store, "CLOUDFLARE_DNS_TOKEN");

    let request = CloudOperation::Observe(Observation::Records(signal_cloud::RecordQuery {
        provider: Provider::Cloudflare,
        zone: DomainName::new("goldragon.criome"),
    }))
    .into_request();

    let listing = match store.handle_ordinary_request(request) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(CloudReply::Observed(signal_cloud::ObservationResult::Records(
                listing,
            ))) => listing,
            other => panic!("unexpected observe reply {other:?}"),
        },
        other => panic!("unexpected frame reply {other:?}"),
    };

    let expected = RecordListing {
        records: vec![DomainNameSystemRecord {
            name: DomainName::new("goldragon.criome"),
            kind: RecordKind::AddressV4,
            value: RecordValue::new("203.0.113.7"),
            proxy_mode: ProxyMode::ProviderProxy,
        }],
    };
    assert_eq!(listing, expected);
    assert_eq!(api.queried_record_zones(), vec!["zone-one".to_owned()]);
    assert_eq!(
        store
            .last_known_records(Provider::Cloudflare, &DomainName::new("goldragon.criome"))
            .expect("last known records"),
        expected
    );
}

#[test]
#[cfg(feature = "cloudflare")]
fn cloudflare_record_observation_requires_credential_environment() {
    let (store, api) = cloudflare_fixture_store(FixtureCredentialSource::missing());
    let registration = MetaOperation::RegisterAccount(Registration {
        provider: Provider::Cloudflare,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new("CLOUDFLARE_DNS_TOKEN"),
    })
    .into_request();
    match store.handle_meta_request(registration) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::RequestRejected(rejection)) => assert_eq!(
                rejection.reason,
                meta_signal_cloud::RejectionReason::CredentialHandleUnknown
            ),
            other => panic!("unexpected registration reply {other:?}"),
        },
        other => panic!("unexpected registration frame reply {other:?}"),
    }

    let request = CloudOperation::Observe(Observation::Records(signal_cloud::RecordQuery {
        provider: Provider::Cloudflare,
        zone: DomainName::new("goldragon.criome"),
    }))
    .into_request();

    match store.handle_ordinary_request(request) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(CloudReply::RequestUnsupported(RequestUnsupported {
                provider,
                capability,
                reason,
            })) => {
                assert_eq!(provider, Some(Provider::Cloudflare));
                assert_eq!(capability, Some(Capability::DomainNameSystemRecords));
                assert_eq!(reason, UnsupportedReason::ProviderNotConfigured);
            }
            other => panic!("unexpected observe reply {other:?}"),
        },
        other => panic!("unexpected frame reply {other:?}"),
    }

    assert!(api.queried_record_zones().is_empty());
    assert!(
        store
            .last_known_records(Provider::Cloudflare, &DomainName::new("goldragon.criome"))
            .is_none()
    );
}

#[test]
#[cfg(feature = "cloudflare")]
fn validation_reports_malformed_dns_record_values() {
    let (store, _api) = cloudflare_fixture_store(FixtureCredentialSource::available());
    configure_cloudflare_account(&store, "CLOUDFLARE_DNS_TOKEN");

    let request = CloudOperation::Validate(signal_cloud::Validation {
        desired_state: signal_cloud::DesiredState {
            provider: Provider::Cloudflare,
            zone: DomainName::new("goldragon.criome"),
            records: vec![DomainNameSystemRecord {
                name: DomainName::new("goldragon.criome"),
                kind: RecordKind::AddressV4,
                value: RecordValue::new("not-an-ip"),
                proxy_mode: ProxyMode::Direct,
            }],
            redirects: vec![],
        },
    })
    .into_request();

    match store.handle_ordinary_request(request) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(CloudReply::Validated(report)) => {
                assert_eq!(report.findings.len(), 1);
                assert_eq!(
                    report.findings[0].severity,
                    signal_cloud::FindingSeverity::Error
                );
            }
            other => panic!("unexpected validation reply {other:?}"),
        },
        other => panic!("unexpected validation frame reply {other:?}"),
    }
}

#[test]
#[cfg(feature = "cloudflare")]
fn meta_policy_allows_approved_dns_plan_application() {
    let (store, _api) = cloudflare_fixture_store(FixtureCredentialSource::available());
    configure_cloudflare_account(&store, "CLOUDFLARE_DNS_TOKEN");

    let plan_request = MetaOperation::PreparePlan(PlanPreparation {
        desired_state: signal_cloud::DesiredState {
            provider: Provider::Cloudflare,
            zone: DomainName::new("goldragon.criome"),
            records: vec![
                DomainNameSystemRecord {
                    name: DomainName::new("goldragon.criome"),
                    kind: RecordKind::AddressV4,
                    value: RecordValue::new("203.0.113.8"),
                    proxy_mode: ProxyMode::ProviderProxy,
                },
                DomainNameSystemRecord {
                    name: DomainName::new("www.goldragon.criome"),
                    kind: RecordKind::CanonicalName,
                    value: RecordValue::new("goldragon.criome"),
                    proxy_mode: ProxyMode::ProviderProxy,
                },
            ],
            redirects: Vec::new(),
        },
    })
    .into_request();
    let plan = match store.handle_meta_request(plan_request) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::PlanPrepared(plan)) => plan,
            other => panic!("unexpected plan reply {other:?}"),
        },
        other => panic!("unexpected plan frame reply {other:?}"),
    };
    assert_eq!(plan.records_to_create.len(), 1);
    assert_eq!(plan.records_to_update.len(), 1);
    assert!(plan.record_names_to_delete.is_empty());

    let approval = MetaOperation::ApprovePlan(meta_signal_cloud::Approval {
        plan: plan.identifier.clone(),
    })
    .into_request();
    assert!(matches!(
        store.handle_meta_request(approval),
        FrameReply::Accepted { .. }
    ));

    let application = MetaOperation::ApplyPlan(meta_signal_cloud::Application {
        plan: plan.identifier,
    })
    .into_request();
    match store.handle_meta_request(application) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::PlanApplied(_)) => {}
            other => panic!("unexpected apply reply {other:?}"),
        },
        other => panic!("unexpected apply frame reply {other:?}"),
    }

    let expected = RecordListing {
        records: vec![
            DomainNameSystemRecord {
                name: DomainName::new("goldragon.criome"),
                kind: RecordKind::AddressV4,
                value: RecordValue::new("203.0.113.8"),
                proxy_mode: ProxyMode::ProviderProxy,
            },
            DomainNameSystemRecord {
                name: DomainName::new("www.goldragon.criome"),
                kind: RecordKind::CanonicalName,
                value: RecordValue::new("goldragon.criome"),
                proxy_mode: ProxyMode::ProviderProxy,
            },
        ],
    };
    assert_eq!(
        store
            .last_known_records(Provider::Cloudflare, &DomainName::new("goldragon.criome"))
            .expect("last known records"),
        expected
    );
}

#[test]
#[cfg(feature = "cloudflare")]
fn domain_projection_prepares_and_applies_cloudflare_dns_plan() {
    let (store, _api) = cloudflare_fixture_store(FixtureCredentialSource::available());
    configure_cloudflare_account(&store, "CLOUDFLARE_DNS_TOKEN");

    let plan_request = MetaOperation::PrepareProjection(ProjectionPreparation {
        provider: Provider::Cloudflare,
        projection: Projection {
            query: ProjectionQuery {
                domain: signal_domain_criome::DomainName::new("goldragon.criome"),
                scope: ProjectionScope::PublicRecords,
            },
            records: vec![signal_domain_criome::DomainNameSystemRecord {
                name: signal_domain_criome::DomainName::new("goldragon.criome"),
                kind: signal_domain_criome::RecordKind::AddressV4,
                value: signal_domain_criome::RecordValue::new("203.0.113.10"),
            }],
            redirects: vec![],
        },
    })
    .into_request();
    let plan = match store.handle_meta_request(plan_request) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::PlanPrepared(plan)) => plan,
            other => panic!("unexpected projection plan reply {other:?}"),
        },
        other => panic!("unexpected projection plan frame reply {other:?}"),
    };
    assert_eq!(plan.records_to_create.len(), 0);
    assert_eq!(plan.records_to_update.len(), 1);
    assert_eq!(plan.records_to_update[0].proxy_mode, ProxyMode::Direct);

    let approval = MetaOperation::ApprovePlan(meta_signal_cloud::Approval {
        plan: plan.identifier.clone(),
    })
    .into_request();
    assert!(matches!(
        store.handle_meta_request(approval),
        FrameReply::Accepted { .. }
    ));

    let application = MetaOperation::ApplyPlan(meta_signal_cloud::Application {
        plan: plan.identifier,
    })
    .into_request();
    match store.handle_meta_request(application) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::PlanApplied(_)) => {}
            other => panic!("unexpected projection apply reply {other:?}"),
        },
        other => panic!("unexpected projection apply frame reply {other:?}"),
    }

    let expected = RecordListing {
        records: vec![DomainNameSystemRecord {
            name: DomainName::new("goldragon.criome"),
            kind: RecordKind::AddressV4,
            value: RecordValue::new("203.0.113.10"),
            proxy_mode: ProxyMode::Direct,
        }],
    };
    assert_eq!(
        store
            .last_known_records(Provider::Cloudflare, &DomainName::new("goldragon.criome"))
            .expect("last known records"),
        expected
    );
}

#[test]
#[cfg(feature = "cloudflare")]
fn store_answers_schema_meta_registration_through_provider_logic() {
    let (store, _api) = cloudflare_fixture_store(FixtureCredentialSource::available());
    let output = store.handle_schema_meta_input(meta_schema::Input::RegisterAccount(
        meta_schema::Registration {
            provider: meta_schema::Provider::Cloudflare,
            provider_account: String::from("primary"),
            credential_handle: String::from("cloudflare-dns-token"),
        },
    ));
    match output {
        meta_schema::Output::AccountRegistered(registered) => {
            assert_eq!(registered.provider, meta_schema::Provider::Cloudflare);
            assert_eq!(registered.provider_account, "primary");
        }
        other => panic!("unexpected schema meta output {other:?}"),
    }
}

#[test]
fn runtime_slice_does_not_reintroduce_signal_core_or_provider_access_in_cli() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("manifest");
    assert!(!manifest.contains("signal-core"));
    assert!(!Path::new("src/daemon.rs").exists());
    assert!(!Path::new("src/frame_io.rs").exists());

    let client = std::fs::read_to_string("src/client.rs").expect("client source");
    assert!(!client.contains("reqwest"));
    assert!(!client.contains("ureq"));
    assert!(!client.contains("Cloudflare"));
    assert!(!client.contains("CredentialHandle"));
    assert!(!client.contains("ExchangeFrameBody"));
    assert!(!client.contains("Handshake"));
}

fn daemon_configuration(directory: &Path) -> DaemonConfiguration {
    DaemonConfiguration {
        ordinary_socket_path: directory.join("cloud.sock").display().to_string(),
        ordinary_socket_mode: 0o600,
        meta_socket_path: directory.join("cloud-meta.sock").display().to_string(),
        meta_socket_mode: 0o600,
    }
}

fn wait_for_socket(socket: &Path) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if socket.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("socket was not created: {}", socket.display());
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
