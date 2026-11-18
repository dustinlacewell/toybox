fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() < 2 || args.len() > 3 || (args.len() == 3 && args[2] != "--apply") {
        eprintln!("Usage: repair-colors <library-root> [--apply]"); std::process::exit(2);
    }
    match toybox_core::maintenance_colors::repair_colors(std::path::Path::new(&args[1]), args.len() == 3) {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => { eprintln!("{error}"); std::process::exit(1); }
    }
}
