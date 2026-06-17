//! Hetzner Phase 1 adapter tests: the mock `Api` returns canned `ApiServer`
//! values and the assertions confirm `ProviderClient` and the `Store` host
//! handlers map Hetzner responses onto `signal_cloud::CloudHost`.

#![cfg(feature = "hetzner")]

use std::sync::{Arc, Mutex};

use cloud::Store;
use cloud::hetzner::{
    Api, ApiServer, CredentialSource, Error, ProviderClient, Result, ServerSpec, Token,
};
use meta_signal_cloud::{
    CredentialHandle, Operation as MetaOperation, Registration, Reply as MetaReply,
};
use signal_cloud::{
    CloudHost, DomainName, HostIdentifier, HostQuery, HostStatus, ImageName, IpAddress,
    Observation, ObservationResult, Operation as CloudOperation, Provider, ProviderAccount,
    Reply as CloudReply, ServerType,
};
use signal_frame::{Reply as FrameReply, RequestPayload, SubReply};

#[derive(Debug, Default)]
struct FixtureCredentialSource;

impl CredentialSource for FixtureCredentialSource {
    fn token(&self, _handle: &CredentialHandle) -> Result<Token> {
        Ok(Token::new("fixture-token"))
    }
}

#[derive(Debug)]
struct MissingCredentialSource;

impl CredentialSource for MissingCredentialSource {
    fn token(&self, handle: &CredentialHandle) -> Result<Token> {
        Err(Error::CredentialUnavailable(handle.as_str().to_owned()))
    }
}

#[derive(Debug)]
struct FixtureHetznerApi {
    servers: Mutex<Vec<ApiServer>>,
    created: Mutex<Vec<ServerSpec>>,
    deleted: Mutex<Vec<String>>,
}

impl FixtureHetznerApi {
    fn new() -> Self {
        Self {
            servers: Mutex::new(vec![ApiServer {
                identifier: HostIdentifier::new("4711"),
                name: DomainName::new("edge-one"),
                server_type: ServerType::new("cx22"),
                image: ImageName::new("debian-12"),
                ipv4: IpAddress::new("203.0.113.7"),
                status: HostStatus::Running,
            }]),
            created: Mutex::new(Vec::new()),
            deleted: Mutex::new(Vec::new()),
        }
    }

    fn deleted(&self) -> Vec<String> {
        self.deleted.lock().expect("deleted").clone()
    }
}

impl Api for FixtureHetznerApi {
    fn ensure_ssh_key(&self, _token: &Token, _name: &str, _public_key: &str) -> Result<i64> {
        Ok(99)
    }

    fn create_server(&self, _token: &Token, spec: &ServerSpec) -> Result<ApiServer> {
        self.created.lock().expect("created").push(spec.clone());
        let created = ApiServer {
            identifier: HostIdentifier::new("5012"),
            name: DomainName::new(spec.name.clone()),
            server_type: ServerType::new(spec.server_type.clone()),
            image: ImageName::new(spec.image.clone()),
            ipv4: IpAddress::new("203.0.113.40"),
            status: HostStatus::Initializing,
        };
        self.servers.lock().expect("servers").push(created.clone());
        Ok(created)
    }

    fn get_server(&self, _token: &Token, identifier: &HostIdentifier) -> Result<ApiServer> {
        self.servers
            .lock()
            .expect("servers")
            .iter()
            .find(|server| server.identifier == *identifier)
            .cloned()
            .ok_or_else(|| Error::HostNotFound(identifier.as_str().to_owned()))
    }

    fn list_servers(&self, _token: &Token) -> Result<Vec<ApiServer>> {
        Ok(self.servers.lock().expect("servers").clone())
    }

    fn delete_server(&self, _token: &Token, identifier: &HostIdentifier) -> Result<()> {
        self.deleted
            .lock()
            .expect("deleted")
            .push(identifier.as_str().to_owned());
        self.servers
            .lock()
            .expect("servers")
            .retain(|server| server.identifier != *identifier);
        Ok(())
    }
}

fn provider_client(api: Arc<FixtureHetznerApi>) -> ProviderClient {
    ProviderClient::new(api, Arc::new(FixtureCredentialSource))
}

#[test]
fn create_host_maps_hetzner_server_to_cloud_host() {
    let api = Arc::new(FixtureHetznerApi::new());
    let client = provider_client(api.clone());
    let credential = CredentialHandle::new("HCLOUD_TOKEN");
    let account = ProviderAccount::new("primary");
    let spec = ServerSpec {
        name: "edge-two".to_owned(),
        server_type: "cx32".to_owned(),
        image: "debian-12".to_owned(),
        ssh_key_ids: vec![99],
        location: Some("fsn1".to_owned()),
    };

    let host = client
        .create_host(&credential, &account, &spec)
        .expect("create host");

    assert_eq!(
        host,
        CloudHost {
            provider: Provider::Hetzner,
            account: ProviderAccount::new("primary"),
            identifier: HostIdentifier::new("5012"),
            name: DomainName::new("edge-two"),
            server_type: ServerType::new("cx32"),
            image: ImageName::new("debian-12"),
            ipv4: IpAddress::new("203.0.113.40"),
            status: HostStatus::Initializing,
        }
    );
    assert_eq!(api.created.lock().expect("created").len(), 1);
}

#[test]
fn observe_hosts_maps_every_server() {
    let api = Arc::new(FixtureHetznerApi::new());
    let client = provider_client(api);
    let credential = CredentialHandle::new("HCLOUD_TOKEN");
    let account = ProviderAccount::new("primary");

    let hosts = client
        .observe_hosts(&credential, &account)
        .expect("observe hosts");

    assert_eq!(
        hosts,
        vec![CloudHost {
            provider: Provider::Hetzner,
            account: ProviderAccount::new("primary"),
            identifier: HostIdentifier::new("4711"),
            name: DomainName::new("edge-one"),
            server_type: ServerType::new("cx22"),
            image: ImageName::new("debian-12"),
            ipv4: IpAddress::new("203.0.113.7"),
            status: HostStatus::Running,
        }]
    );
}

#[test]
fn destroy_host_forwards_identifier_to_api() {
    let api = Arc::new(FixtureHetznerApi::new());
    let client = provider_client(api.clone());
    let credential = CredentialHandle::new("HCLOUD_TOKEN");

    client
        .destroy_host(&credential, &HostIdentifier::new("4711"))
        .expect("destroy host");

    assert_eq!(api.deleted(), vec!["4711".to_owned()]);
    assert!(api.servers.lock().expect("servers").is_empty());
}

#[test]
fn create_host_surfaces_missing_credential() {
    let api = Arc::new(FixtureHetznerApi::new());
    let client = ProviderClient::new(api, Arc::new(MissingCredentialSource));
    let credential = CredentialHandle::new("HCLOUD_TOKEN");
    let account = ProviderAccount::new("primary");
    let spec = ServerSpec {
        name: "edge-two".to_owned(),
        server_type: "cx32".to_owned(),
        image: "debian-12".to_owned(),
        ssh_key_ids: Vec::new(),
        location: None,
    };

    let error = client
        .create_host(&credential, &account, &spec)
        .expect_err("missing credential should error");
    assert!(matches!(error, Error::CredentialUnavailable(_)));
}

fn hetzner_store(api: Arc<FixtureHetznerApi>) -> Store {
    Store::with_hetzner_provider(ProviderClient::new(api, Arc::new(FixtureCredentialSource)))
}

fn register_hetzner_account(store: &Store) {
    let registration = MetaOperation::RegisterAccount(Registration {
        provider: Provider::Hetzner,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new("HCLOUD_TOKEN"),
    })
    .into_request();
    assert!(matches!(
        store.handle_meta_request(registration),
        FrameReply::Accepted { .. }
    ));
}

#[test]
fn store_observes_hetzner_hosts_through_provider() {
    let api = Arc::new(FixtureHetznerApi::new());
    let store = hetzner_store(api);
    register_hetzner_account(&store);

    let request = CloudOperation::Observe(Observation::Servers(HostQuery {
        provider: Provider::Hetzner,
        account: None,
    }))
    .into_request();

    let listing = match store.handle_ordinary_request(request) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(CloudReply::Observed(ObservationResult::Servers(listing))) => listing,
            other => panic!("unexpected observe reply {other:?}"),
        },
        other => panic!("unexpected frame reply {other:?}"),
    };

    assert_eq!(
        listing.hosts,
        vec![CloudHost {
            provider: Provider::Hetzner,
            account: ProviderAccount::new("primary"),
            identifier: HostIdentifier::new("4711"),
            name: DomainName::new("edge-one"),
            server_type: ServerType::new("cx22"),
            image: ImageName::new("debian-12"),
            ipv4: IpAddress::new("203.0.113.7"),
            status: HostStatus::Running,
        }]
    );
}

#[test]
fn store_prepares_and_applies_a_host_create_plan() {
    let api = Arc::new(FixtureHetznerApi::new());
    let store = hetzner_store(api.clone());
    register_hetzner_account(&store);

    let preparation = MetaOperation::PrepareHostPlan(meta_signal_cloud::HostPlanPreparation {
        desired_host_state: meta_signal_cloud::DesiredHostState {
            provider: Provider::Hetzner,
            host_name: DomainName::new("edge-three"),
            server_type: meta_signal_cloud::ServerType::new("cx22"),
            image_name: meta_signal_cloud::ImageName::new("debian-12"),
            ssh_key_name: meta_signal_cloud::SshKeyName::new("operator"),
        },
    })
    .into_request();

    let plan = match store.handle_meta_request(preparation) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::HostPlanPrepared(plan)) => plan,
            other => panic!("unexpected prepare reply {other:?}"),
        },
        other => panic!("unexpected frame reply {other:?}"),
    };
    assert_eq!(plan.intent, meta_signal_cloud::HostIntent::Create);

    let approval = MetaOperation::ApprovePlan(meta_signal_cloud::Approval {
        plan: plan.identifier.clone(),
    })
    .into_request();
    assert!(matches!(
        store.handle_meta_request(approval),
        FrameReply::Accepted { .. }
    ));

    let application = MetaOperation::ApplyPlan(meta_signal_cloud::Application {
        plan: plan.identifier.clone(),
    })
    .into_request();
    match store.handle_meta_request(application) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::PlanApplied(applied)) => {
                assert_eq!(applied.plan, plan.identifier)
            }
            other => panic!("unexpected apply reply {other:?}"),
        },
        other => panic!("unexpected frame reply {other:?}"),
    }

    assert_eq!(api.created.lock().expect("created").len(), 1);
    assert_eq!(
        api.created.lock().expect("created")[0].name,
        "edge-three".to_owned()
    );
}
