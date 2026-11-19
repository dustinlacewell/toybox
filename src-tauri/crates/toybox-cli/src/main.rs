//! `tbx`: Toybox on the command line. See docs/cli.md.
//!
//! This crate parses arguments, calls `toybox-host` and formats the answer. The libraries,
//! plugins and settings logic lives in `toybox-host`; running plugin JavaScript is in `plugin_runtime`.

mod app;
mod cli;
mod cmd_edit;
mod cmd_info;
mod cmd_items;
mod cmd_library;
mod cmd_plugin;
mod cmd_run;
mod cmd_transfer;
mod ctx;
mod error;
mod json;
mod out;
mod plugin_runtime;
mod prompt;
mod selection;

fn main() {
    std::process::exit(app::run());
}
