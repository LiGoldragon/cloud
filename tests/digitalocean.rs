//! DigitalOcean Phase 1 adapter tests: the mock `Api` returns canned
//! `ApiServer` values and the assertions confirm `ProviderClient` and the
//! `Store` host handlers map droplet responses onto `signal_cloud::CloudHost`.

#![cfg(feature = "digitalocean")]

use std::sync::{Arc, Mutex};

use cloud::Store;
use cloud::digitalocean::{
    Api, ApiServer, CredentialSource, Error, ProviderClient, Result, ServerSpec, Token,
};
use meta_signal_cloud::{
    CredentialHandle, HostDestruction, Operation as MetaOperation, Registration, Reply as MetaReply,
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
struct FixtureDigitalOceanApi {
    servers: Mutex<Vec<ApiServer>>,
    created: Mutex<Vec<ServerSpec>>,
    deleted: Mutex<Vec<String>>,
}

impl FixtureDigitalOceanApi {
    fn new() -> Self {
        Self {
            servers: Mutex::new(vec![ApiServer {
                identifier: HostIdentifier::new("4711"),
                name: DomainName::new("edge-one"),
                server_type: ServerType::new("s-1vcpu-512mb-10gb"),
                image: ImageName::new("ubuntu-24-04-x64"),
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

impl Api for FixtureDigitalOceanApi {
    fn ensure_ssh_key(&self, _token: &Token, _name: &str, _public_key: &str) -> Result<String> {
        Ok("aa:bb:cc:dd".to_owned())
    }

    fn delete_ssh_key(&self, _token: &Token, _fingerprint: &str) -> Result<()> {
        Ok(())
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

fn provider_client(api: Arc<FixtureDigitalOceanApi>) -> ProviderClient {
    ProviderClient::new(api, Arc::new(FixtureCredentialSource))
}

#[test]
fn create_host_maps_droplet_to_cloud_host() {
    let api = Arc::new(FixtureDigitalOceanApi::new());
    let client = provider_client(api.clone());
    let credential = CredentialHandle::new("DIGITALOCEAN_ACCESS_TOKEN");
    let account = ProviderAccount::new("primary");
    let spec = ServerSpec {
        name: "edge-two".to_owned(),
        server_type: "s-1vcpu-1gb".to_owned(),
        image: "ubuntu-24-04-x64".to_owned(),
        ssh_keys: vec!["operator".to_owned()],
        location: Some("nyc1".to_owned()),
    };

    let host = client
        .create_host(&credential, &account, &spec)
        .expect("create host");

    assert_eq!(
        host,
        CloudHost {
            provider: Provider::DigitalOcean,
            account: ProviderAccount::new("primary"),
            identifier: HostIdentifier::new("5012"),
            name: DomainName::new("edge-two"),
            server_type: ServerType::new("s-1vcpu-1gb"),
            image: ImageName::new("ubuntu-24-04-x64"),
            ipv4: IpAddress::new("203.0.113.40"),
            status: HostStatus::Initializing,
        }
    );
    assert_eq!(api.created.lock().expect("created").len(), 1);
}

#[test]
fn observe_hosts_maps_every_droplet() {
    let api = Arc::new(FixtureDigitalOceanApi::new());
    let client = provider_client(api);
    let credential = CredentialHandle::new("DIGITALOCEAN_ACCESS_TOKEN");
    let account = ProviderAccount::new("primary");

    let hosts = client
        .observe_hosts(&credential, &account)
        .expect("observe hosts");

    assert_eq!(
        hosts,
        vec![CloudHost {
            provider: Provider::DigitalOcean,
            account: ProviderAccount::new("primary"),
            identifier: HostIdentifier::new("4711"),
            name: DomainName::new("edge-one"),
            server_type: ServerType::new("s-1vcpu-512mb-10gb"),
            image: ImageName::new("ubuntu-24-04-x64"),
            ipv4: IpAddress::new("203.0.113.7"),
            status: HostStatus::Running,
        }]
    );
}

#[test]
fn destroy_host_forwards_identifier_to_api() {
    let api = Arc::new(FixtureDigitalOceanApi::new());
    let client = provider_client(api.clone());
    let credential = CredentialHandle::new("DIGITALOCEAN_ACCESS_TOKEN");

    client
        .destroy_host(&credential, &HostIdentifier::new("4711"))
        .expect("destroy host");

    assert_eq!(api.deleted(), vec!["4711".to_owned()]);
    assert!(api.servers.lock().expect("servers").is_empty());
}

#[test]
fn create_host_surfaces_missing_credential() {
    let api = Arc::new(FixtureDigitalOceanApi::new());
    let client = ProviderClient::new(api, Arc::new(MissingCredentialSource));
    let credential = CredentialHandle::new("DIGITALOCEAN_ACCESS_TOKEN");
    let account = ProviderAccount::new("primary");
    let spec = ServerSpec {
        name: "edge-two".to_owned(),
        server_type: "s-1vcpu-1gb".to_owned(),
        image: "ubuntu-24-04-x64".to_owned(),
        ssh_keys: Vec::new(),
        location: None,
    };

    let error = client
        .create_host(&credential, &account, &spec)
        .expect_err("missing credential should error");
    assert!(matches!(error, Error::CredentialUnavailable(_)));
}

fn digitalocean_store(api: Arc<FixtureDigitalOceanApi>) -> Store {
    Store::with_digitalocean_provider(ProviderClient::new(api, Arc::new(FixtureCredentialSource)))
}

fn register_digitalocean_account(store: &Store) {
    let registration = MetaOperation::RegisterAccount(Registration {
        provider: Provider::DigitalOcean,
        account: ProviderAccount::new("primary"),
        credential: CredentialHandle::new("DIGITALOCEAN_ACCESS_TOKEN"),
    })
    .into_request();
    assert!(matches!(
        store.handle_meta_request(registration),
        FrameReply::Accepted { .. }
    ));
}

#[test]
fn store_observes_digitalocean_hosts_through_provider() {
    let api = Arc::new(FixtureDigitalOceanApi::new());
    let store = digitalocean_store(api);
    register_digitalocean_account(&store);

    let request = CloudOperation::Observe(Observation::Servers(HostQuery {
        provider: Provider::DigitalOcean,
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
            provider: Provider::DigitalOcean,
            account: ProviderAccount::new("primary"),
            identifier: HostIdentifier::new("4711"),
            name: DomainName::new("edge-one"),
            server_type: ServerType::new("s-1vcpu-512mb-10gb"),
            image: ImageName::new("ubuntu-24-04-x64"),
            ipv4: IpAddress::new("203.0.113.7"),
            status: HostStatus::Running,
        }]
    );
}

#[test]
fn store_prepares_and_applies_a_host_create_plan() {
    let api = Arc::new(FixtureDigitalOceanApi::new());
    let store = digitalocean_store(api.clone());
    register_digitalocean_account(&store);

    let preparation = MetaOperation::PrepareHostPlan(meta_signal_cloud::HostPlanPreparation {
        desired_host_state: meta_signal_cloud::DesiredHostState {
            provider: Provider::DigitalOcean,
            host_name: DomainName::new("edge-three"),
            server_type: meta_signal_cloud::ServerType::new("s-1vcpu-512mb-10gb"),
            image_name: meta_signal_cloud::ImageName::new("ubuntu-24-04-x64"),
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
    // The plan's ssh_key_name was threaded into the droplet create call as a
    // name; the adapter resolves it to a fingerprint before the POST.
    assert_eq!(
        api.created.lock().expect("created")[0].ssh_keys,
        vec!["operator".to_owned()]
    );
}

#[test]
fn destroy_host_by_name_resolves_identifier_before_delete() {
    let api = Arc::new(FixtureDigitalOceanApi::new());
    let client = provider_client(api.clone());
    let credential = CredentialHandle::new("DIGITALOCEAN_ACCESS_TOKEN");

    client
        .destroy_host_by_name(&credential, "edge-one")
        .expect("destroy host by name");

    // "edge-one" resolved to its droplet id 4711 before the delete call — the
    // delete endpoint needs the numeric id, never the node name.
    assert_eq!(api.deleted(), vec!["4711".to_owned()]);
    assert!(api.servers.lock().expect("servers").is_empty());
}

#[test]
fn destroy_host_by_name_treats_missing_host_as_already_gone() {
    let api = Arc::new(FixtureDigitalOceanApi::new());
    let client = provider_client(api.clone());
    let credential = CredentialHandle::new("DIGITALOCEAN_ACCESS_TOKEN");

    client
        .destroy_host_by_name(&credential, "absent-host")
        .expect("missing host is already gone");

    assert!(api.deleted().is_empty());
}

fn approve_and_apply_plan(store: &Store, plan: &meta_signal_cloud::PlanIdentifier) {
    let approval = MetaOperation::ApprovePlan(meta_signal_cloud::Approval { plan: plan.clone() })
        .into_request();
    assert!(matches!(
        store.handle_meta_request(approval),
        FrameReply::Accepted { .. }
    ));

    let application =
        MetaOperation::ApplyPlan(meta_signal_cloud::Application { plan: plan.clone() })
            .into_request();
    match store.handle_meta_request(application) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::PlanApplied(applied)) => assert_eq!(&applied.plan, plan),
            other => panic!("unexpected apply reply {other:?}"),
        },
        other => panic!("unexpected frame reply {other:?}"),
    }
}

fn observe_host_names(store: &Store) -> Vec<String> {
    let request = CloudOperation::Observe(Observation::Servers(HostQuery {
        provider: Provider::DigitalOcean,
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
    listing
        .hosts
        .iter()
        .map(|host| host.name.as_str().to_owned())
        .collect()
}

#[test]
fn full_host_lifecycle_runs_through_the_store_handlers() {
    // RegisterAccount -> PrepareHostPlan(Create) -> Approve -> Apply ->
    // Observe(present) -> PrepareHostDestruction -> Approve -> Apply ->
    // Observe(gone), driven through the real Store handlers with the mock Api.
    let api = Arc::new(FixtureDigitalOceanApi::new());
    let store = digitalocean_store(api.clone());
    register_digitalocean_account(&store);

    // The fixture is born with one host ("edge-one"); the create plan adds a
    // second host ("edge-four") to provision and later destroy.
    assert_eq!(observe_host_names(&store), vec!["edge-one".to_owned()]);

    let preparation = MetaOperation::PrepareHostPlan(meta_signal_cloud::HostPlanPreparation {
        desired_host_state: meta_signal_cloud::DesiredHostState {
            provider: Provider::DigitalOcean,
            host_name: DomainName::new("edge-four"),
            server_type: meta_signal_cloud::ServerType::new("s-1vcpu-512mb-10gb"),
            image_name: meta_signal_cloud::ImageName::new("ubuntu-24-04-x64"),
            ssh_key_name: meta_signal_cloud::SshKeyName::new("operator"),
        },
    })
    .into_request();
    let create_plan = match store.handle_meta_request(preparation) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::HostPlanPrepared(plan)) => plan,
            other => panic!("unexpected prepare reply {other:?}"),
        },
        other => panic!("unexpected frame reply {other:?}"),
    };
    assert_eq!(create_plan.intent, meta_signal_cloud::HostIntent::Create);

    approve_and_apply_plan(&store, &create_plan.identifier);

    // The applied Create plan provisioned "edge-four" alongside the fixture host.
    assert_eq!(
        observe_host_names(&store),
        vec!["edge-one".to_owned(), "edge-four".to_owned()]
    );

    // PrepareHostDestruction mints a Destroy plan reusing HostPlanPrepared.
    let destruction = MetaOperation::PrepareHostDestruction(HostDestruction {
        provider: Provider::DigitalOcean,
        host_name: DomainName::new("edge-four"),
    })
    .into_request();
    let destroy_plan = match store.handle_meta_request(destruction) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail().0 {
            SubReply::Ok(MetaReply::HostPlanPrepared(plan)) => plan,
            other => panic!("unexpected destruction reply {other:?}"),
        },
        other => panic!("unexpected frame reply {other:?}"),
    };
    assert_eq!(destroy_plan.intent, meta_signal_cloud::HostIntent::Destroy);
    assert_eq!(destroy_plan.host_name, DomainName::new("edge-four"));
    // The create-only fields are minted empty on a Destroy plan.
    assert_eq!(destroy_plan.server_type.as_str(), "");
    assert_eq!(destroy_plan.image_name.as_str(), "");
    assert_eq!(destroy_plan.ssh_key_name.as_str(), "");

    approve_and_apply_plan(&store, &destroy_plan.identifier);

    // The Destroy plan resolved "edge-four" to its droplet id before deleting it;
    // only the fixture host remains.
    assert_eq!(observe_host_names(&store), vec!["edge-one".to_owned()]);
    assert_eq!(api.deleted(), vec!["5012".to_owned()]);
}
