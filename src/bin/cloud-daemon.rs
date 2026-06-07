fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("(DaemonRejected \"{error}\")");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<(), cloud::schema::daemon::DaemonError<cloud::schema_daemon::CloudDaemon>> {
    cloud::CloudDaemonCommand::from_environment().run()
}
