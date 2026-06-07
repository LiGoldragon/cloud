use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use meta_signal_cloud::{ChannelRequest as MetaRequest, Reply as MetaReply};
use nota_next::{NotaEncode, NotaSource};
use signal_cloud::{Reply as CloudReply, Request as CloudRequest};
use signal_frame::{
    CommandLineSocket, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, HandshakeReply,
    HandshakeRequest, LaneSequence, Reply as FrameReply, SessionEpoch, SubReply,
};

use crate::frame_io::{MetaFrameIo, OrdinaryFrameIo};
use crate::{Error, Result};

const DEFAULT_ORDINARY_SOCKET_PATH: &str = "/run/cloud/cloud.sock";
const DEFAULT_META_SOCKET_PATH: &str = "/run/cloud/cloud-meta.sock";
const ORDINARY_SOCKET_ENVIRONMENT_VARIABLE: &str = "CLOUD_SOCKET_PATH";
const META_SOCKET_ENVIRONMENT_VARIABLE: &str = "CLOUD_META_SOCKET_PATH";

signal_frame::signal_cli! {
    pub struct CommandLineDispatch {
        working signal_cloud::Operation;
        meta meta_signal_cloud::Operation;
    }
}

pub struct Client {
    ordinary_socket_path: PathBuf,
    meta_socket_path: PathBuf,
}

impl Client {
    pub fn with_sockets(
        ordinary_socket_path: impl Into<PathBuf>,
        meta_socket_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ordinary_socket_path: ordinary_socket_path.into(),
            meta_socket_path: meta_socket_path.into(),
        }
    }

    pub fn from_environment() -> Self {
        let ordinary_socket_path = std::env::var_os(ORDINARY_SOCKET_ENVIRONMENT_VARIABLE)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_ORDINARY_SOCKET_PATH));
        let meta_socket_path = std::env::var_os(META_SOCKET_ENVIRONMENT_VARIABLE)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_META_SOCKET_PATH));
        Self::with_sockets(ordinary_socket_path, meta_socket_path)
    }

    pub fn send_working(&self, request: CloudRequest) -> Result<CloudReply> {
        let mut stream = UnixStream::connect(&self.ordinary_socket_path)?;
        self.handshake_working(&mut stream)?;
        let exchange = fresh_exchange();
        let frame = signal_cloud::Frame::new(ExchangeFrameBody::Request { exchange, request });
        OrdinaryFrameIo::write(&mut stream, &frame)?;
        stream.flush()?;

        let reply = OrdinaryFrameIo::read(&mut stream)?;
        match reply.into_body() {
            ExchangeFrameBody::Reply {
                exchange: reply_exchange,
                reply,
            } if reply_exchange == exchange => Self::unwrap_single_reply(reply),
            _ => Err(Error::UnexpectedFrame),
        }
    }

    pub fn send_meta(&self, request: MetaRequest) -> Result<MetaReply> {
        let mut stream = UnixStream::connect(&self.meta_socket_path)?;
        self.handshake_meta(&mut stream)?;
        let exchange = fresh_exchange();
        let frame = meta_signal_cloud::Frame::new(ExchangeFrameBody::Request { exchange, request });
        MetaFrameIo::write(&mut stream, &frame)?;
        stream.flush()?;

        let reply = MetaFrameIo::read(&mut stream)?;
        match reply.into_body() {
            ExchangeFrameBody::Reply {
                exchange: reply_exchange,
                reply,
            } if reply_exchange == exchange => Self::unwrap_single_meta_reply(reply),
            _ => Err(Error::UnexpectedFrame),
        }
    }

    pub fn run_from_environment() -> Result<String> {
        let request = CliRequest::from_arguments(std::env::args_os().skip(1))?;
        let client = Self::from_environment();
        match request {
            CliRequest::Working(request) => encode_reply(&client.send_working(request)?),
            CliRequest::Meta(request) => encode_reply(&client.send_meta(request)?),
        }
    }

    fn handshake_working(&self, stream: &mut UnixStream) -> Result<()> {
        let frame = signal_cloud::Frame::new(ExchangeFrameBody::HandshakeRequest(
            HandshakeRequest::current(),
        ));
        OrdinaryFrameIo::write(stream, &frame)?;
        let reply = OrdinaryFrameIo::read(stream)?;
        match reply.into_body() {
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_)) => Ok(()),
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Rejected(_)) => {
                Err(Error::HandshakeRejected)
            }
            _ => Err(Error::UnexpectedFrame),
        }
    }

    fn handshake_meta(&self, stream: &mut UnixStream) -> Result<()> {
        let frame = meta_signal_cloud::Frame::new(ExchangeFrameBody::HandshakeRequest(
            HandshakeRequest::current(),
        ));
        MetaFrameIo::write(stream, &frame)?;
        let reply = MetaFrameIo::read(stream)?;
        match reply.into_body() {
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_)) => Ok(()),
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Rejected(_)) => {
                Err(Error::HandshakeRejected)
            }
            _ => Err(Error::UnexpectedFrame),
        }
    }

    fn unwrap_single_reply(reply: FrameReply<CloudReply>) -> Result<CloudReply> {
        match reply {
            FrameReply::Accepted { per_operation, .. } => {
                match per_operation.into_head_and_tail() {
                    (SubReply::Ok(payload), tail) if tail.is_empty() => Ok(payload),
                    _ => Err(Error::SignalRequestFailed),
                }
            }
            FrameReply::Rejected { .. } => Err(Error::SignalRequestRejected),
        }
    }

    fn unwrap_single_meta_reply(reply: FrameReply<MetaReply>) -> Result<MetaReply> {
        match reply {
            FrameReply::Accepted { per_operation, .. } => {
                match per_operation.into_head_and_tail() {
                    (SubReply::Ok(payload), tail) if tail.is_empty() => Ok(payload),
                    _ => Err(Error::SignalRequestFailed),
                }
            }
            FrameReply::Rejected { .. } => Err(Error::SignalRequestRejected),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliRequest {
    Working(CloudRequest),
    Meta(MetaRequest),
}

impl CliRequest {
    pub fn from_arguments<I, S>(arguments: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let arguments: Vec<OsString> = arguments
            .into_iter()
            .map(|argument| argument.as_ref().to_owned())
            .collect();
        let [argument] = arguments.as_slice() else {
            return Err(Error::ExpectedSingleArgument);
        };
        let text = argument.to_str().ok_or(Error::ExpectedSingleArgument)?;
        if text.starts_with("--") {
            return Err(Error::FlagArgument(text.to_owned()));
        }
        let trimmed = text.trim_start();
        let source = if trimmed.starts_with('(') || trimmed.starts_with('[') {
            text.to_owned()
        } else {
            std::fs::read_to_string(PathBuf::from(argument))?
        };
        Self::from_nota(&source)
    }

    pub fn from_nota(text: &str) -> Result<Self> {
        match signal_frame::RequestHead::from_text(text)?
            .route::<signal_cloud::Operation, meta_signal_cloud::Operation>()?
        {
            CommandLineSocket::Working => Self::decode_working(text),
            CommandLineSocket::Meta => Self::decode_meta(text),
        }
    }

    fn decode_working(text: &str) -> Result<Self> {
        let payload = NotaSource::new(text).parse::<CloudRequest>()?;
        Ok(Self::Working(payload))
    }

    fn decode_meta(text: &str) -> Result<Self> {
        let payload = NotaSource::new(text).parse::<MetaRequest>()?;
        Ok(Self::Meta(payload))
    }
}

fn fresh_exchange() -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    )
}

fn encode_reply(reply: &impl NotaEncode) -> Result<String> {
    Ok(reply.to_nota())
}
