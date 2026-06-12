fn main() {
    match cloud::client::Client::run_meta_from_environment() {
        Ok(reply) => println!("{reply}"),
        Err(error) => {
            eprintln!("meta-cloud: {error}");
            std::process::exit(2);
        }
    }
}
