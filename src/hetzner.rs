//! Hetzner Cloud compute provider adapter (HTTP, blocking ureq).
//!
//! Mirrors `src/cloudflare.rs`: a typed `Error`/`Result`, a `Token` plus a
//! `CredentialSource` that reads it from the environment, a sync engine
//! `trait Api`, an `HttpApi` that speaks the Hetzner Cloud v1 REST API, and a
//! `ProviderClient` that maps Hetzner responses onto `signal_cloud::CloudHost`.
//!
//! Phase 1 is synchronous like Cloudflare: `create_host` / `observe_hosts` /
//! `destroy_host` are single fast REST calls invoked from the live `Store`. The
//! long nixos-anywhere install and any `spawn_blocking` job-registry deferral
//! are explicitly Phase 2.

use std::fmt;
use std::sync::Arc;

use meta_signal_cloud::CredentialHandle;
use serde::{Deserialize, Serialize};
use signal_cloud::{
    CloudHost, DomainName, HostIdentifier, HostStatus, ImageName, IpAddress, Provider,
    ProviderAccount, ServerType,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("credential handle is not available in the environment: {0}")]
    CredentialUnavailable(String),

    #[error("Hetzner request failed: {0}")]
    RequestFailed(String),

    #[error("Hetzner rejected request: {0}")]
    RequestRejected(String),

    #[error("Hetzner host was not found: {0}")]
    HostNotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// The Hetzner Cloud API token. The token never leaves this module's REST edge.
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

/// Reads the Hetzner token from the environment variable named by the
/// registered credential handle. The flake injects `HCLOUD_TOKEN` from gopass,
/// and `RegisterAccount` carries `HCLOUD_TOKEN` as the credential handle.
#[derive(Debug, Default)]
pub struct EnvironmentCredentialSource;

impl EnvironmentCredentialSource {
    /// The conventional environment variable a Hetzner credential handle names.
    pub const TOKEN_ENVIRONMENT_VARIABLE: &str = "HCLOUD_TOKEN";
}

impl CredentialSource for EnvironmentCredentialSource {
    fn token(&self, handle: &CredentialHandle) -> Result<Token> {
        std::env::var(handle.as_str())
            .map(Token::new)
            .map_err(|_| Error::CredentialUnavailable(handle.as_str().to_owned()))
    }
}

/// The desired shape of a server before it exists on Hetzner.
#[derive(Debug, Clone)]
pub struct ServerSpec {
    pub name: String,
    pub server_type: String,
    pub image: String,
    /// Hetzner SSH-key names (its `ssh_keys` create field accepts names); the
    /// key must already exist in the project, so the durable CriomOS root key
    /// is registered once as a project resource and referenced here by name.
    pub ssh_keys: Vec<String>,
    pub location: Option<String>,
}

/// A server as Hetzner reports it, normalized into typed domain values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiServer {
    pub identifier: HostIdentifier,
    pub name: DomainName,
    pub server_type: ServerType,
    pub image: ImageName,
    pub ipv4: IpAddress,
    pub status: HostStatus,
}

/// The synchronous engine trait every Hetzner REST mechanism implements. Tests
/// substitute a canned implementation; production uses `HttpApi`.
pub trait Api: Send + Sync {
    fn ensure_ssh_key(&self, token: &Token, name: &str, public_key: &str) -> Result<i64>;
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
            base_url: "https://api.hetzner.cloud".to_owned(),
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

    /// Hetzner returns the typed body directly (no `{success, result, errors}`
    /// envelope); a non-2xx is a `ureq::Error::Status` whose body carries
    /// `{error:{code,message}}`.
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
            Ok(envelope) => envelope.error.message,
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
    fn ensure_ssh_key(&self, token: &Token, name: &str, public_key: &str) -> Result<i64> {
        let existing: SshKeysEnvelope = self.get(token, "/v1/ssh_keys", &[("name", name)])?;
        if let Some(key) = existing.ssh_keys.into_iter().find(|key| key.name == name) {
            return Ok(key.id);
        }
        let created: SshKeyEnvelope = self.post(
            token,
            "/v1/ssh_keys",
            &SshKeyPayload {
                name: name.to_owned(),
                public_key: public_key.to_owned(),
            },
        )?;
        Ok(created.ssh_key.id)
    }

    fn create_server(&self, token: &Token, spec: &ServerSpec) -> Result<ApiServer> {
        let envelope: ServerEnvelope =
            self.post(token, "/v1/servers", &ServerPayload::from_spec(spec))?;
        Ok(envelope.server.into_api_server())
    }

    fn get_server(&self, token: &Token, identifier: &HostIdentifier) -> Result<ApiServer> {
        let path = format!("/v1/servers/{}", identifier.as_str());
        let envelope: ServerEnvelope = self.get(token, &path, &[])?;
        Ok(envelope.server.into_api_server())
    }

    fn list_servers(&self, token: &Token) -> Result<Vec<ApiServer>> {
        let envelope: ServersEnvelope = self.get(token, "/v1/servers", &[])?;
        Ok(envelope
            .servers
            .into_iter()
            .map(ServerRecord::into_api_server)
            .collect())
    }

    fn delete_server(&self, token: &Token, identifier: &HostIdentifier) -> Result<()> {
        let path = format!("/v1/servers/{}", identifier.as_str());
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

    /// Destroys the host whose Hetzner name matches `name`, resolving the
    /// numeric server identifier the delete endpoint requires (the plan carries
    /// the node name, not the provider id). A name with no live server is
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
            .field("api", &"<hetzner api>")
            .field("credentials", &"<credential source>")
            .finish()
    }
}

/// Projects a Hetzner `ApiServer` plus the owning account into the wire
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
            provider: Provider::Hetzner,
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
    error: ApiError,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Debug, Serialize)]
struct ServerPayload {
    name: String,
    server_type: String,
    image: String,
    ssh_keys: Vec<String>,
    start_after_create: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
}

impl ServerPayload {
    fn from_spec(spec: &ServerSpec) -> Self {
        Self {
            name: spec.name.clone(),
            server_type: spec.server_type.clone(),
            image: spec.image.clone(),
            ssh_keys: spec.ssh_keys.clone(),
            start_after_create: true,
            location: spec.location.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
struct SshKeyPayload {
    name: String,
    public_key: String,
}

#[derive(Debug, Deserialize)]
struct ServerEnvelope {
    server: ServerRecord,
}

#[derive(Debug, Deserialize)]
struct ServersEnvelope {
    servers: Vec<ServerRecord>,
}

#[derive(Debug, Deserialize)]
struct ServerRecord {
    id: i64,
    name: String,
    status: String,
    public_net: PublicNet,
    server_type: ServerTypeRef,
    image: Option<ImageRef>,
}

impl ServerRecord {
    fn into_api_server(self) -> ApiServer {
        let ipv4 = self
            .public_net
            .ipv4
            .map(|address| address.ip)
            .unwrap_or_default();
        let image = self.image.map(|image| image.name).unwrap_or_default();
        ApiServer {
            identifier: HostIdentifier::new(self.id.to_string()),
            name: DomainName::new(self.name),
            server_type: ServerType::new(self.server_type.name),
            image: ImageName::new(image),
            ipv4: IpAddress::new(ipv4),
            status: HetznerStatus::new(self.status).into_host_status(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PublicNet {
    ipv4: Option<Ipv4Ref>,
}

#[derive(Debug, Deserialize)]
struct Ipv4Ref {
    ip: String,
}

#[derive(Debug, Deserialize)]
struct ServerTypeRef {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ImageRef {
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
    id: i64,
    name: String,
}

/// Maps the Hetzner server status string onto the wire `HostStatus`.
struct HetznerStatus {
    status: String,
}

impl HetznerStatus {
    fn new(status: String) -> Self {
        Self { status }
    }

    fn into_host_status(self) -> HostStatus {
        match self.status.as_str() {
            "initializing" | "starting" => HostStatus::Initializing,
            "running" => HostStatus::Running,
            "stopping" | "off" => HostStatus::Stopped,
            "deleting" => HostStatus::Deleting,
            _ => HostStatus::Unknown,
        }
    }
}
