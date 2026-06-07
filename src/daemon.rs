use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use signal_frame::ExchangeFrameBody;

use crate::frame_io::{MetaFrameIo, OrdinaryFrameIo, handshake_reply_for};
use crate::{DaemonConfiguration, Error, Result, Store};

pub struct Daemon {
    configuration: DaemonConfiguration,
}

impl Daemon {
    pub fn new(configuration: DaemonConfiguration) -> Self {
        Self { configuration }
    }

    pub fn run(self) -> Result<()> {
        let store = Arc::new(Mutex::new(Store::new()));
        let ordinary_listener = Self::bind_socket(
            &self.configuration.ordinary_socket_path,
            self.configuration.ordinary_socket_mode,
        )?;
        let meta_listener = Self::bind_socket(
            &self.configuration.meta_socket_path,
            self.configuration.meta_socket_mode,
        )?;

        let ordinary_store = Arc::clone(&store);
        thread::spawn(move || Self::run_ordinary_listener(ordinary_listener, ordinary_store));

        let meta_store = Arc::clone(&store);
        thread::spawn(move || Self::run_meta_listener(meta_listener, meta_store));

        loop {
            thread::sleep(Duration::from_secs(60));
        }
    }

    pub fn serve_ordinary_stream(store: &Store, stream: &mut UnixStream) -> Result<()> {
        loop {
            let frame = OrdinaryFrameIo::read(stream)?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = signal_cloud::Frame::new(signal_cloud::FrameBody::HandshakeReply(
                        handshake_reply_for(request.version()),
                    ));
                    OrdinaryFrameIo::write(stream, &reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = store.handle_ordinary_request(request);
                    let frame = signal_cloud::Frame::new(signal_cloud::FrameBody::Reply {
                        exchange,
                        reply,
                    });
                    OrdinaryFrameIo::write(stream, &frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }

    pub fn serve_meta_stream(store: &Store, stream: &mut UnixStream) -> Result<()> {
        loop {
            let frame = MetaFrameIo::read(stream)?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = meta_signal_cloud::Frame::new(
                        meta_signal_cloud::FrameBody::HandshakeReply(handshake_reply_for(
                            request.version(),
                        )),
                    );
                    MetaFrameIo::write(stream, &reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = store.handle_meta_request(request);
                    let frame =
                        meta_signal_cloud::Frame::new(meta_signal_cloud::FrameBody::Reply {
                            exchange,
                            reply,
                        });
                    MetaFrameIo::write(stream, &frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }

    fn run_ordinary_listener(listener: UnixListener, store: Arc<Mutex<Store>>) {
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    if let Err(error) = Self::serve_ordinary_stream_shared(&store, &mut stream) {
                        eprintln!("(OrdinarySocketError \"{error}\")");
                    }
                }
                Err(error) => eprintln!("(OrdinaryAcceptError \"{error}\")"),
            }
        }
    }

    fn run_meta_listener(listener: UnixListener, store: Arc<Mutex<Store>>) {
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    if let Err(error) = Self::serve_meta_stream_shared(&store, &mut stream) {
                        eprintln!("(MetaSocketError \"{error}\")");
                    }
                }
                Err(error) => eprintln!("(MetaAcceptError \"{error}\")"),
            }
        }
    }

    fn serve_ordinary_stream_shared(
        store: &Arc<Mutex<Store>>,
        stream: &mut UnixStream,
    ) -> Result<()> {
        loop {
            let frame = OrdinaryFrameIo::read(stream)?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = signal_cloud::Frame::new(signal_cloud::FrameBody::HandshakeReply(
                        handshake_reply_for(request.version()),
                    ));
                    OrdinaryFrameIo::write(stream, &reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = {
                        let store = store.lock().map_err(|_| Error::StorePoisoned)?;
                        store.handle_ordinary_request(request)
                    };
                    let frame = signal_cloud::Frame::new(signal_cloud::FrameBody::Reply {
                        exchange,
                        reply,
                    });
                    OrdinaryFrameIo::write(stream, &frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }

    fn serve_meta_stream_shared(store: &Arc<Mutex<Store>>, stream: &mut UnixStream) -> Result<()> {
        loop {
            let frame = MetaFrameIo::read(stream)?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = meta_signal_cloud::Frame::new(
                        meta_signal_cloud::FrameBody::HandshakeReply(handshake_reply_for(
                            request.version(),
                        )),
                    );
                    MetaFrameIo::write(stream, &reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = {
                        let store = store.lock().map_err(|_| Error::StorePoisoned)?;
                        store.handle_meta_request(request)
                    };
                    let frame =
                        meta_signal_cloud::Frame::new(meta_signal_cloud::FrameBody::Reply {
                            exchange,
                            reply,
                        });
                    MetaFrameIo::write(stream, &frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }

    fn bind_socket(path: impl AsRef<Path>, mode: u32) -> Result<UnixListener> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.file_type().is_socket() {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("refusing to replace non-socket path {}", path.display()),
                )));
            }
            fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        Ok(listener)
    }
}
