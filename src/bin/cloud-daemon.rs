fn main() {
    match cloud::CloudDaemonCommand::from_environment().run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("cloud-daemon: {error}");
            std::process::exit(2);
        }
    }
}
