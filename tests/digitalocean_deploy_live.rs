//! Live CriomOS-on-DigitalOcean deploy harness (re-usable, Tier 1).
//!
//! Provisions a DigitalOcean droplet FROM A PRE-MADE IMAGE — a CriomOS
//! snapshot numeric id via `CRIOMOS_IMAGE`, or a stock distribution slug —
//! through the production in-process adapter (`digitalocean::HttpApi`, not
//! `doctl`), polls it to `Running`, optionally SSHes in to confirm the running
//! system is CriomOS and (when `DEPLOY_FLAKE` is set) pushes a generation with
//! `nixos-rebuild switch --target-host`, then ALWAYS destroys every resource
//! through a `Drop` guard. `#[ignore]` + feature-gated so CI never spends money.
//!
//! Boot-mode note (verified 2026-06): DigitalOcean droplets boot legacy
//! BIOS/GRUB, never UEFI, so a deploy here is the bootloader-agnostic
//! `nixos-rebuild switch` (`switch-to-configuration switch`), NOT lojix's
//! `bootctl`/`BootOnce` activation, which requires systemd-boot UEFI.
//!
//! Run (mode 2, stock image, ssh-reachable confirm — works on the current token):
//!   export DIGITALOCEAN_ACCESS_TOKEN=$(gopass show -o digitalocean.com/api-token)
//!   cargo test --features digitalocean --test digitalocean_deploy_live -- --ignored --nocapture
//!
//! Run (mode 1, pre-made CriomOS image, once a snapshot id exists):
//!   CRIOMOS_IMAGE=<numeric-snapshot-id> DO_REGION=<image-home-region> \
//!     cargo test --features digitalocean --test digitalocean_deploy_live -- --ignored --nocapture
#![cfg(feature = "digitalocean")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cloud::digitalocean::{
    Api, ApiServer, DEFAULT_IMAGE, DEFAULT_REGION, DEFAULT_SIZE, HttpApi,
    Result as DigitalOceanResult, ServerSpec, Token,
};
use signal_cloud::{HostIdentifier, HostStatus};

/// Prefix on every resource the harness creates, so the flake wrapper's
/// safety-net sweep can prefix-match without colliding with real droplets.
const RESOURCE_PREFIX: &str = "criome-deploy-test";

#[test]
#[ignore = "live: provisions a real DigitalOcean droplet (spends money); needs DIGITALOCEAN_ACCESS_TOKEN + ssh-keygen"]
fn criomos_deploys_on_digitalocean_and_always_destroys() {
    let token = Token::new(
        std::env::var("DIGITALOCEAN_ACCESS_TOKEN").expect("DIGITALOCEAN_ACCESS_TOKEN must be set"),
    );
    let parameters = DeployParameters::from_environment();
    let api = HttpApi::new();
    let key = TemporarySshKey::new(RESOURCE_PREFIX);
    let mut cleanup = DeployCleanup::new(&api, &token);

    let fingerprint = api
        .ensure_ssh_key(&token, key.name(), key.public_key())
        .expect("ensure_ssh_key against the live account");
    cleanup.track_ssh_key(fingerprint.clone());
    println!("ssh key ready: {} ({fingerprint})", key.name());
    println!(
        "mode: {} (image={})",
        if parameters.is_custom_image() {
            "1 — pre-made image (custom snapshot id)"
        } else {
            "2 — stock distribution slug"
        },
        parameters.image,
    );

    let spec = ServerSpec {
        name: key.name().to_owned(),
        server_type: parameters.size.clone(),
        image: parameters.image.clone(),
        ssh_keys: vec![key.name().to_owned()],
        location: Some(parameters.region.clone()),
    };
    let created = api.create_server(&token, &spec).expect("create_server");
    let identifier = created.identifier.clone();
    cleanup.track_droplet(identifier.clone());
    println!(
        "droplet created: id={} name={} status={:?}",
        identifier.as_str(),
        key.name(),
        created.status,
    );

    let host = DropletPoll::new(&api, &token, &identifier)
        .until_running(&parameters.poll)
        .expect("droplet reached Running with an IPv4 within the poll budget");
    println!(
        "final observed: status={:?} ipv4={}",
        host.status,
        host.ipv4.as_str(),
    );

    let deploy_level = DeployConfirmation::new(&host, key.private_key_path(), &parameters).resolve();
    println!("criomos-confirm: {deploy_level}");

    // Explicit teardown BEFORE the asserts, so a clean run leaves nothing
    // running even while we assert; the Drop guard still covers every
    // early-return / panic path.
    cleanup.tear_down_and_log();

    let ipv4 = host.ipv4.as_str().to_owned();
    assert!(
        ipv4.contains('.'),
        "a Running droplet must expose an IPv4; got {ipv4}",
    );
    println!(
        "DEPLOY WITNESS droplet_id={} ipv4={ipv4} region={} image={} deploy={} result=OK",
        identifier.as_str(),
        parameters.region,
        parameters.image,
        deploy_level.as_witness_field(),
    );
}

/// Every knob the harness reads, each with a `digitalocean` module-const
/// default so an unset run still does something cheap and safe.
struct DeployParameters {
    image: String,
    region: String,
    size: String,
    ssh_confirm: bool,
    marker: String,
    deploy_flake: Option<String>,
    deploy_attribute: String,
    poll: PollBudget,
    ssh_attempts: u32,
}

impl DeployParameters {
    fn from_environment() -> Self {
        Self {
            image: Self::variable_or("CRIOMOS_IMAGE", DEFAULT_IMAGE),
            region: Self::variable_or("DO_REGION", DEFAULT_REGION),
            size: Self::variable_or("DO_SIZE", DEFAULT_SIZE),
            ssh_confirm: std::env::var("DEPLOY_SSH_CONFIRM")
                .map(|value| value != "0")
                .unwrap_or(true),
            // Default marker matches any NixOS node; a CriomOS image can set a
            // sharper string (e.g. a `/etc/os-release` ID or a node marker).
            marker: Self::variable_or("CRIOMOS_MARKER", "ID=nixos"),
            deploy_flake: std::env::var("DEPLOY_FLAKE").ok().filter(|value| !value.is_empty()),
            deploy_attribute: Self::variable_or("DEPLOY_ATTRIBUTE", "target"),
            poll: PollBudget::from_environment(),
            ssh_attempts: Self::number_or("DEPLOY_SSH_ATTEMPTS", 30),
        }
    }

    fn variable_or(name: &str, fallback: &str) -> String {
        std::env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| fallback.to_owned())
    }

    fn number_or(name: &str, fallback: u32) -> u32 {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback)
    }

    /// A numeric image string is a pre-made (CriomOS) snapshot id (mode 1); a
    /// slug like `ubuntu-24-04-x64` is a stock distribution image (mode 2).
    fn is_custom_image(&self) -> bool {
        !self.image.is_empty() && self.image.chars().all(|character| character.is_ascii_digit())
    }
}

struct PollBudget {
    interval: Duration,
    attempts: u32,
}

impl PollBudget {
    fn from_environment() -> Self {
        Self {
            interval: Duration::from_secs(u64::from(DeployParameters::number_or(
                "DEPLOY_POLL_SECONDS",
                5,
            ))),
            attempts: DeployParameters::number_or("DEPLOY_POLL_ATTEMPTS", 36),
        }
    }
}

/// Polls one droplet to `Running` over the live adapter.
struct DropletPoll<'a> {
    api: &'a HttpApi,
    token: &'a Token,
    identifier: &'a HostIdentifier,
}

impl<'a> DropletPoll<'a> {
    fn new(api: &'a HttpApi, token: &'a Token, identifier: &'a HostIdentifier) -> Self {
        Self {
            api,
            token,
            identifier,
        }
    }

    /// Returns the first observation that is `Running` with an IPv4, or the
    /// last observation if the budget is exhausted without reaching it.
    fn until_running(&self, budget: &PollBudget) -> Option<ApiServer> {
        let mut latest = None;
        for attempt in 0..budget.attempts {
            match self.api.get_server(self.token, self.identifier) {
                Ok(host) => {
                    let running =
                        host.status == HostStatus::Running && host.ipv4.as_str().contains('.');
                    latest = Some(host);
                    if running {
                        return latest;
                    }
                }
                Err(error) => eprintln!("poll {attempt}: get_server error: {error}"),
            }
            sleep(budget.interval);
        }
        latest
    }
}

/// Resolves the honest confirm level of a provisioned node: it SSHes in,
/// optionally pushes a generation with `nixos-rebuild switch`, and reads the
/// OS marker. A data-bearing type so `resolve` is a real method.
struct DeployConfirmation<'a> {
    host: &'a ApiServer,
    private_key_path: &'a Path,
    parameters: &'a DeployParameters,
}

impl<'a> DeployConfirmation<'a> {
    fn new(host: &'a ApiServer, private_key_path: &'a Path, parameters: &'a DeployParameters) -> Self {
        Self {
            host,
            private_key_path,
            parameters,
        }
    }

    fn resolve(&self) -> DeployLevel {
        if !self.parameters.ssh_confirm {
            return DeployLevel::RunningOnly;
        }
        let address = self.host.ipv4.as_str();
        if !self.wait_for_ssh(address) {
            eprintln!("ssh never became reachable on {address}; reporting running-only");
            return DeployLevel::RunningOnly;
        }

        if let Some(flake) = &self.parameters.deploy_flake {
            let target = format!("{flake}#{}", self.parameters.deploy_attribute);
            println!("deploy: nixos-rebuild switch --flake {target} --target-host root@{address}");
            if !self.run_remote_switch(address, flake) {
                return DeployLevel::DeployFailed;
            }
        }

        match self.read_release(address) {
            Some(release) if release.contains(&self.parameters.marker) => {
                println!("criomos-confirm marker matched: {}", self.parameters.marker);
                DeployLevel::CriomosConfirmed
            }
            Some(_) => DeployLevel::SshReachable,
            None => DeployLevel::SshReachable,
        }
    }

    fn wait_for_ssh(&self, address: &str) -> bool {
        for attempt in 0..self.parameters.ssh_attempts {
            if self.ssh(address, "true").map(|status| status.success()).unwrap_or(false) {
                return true;
            }
            if attempt + 1 < self.parameters.ssh_attempts {
                sleep(Duration::from_secs(5));
            }
        }
        false
    }

    fn run_remote_switch(&self, address: &str, flake: &str) -> bool {
        let target = format!("{flake}#{}", self.parameters.deploy_attribute);
        let status = Command::new("nixos-rebuild")
            .arg("switch")
            .arg("--flake")
            .arg(&target)
            .arg("--target-host")
            .arg(format!("root@{address}"))
            .arg("--option")
            .arg("accept-flake-config")
            .arg("true")
            .env("NIX_SSHOPTS", self.ssh_options().join(" "))
            .status();
        match status {
            Ok(status) if status.success() => {
                println!("deploy: nixos-rebuild switch succeeded");
                true
            }
            Ok(status) => {
                eprintln!("deploy: nixos-rebuild switch failed: {status}");
                false
            }
            Err(error) => {
                eprintln!("deploy: could not spawn nixos-rebuild ({error}); is it on PATH?");
                false
            }
        }
    }

    fn read_release(&self, address: &str) -> Option<String> {
        let output = Command::new("ssh")
            .args(self.ssh_options())
            .arg(format!("root@{address}"))
            .arg("cat /etc/os-release")
            .output()
            .ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            None
        }
    }

    fn ssh(&self, address: &str, remote_command: &str) -> std::io::Result<std::process::ExitStatus> {
        Command::new("ssh")
            .args(self.ssh_options())
            .arg(format!("root@{address}"))
            .arg(remote_command)
            .status()
    }

    fn ssh_options(&self) -> Vec<String> {
        vec![
            "-i".to_owned(),
            self.private_key_path.display().to_string(),
            "-o".to_owned(),
            "StrictHostKeyChecking=accept-new".to_owned(),
            "-o".to_owned(),
            "UserKnownHostsFile=/dev/null".to_owned(),
            "-o".to_owned(),
            "ConnectTimeout=10".to_owned(),
            "-o".to_owned(),
            "BatchMode=yes".to_owned(),
        ]
    }
}

/// The honest level a deploy reached — surfaced in the witness line so a
/// reader never mistakes "ssh worked" for "CriomOS is running."
enum DeployLevel {
    CriomosConfirmed,
    SshReachable,
    RunningOnly,
    DeployFailed,
}

impl DeployLevel {
    fn as_witness_field(&self) -> &'static str {
        match self {
            DeployLevel::CriomosConfirmed => "criomos-confirmed",
            DeployLevel::SshReachable => "ssh-reachable",
            DeployLevel::RunningOnly => "running-only",
            DeployLevel::DeployFailed => "deploy-failed",
        }
    }
}

impl std::fmt::Display for DeployLevel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let description = match self {
            DeployLevel::CriomosConfirmed => "criomos-confirmed (ssh read the CriomOS marker)",
            DeployLevel::SshReachable => "ssh-reachable (connected; no CriomOS marker)",
            DeployLevel::RunningOnly => "running-only (ssh confirm disabled or unreachable)",
            DeployLevel::DeployFailed => "deploy-failed (nixos-rebuild switch did not succeed)",
        };
        formatter.write_str(description)
    }
}

/// A throwaway ed25519 key whose public half DigitalOcean injects into the
/// droplet's `root` at first boot and whose private half stays in a tempdir —
/// exactly the identity the deploy step needs to reach the node.
struct TemporarySshKey {
    name: String,
    public_key: String,
    private_key_path: PathBuf,
    _directory: tempfile::TempDir,
}

impl TemporarySshKey {
    fn new(prefix: &str) -> Self {
        let directory = tempfile::tempdir().expect("temporary ssh-key directory");
        let name = Self::unique_name(prefix);
        let private_key_path = directory.path().join("deploy-key");
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
        let public_key = std::fs::read_to_string(private_key_path.with_extension("pub"))
            .expect("read generated public key");
        Self {
            name,
            public_key,
            private_key_path,
            _directory: directory,
        }
    }

    fn unique_name(prefix: &str) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        format!("{prefix}-{}-{timestamp}", std::process::id())
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn public_key(&self) -> &str {
        &self.public_key
    }

    fn private_key_path(&self) -> &Path {
        &self.private_key_path
    }
}

/// Generalizes the Tier-1 `LiveCleanup`: holds each created resource as an
/// `Option<typed-id>` and tears them down in dependency order, retrying on
/// failure. `Drop` fires on success, `assert!` failure, `?`-return, and panic.
struct DeployCleanup<'a> {
    api: &'a HttpApi,
    token: &'a Token,
    droplet: Option<HostIdentifier>,
    ssh_key_fingerprint: Option<String>,
}

impl<'a> DeployCleanup<'a> {
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

    /// Tears down in dependency order (droplet, then the key it authorized) and
    /// logs each outcome, so a clean run's witness reflects an emptied account.
    fn tear_down_and_log(&mut self) {
        println!("destroy issued: {:?}", self.destroy_droplet());
        println!("ssh key delete issued: {:?}", self.delete_ssh_key());
    }
}

impl Drop for DeployCleanup<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.destroy_droplet() {
            eprintln!("live cleanup failed to destroy droplet: {error}");
        }
        if let Err(error) = self.delete_ssh_key() {
            eprintln!("live cleanup failed to delete ssh key: {error}");
        }
    }
}
