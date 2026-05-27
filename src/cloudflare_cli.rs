//! Cloudflare CLI shell-out adapter.
//!
//! This adapter uses `flarectl --json` for the first production DNS
//! get/set path. It keeps the cloud component on the existing hand-written
//! `signal_channel!` stack while making Cloudflare's own CLI the provider
//! boundary for DNS records.

use std::process::Command;
use std::sync::Arc;

use serde::Deserialize;
use signal_cloud::{
    DomainName, DomainNameSystemRecord, ProxyMode, RecordKind, RecordValue, ZoneIdentifier,
};

use crate::cloudflare::{Api, ApiRecord, ApiZone, Error, RecordIdentifier, Result, Token};

const TOKEN_ENVIRONMENT_VARIABLE: &str = "CF_API_TOKEN";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlarectlBinary(String);

impl FlarectlBinary {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for FlarectlBinary {
    fn default() -> Self {
        Self::new("flarectl")
    }
}

pub trait CommandRunner: Send + Sync {
    fn run(&self, binary: &FlarectlBinary, arguments: &[String], token: &Token) -> Result<Vec<u8>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl CommandRunner for ProcessRunner {
    fn run(&self, binary: &FlarectlBinary, arguments: &[String], token: &Token) -> Result<Vec<u8>> {
        let output = Command::new(binary.as_str())
            .args(arguments)
            .env(TOKEN_ENVIRONMENT_VARIABLE, token.as_str())
            .output()
            .map_err(|error| Error::RequestFailed(format!("flarectl spawn: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::RequestRejected(format!(
                "flarectl exited {}: {}",
                output.status,
                stderr.trim()
            )));
        }
        Ok(output.stdout)
    }
}

#[derive(Clone)]
pub struct FlarectlApi {
    binary: FlarectlBinary,
    runner: Arc<dyn CommandRunner>,
}

impl FlarectlApi {
    pub fn new() -> Self {
        Self::with_runner(Arc::new(ProcessRunner))
    }

    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            binary: FlarectlBinary::default(),
            runner,
        }
    }

    pub fn with_binary(mut self, binary: FlarectlBinary) -> Self {
        self.binary = binary;
        self
    }

    fn execute(&self, arguments: Vec<String>, token: &Token) -> Result<Vec<u8>> {
        self.runner.run(&self.binary, &arguments, token)
    }

    fn parse_json<Target>(bytes: &[u8]) -> Result<Target>
    where
        Target: for<'de> Deserialize<'de>,
    {
        serde_json::from_slice(bytes)
            .map_err(|error| Error::RequestFailed(format!("flarectl json parse: {error}")))
    }

    fn zone_name(&self, token: &Token, zone: &ZoneIdentifier) -> Result<DomainName> {
        let zones = self.zones(token, None)?;
        zones
            .iter()
            .find(|candidate| {
                candidate.identifier == *zone || candidate.name.as_str() == zone.as_str()
            })
            .map(|candidate| candidate.name.clone())
            .ok_or_else(|| Error::ZoneNotFound(zone.as_str().to_owned()))
    }

    fn record_arguments(zone: &DomainName, record: &DomainNameSystemRecord) -> Vec<String> {
        let mut arguments = vec![
            "--zone".to_owned(),
            zone.as_str().to_owned(),
            "--name".to_owned(),
            record.name.as_str().to_owned(),
            "--type".to_owned(),
            RecordKindName::from_record_kind(record.kind).to_owned(),
            "--content".to_owned(),
            record.value.as_str().to_owned(),
        ];
        if record.proxy_mode == ProxyMode::ProviderProxy {
            arguments.push("--proxy".to_owned());
        }
        arguments
    }

    fn find_record_after_mutation(
        &self,
        token: &Token,
        zone: &ZoneIdentifier,
        record: &DomainNameSystemRecord,
    ) -> Result<ApiRecord> {
        self.records(token, zone)?
            .into_iter()
            .find(|candidate| candidate.name == record.name && candidate.kind == record.kind)
            .ok_or_else(|| {
                Error::RequestFailed(format!(
                    "flarectl did not return DNS record {} {:?} after mutation",
                    record.name.as_str(),
                    record.kind
                ))
            })
    }
}

impl Default for FlarectlApi {
    fn default() -> Self {
        Self::new()
    }
}

impl Api for FlarectlApi {
    fn zones(&self, token: &Token, name: Option<&DomainName>) -> Result<Vec<ApiZone>> {
        let output = self.execute(
            vec!["--json".to_owned(), "zone".to_owned(), "list".to_owned()],
            token,
        )?;
        let zones: Vec<FlarectlZone> = Self::parse_json(&output)?;
        let zones = zones.into_iter().map(ApiZone::from);
        Ok(match name {
            Some(name) => zones.filter(|zone| zone.name == *name).collect(),
            None => zones.collect(),
        })
    }

    fn records(&self, token: &Token, zone: &ZoneIdentifier) -> Result<Vec<ApiRecord>> {
        let zone = self.zone_name(token, zone)?;
        let output = self.execute(
            vec![
                "--json".to_owned(),
                "dns".to_owned(),
                "list".to_owned(),
                "--zone".to_owned(),
                zone.as_str().to_owned(),
            ],
            token,
        )?;
        let records: Vec<FlarectlRecord> = Self::parse_json(&output)?;
        records.into_iter().map(ApiRecord::try_from).collect()
    }

    fn create_record(
        &self,
        token: &Token,
        zone: &ZoneIdentifier,
        record: &DomainNameSystemRecord,
    ) -> Result<ApiRecord> {
        let zone_name = self.zone_name(token, zone)?;
        let mut arguments = vec!["--json".to_owned(), "dns".to_owned(), "create".to_owned()];
        arguments.extend(Self::record_arguments(&zone_name, record));
        self.execute(arguments, token)?;
        self.find_record_after_mutation(token, zone, record)
    }

    fn update_record(
        &self,
        token: &Token,
        zone: &ZoneIdentifier,
        identifier: &RecordIdentifier,
        record: &DomainNameSystemRecord,
    ) -> Result<ApiRecord> {
        let zone_name = self.zone_name(token, zone)?;
        let mut arguments = vec![
            "--json".to_owned(),
            "dns".to_owned(),
            "update".to_owned(),
            "--id".to_owned(),
            identifier.as_str().to_owned(),
        ];
        arguments.extend(Self::record_arguments(&zone_name, record));
        self.execute(arguments, token)?;
        self.find_record_after_mutation(token, zone, record)
    }

    fn delete_record(
        &self,
        token: &Token,
        zone: &ZoneIdentifier,
        identifier: &RecordIdentifier,
    ) -> Result<()> {
        let zone = self.zone_name(token, zone)?;
        self.execute(
            vec![
                "--json".to_owned(),
                "dns".to_owned(),
                "delete".to_owned(),
                "--zone".to_owned(),
                zone.as_str().to_owned(),
                "--id".to_owned(),
                identifier.as_str().to_owned(),
            ],
            token,
        )?;
        Ok(())
    }
}

impl std::fmt::Debug for FlarectlApi {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FlarectlApi")
            .field("binary", &self.binary)
            .field("runner", &"<command runner>")
            .finish()
    }
}

#[derive(Debug, Deserialize)]
struct FlarectlZone {
    #[serde(alias = "ID", alias = "id")]
    identifier: String,
    #[serde(alias = "Name", alias = "name", alias = "Zone")]
    name: String,
}

impl From<FlarectlZone> for ApiZone {
    fn from(zone: FlarectlZone) -> Self {
        Self {
            identifier: ZoneIdentifier::new(zone.identifier),
            name: DomainName::new(zone.name),
        }
    }
}

#[derive(Debug, Deserialize)]
struct FlarectlRecord {
    #[serde(alias = "ID", alias = "id")]
    identifier: String,
    #[serde(alias = "Name", alias = "name")]
    name: String,
    #[serde(alias = "Type", alias = "type")]
    kind: String,
    #[serde(alias = "Content", alias = "content")]
    content: String,
    #[serde(default, alias = "Proxy", alias = "Proxied", alias = "proxied")]
    proxied: Option<StringOrBool>,
}

impl TryFrom<FlarectlRecord> for ApiRecord {
    type Error = Error;

    fn try_from(record: FlarectlRecord) -> Result<Self> {
        Ok(Self {
            identifier: RecordIdentifier::new(record.identifier),
            name: DomainName::new(record.name),
            kind: RecordKindName::new(record.kind).into_record_kind()?,
            value: RecordValue::new(record.content),
            proxy_mode: if record.proxied.is_some_and(StringOrBool::into_bool) {
                ProxyMode::ProviderProxy
            } else {
                ProxyMode::Direct
            },
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StringOrBool {
    Bool(bool),
    String(String),
}

impl StringOrBool {
    fn into_bool(self) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::String(value) => value == "true",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordKindName(String);

impl RecordKindName {
    fn new(value: String) -> Self {
        Self(value)
    }

    fn from_record_kind(kind: RecordKind) -> &'static str {
        match kind {
            RecordKind::AddressV4 => "A",
            RecordKind::AddressV6 => "AAAA",
            RecordKind::CanonicalName => "CNAME",
            RecordKind::Text => "TXT",
            RecordKind::MailExchange => "MX",
            RecordKind::NameServer => "NS",
            RecordKind::Pointer => "PTR",
            RecordKind::Service => "SRV",
            RecordKind::CertificateAuthorityAuthorization => "CAA",
            RecordKind::SecureShellFingerprint => "SSHFP",
            RecordKind::TransportLayerSecurityAuthentication => "TLSA",
            RecordKind::UniformResourceIdentifier => "URI",
            RecordKind::ServiceBinding => "SVCB",
            RecordKind::HttpsBinding => "HTTPS",
            RecordKind::Location => "LOC",
        }
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug)]
    struct CapturingRunner {
        outputs: Mutex<Vec<Vec<u8>>>,
        captured: Mutex<Vec<(FlarectlBinary, Vec<String>)>>,
    }

    impl CapturingRunner {
        fn with_outputs(outputs: Vec<Vec<u8>>) -> Arc<Self> {
            Arc::new(Self {
                outputs: Mutex::new(outputs),
                captured: Mutex::new(Vec::new()),
            })
        }

        fn with_output(bytes: Vec<u8>) -> Arc<Self> {
            Self::with_outputs(vec![bytes])
        }

        fn last(&self) -> (FlarectlBinary, Vec<String>) {
            self.captured
                .lock()
                .expect("captured mutex")
                .last()
                .cloned()
                .expect("at least one invocation")
        }
    }

    impl CommandRunner for CapturingRunner {
        fn run(
            &self,
            binary: &FlarectlBinary,
            arguments: &[String],
            _token: &Token,
        ) -> Result<Vec<u8>> {
            self.captured
                .lock()
                .expect("captured mutex")
                .push((binary.clone(), arguments.to_vec()));
            let mut outputs = self.outputs.lock().expect("output mutex");
            if outputs.len() == 1 {
                return Ok(outputs[0].clone());
            }
            Ok(outputs.remove(0))
        }
    }

    #[test]
    fn zone_list_uses_flarectl_json_zone_list() {
        let runner = CapturingRunner::with_output(b"[]".to_vec());
        let api = FlarectlApi::with_runner(runner.clone());
        let zones = api.zones(&Token::new("ignored"), None).expect("zones");
        assert_eq!(zones, Vec::new());
        let (binary, arguments) = runner.last();
        assert_eq!(binary.as_str(), "flarectl");
        assert_eq!(arguments, vec!["--json", "zone", "list"]);
    }

    #[test]
    fn records_resolves_zone_identifier_to_zone_name_then_lists_dns() {
        let runner = CapturingRunner::with_outputs(vec![
            br#"[{"ID":"zone-one","Name":"goldragon.criome"}]"#.to_vec(),
            br#"[{"ID":"record-one","Name":"goldragon.criome","Type":"A","Content":"203.0.113.7","Proxy":"true"}]"#.to_vec(),
        ]);
        let api = FlarectlApi::with_runner(runner.clone());
        let records = api
            .records(&Token::new("ignored"), &ZoneIdentifier::new("zone-one"))
            .expect("records");
        let (_, arguments) = runner.last();
        assert_eq!(
            arguments,
            vec!["--json", "dns", "list", "--zone", "goldragon.criome"]
        );
        assert_eq!(records[0].identifier, RecordIdentifier::new("record-one"));
        assert_eq!(records[0].proxy_mode, ProxyMode::ProviderProxy);
    }

    #[test]
    fn create_record_uses_flarectl_dns_create() {
        let runner = CapturingRunner::with_outputs(vec![
            br#"[{"ID":"zone-one","Name":"goldragon.criome"}]"#.to_vec(),
            b"[]".to_vec(),
            br#"[{"ID":"zone-one","Name":"goldragon.criome"}]"#.to_vec(),
            br#"[{"ID":"record-two","Name":"www.goldragon.criome","Type":"CNAME","Content":"goldragon.criome","Proxy":"true"}]"#.to_vec(),
        ]);
        let api = FlarectlApi::with_runner(runner.clone());
        let record = DomainNameSystemRecord {
            name: DomainName::new("www.goldragon.criome"),
            kind: RecordKind::CanonicalName,
            value: RecordValue::new("goldragon.criome"),
            proxy_mode: ProxyMode::ProviderProxy,
        };
        let created = api
            .create_record(
                &Token::new("ignored"),
                &ZoneIdentifier::new("zone-one"),
                &record,
            )
            .expect("created");
        assert_eq!(created.identifier, RecordIdentifier::new("record-two"));
        let captured = runner.captured.lock().expect("captured");
        assert!(captured.iter().any(|(_, arguments)| arguments
            == &vec![
                "--json".to_owned(),
                "dns".to_owned(),
                "create".to_owned(),
                "--zone".to_owned(),
                "goldragon.criome".to_owned(),
                "--name".to_owned(),
                "www.goldragon.criome".to_owned(),
                "--type".to_owned(),
                "CNAME".to_owned(),
                "--content".to_owned(),
                "goldragon.criome".to_owned(),
                "--proxy".to_owned(),
            ]));
    }

    #[test]
    fn record_kind_round_trips() {
        let kinds = [
            RecordKind::AddressV4,
            RecordKind::AddressV6,
            RecordKind::CanonicalName,
            RecordKind::Text,
            RecordKind::MailExchange,
        ];
        for kind in kinds {
            let name = RecordKindName::from_record_kind(kind).to_owned();
            assert_eq!(RecordKindName::new(name).into_record_kind().unwrap(), kind);
        }
    }
}
