use clap::Parser;

fn main() {
    let cli = toybox_server::cli::Cli::parse();
    match toybox_server::cli::run(cli) {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("toybox-server: {message}");
            std::process::exit(1);
        }
    }
}
