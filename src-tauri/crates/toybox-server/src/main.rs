use clap::Parser;

fn main() {
    let cli = toybox_server::cli::Cli::parse();
    if let Err(message) = toybox_server::cli::run(cli) {
        eprintln!("toybox-server: {message}");
        std::process::exit(1);
    }
}
