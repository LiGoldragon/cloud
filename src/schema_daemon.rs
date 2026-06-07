//! cloud's daemon hooks — the only daemon code cloud hand-writes.
//!
//! The uniform daemon skeleton (the `DaemonCommand` argv parsing, the working
//! decode -> execute -> encode spine, the two-tier
//! `ActorMultiListenerDaemon` bind, and the `ExitReport` entry) is EMITTED into
//! `src/schema/daemon.rs` by
//! schema-rust-next's daemon emitter, driven by the two-tier `NexusDaemonShape`
//! in `build.rs`. cloud fills only the record-1488 escape hatches through `impl
//! ComponentDaemon for CloudDaemon`: how to load its `Configuration`, how to
//! open the shared `Arc<SchemaStore>` (`build_runtime`), how one ordinary
//! `Input` becomes one `Output` (`handle_working_input`), and the meta-only
//! meta tier (`handle_meta_connection`, whose meta wire codec is
//! component-owned).
//!
//! This retires the prior hand-written `SchemaDaemon` / `CloudRuntime` /
//! `serve_*` / `ListenerRole` plumbing into the emitted output (report 542 /
//! Spirit ocu7): cloud is the first multi-listener triad_main pilot and the
//! first whose working contract is a dependency crate (`signal-cloud`).

use std::sync::Arc;
use std::time::Duration;

use signal_cloud::schema::lib::{Input, Output};
use tokio::io::AsyncWriteExt;
use triad_runtime::{
    AcceptedConnection, ConnectionContext, FrameBody, LengthPrefixedCodec, MaximumFrameLength,
};

use crate::schema::daemon::{ComponentDaemon, DaemonBinder, DaemonError};
use crate::schema::nexus::{SignalInput, SignalOutput};
use crate::schema_runtime::SchemaRuntime;
use crate::schema_store::SchemaStore;
use crate::{DaemonConfiguration, Error, Result};

/// Maximum inbound meta-request-frame body the daemon accepts (8 MiB). A cloud
/// request is a few hundred bytes; this bounds a hostile length prefix far below
/// the 4 GiB the u32-prefix codec default would pre-allocate. The working tier's
/// frame bound is the emitted spine's concern; this guards the component-owned
/// meta escape hatch.
const MAXIMUM_REQUEST_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// How long the meta handler waits for a connected client to send its request
/// frame before dropping the stream. A legitimate client sends immediately.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// The type-level selector for cloud's emitted daemon. It carries no runtime
/// data — it is the marker the emitted `DaemonCommand<CloudDaemon>` and the
/// generated runtime dispatch on, selecting cloud's `Configuration` / `Engine`
/// / `Error` through the `ComponentDaemon` associated types.
pub struct CloudDaemon;

impl ComponentDaemon for CloudDaemon {
    type Configuration = DaemonConfiguration;
    type ConfigurationError = Error;
    /// The engine is the SHARED durable store. Each request builds its own
    /// per-request `SchemaRuntime` over a clone of this `Arc<SchemaStore>`
    /// (intent 2alg: per-request pipeline cursor, the durable tables the only
    /// shared, briefly-locked point), so the emitted spine can hold the engine
    /// behind a shared reference and serve connections concurrently.
    type Engine = Arc<SchemaStore>;
    type Error = Error;

    const PROCESS_NAME: &'static str = "cloud-daemon";

    fn load_configuration(
        path: &std::path::Path,
    ) -> std::result::Result<Self::Configuration, Self::ConfigurationError> {
        let bytes = std::fs::read(path)?;
        DaemonConfiguration::from_rkyv_bytes(&bytes)
    }

    fn build_runtime(_configuration: &Self::Configuration) -> Result<Self::Engine> {
        Ok(Arc::new(SchemaStore::new()))
    }

    /// Run one decoded ordinary `Input` through a per-request engine over the
    /// shared store and return the ordinary `Output`. The schema engine is the
    /// single routing brain; cloud does not classify by origin yet, so the
    /// peer-credential `connection` is unused.
    fn handle_working_input(
        engine: &Self::Engine,
        input: Input,
        _connection: &ConnectionContext,
    ) -> Result<Output> {
        match SchemaRuntime::reply_to_signal(engine.clone(), SignalInput::OrdinaryInput(input)) {
            SignalOutput::OrdinaryOutput(output) => Ok(output),
            SignalOutput::MetaOutput(_) => Err(Error::UnexpectedFrame),
        }
    }

    /// Serve one meta request end to end: decode a meta-signal-cloud
    /// `Input` off the length-prefixed frame, drive it through a per-request
    /// engine, and write the meta `Output` back. The meta wire codec is
    /// component-owned (the emitter routes the meta socket here rather than
    /// emitting a second frame spine), so this owns the full read/handle/write.
    async fn handle_meta_connection(
        engine: &Self::Engine,
        mut connection: AcceptedConnection,
    ) -> Result<()> {
        let codec = LengthPrefixedCodec::new(MaximumFrameLength::new(MAXIMUM_REQUEST_FRAME_BYTES));
        let body = tokio::time::timeout(
            REQUEST_READ_TIMEOUT,
            codec.read_body_async(connection.stream_mut()),
        )
        .await
        .map_err(|_| Error::RequestReadTimedOut)??;
        let (_route, input) =
            meta_signal_cloud::schema::lib::Input::decode_signal_frame(body.bytes())?;
        let reply =
            match SchemaRuntime::reply_to_signal(engine.clone(), SignalInput::MetaInput(input)) {
                SignalOutput::MetaOutput(output) => output,
                SignalOutput::OrdinaryOutput(_) => return Err(Error::UnexpectedFrame),
            };
        codec
            .write_body_async(
                connection.stream_mut(),
                &FrameBody::new(reply.encode_signal_frame()?),
            )
            .await?;
        connection.stream_mut().flush().await?;
        Ok(())
    }
}

/// A thin convenience wrapper so in-process launchers and tests keep the
/// familiar `SchemaDaemon::new(configuration).run()` surface over the emitted
/// `ComponentDaemon` binder. The bin uses the emitted `DaemonEntry` directly.
pub struct SchemaDaemon {
    configuration: DaemonConfiguration,
}

impl SchemaDaemon {
    pub fn new(configuration: DaemonConfiguration) -> Self {
        Self { configuration }
    }

    pub fn run(self) -> std::result::Result<(), DaemonError<CloudDaemon>> {
        tokio::runtime::Runtime::new()
            .map_err(DaemonError::Runtime)?
            .block_on(async {
                CloudDaemon::bind(self.configuration)?
                    .run()
                    .await
                    .map_err(DaemonError::from)
            })
    }
}
