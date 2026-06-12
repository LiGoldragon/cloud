use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use meta_signal_cloud::Operation as MetaOperation;
use meta_signal_cloud::schema::lib as meta;
use nota_next::{NotaEncode, NotaSource};
use signal_cloud::Operation as CloudOperation;
use signal_cloud::schema::lib as ordinary;
use triad_runtime::{FrameBody, LengthPrefixedCodec};

use crate::schema_bridge::{
    SchemaCloudInput, SchemaCloudOutput, SchemaMetaInput, SchemaMetaOutput,
};
use crate::{Error, Result};

const DEFAULT_ORDINARY_SOCKET_PATH: &str = "/run/cloud/cloud.sock";
const DEFAULT_META_SOCKET_PATH: &str = "/run/cloud/cloud-meta.sock";
const ORDINARY_SOCKET_ENVIRONMENT_VARIABLE: &str = "CLOUD_SOCKET_PATH";
const META_SOCKET_ENVIRONMENT_VARIABLE: &str = "CLOUD_META_SOCKET_PATH";

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

    pub fn send_working(&self, input: ordinary::Input) -> Result<ordinary::Output> {
        let mut stream = UnixStream::connect(&self.ordinary_socket_path)?;
        SchemaConnection::new(&mut stream).exchange_working(input)
    }

    pub fn send_meta(&self, input: meta::Input) -> Result<meta::Output> {
        let mut stream = UnixStream::connect(&self.meta_socket_path)?;
        SchemaConnection::new(&mut stream).exchange_meta(input)
    }

    pub fn run_working_from_environment() -> Result<String> {
        let input =
            CommandLineInput::from_arguments(std::env::args_os().skip(1))?.into_working_input()?;
        let client = Self::from_environment();
        let reply = SchemaCloudOutput::new(client.send_working(input)?).into_reply();
        Self::encode_reply(&reply)
    }

    pub fn run_meta_from_environment() -> Result<String> {
        let input =
            CommandLineInput::from_arguments(std::env::args_os().skip(1))?.into_meta_input()?;
        let client = Self::from_environment();
        let reply = SchemaMetaOutput::new(client.send_meta(input)?).into_reply();
        Self::encode_reply(&reply)
    }

    pub fn working_input_from_nota(text: &str) -> Result<ordinary::Input> {
        CommandLineInput::from_nota(text).into_working_input()
    }

    pub fn meta_input_from_nota(text: &str) -> Result<meta::Input> {
        CommandLineInput::from_nota(text).into_meta_input()
    }

    fn encode_reply(reply: &impl NotaEncode) -> Result<String> {
        Ok(reply.to_nota())
    }
}

pub struct SchemaConnection<'stream> {
    stream: &'stream mut UnixStream,
}

impl<'stream> SchemaConnection<'stream> {
    pub fn new(stream: &'stream mut UnixStream) -> Self {
        Self { stream }
    }

    pub fn exchange_working(&mut self, input: ordinary::Input) -> Result<ordinary::Output> {
        let codec = LengthPrefixedCodec::default();
        codec.write_body(self.stream, &FrameBody::new(input.encode_signal_frame()?))?;
        self.stream.flush()?;
        let body = codec.read_body(self.stream)?;
        let (_route, output) = ordinary::Output::decode_signal_frame(body.bytes())?;
        Ok(output)
    }

    pub fn exchange_meta(&mut self, input: meta::Input) -> Result<meta::Output> {
        let codec = LengthPrefixedCodec::default();
        codec.write_body(self.stream, &FrameBody::new(input.encode_signal_frame()?))?;
        self.stream.flush()?;
        let body = codec.read_body(self.stream)?;
        let (_route, output) = meta::Output::decode_signal_frame(body.bytes())?;
        Ok(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineInput {
    text: String,
}

impl CommandLineInput {
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
            std::fs::read_to_string(PathBuf::from(argument.as_os_str()))?
        };
        Ok(Self { text: source })
    }

    pub fn from_nota(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }

    pub fn into_working_input(self) -> Result<ordinary::Input> {
        let payload = NotaSource::new(&self.text).parse::<CloudOperation>()?;
        Ok(SchemaCloudInput::from_operation(payload).into_input())
    }

    pub fn into_meta_input(self) -> Result<meta::Input> {
        let payload = NotaSource::new(&self.text).parse::<MetaOperation>()?;
        Ok(SchemaMetaInput::from_operation(payload).into_input())
    }
}
