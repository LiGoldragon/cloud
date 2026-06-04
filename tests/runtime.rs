use std::os::unix::net::UnixStream;
#[cfg(feature = "cloudflare")]
use std::sync::{Arc, Mutex};
use std::thread;

use cloud::Store;
use cloud::client::{CliRequest, CommandLineDispatch};
#[cfg(feature = "cloudflare")]
use cloud::cloudflare::{
    Api as CloudflareApi, ApiRecord, ApiZone, CredentialSource, RecordIdentifier, Token,
};
use cloud::daemon::Daemon;
use cloud::frame_io::{MetaFrameIo, OrdinaryFrameIo};
#[cfg(feature = "cloudflare")]
use meta_signal_cloud::{
    CapabilityDirective, CapabilityPolicy, PlanPreparation, Policy, ProjectionPreparation,
    ZonePolicy,
};
use meta_signal_cloud::{
    CredentialHandle, Operation as MetaOperation, Registration, Reply as MetaReply,
};
use nota_codec::NotaEncode;
use signal_cloud::{
    Capability, CapabilityReport, CapabilityState, DomainName, Observation,
    Operation as CloudOperation, Provider, ProviderAccount, Reply as CloudReply,
    RequestUnsupported, UnsupportedReason,
};
#[cfg(feature = "cloudflare")]
use signal_cloud::{
    DomainNameSystemRecord, ProxyMode, RecordKind, RecordListing, RecordValue, ZoneIdentifier,
};
use signal_domain_criome::{Projection, ProjectionQuery, ProjectionScope};
use signal_frame::{
    CommandLineSocket, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, HandshakeReply,
    HandshakeRequest, LaneSequence, Reply as FrameReply, RequestPayload, SessionEpoch, SubReply,
};

fn encode_to_text(value: &impl NotaEncode) -> String {
    let mut encoder = nota_codec::Encoder::new();
    value.encode(&mut encoder).expect("encode");
    encoder.into_string()
}

fn exchange() -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    )
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
        store.handle_owner_request(registration),
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
        store.handle_owner_request(policy),
        FrameReply::Accepted { .. }
    ));
}

fn capability_report_over_socket(
    store: Store,
    provider: Provider,
    capability: Capability,
) -> CapabilityReport {
    let (mut client_stream, mut daemon_stream) = UnixStream::pair().expect("socket pair");

    thread::spawn(move || {
        Daemon::serve_ordinary_stream(&store, &mut daemon_stream).expect("daemon serves");
    });

    let handshake = signal_cloud::Frame::new(ExchangeFrameBody::HandshakeRequest(
        HandshakeRequest::current(),
    ));
    OrdinaryFrameIo::write(&mut client_stream, &handshake).expect("write handshake");
    let handshake_reply = OrdinaryFrameIo::read(&mut client_stream).expect("read handshake");
    assert!(matches!(
        handshake_reply.into_body(),
        ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_))
    ));

    let operation =
        CloudOperation::Observe(Observation::Capabilities(signal_cloud::CapabilityQuery {
            provider: Some(provider),
            capability: Some(capability),
        }));
    let exchange = exchange();
    let request = operation.into_request();
    let frame = signal_cloud::Frame::new(ExchangeFrameBody::Request { exchange, request });
    OrdinaryFrameIo::write(&mut client_stream, &frame).expect("write request");

    let reply = OrdinaryFrameIo::read(&mut client_stream).expect("read reply");
    match reply.into_body() {
        ExchangeFrameBody::Reply {
            exchange: reply_exchange,
            reply: FrameReply::Accepted { per_operation, .. },
        } => {
            assert_eq!(reply_exchange, exchange);
            let (head, tail) = per_operation.into_head_and_tail();
            assert!(tail.is_empty());
            match head {
                SubReply::Ok(CloudReply::Observed(
                    signal_cloud::ObservationResult::Capabilities(report),
                )) => report,
                other => panic!("unexpected reply {other:?}"),
            }
        }
        other => panic!("unexpected frame {other:?}"),
    }
}

#[test]
fn command_line_dispatch_routes_working_and_owner_heads() {
    let dispatch = CommandLineDispatch::new();

    assert_eq!(
        dispatch.route_head("Observe").expect("working head"),
        CommandLineSocket::Working
    );
    assert_eq!(
        dispatch.route_head("RegisterAccount").expect("owner head"),
        CommandLineSocket::Owner
    );
    assert_eq!(
        dispatch.route_head("PreparePlan").expect("owner head"),
        CommandLineSocket::Owner
    );
    assert_eq!(
        dispatch
            .route_head("PrepareProjection")
            .expect("owner head"),
        CommandLineSocket::Owner
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
fn command_line_request_decodes_owner_contract_by_head() {
    let request = MetaOperation::RegisterAccount(Registration {
        provider: Provider::Cloudflare,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new("cloudflare-dns-token"),
    })
    .into_request();
    let text = encode_to_text(&request);

    match CliRequest::from_nota(&text).expect("owner request") {
        CliRequest::Owner(decoded) => assert_eq!(decoded, request),
        other => panic!("expected owner request, got {other:?}"),
    }
}

#[test]
fn daemon_answers_working_capability_observation_over_frame_socket() {
    let report = capability_report_over_socket(
        Store::new(),
        Provider::Cloudflare,
        Capability::DomainNameSystemRecords,
    );
    let expected_state = if cfg!(feature = "cloudflare") {
        CapabilityState::Compiled
    } else {
        CapabilityState::NotBuilt
    };

    assert_eq!(report.capabilities.len(), 1);
    assert_eq!(report.capabilities[0].state, expected_state);
}

#[test]
#[cfg(not(feature = "google-cloud"))]
fn daemon_reports_not_built_provider_over_frame_socket() {
    let report = capability_report_over_socket(
        Store::new(),
        Provider::GoogleCloud,
        Capability::DomainNameSystemRecords,
    );

    assert_eq!(report.capabilities.len(), 1);
    assert_eq!(report.capabilities[0].state, CapabilityState::NotBuilt);
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
        store.handle_owner_request(registration),
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
    match store.handle_owner_request(registration) {
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
fn owner_policy_allows_approved_dns_plan_application() {
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
    let plan = match store.handle_owner_request(plan_request) {
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
        store.handle_owner_request(approval),
        FrameReply::Accepted { .. }
    ));

    let application = MetaOperation::ApplyPlan(meta_signal_cloud::Application {
        plan: plan.identifier,
    })
    .into_request();
    match store.handle_owner_request(application) {
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
    let plan = match store.handle_owner_request(plan_request) {
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
        store.handle_owner_request(approval),
        FrameReply::Accepted { .. }
    ));

    let application = MetaOperation::ApplyPlan(meta_signal_cloud::Application {
        plan: plan.identifier,
    })
    .into_request();
    match store.handle_owner_request(application) {
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
fn daemon_answers_owner_registration_over_frame_socket() {
    let (store, _api) = cloudflare_fixture_store(FixtureCredentialSource::available());
    let (mut client_stream, mut daemon_stream) = UnixStream::pair().expect("socket pair");

    thread::spawn(move || {
        Daemon::serve_owner_stream(&store, &mut daemon_stream).expect("daemon serves");
    });

    let handshake = meta_signal_cloud::Frame::new(ExchangeFrameBody::HandshakeRequest(
        HandshakeRequest::current(),
    ));
    MetaFrameIo::write(&mut client_stream, &handshake).expect("write handshake");
    let handshake_reply = MetaFrameIo::read(&mut client_stream).expect("read handshake");
    assert!(matches!(
        handshake_reply.into_body(),
        ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_))
    ));

    let operation = MetaOperation::RegisterAccount(Registration {
        provider: Provider::Cloudflare,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new("cloudflare-dns-token"),
    });
    let exchange = exchange();
    let request = operation.into_request();
    let frame = meta_signal_cloud::Frame::new(ExchangeFrameBody::Request { exchange, request });
    MetaFrameIo::write(&mut client_stream, &frame).expect("write request");

    let reply = MetaFrameIo::read(&mut client_stream).expect("read reply");
    match reply.into_body() {
        ExchangeFrameBody::Reply {
            exchange: reply_exchange,
            reply: FrameReply::Accepted { per_operation, .. },
        } => {
            assert_eq!(reply_exchange, exchange);
            assert!(matches!(
                per_operation.head(),
                SubReply::Ok(MetaReply::AccountRegistered(_))
            ));
        }
        other => panic!("unexpected frame {other:?}"),
    }
}

#[test]
fn runtime_slice_does_not_reintroduce_signal_core_or_provider_access_in_cli() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("manifest");
    assert!(!manifest.contains("signal-core"));

    let client = std::fs::read_to_string("src/client.rs").expect("client source");
    assert!(!client.contains("reqwest"));
    assert!(!client.contains("ureq"));
    assert!(!client.contains("Cloudflare"));
    assert!(!client.contains("CredentialHandle"));
}
