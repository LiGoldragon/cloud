use cloud::schema::{nexus, sema};
use cloud::schema_runtime::SchemaRuntime;
use meta_signal_cloud::schema::lib as meta;
use signal_cloud::schema::lib as ordinary;

use nexus::NexusEngine;
use sema::SemaEngine;

#[test]
fn ordinary_capability_observation_flows_through_generated_nexus_and_sema() {
    let mut runtime = SchemaRuntime::new();
    let input = ordinary::Input::observe(ordinary::Observation::capabilities(
        ordinary::CapabilityQuery {
            provider: Some(ordinary::Provider::Cloudflare),
            capability: Some(ordinary::Capability::DomainNameSystemRecords),
        },
    ));

    let nexus_action = runtime
        .decide(
            nexus::NexusWork::signal_arrived(nexus::SignalInput::ordinary_input(input.clone()))
                .with_origin_route(nexus::OriginRoute::new(7)),
        )
        .into_root();

    let sema_input = match nexus_action {
        nexus::NexusAction::CommandSemaRead(command) => command.into_payload(),
        other => panic!("expected SEMA read command, got {other:?}"),
    };

    let sema_output = runtime
        .observe(sema_input.with_origin_route(sema::OriginRoute::new(7)))
        .into_root();

    let report = match sema_output.clone() {
        sema::SemaReadOutput::Observed(observed) => match observed.into_payload() {
            ordinary::ObservationResult::Capabilities(report) => report,
            other => panic!("expected capability report, got {other:?}"),
        },
        other => panic!("expected capability report, got {other:?}"),
    };
    assert_eq!(report.payload().len(), 1);
    assert_eq!(
        report.payload()[0].capability_state,
        ordinary::CapabilityState::Compiled
    );

    let reply = runtime
        .decide(
            nexus::NexusWork::sema_read_completed(sema_output)
                .with_origin_route(nexus::OriginRoute::new(8)),
        )
        .into_root();

    match reply {
        nexus::NexusAction::ReplyToSignal(reply) => match reply.into_payload() {
            nexus::SignalOutput::OrdinaryOutput(ordinary::Output::Observed(observed)) => {
                match observed {
                    ordinary::ObservationResult::Capabilities(report) => {
                        assert_eq!(report.payload().len(), 1);
                    }
                    other => panic!("expected capability report, got {other:?}"),
                }
            }
            other => panic!("expected ordinary signal reply, got {other:?}"),
        },
        other => panic!("expected ordinary signal reply, got {other:?}"),
    }
}

#[test]
fn meta_registration_flows_through_generated_nexus_and_sema() {
    let mut runtime = SchemaRuntime::new();
    let registration = meta::Registration {
        provider: meta::Provider::Cloudflare,
        provider_account: meta::ProviderAccount::new("primary"),
        credential_handle: meta::CredentialHandle::new("cloudflare/api-token"),
    };

    let nexus_action = runtime
        .decide(
            nexus::NexusWork::signal_arrived(nexus::SignalInput::meta_input(
                meta::Input::register_account(registration.clone()),
            ))
            .with_origin_route(nexus::OriginRoute::new(11)),
        )
        .into_root();

    let sema_input = match nexus_action {
        nexus::NexusAction::CommandSemaWrite(command) => command.into_payload(),
        other => panic!("expected SEMA write command, got {other:?}"),
    };

    let sema_output = runtime
        .apply(sema_input.with_origin_route(sema::OriginRoute::new(12)))
        .into_root();
    let reply = runtime
        .decide(
            nexus::NexusWork::sema_write_completed(sema_output)
                .with_origin_route(nexus::OriginRoute::new(12)),
        )
        .into_root();

    assert_eq!(runtime.accounts().len(), 1);

    match reply {
        nexus::NexusAction::ReplyToSignal(reply) => match reply.into_payload() {
            nexus::SignalOutput::MetaOutput(meta::Output::AccountRegistered(payload)) => {
                assert_eq!(payload.provider, meta::Provider::Cloudflare);
                assert_eq!(payload.provider_account.payload(), "primary");
            }
            other => panic!("expected meta signal reply, got {other:?}"),
        },
        other => panic!("expected meta signal reply, got {other:?}"),
    }
}
