//! What every command is given: the global options, the host and the output rules.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use clap::ArgMatches;
use serde::Serialize;
use toybox_host::libraries::LibraryInfo;
use toybox_host::Host;

use crate::error::{CliError, CliResult};
use crate::out;

/// The global options after parsing.
#[derive(Debug, Clone, Default)]
pub struct Globals {
    pub library: Option<String>,
    pub json: bool,
    pub quiet: bool,
    pub yes: bool,
    pub verbose: bool,
    /// Colour on standard error.
    pub color: bool,
}

impl Globals {
    pub fn from_matches(matches: &ArgMatches) -> Globals {
        let flag = |name: &str| matches.try_get_one::<bool>(name).ok().flatten().copied().unwrap_or(false);
        Globals {
            library: matches.try_get_one::<String>("library").ok().flatten().cloned().filter(|text| !text.trim().is_empty()),
            json: flag("json"),
            quiet: flag("quiet"),
            yes: flag("yes"),
            verbose: flag("verbose"),
            color: out::color_wanted(matches.try_get_one::<String>("color").ok().flatten().map(String::as_str).unwrap_or("auto")),
        }
    }
}

/// Where this run's data comes from, for `status` and `debug paths`.
#[derive(Debug, Clone)]
pub enum Origin {
    Home,
    AppId(String),
}

pub struct Ctx {
    pub g: Globals,
    pub host: Arc<Host>,
    pub origin: Origin,
    /// `TOYBOX_SECRETS` names a file instead of the credential store.
    pub secrets_file: Option<std::path::PathBuf>,
    opened: RefCell<HashSet<String>>,
}

impl Ctx {
    pub fn new(g: Globals, host: Arc<Host>, origin: Origin, secrets_file: Option<std::path::PathBuf>) -> Ctx {
        Ctx { g, host, origin, secrets_file, opened: RefCell::new(HashSet::new()) }
    }

    /// The library to act on: `-L`, else the default library.
    pub fn library(&self) -> CliResult<LibraryInfo> {
        Ok(self.host.target_library(self.g.library.as_deref())?)
    }

    /// The library named by a command argument, else `-L`, else the default.
    pub fn library_or(&self, named: Option<&str>) -> CliResult<LibraryInfo> {
        match named {
            Some(name) => Ok(self.host.find_library(name)?),
            None => self.library(),
        }
    }

    /// Make the library ready to be read (once per run): migrations, a current catalog,
    /// for a remote library an update from its server.
    pub fn open(&self, library: &LibraryInfo) -> CliResult<()> {
        if !self.opened.borrow_mut().insert(library.id.clone()) {
            return Ok(());
        }
        if !library.available {
            let what = if library.remote.is_some() { "has never been reachable" } else { "cannot be read (is the drive connected?)" };
            return Err(CliError::failed(format!("library '{}' {what}", library.name)).hint("see `tbx library list`"));
        }
        self.host.open_library(&library.id, false)?;
        Ok(())
    }

    /// Open the library and refuse when it cannot be changed.
    pub fn open_for_write(&self, library: &LibraryInfo) -> CliResult<()> {
        self.open(library)?;
        let current = self.host.library_info(&library.id)?;
        if !current.can_write {
            let why = match &current.remote {
                Some(remote) if remote.offline => "its server cannot be reached".to_string(),
                Some(_) => "its token does not allow writing".to_string(),
                None => "its folder is read-only".to_string(),
            };
            return Err(CliError::failed(format!("library '{}' cannot be changed: {why}", library.name)));
        }
        Ok(())
    }

    /// Print a result: the document under `--json`, else `text`.
    pub fn output<T: Serialize>(&self, doc: &T, text: &str) -> CliResult<()> {
        if self.g.json {
            out::json(doc)
        } else {
            out::stdout(text);
            Ok(())
        }
    }

    /// Print a summary for people: the document under `--json`, `text` unless `--quiet`.
    pub fn report<T: Serialize>(&self, doc: &T, text: &str) -> CliResult<()> {
        if self.g.json {
            out::json(doc)
        } else {
            if !self.g.quiet {
                out::stdout(text);
            }
            Ok(())
        }
    }

    /// Progress on standard error.
    pub fn log(&self, message: &str) {
        if !self.g.quiet {
            out::stderr(&format!("{message}\n"));
        }
    }

    pub fn warn(&self, message: &str) {
        if !self.g.quiet {
            out::stderr(&format!("{}: {message}\n", out::paint("warning", "1;33", self.g.color)));
        }
    }

    /// Whether progress may be drawn on a terminal.
    pub fn progress_visible(&self) -> bool {
        !self.g.quiet && !self.g.json && out::stderr_is_terminal()
    }
}
