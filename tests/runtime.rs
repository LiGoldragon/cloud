use std::os::unix::net::UnixStream;
use std::thread;

use cloud::Store;
use cloud::client::{CliRequest, CommandLineDispatch};
use cloud::daemon::Daemon;
use cloud::frame_io::{OrdinaryFrameIo, OwnerFrameIo};
use nota_codec::NotaEncode;
use owner_signal_cloud::{
    CapabilityDirective, CapabilityPolicy, CredentialHandle, Operation as OwnerOperation,
    PlanPreparation, Policy, Registration, Reply as OwnerReply, ZonePolicy,
};
use signal_cloud::{
    Capability, CapabilityState, DomainName, Observation, Operation as CloudOperation, Provider,
    ProviderAccount, Reply as CloudReply,
};
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
    let request = OwnerOperation::RegisterAccount(Registration {
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
    let store = Store::new();
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
            provider: Some(Provider::Cloudflare),
            capability: Some(Capability::DomainNameSystemRecords),
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
                )) => {
                    assert_eq!(report.capabilities.len(), 1);
                    assert_eq!(report.capabilities[0].state, CapabilityState::Compiled);
                }
                other => panic!("unexpected reply {other:?}"),
            }
        }
        other => panic!("unexpected frame {other:?}"),
    }
}

#[test]
fn owner_policy_enables_planning_but_apply_requires_provider_authority() {
    let store = Store::new();

    let registration = OwnerOperation::RegisterAccount(Registration {
        provider: Provider::Cloudflare,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new("cloudflare-dns-token"),
    })
    .into_request();
    match store.handle_owner_request(registration) {
        FrameReply::Accepted { per_operation, .. } => {
            assert!(matches!(
                per_operation.head(),
                SubReply::Ok(OwnerReply::AccountRegistered(_))
            ));
        }
        other => panic!("unexpected registration reply {other:?}"),
    }

    let policy = OwnerOperation::SetPolicy(Policy {
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

    let plan_request = OwnerOperation::PreparePlan(PlanPreparation {
        desired_state: signal_cloud::DesiredState {
            provider: Provider::Cloudflare,
            zone: DomainName::new("goldragon.criome"),
            records: Vec::new(),
            redirects: Vec::new(),
        },
    })
    .into_request();
    let plan = match store.handle_owner_request(plan_request) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(OwnerReply::PlanPrepared(plan)) => plan,
            other => panic!("unexpected plan reply {other:?}"),
        },
        other => panic!("unexpected plan frame reply {other:?}"),
    };

    let approval = OwnerOperation::ApprovePlan(owner_signal_cloud::Approval {
        plan: plan.identifier.clone(),
    })
    .into_request();
    assert!(matches!(
        store.handle_owner_request(approval),
        FrameReply::Accepted { .. }
    ));

    let application = OwnerOperation::ApplyPlan(owner_signal_cloud::Application {
        plan: plan.identifier,
    })
    .into_request();
    match store.handle_owner_request(application) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(OwnerReply::RequestRejected(rejection)) => {
                assert_eq!(
                    rejection.reason,
                    owner_signal_cloud::RejectionReason::CapabilityUnauthorized
                );
            }
            other => panic!("unexpected apply reply {other:?}"),
        },
        other => panic!("unexpected apply frame reply {other:?}"),
    }
}

#[test]
fn daemon_answers_owner_registration_over_frame_socket() {
    let store = Store::new();
    let (mut client_stream, mut daemon_stream) = UnixStream::pair().expect("socket pair");

    thread::spawn(move || {
        Daemon::serve_owner_stream(&store, &mut daemon_stream).expect("daemon serves");
    });

    let handshake = owner_signal_cloud::Frame::new(ExchangeFrameBody::HandshakeRequest(
        HandshakeRequest::current(),
    ));
    OwnerFrameIo::write(&mut client_stream, &handshake).expect("write handshake");
    let handshake_reply = OwnerFrameIo::read(&mut client_stream).expect("read handshake");
    assert!(matches!(
        handshake_reply.into_body(),
        ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_))
    ));

    let operation = OwnerOperation::RegisterAccount(Registration {
        provider: Provider::Cloudflare,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new("cloudflare-dns-token"),
    });
    let exchange = exchange();
    let request = operation.into_request();
    let frame = owner_signal_cloud::Frame::new(ExchangeFrameBody::Request { exchange, request });
    OwnerFrameIo::write(&mut client_stream, &frame).expect("write request");

    let reply = OwnerFrameIo::read(&mut client_stream).expect("read reply");
    match reply.into_body() {
        ExchangeFrameBody::Reply {
            exchange: reply_exchange,
            reply: FrameReply::Accepted { per_operation, .. },
        } => {
            assert_eq!(reply_exchange, exchange);
            assert!(matches!(
                per_operation.head(),
                SubReply::Ok(OwnerReply::AccountRegistered(_))
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
