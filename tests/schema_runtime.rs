use cloud::schema::{nexus, sema};
use cloud::schema_runtime::SchemaRuntime;
use meta_signal_cloud::schema::lib as meta;
use signal_cloud::schema::lib as ordinary;

use nexus::NexusEngine;
use sema::SemaEngine;

#[test]
fn ordinary_capability_observation_flows_through_generated_nexus_and_sema() {
    let mut runtime = SchemaRuntime::new();
    let input = ordinary::Input::Observe(ordinary::Observation::Capabilities(
        ordinary::CapabilityQuery {
            provider: Some(ordinary::Provider::Cloudflare),
            capability: Some(ordinary::Capability::DomainNameSystemRecords),
        },
    ));

    let nexus_action = runtime
        .execute(
            nexus::NexusWork::SignalArrived(nexus::SignalInput::OrdinaryInput(input))
                .with_origin_route(nexus::OriginRoute(7)),
        )
        .into_root();

    let sema_input = match nexus_action {
        nexus::NexusAction::CommandSemaRead(sema::SemaReadInput::Observe(observation)) => {
            observation
        }
        other => panic!("expected SEMA read command, got {other:?}"),
    };

    let sema_output = runtime
        .observe(sema::SemaReadInput::Observe(sema_input).with_origin_route(sema::OriginRoute(7)))
        .into_root();

    let report = match sema_output.clone() {
        sema::SemaReadOutput::Observed(ordinary::ObservationResult::Capabilities(report)) => report,
        other => panic!("expected capability report, got {other:?}"),
    };
    assert_eq!(report.payload().len(), 1);
    assert_eq!(
        report.payload()[0].capability_state,
        ordinary::CapabilityState::Compiled
    );

    let reply = runtime
        .execute(
            nexus::NexusWork::SemaReadCompleted(sema_output)
                .with_origin_route(nexus::OriginRoute(8)),
        )
        .into_root();

    match reply {
        nexus::NexusAction::ReplyToSignal(nexus::SignalOutput::OrdinaryOutput(
            ordinary::Output::Observed(ordinary::ObservationResult::Capabilities(report)),
        )) => assert_eq!(report.payload().len(), 1),
        other => panic!("expected ordinary signal reply, got {other:?}"),
    }
}

#[test]
fn meta_registration_flows_through_generated_nexus_and_sema() {
    let mut runtime = SchemaRuntime::new();
    let registration = meta::Registration {
        provider: meta::Provider::Cloudflare,
        provider_account: String::from("primary"),
        credential_handle: String::from("cloudflare/api-token"),
    };

    let nexus_action = runtime
        .execute(
            nexus::NexusWork::SignalArrived(nexus::SignalInput::MetaInput(
                meta::Input::RegisterAccount(registration),
            ))
            .with_origin_route(nexus::OriginRoute(11)),
        )
        .into_root();

    let sema_input = match nexus_action {
        nexus::NexusAction::CommandSemaWrite(sema::SemaWriteInput::RegisterAccount(payload)) => {
            payload
        }
        other => panic!("expected SEMA write command, got {other:?}"),
    };

    let sema_output = runtime
        .apply(
            sema::SemaWriteInput::RegisterAccount(sema_input)
                .with_origin_route(sema::OriginRoute(11)),
        )
        .into_root();

    assert_eq!(runtime.accounts().len(), 1);

    let reply = runtime
        .execute(
            nexus::NexusWork::SemaWriteCompleted(sema_output)
                .with_origin_route(nexus::OriginRoute(12)),
        )
        .into_root();

    match reply {
        nexus::NexusAction::ReplyToSignal(nexus::SignalOutput::MetaOutput(
            meta::Output::AccountRegistered(payload),
        )) => {
            assert_eq!(payload.provider, meta::Provider::Cloudflare);
            assert_eq!(payload.provider_account, "primary");
        }
        other => panic!("expected meta signal reply, got {other:?}"),
    }
}
