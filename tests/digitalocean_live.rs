//! Live DigitalOcean lifecycle smoke test (adapter / Tier 1).
//!
//! Drives `digitalocean::HttpApi` directly against the real DigitalOcean v2
//! REST API: create a unique throwaway SSH key, create the cheapest droplet,
//! poll until it reports `Running`, then destroy both resources. `#[ignore]` by
//! default because it spends real (sub-cent) money and needs live credentials.
//!
//! Run:
//!   export DIGITALOCEAN_ACCESS_TOKEN=$(gopass show -o digitalocean.com/api-token)
//!   cargo test --features digitalocean --test digitalocean_live -- --ignored --nocapture
//!
//! The test also needs `ssh-keygen` on PATH so each run can mint a unique public
//! key. That makes the SSH-key registration a real pre-droplet write-scope probe.
#![cfg(feature = "digitalocean")]

use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cloud::digitalocean::{
    Api, DEFAULT_IMAGE, DEFAULT_REGION, DEFAULT_SIZE, HttpApi, Result as DigitalOceanResult,
    ServerSpec, Token,
};
use signal_cloud::{HostIdentifier, HostStatus};

#[test]
#[ignore = "live: spends real DigitalOcean money; needs DIGITALOCEAN_ACCESS_TOKEN + ssh-keygen"]
fn digitalocean_full_lifecycle_runs_against_the_real_api() {
    let token = Token::new(
        std::env::var("DIGITALOCEAN_ACCESS_TOKEN").expect("DIGITALOCEAN_ACCESS_TOKEN must be set"),
    );
    let api = HttpApi::new();
    let key = TemporarySshKey::new();
    let mut cleanup = LiveCleanup::new(&api, &token);

    let fingerprint = api
        .ensure_ssh_key(&token, key.name(), key.public_key())
        .expect("ensure_ssh_key against the live account");
    cleanup.track_ssh_key(fingerprint.clone());
    println!("ssh key ready: {} ({fingerprint})", key.name());

    let spec = ServerSpec {
        name: key.name().to_owned(),
        server_type: DEFAULT_SIZE.to_owned(),
        image: DEFAULT_IMAGE.to_owned(),
        ssh_keys: vec![key.name().to_owned()],
        location: Some(DEFAULT_REGION.to_owned()),
    };

    let created = api.create_server(&token, &spec).expect("create_server");
    let identifier = created.identifier.clone();
    cleanup.track_droplet(identifier.clone());
    println!(
        "droplet created: id={} name={} status={:?}",
        identifier.as_str(),
        key.name(),
        created.status
    );

    let mut latest = Some(created);
    let mut reached_running = false;
    for attempt in 0..36 {
        match api.get_server(&token, &identifier) {
            Ok(host) => {
                let running = host.status == HostStatus::Running;
                latest = Some(host);
                if running {
                    reached_running = true;
                    break;
                }
            }
            Err(error) => eprintln!("poll {attempt}: get_server error: {error}"),
        }
        sleep(Duration::from_secs(5));
    }

    if let Some(host) = &latest {
        println!(
            "final observed: status={:?} ipv4={}",
            host.status,
            host.ipv4.as_str()
        );
    }

    let destroy = cleanup.destroy_droplet();
    println!("destroy issued: {destroy:?}");
    let key_delete = cleanup.delete_ssh_key();
    println!("ssh key delete issued: {key_delete:?}");

    let host = latest.expect("at least one observation");
    assert!(
        reached_running,
        "droplet never reached Running within 3 minutes; last status={:?}",
        host.status
    );
    assert!(
        host.ipv4.as_str().contains('.'),
        "a Running droplet should expose an IPv4; got {}",
        host.ipv4.as_str()
    );
    destroy.expect("delete_server");
    key_delete.expect("delete_ssh_key");
    println!(
        "LIVE lifecycle OK: created -> Running (ipv4 {}) -> destroyed",
        host.ipv4.as_str()
    );
}

struct TemporarySshKey {
    name: String,
    public_key: String,
    _directory: tempfile::TempDir,
}

impl TemporarySshKey {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary ssh-key directory");
        let name = Self::unique_name();
        let private_key_path = directory.path().join("digitalocean-live-test");
        let status = Command::new("ssh-keygen")
            .arg("-t")
            .arg("ed25519")
            .arg("-N")
            .arg("")
            .arg("-C")
            .arg(&name)
            .arg("-f")
            .arg(&private_key_path)
            .status()
            .expect("run ssh-keygen");
        assert!(status.success(), "ssh-keygen must create a throwaway key");
        let public_key = std::fs::read_to_string(Self::public_key_path(&private_key_path))
            .expect("read generated public key");
        Self {
            name,
            public_key,
            _directory: directory,
        }
    }

    fn unique_name() -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        format!("criome-live-test-{}-{timestamp}", std::process::id())
    }

    fn public_key_path(private_key_path: &std::path::Path) -> PathBuf {
        private_key_path.with_extension("pub")
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn public_key(&self) -> &str {
        &self.public_key
    }
}

struct LiveCleanup<'a> {
    api: &'a HttpApi,
    token: &'a Token,
    droplet: Option<HostIdentifier>,
    ssh_key_fingerprint: Option<String>,
}

impl<'a> LiveCleanup<'a> {
    fn new(api: &'a HttpApi, token: &'a Token) -> Self {
        Self {
            api,
            token,
            droplet: None,
            ssh_key_fingerprint: None,
        }
    }

    fn track_droplet(&mut self, identifier: HostIdentifier) {
        self.droplet = Some(identifier);
    }

    fn track_ssh_key(&mut self, fingerprint: String) {
        self.ssh_key_fingerprint = Some(fingerprint);
    }

    fn destroy_droplet(&mut self) -> DigitalOceanResult<()> {
        match self.droplet.take() {
            Some(identifier) => {
                let outcome = self.api.delete_server(self.token, &identifier);
                if outcome.is_err() {
                    self.droplet = Some(identifier);
                }
                outcome
            }
            None => Ok(()),
        }
    }

    fn delete_ssh_key(&mut self) -> DigitalOceanResult<()> {
        match self.ssh_key_fingerprint.take() {
            Some(fingerprint) => {
                let outcome = self.api.delete_ssh_key(self.token, &fingerprint);
                if outcome.is_err() {
                    self.ssh_key_fingerprint = Some(fingerprint);
                }
                outcome
            }
            None => Ok(()),
        }
    }
}

impl Drop for LiveCleanup<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.destroy_droplet() {
            eprintln!("live cleanup failed to destroy droplet: {error}");
        }
        if let Err(error) = self.delete_ssh_key() {
            eprintln!("live cleanup failed to delete ssh key: {error}");
        }
    }
}
