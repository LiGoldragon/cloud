//! Live wiring test for the cloud schema-engine daemon: bind the two
//! authority-tiered sockets, drive a real request through the emitted
//! `ActorMultiListenerDaemon` -> `decode_signal_frame` ->
//! `SchemaRuntime::execute` -> reply, and decode the reply off the socket.
//! Proves the schema engine is wired to a live daemon (not just unit-tested in
//! `tests/schema_runtime.rs`).

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

static DAEMON_INSTANCE: AtomicU32 = AtomicU32::new(0);

use cloud::DaemonConfiguration;
use cloud::schema_daemon::SchemaDaemon;
use meta_signal_cloud::schema::lib as meta;
use signal_cloud::schema::lib as ordinary;
use triad_runtime::{FrameBody, LengthPrefixedCodec};

/// A connected client speaking the length-prefixed schema-frame wire. Holds the
/// stream and the codec so a request can be written and the reply read back.
struct SocketClient {
    stream: UnixStream,
    codec: LengthPrefixedCodec,
}

impl SocketClient {
    fn connect(path: &PathBuf) -> Self {
        let stream = Self::connect_with_retry(path);
        Self {
            stream,
            codec: LengthPrefixedCodec::default(),
        }
    }

    /// The daemon binds its sockets on its own thread; retry briefly until the
    /// socket file exists and accepts a connection.
    fn connect_with_retry(path: &PathBuf) -> UnixStream {
        for _ in 0..100 {
            if let Ok(stream) = UnixStream::connect(path) {
                return stream;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("daemon socket {path:?} never became connectable");
    }

    fn request_ordinary(&mut self, input: ordinary::Input) -> ordinary::Output {
        let body = FrameBody::new(input.encode_signal_frame().expect("encode ordinary input"));
        self.codec
            .write_body(&mut self.stream, &body)
            .expect("write ordinary request");
        let reply = self.codec.read_body(&mut self.stream).expect("read reply");
        let (_, output) =
            ordinary::Output::decode_signal_frame(reply.bytes()).expect("decode ordinary output");
        output
    }

    fn request_meta(&mut self, input: meta::Input) -> meta::Output {
        let body = FrameBody::new(input.encode_signal_frame().expect("encode meta input"));
        self.codec
            .write_body(&mut self.stream, &body)
            .expect("write meta request");
        let reply = self.codec.read_body(&mut self.stream).expect("read reply");
        let (_, output) =
            meta::Output::decode_signal_frame(reply.bytes()).expect("decode meta output");
        output
    }
}

fn spawn_daemon() -> (PathBuf, PathBuf) {
    let instance = DAEMON_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "cloud-schema-daemon-{}-{instance}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create daemon socket directory");
    let ordinary_socket_path = directory.join("cloud.sock");
    let meta_socket_path = directory.join("cloud-meta.sock");
    let configuration = DaemonConfiguration {
        ordinary_socket_path: ordinary_socket_path.to_string_lossy().into_owned(),
        ordinary_socket_mode: 0o600,
        meta_socket_path: meta_socket_path.to_string_lossy().into_owned(),
        meta_socket_mode: 0o600,
    };
    thread::spawn(move || {
        let _ = SchemaDaemon::new(configuration).run();
    });
    (ordinary_socket_path, meta_socket_path)
}

#[test]
fn schema_daemon_serves_ordinary_capability_observation_over_socket() {
    let (ordinary_socket_path, _meta_socket_path) = spawn_daemon();
    let mut client = SocketClient::connect(&ordinary_socket_path);

    let output = client.request_ordinary(ordinary::Input::Observe(
        ordinary::Observation::Capabilities(ordinary::CapabilityQuery {
            provider: Some(ordinary::Provider::Cloudflare),
            capability: Some(ordinary::Capability::DomainNameSystemRecords),
        }),
    ));

    match output {
        ordinary::Output::Observed(ordinary::ObservationResult::Capabilities(report)) => {
            assert_eq!(report.payload().len(), 1);
            assert_eq!(
                report.payload()[0].capability_state,
                ordinary::CapabilityState::Compiled
            );
        }
        other => panic!("expected capability observation reply, got {other:?}"),
    }
}

#[test]
fn schema_daemon_serves_meta_registration_over_socket() {
    let (_ordinary_socket_path, meta_socket_path) = spawn_daemon();
    let mut client = SocketClient::connect(&meta_socket_path);

    let output = client.request_meta(meta::Input::RegisterAccount(meta::Registration {
        provider: meta::Provider::Cloudflare,
        provider_account: String::from("primary"),
        credential_handle: String::from("cloudflare/api-token"),
    }));

    match output {
        meta::Output::AccountRegistered(registered) => {
            assert_eq!(registered.provider, meta::Provider::Cloudflare);
            assert_eq!(registered.provider_account, "primary");
        }
        other => panic!("expected account-registered reply, got {other:?}"),
    }
}
