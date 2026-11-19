//! `tbx self install|uninstall|status`: put this `tbx` on the user's PATH (or take it off), and say
//! whether a terminal finds it. The Windows installer runs `tbx self install` so the logic is the
//! one the app's Settings uses (`toybox_host::cli_install`).
//!
//! It does not open the Toybox data, so it works from an installer and on a machine with no libraries.

use std::ffi::OsString;

use toybox_host::cli_install::{self, System};

const USAGE: &str = "usage: tbx self <install|uninstall|status> [--json] [--quiet]";

pub fn run(args: &[OsString]) -> i32 {
    let words: Vec<String> = args.iter().map(|a| a.to_string_lossy().to_string()).collect();
    let json = words.iter().any(|w| w == "--json");
    let quiet = words.iter().any(|w| w == "--quiet" || w == "-q");
    let Some(action) = words.iter().find(|w| !w.starts_with('-')).cloned() else {
        eprintln!("tbx: choose install, uninstall or status\n{USAGE}");
        return 2;
    };
    let exe = match std::env::current_exe().and_then(|p| p.canonicalize()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("tbx: cannot tell where this program is: {error}");
            return 1;
        }
    };
    // Windows hands back \?\ paths; the PATH entry must be a plain path.
    let exe = std::path::PathBuf::from(exe.to_string_lossy().trim_start_matches(r"\\?\").to_string());
    let env = System;
    match action.as_str() {
        "status" => {
            let status = cli_install::status(&env, Some(&exe), env!("CARGO_PKG_VERSION"));
            if json {
                println!("{}", serde_json::to_string_pretty(&status).unwrap_or_default());
            } else {
                println!("tbx {} at {}", env!("CARGO_PKG_VERSION"), exe.display());
                println!("{}", status.message);
            }
            0
        }
        "install" | "uninstall" => {
            let result = if action == "install" { cli_install::install(&env, &exe) } else { cli_install::uninstall(&env, &exe) };
            match result {
                Ok(change) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&change).unwrap_or_default());
                    } else if !quiet {
                        let what = match (action.as_str(), change.changed) {
                            ("install", true) => "tbx is installed for your user",
                            ("install", false) => "tbx was already installed",
                            (_, true) => "tbx is removed from your user",
                            _ => "tbx was not installed",
                        };
                        println!("{what}{}", change.program.map(|p| format!(" ({p})")).unwrap_or_default());
                        if let Some(note) = change.note {
                            println!("{note}");
                        }
                    }
                    0
                }
                Err(message) => {
                    eprintln!("tbx: {message}");
                    1
                }
            }
        }
        _ => {
            eprintln!("tbx: unknown action '{action}'\n{USAGE}");
            2
        }
    }
}
