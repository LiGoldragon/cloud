use std::fmt;
use std::sync::Arc;

use owner_signal_cloud::CredentialHandle;
use serde::Deserialize;
use signal_cloud::{
    DomainName, DomainNameSystemRecord, Provider, ProviderAccount, ProxyMode, RecordKind,
    RecordListing, RecordValue, Zone, ZoneIdentifier,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("credential handle is not available in the environment: {0}")]
    CredentialUnavailable(String),

    #[error("Cloudflare request failed: {0}")]
    RequestFailed(String),

    #[error("Cloudflare rejected request: {0}")]
    RequestRejected(String),

    #[error("Cloudflare returned an unsupported DNS record kind: {0}")]
    UnsupportedRecordKind(String),

    #[error("Cloudflare zone was not found: {0}")]
    ZoneNotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

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

#[derive(Debug, Default)]
pub struct EnvironmentCredentialSource;

impl CredentialSource for EnvironmentCredentialSource {
    fn token(&self, handle: &CredentialHandle) -> Result<Token> {
        std::env::var(handle.as_str())
            .map(Token::new)
            .map_err(|_| Error::CredentialUnavailable(handle.as_str().to_owned()))
    }
}

pub trait Api: Send + Sync {
    fn zones(&self, token: &Token, name: Option<&DomainName>) -> Result<Vec<ApiZone>>;
    fn records(&self, token: &Token, zone: &ZoneIdentifier) -> Result<Vec<ApiRecord>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiZone {
    pub identifier: ZoneIdentifier,
    pub name: DomainName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRecord {
    pub name: DomainName,
    pub kind: RecordKind,
    pub value: RecordValue,
    pub proxy_mode: ProxyMode,
}

#[derive(Debug, Clone)]
pub struct HttpApi {
    base_url: String,
}

impl HttpApi {
    pub fn new() -> Self {
        Self {
            base_url: "https://api.cloudflare.com/client/v4".to_owned(),
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
        let response = request
            .call()
            .map_err(|error| Error::RequestFailed(error.to_string()))?;
        let envelope: Envelope<ResultBody> = response
            .into_json()
            .map_err(|error| Error::RequestFailed(error.to_string()))?;
        envelope.into_result()
    }
}

impl Default for HttpApi {
    fn default() -> Self {
        Self::new()
    }
}

impl Api for HttpApi {
    fn zones(&self, token: &Token, name: Option<&DomainName>) -> Result<Vec<ApiZone>> {
        let query = name
            .map(|name| vec![("name", name.as_str())])
            .unwrap_or_default();
        let zones: Vec<ZoneRecord> = self.get(token, "/zones", &query)?;
        zones.into_iter().map(ApiZone::try_from).collect()
    }

    fn records(&self, token: &Token, zone: &ZoneIdentifier) -> Result<Vec<ApiRecord>> {
        let path = format!("/zones/{}/dns_records", zone.as_str());
        let records: Vec<RecordRecord> = self.get(token, &path, &[])?;
        records.into_iter().map(ApiRecord::try_from).collect()
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

    pub fn zones(
        &self,
        account: &ProviderAccount,
        credential: &CredentialHandle,
        names: &[DomainName],
    ) -> Result<Vec<Zone>> {
        let token = self.credentials.token(credential)?;
        let mut zones = Vec::new();
        if names.is_empty() {
            zones.extend(self.api.zones(&token, None)?.into_iter().map(|zone| Zone {
                provider: Provider::Cloudflare,
                account: account.clone(),
                identifier: zone.identifier,
                name: zone.name,
            }));
            return Ok(zones);
        }
        for name in names {
            zones.extend(
                self.api
                    .zones(&token, Some(name))?
                    .into_iter()
                    .map(|zone| Zone {
                        provider: Provider::Cloudflare,
                        account: account.clone(),
                        identifier: zone.identifier,
                        name: zone.name,
                    }),
            );
        }
        Ok(zones)
    }

    pub fn records(
        &self,
        credential: &CredentialHandle,
        zone: &ZoneIdentifier,
    ) -> Result<RecordListing> {
        let token = self.credentials.token(credential)?;
        let records = self
            .api
            .records(&token, zone)?
            .into_iter()
            .map(|record| DomainNameSystemRecord {
                name: record.name,
                kind: record.kind,
                value: record.value,
                proxy_mode: record.proxy_mode,
            })
            .collect();
        Ok(RecordListing { records })
    }
}

impl fmt::Debug for ProviderClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderClient")
            .field("api", &"<cloudflare api>")
            .field("credentials", &"<credential source>")
            .finish()
    }
}

#[derive(Debug, Deserialize)]
struct Envelope<ResultBody> {
    success: bool,
    result: ResultBody,
    errors: Vec<ApiMessage>,
}

impl<ResultBody> Envelope<ResultBody> {
    fn into_result(self) -> Result<ResultBody> {
        if self.success {
            Ok(self.result)
        } else {
            Err(Error::RequestRejected(
                self.errors
                    .into_iter()
                    .map(|error| error.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            ))
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiMessage {
    message: String,
}

#[derive(Debug, Deserialize)]
struct ZoneRecord {
    id: String,
    name: String,
}

impl TryFrom<ZoneRecord> for ApiZone {
    type Error = Error;

    fn try_from(record: ZoneRecord) -> Result<Self> {
        Ok(Self {
            identifier: ZoneIdentifier::new(record.id),
            name: DomainName::new(record.name),
        })
    }
}

#[derive(Debug, Deserialize)]
struct RecordRecord {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    content: String,
    proxied: Option<bool>,
}

impl TryFrom<RecordRecord> for ApiRecord {
    type Error = Error;

    fn try_from(record: RecordRecord) -> Result<Self> {
        Ok(Self {
            name: DomainName::new(record.name),
            kind: RecordKindName::new(record.kind).into_record_kind()?,
            value: RecordValue::new(record.content),
            proxy_mode: if record.proxied.unwrap_or(false) {
                ProxyMode::ProviderProxy
            } else {
                ProxyMode::Direct
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordKindName(String);

impl RecordKindName {
    fn new(value: String) -> Self {
        Self(value)
    }

    fn into_record_kind(self) -> Result<RecordKind> {
        match self.0.as_str() {
            "A" => Ok(RecordKind::AddressV4),
            "AAAA" => Ok(RecordKind::AddressV6),
            "CNAME" => Ok(RecordKind::CanonicalName),
            "TXT" => Ok(RecordKind::Text),
            "MX" => Ok(RecordKind::MailExchange),
            "NS" => Ok(RecordKind::NameServer),
            "PTR" => Ok(RecordKind::Pointer),
            "SRV" => Ok(RecordKind::Service),
            "CAA" => Ok(RecordKind::CertificateAuthorityAuthorization),
            "SSHFP" => Ok(RecordKind::SecureShellFingerprint),
            "TLSA" => Ok(RecordKind::TransportLayerSecurityAuthentication),
            "URI" => Ok(RecordKind::UniformResourceIdentifier),
            "SVCB" => Ok(RecordKind::ServiceBinding),
            "HTTPS" => Ok(RecordKind::HttpsBinding),
            "LOC" => Ok(RecordKind::Location),
            other => Err(Error::UnsupportedRecordKind(other.to_owned())),
        }
    }
}
