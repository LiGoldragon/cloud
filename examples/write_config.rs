use std::path::PathBuf;

use cloud::{CloudDaemonConfigurationFile, DaemonConfiguration};

fn main() {
    ConfigurationRequest::from_environment().write();
}

struct ConfigurationRequest {
    output_path: PathBuf,
    ordinary_socket_path: String,
    meta_socket_path: String,
}

impl ConfigurationRequest {
    fn from_environment() -> Self {
        Self::from_arguments(std::env::args().skip(1).collect())
    }

    fn from_arguments(arguments: Vec<String>) -> Self {
        let mut arguments = arguments.into_iter();
        let output_path = arguments
            .next()
            .map(PathBuf::from)
            .expect("out.rkyv path argument");
        let ordinary_socket_path = arguments.next().expect("ordinary socket path argument");
        let meta_socket_path = arguments.next().expect("meta socket path argument");
        assert!(
            arguments.next().is_none(),
            "usage: write_config <out.rkyv> <ordinary.sock> <meta.sock>"
        );
        Self {
            output_path,
            ordinary_socket_path,
            meta_socket_path,
        }
    }

    fn configuration(&self) -> DaemonConfiguration {
        DaemonConfiguration {
            ordinary_socket_path: self.ordinary_socket_path.clone(),
            ordinary_socket_mode: 0o600,
            meta_socket_path: self.meta_socket_path.clone(),
            meta_socket_mode: 0o600,
        }
    }

    fn write(&self) {
        CloudDaemonConfigurationFile::new(&self.output_path)
            .write_configuration(&self.configuration())
            .expect("write rkyv daemon configuration");
    }
}
