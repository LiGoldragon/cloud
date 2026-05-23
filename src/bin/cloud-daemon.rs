fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("(DaemonRejected \"{error}\")");
            std::process::exit(2);
        }
    }
}

fn run() -> cloud::Result<()> {
    let configuration = nota_config::ConfigurationSource::from_argv()?.decode()?;
    cloud::daemon::Daemon::new(configuration).run()
}
