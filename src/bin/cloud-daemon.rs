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
    cloud::CloudDaemonCommand::from_environment().run()
}
