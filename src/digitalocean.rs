//! DigitalOcean compute provider adapter (HTTP, blocking ureq).
//!
//! Mirrors `src/hetzner.rs`: a typed `Error`/`Result`, a `Token` plus a
//! `CredentialSource` that reads it from the environment, a sync engine
//! `trait Api`, an `HttpApi` that speaks the DigitalOcean v2 REST API, and a
//! `ProviderClient` that maps droplet responses onto `signal_cloud::CloudHost`.
//!
//! Phase 1 is synchronous like Hetzner: `create_host` / `observe_hosts` /
//! `destroy_host` are single fast REST calls invoked from the live `Store`.
//!
//! DigitalOcean differs from Hetzner in three places: the error body is a
//! top-level `{message, id}` envelope (no `.error` wrapper); droplets live under
//! `/v2/droplets` and account keys under `/v2/account/keys`; and the droplet
//! create `ssh_keys` array carries fingerprints, not names, so `create_server`
//! resolves each SSH-key name to its fingerprint before the droplet POST.

use std::fmt;
use std::sync::Arc;

use meta_signal_cloud::CredentialHandle;
use serde::{Deserialize, Serialize};
use signal_cloud::{
    CloudHost, DomainName, HostIdentifier, HostStatus, ImageName, IpAddress, Provider,
    ProviderAccount, ServerType,
};

/// The cheapest usable droplet size slug ($4/mo, 1 vCPU / 512 MB / 10 GB).
pub const DEFAULT_SIZE: &str = "s-1vcpu-512mb-10gb";
/// A sensible default region for live testing.
pub const DEFAULT_REGION: &str = "nyc1";
/// The default droplet image slug.
pub const DEFAULT_IMAGE: &str = "ubuntu-24-04-x64";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("credential handle is not available in the environment: {0}")]
    CredentialUnavailable(String),

    #[error("DigitalOcean request failed: {0}")]
    RequestFailed(String),

    #[error("DigitalOcean rejected request: {0}")]
    RequestRejected(String),

    #[error("DigitalOcean host was not found: {0}")]
    HostNotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// The DigitalOcean API token. The token never leaves this module's REST edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub trait CredentialSource: Send + Sync {
    fn token(&self, handle: &CredentialHandle) -> Result<Token>;
}

/// Reads the DigitalOcean token from the environment variable named by the
/// registered credential handle. The flake injects `DIGITALOCEAN_ACCESS_TOKEN`
/// from gopass, and `RegisterAccount` carries `DIGITALOCEAN_ACCESS_TOKEN` as the
/// credential handle.
#[derive(Debug, Default)]
pub struct EnvironmentCredentialSource;

impl EnvironmentCredentialSource {
    /// The conventional environment variable a DigitalOcean credential handle
    /// names; this is the variable the official `doctl` tooling reads.
    pub const TOKEN_ENVIRONMENT_VARIABLE: &str = "DIGITALOCEAN_ACCESS_TOKEN";
}

impl CredentialSource for EnvironmentCredentialSource {
    fn token(&self, handle: &CredentialHandle) -> Result<Token> {
        std::env::var(handle.as_str())
            .map(Token::new)
            .map_err(|_| Error::CredentialUnavailable(handle.as_str().to_owned()))
    }
}

/// The desired shape of a droplet before it exists on DigitalOcean.
#[derive(Debug, Clone)]
pub struct ServerSpec {
    pub name: String,
    pub server_type: String,
    pub image: String,
    /// SSH-key names. DigitalOcean's droplet create `ssh_keys` array wants
    /// fingerprints, not names, so `create_server` resolves each name to its
    /// fingerprint against `/v2/account/keys` before the droplet POST. Keeping
    /// names here keeps the `Store` apply path uniform with Hetzner: the plan's
    /// `ssh_key_name` flows through unchanged.
    pub ssh_keys: Vec<String>,
    pub location: Option<String>,
}

/// A droplet as DigitalOcean reports it, normalized into typed domain values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiServer {
    pub identifier: HostIdentifier,
    pub name: DomainName,
    pub server_type: ServerType,
    pub image: ImageName,
    pub ipv4: IpAddress,
    pub status: HostStatus,
}

/// The synchronous engine trait every DigitalOcean REST mechanism implements.
/// Tests substitute a canned implementation; production uses `HttpApi`.
pub trait Api: Send + Sync {
    fn ensure_ssh_key(&self, token: &Token, name: &str, public_key: &str) -> Result<String>;
    fn delete_ssh_key(&self, token: &Token, fingerprint: &str) -> Result<()>;
    fn create_server(&self, token: &Token, spec: &ServerSpec) -> Result<ApiServer>;
    fn get_server(&self, token: &Token, identifier: &HostIdentifier) -> Result<ApiServer>;
    fn list_servers(&self, token: &Token) -> Result<Vec<ApiServer>>;
    fn delete_server(&self, token: &Token, identifier: &HostIdentifier) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct HttpApi {
    base_url: String,
}

impl HttpApi {
    pub fn new() -> Self {
        Self {
            base_url: "https://api.digitalocean.com".to_owned(),
        }
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    fn get<ResultBody>(
        &self,
        token: &Token,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<ResultBody>
    where
        ResultBody: for<'de> Deserialize<'de>,
    {
        let url = format!("{}{}", self.base_url, path);
        let authorization = format!("Bearer {}", token.as_str());
        let request = query.iter().fold(
            ureq::get(&url)
                .set("Authorization", &authorization)
                .set("Accept", "application/json"),
            |request, (name, value)| request.query(name, value),
        );
        Self::decode_call(request.call())
    }

    fn post<ResultBody, RequestBody>(
        &self,
        token: &Token,
        path: &str,
        body: &RequestBody,
    ) -> Result<ResultBody>
    where
        ResultBody: for<'de> Deserialize<'de>,
        RequestBody: Serialize,
    {
        let url = format!("{}{}", self.base_url, path);
        let authorization = format!("Bearer {}", token.as_str());
        Self::decode_call(
            ureq::post(&url)
                .set("Authorization", &authorization)
                .set("Accept", "application/json")
                .set("Content-Type", "application/json")
                .send_json(body),
        )
    }

    /// DigitalOcean's droplet delete returns `204 No Content` (no body); a `404`
    /// for an already-absent droplet is also success.
    fn delete(&self, token: &Token, path: &str) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let authorization = format!("Bearer {}", token.as_str());
        match ureq::delete(&url)
            .set("Authorization", &authorization)
            .set("Accept", "application/json")
            .call()
        {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(404, _)) => Ok(()),
            Err(error) => Err(Self::error_from_transport(error)),
        }
    }

    /// DigitalOcean returns the typed body directly inside an envelope keyed by
    /// the resource; a non-2xx is a `ureq::Error::Status` whose body carries a
    /// top-level `{message, id}` (no `.error` wrapper, unlike Hetzner).
    fn decode_call<ResultBody>(
        outcome: std::result::Result<ureq::Response, ureq::Error>,
    ) -> Result<ResultBody>
    where
        ResultBody: for<'de> Deserialize<'de>,
    {
        match outcome {
            Ok(response) => response
                .into_json()
                .map_err(|error| Error::RequestFailed(error.to_string())),
            Err(error) => Err(Self::error_from_transport(error)),
        }
    }

    fn error_from_transport(error: ureq::Error) -> Error {
        match error {
            ureq::Error::Status(404, response) => {
                Error::HostNotFound(Self::message_from_response(response))
            }
            ureq::Error::Status(_, response) => {
                Error::RequestRejected(Self::message_from_response(response))
            }
            ureq::Error::Transport(transport) => Error::RequestFailed(transport.to_string()),
        }
    }

    fn message_from_response(response: ureq::Response) -> String {
        match response.into_json::<ErrorEnvelope>() {
            Ok(envelope) => envelope.message,
            Err(error) => error.to_string(),
        }
    }
}

impl Default for HttpApi {
    fn default() -> Self {
        Self::new()
    }
}

impl Api for HttpApi {
    /// Resolves an SSH-key name to its DigitalOcean fingerprint, registering the
    /// key if it is absent. A match is by name or by identical public key.
    fn ensure_ssh_key(&self, token: &Token, name: &str, public_key: &str) -> Result<String> {
        let existing: SshKeysEnvelope = self.get(token, "/v2/account/keys", &[])?;
        if let Some(key) = existing.ssh_keys.into_iter().find(|key| {
            key.name == name || (!public_key.is_empty() && key.public_key == public_key)
        }) {
            return Ok(key.fingerprint);
        }
        let created: SshKeyEnvelope = self.post(
            token,
            "/v2/account/keys",
            &SshKeyPayload {
                name: name.to_owned(),
                public_key: public_key.to_owned(),
            },
        )?;
        Ok(created.ssh_key.fingerprint)
    }

    fn delete_ssh_key(&self, token: &Token, fingerprint: &str) -> Result<()> {
        let path = format!("/v2/account/keys/{fingerprint}");
        self.delete(token, &path)
    }

    /// Resolves each SSH-key name to its fingerprint, then POSTs the droplet.
    /// A name with no live account key is a registration concern (out of scope
    /// for create) and is dropped from the create array.
    fn create_server(&self, token: &Token, spec: &ServerSpec) -> Result<ApiServer> {
        let keys: SshKeysEnvelope = self.get(token, "/v2/account/keys", &[])?;
        let ssh_key_fingerprints = spec
            .ssh_keys
            .iter()
            .filter_map(|name| {
                keys.ssh_keys
                    .iter()
                    .find(|key| &key.name == name)
                    .map(|key| key.fingerprint.clone())
            })
            .collect();
        let envelope: DropletEnvelope = self.post(
            token,
            "/v2/droplets",
            &DropletPayload::from_spec(spec, ssh_key_fingerprints),
        )?;
        Ok(envelope.droplet.into_api_server())
    }

    fn get_server(&self, token: &Token, identifier: &HostIdentifier) -> Result<ApiServer> {
        let path = format!("/v2/droplets/{}", identifier.as_str());
        let envelope: DropletEnvelope = self.get(token, &path, &[])?;
        Ok(envelope.droplet.into_api_server())
    }

    fn list_servers(&self, token: &Token) -> Result<Vec<ApiServer>> {
        let envelope: DropletsEnvelope = self.get(token, "/v2/droplets", &[])?;
        Ok(envelope
            .droplets
            .into_iter()
            .map(DropletRecord::into_api_server)
            .collect())
    }

    fn delete_server(&self, token: &Token, identifier: &HostIdentifier) -> Result<()> {
        let path = format!("/v2/droplets/{}", identifier.as_str());
        self.delete(token, &path)
    }
}

#[derive(Clone)]
pub struct ProviderClient {
    api: Arc<dyn Api>,
    credentials: Arc<dyn CredentialSource>,
}

impl ProviderClient {
    pub fn production() -> Self {
        Self::new(
            Arc::new(HttpApi::new()),
            Arc::new(EnvironmentCredentialSource),
        )
    }

    pub fn new(api: Arc<dyn Api>, credentials: Arc<dyn CredentialSource>) -> Self {
        Self { api, credentials }
    }

    pub fn verify_credential(&self, credential: &CredentialHandle) -> Result<()> {
        self.credentials.token(credential).map(|_| ())
    }

    pub fn create_host(
        &self,
        credential: &CredentialHandle,
        account: &ProviderAccount,
        spec: &ServerSpec,
    ) -> Result<CloudHost> {
        let token = self.credentials.token(credential)?;
        let server = self.api.create_server(&token, spec)?;
        Ok(HostMapping::new(account.clone(), server).into_cloud_host())
    }

    pub fn observe_hosts(
        &self,
        credential: &CredentialHandle,
        account: &ProviderAccount,
    ) -> Result<Vec<CloudHost>> {
        let token = self.credentials.token(credential)?;
        Ok(self
            .api
            .list_servers(&token)?
            .into_iter()
            .map(|server| HostMapping::new(account.clone(), server).into_cloud_host())
            .collect())
    }

    pub fn destroy_host(
        &self,
        credential: &CredentialHandle,
        identifier: &HostIdentifier,
    ) -> Result<()> {
        let token = self.credentials.token(credential)?;
        self.api.delete_server(&token, identifier)
    }

    /// Destroys the host whose droplet name matches `name`, resolving the
    /// numeric droplet identifier the delete endpoint requires (the plan carries
    /// the node name, not the provider id). A name with no live droplet is
    /// already gone and reported as success.
    pub fn destroy_host_by_name(&self, credential: &CredentialHandle, name: &str) -> Result<()> {
        let token = self.credentials.token(credential)?;
        match self
            .api
            .list_servers(&token)?
            .into_iter()
            .find(|server| server.name.as_str() == name)
        {
            Some(server) => self.api.delete_server(&token, &server.identifier),
            None => Ok(()),
        }
    }
}

impl fmt::Debug for ProviderClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderClient")
            .field("api", &"<digitalocean api>")
            .field("credentials", &"<credential source>")
            .finish()
    }
}

/// Projects a DigitalOcean `ApiServer` plus the owning account into the wire
/// `CloudHost`.
struct HostMapping {
    account: ProviderAccount,
    server: ApiServer,
}

impl HostMapping {
    fn new(account: ProviderAccount, server: ApiServer) -> Self {
        Self { account, server }
    }

    fn into_cloud_host(self) -> CloudHost {
        CloudHost {
            provider: Provider::DigitalOcean,
            account: self.account,
            identifier: self.server.identifier,
            name: self.server.name,
            server_type: self.server.server_type,
            image: self.server.image,
            ipv4: self.server.ipv4,
            status: self.server.status,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    message: String,
}

#[derive(Debug, Serialize)]
struct DropletPayload {
    name: String,
    size: String,
    image: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ssh_keys: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<String>,
    ipv6: bool,
    monitoring: bool,
}

impl DropletPayload {
    fn from_spec(spec: &ServerSpec, ssh_keys: Vec<String>) -> Self {
        Self {
            name: spec.name.clone(),
            size: spec.server_type.clone(),
            image: spec.image.clone(),
            ssh_keys,
            region: spec.location.clone(),
            // DigitalOcean's ipv6 networking, like its monitoring agent, is
            // only honored on DO's own distribution images — a custom image
            // (CriomOS CloudNode) is rejected "image is not compatible with
            // ipv6". A node that wants ipv6 carries it through its own network
            // config, not DO's create-time flag. (Both flags should become
            // desired-state fields on ServerSpec rather than hardcoded.)
            ipv6: false,
            // DigitalOcean's monitoring agent is only supported on DO's own
            // distribution images — creating a droplet from a custom image
            // (e.g. a CriomOS CloudNode image) with monitoring=true is rejected
            // "Monitoring is not supported for this image." CriomOS nodes carry
            // their own observability and do not use DO's metrics agent.
            monitoring: false,
        }
    }
}

#[derive(Debug, Serialize)]
struct SshKeyPayload {
    name: String,
    public_key: String,
}

#[derive(Debug, Deserialize)]
struct DropletEnvelope {
    droplet: DropletRecord,
}

#[derive(Debug, Deserialize)]
struct DropletsEnvelope {
    droplets: Vec<DropletRecord>,
}

#[derive(Debug, Deserialize)]
struct DropletRecord {
    id: u64,
    name: String,
    status: String,
    size_slug: String,
    networks: Networks,
    image: Option<DropletImage>,
}

impl DropletRecord {
    fn into_api_server(self) -> ApiServer {
        let ipv4 = self
            .networks
            .v4
            .into_iter()
            .find(|network| network.kind == "public")
            .map(|network| network.ip_address)
            .unwrap_or_default();
        let image = self
            .image
            .map(|image| image.slug.unwrap_or(image.name))
            .unwrap_or_default();
        ApiServer {
            identifier: HostIdentifier::new(self.id.to_string()),
            name: DomainName::new(self.name),
            server_type: ServerType::new(self.size_slug),
            image: ImageName::new(image),
            ipv4: IpAddress::new(ipv4),
            status: DigitalOceanStatus::new(self.status).into_host_status(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Networks {
    #[serde(default)]
    v4: Vec<NetworkV4>,
}

#[derive(Debug, Deserialize)]
struct NetworkV4 {
    ip_address: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct DropletImage {
    slug: Option<String>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct SshKeysEnvelope {
    ssh_keys: Vec<SshKeyRecord>,
}

#[derive(Debug, Deserialize)]
struct SshKeyEnvelope {
    ssh_key: SshKeyRecord,
}

#[derive(Debug, Deserialize)]
struct SshKeyRecord {
    name: String,
    fingerprint: String,
    #[serde(default)]
    public_key: String,
}

/// Maps the DigitalOcean droplet status string onto the wire `HostStatus`.
/// DigitalOcean has no live "deleting" status; a destroyed droplet simply
/// vanishes from the listing.
struct DigitalOceanStatus {
    status: String,
}

impl DigitalOceanStatus {
    fn new(status: String) -> Self {
        Self { status }
    }

    fn into_host_status(self) -> HostStatus {
        match self.status.as_str() {
            "new" => HostStatus::Initializing,
            "active" => HostStatus::Running,
            "off" | "archive" => HostStatus::Stopped,
            _ => HostStatus::Unknown,
        }
    }
}
