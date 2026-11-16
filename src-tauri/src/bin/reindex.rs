fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 { eprintln!("Usage: reindex <library-root> <catalog-cache-path>"); std::process::exit(2); }
    match toybox_lib::maintenance::reindex(std::path::Path::new(&args[1]), std::path::Path::new(&args[2])) {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => { eprintln!("{error}"); std::process::exit(1); }
    }
}
