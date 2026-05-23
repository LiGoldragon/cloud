fn main() {
    match cloud::client::Client::run_from_environment() {
        Ok(reply) => println!("{reply}"),
        Err(error) => {
            eprintln!("(CliRejected \"{error}\")");
            std::process::exit(2);
        }
    }
}
