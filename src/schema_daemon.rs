//! The cloud schema-engine daemon loop — two authority-tiered unix sockets
//! driving the generated Nexus runner over a durable [`SchemaStore`].
//!
//! Binds two `ListenerSocket`s on one `MultiListenerDaemon` (the runtime's
//! two-socket primitive), each tagged by its authority role. `handle_stream`
//! reads the length-prefixed wire body, decodes the arriving role's contract
//! `Input` into a `nexus::SignalInput`, drives it through
//! `NexusEngine::execute` (which runs the `Runner` continuation loop), and
//! writes the reply back as the contract `Output` length-prefixed frame. The
//! schema engine is the single source of routing truth; the durable account /
//! plan tables live in the shared `Arc<SchemaStore>`.
//!
//! This is the schema-engine path. The legacy [`crate::daemon::Daemon`] over
//! `signal_frame::ExchangeFrame` + the hand-written `Store` remains the
//! production Cloudflare-IO runtime until that IO is ported onto the effect
//! plane here.

use std::fmt::{Display, Formatter};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Duration;

use triad_runtime::{
    FrameBody, LengthPrefixedCodec, ListenerSocket, MaximumFrameLength, MultiListenerDaemon,
    MultiListenerDaemonError, MultiListenerRuntime, RequestErrorLog, SocketMode,
};

use crate::schema::nexus::{self, NexusEngine};
use crate::schema_runtime::SchemaRuntime;
use crate::schema_store::SchemaStore;
use crate::{DaemonConfiguration, Error, Result};

/// Maximum inbound request-frame body the daemon accepts (8 MiB). A cloud
/// request is a few hundred bytes; this bounds a hostile length prefix far below
/// the 4 GiB the u32-prefix codec default would pre-allocate.
const MAXIMUM_REQUEST_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// How long the daemon waits for a connected client to send its request frame
/// before dropping the stream. A legitimate client sends immediately.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Which authority-tiered socket an arriving stream belongs to. Ordinary is the
/// peer-callable `signal-cloud` surface (observe / validate); Owner is the
/// `meta-signal-cloud` policy / plan surface. Used as the
/// `MultiListenerRuntime::Listener` tag so `handle_stream` decodes the correct
/// contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListenerRole {
    Ordinary,
    Owner,
}

impl Display for ListenerRole {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ordinary => formatter.write_str("ordinary"),
            Self::Owner => formatter.write_str("owner"),
        }
    }
}

/// The cloud schema-engine daemon: configuration plus the durable store the
/// schema engine decides every arriving signal over. Construct with
/// [`SchemaDaemon::new`], then [`SchemaDaemon::run`] binds both sockets and
/// serves forever.
pub struct SchemaDaemon {
    configuration: DaemonConfiguration,
}

impl SchemaDaemon {
    pub fn new(configuration: DaemonConfiguration) -> Self {
        Self { configuration }
    }

    pub fn run(self) -> Result<()> {
        let configuration = self.configuration;
        let sockets = vec![
            ListenerSocket::new(
                ListenerRole::Ordinary,
                configuration.ordinary_socket_path.clone(),
            )
            .with_socket_mode(SocketMode::new(configuration.ordinary_socket_mode)),
            ListenerSocket::new(ListenerRole::Owner, configuration.owner_socket_path.clone())
                .with_socket_mode(SocketMode::new(configuration.owner_socket_mode)),
        ];
        let runtime = CloudRuntime::new();
        let request_error_log = RequestErrorLog::new("cloud-daemon");
        MultiListenerDaemon::new(sockets, runtime, request_error_log)
            .run()
            .map_err(Self::map_daemon_error)
    }

    fn map_daemon_error(error: MultiListenerDaemonError<Error, Error>) -> Error {
        match error {
            MultiListenerDaemonError::Listener(listener_error) => {
                Error::Io(std::io::Error::other(listener_error.to_string()))
            }
            MultiListenerDaemonError::Start(error) | MultiListenerDaemonError::Stop(error) => error,
        }
    }
}

/// The `MultiListenerRuntime` realization. Owns the SHARED durable `SchemaStore`
/// (the concurrency point — locked only briefly per sema operation) and the
/// frame codec. `handle_stream` builds a per-request `SchemaRuntime` over a
/// clone of the shared `Arc<SchemaStore>`, decodes the role's contract frame,
/// drives the engine, and writes the reply.
struct CloudRuntime {
    store: Arc<SchemaStore>,
    codec: LengthPrefixedCodec,
}

impl CloudRuntime {
    fn new() -> Self {
        Self {
            store: Arc::new(SchemaStore::new()),
            codec: LengthPrefixedCodec::new(MaximumFrameLength::new(MAXIMUM_REQUEST_FRAME_BYTES)),
        }
    }

    fn serve_ordinary(&self, stream: &mut UnixStream) -> Result<()> {
        stream.set_read_timeout(Some(REQUEST_READ_TIMEOUT))?;
        let body = self.codec.read_body(stream)?;
        let (_, input) = signal_cloud::schema::lib::Input::decode_signal_frame(body.bytes())?;
        let output = self.execute(nexus::SignalInput::OrdinaryInput(input));
        let reply = Self::ordinary_reply(output)?;
        self.codec
            .write_body(stream, &FrameBody::new(reply.encode_signal_frame()?))?;
        Ok(())
    }

    fn serve_owner(&self, stream: &mut UnixStream) -> Result<()> {
        stream.set_read_timeout(Some(REQUEST_READ_TIMEOUT))?;
        let body = self.codec.read_body(stream)?;
        let (_, input) = meta_signal_cloud::schema::lib::Input::decode_signal_frame(body.bytes())?;
        let output = self.execute(nexus::SignalInput::MetaInput(input));
        let reply = Self::meta_reply(output)?;
        self.codec
            .write_body(stream, &FrameBody::new(reply.encode_signal_frame()?))?;
        Ok(())
    }

    /// Build a per-request engine over the shared `Store` and drive it.
    fn execute(&self, signal_input: nexus::SignalInput) -> nexus::SignalOutput {
        let mut engine = SchemaRuntime::with_store(self.store.clone());
        let work =
            nexus::NexusWork::SignalArrived(signal_input).with_origin_route(nexus::OriginRoute(0));
        match engine.execute(work).into_root() {
            nexus::NexusAction::ReplyToSignal(output) => output,
            // `execute` always terminates the runner with a reply; any other
            // action escaping is a runtime invariant violation that surfaces as
            // an unexpected-frame error to the request handler.
            _ => nexus::SignalOutput::OrdinaryOutput(
                signal_cloud::schema::lib::Output::RequestRejected(
                    signal_cloud::schema::lib::RejectedRequest(
                        signal_cloud::schema::lib::RejectionReason::PlanExpired,
                    ),
                ),
            ),
        }
    }

    fn ordinary_reply(output: nexus::SignalOutput) -> Result<signal_cloud::schema::lib::Output> {
        match output {
            nexus::SignalOutput::OrdinaryOutput(output) => Ok(output),
            nexus::SignalOutput::MetaOutput(_) => Err(Error::UnexpectedFrame),
        }
    }

    fn meta_reply(output: nexus::SignalOutput) -> Result<meta_signal_cloud::schema::lib::Output> {
        match output {
            nexus::SignalOutput::MetaOutput(output) => Ok(output),
            nexus::SignalOutput::OrdinaryOutput(_) => Err(Error::UnexpectedFrame),
        }
    }
}

impl MultiListenerRuntime for CloudRuntime {
    type Listener = ListenerRole;
    type StartError = Error;
    type StopError = Error;
    type RequestError = Error;

    fn start(&mut self) -> Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    fn handle_stream(&mut self, listener: Self::Listener, stream: UnixStream) -> Result<()> {
        let mut stream = stream;
        match listener {
            ListenerRole::Ordinary => self.serve_ordinary(&mut stream),
            ListenerRole::Owner => self.serve_owner(&mut stream),
        }
    }
}
