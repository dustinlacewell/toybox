//! The command line: `init`, `serve`, `token add|list|revoke`, `status`.
//! Every command returns `Err(message)` for a failure; `main` prints it and
//! exits with status 1 (usage mistakes exit with 2, from the argument parser).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};

use crate::config::{Role, ServerConfig};
use crate::data::{self, DataDir};
use crate::state::{AppState, Options};
use crate::timeutil::fmt_utc;

#[derive(Parser, Debug)]
#[command(
    name = "toybox-server",
    version,
    about = "Serve one Toybox library over HTTP, with access tokens (read, write, admin).",
    long_about = "Serve one Toybox library over HTTP, with access tokens (read, write, admin).\n\n\
Quick start:\n  toybox-server init            create the data folder and an admin token\n  toybox-server serve           start the server (put HTTPS in front, see deploy/README.md)\n  toybox-server token add --name alice --role read\n\n\
Exit status: 0 success, 1 failure, 2 wrong usage."
)]
pub struct Cli {
    /// Folder that holds everything the server keeps (library, tokens, caches).
    #[arg(long, global = true, env = "TOYBOX_DATA", default_value = "/var/lib/toybox", value_name = "DIR")]
    pub data: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Set up a new data folder and print an admin token (shown once).
    Init {
        /// The server name clients display.
        #[arg(long, default_value = "Toybox")]
        name: String,
    },
    /// Run the server.
    Serve {
        /// Address to listen on. The default only accepts connections from this machine.
        #[arg(long, env = "TOYBOX_LISTEN", default_value = "127.0.0.1:8787", value_name = "ADDR:PORT")]
        listen: String,
        /// Largest single file an import may upload, in MB.
        #[arg(long, default_value_t = 2048, value_name = "MB")]
        max_file_mb: u64,
        /// Largest total size of one import, in MB.
        #[arg(long, default_value_t = 8192, value_name = "MB")]
        max_upload_mb: u64,
    },
    /// Manage access tokens.
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },
    /// Install the latest signed release and restart the service (with --check: exit status 10 when a newer release exists).
    Update {
        /// Only look: exit 0 when up to date, 10 when an update is available. Changes nothing.
        #[arg(long)]
        check: bool,
        /// Do not ask before installing.
        #[arg(long)]
        yes: bool,
        /// Release manifest address (https://...) or a folder holding toybox-server.json and the files. Default: updateUrl in server.json, else the release page this build comes from.
        #[arg(long, value_name = "URL|DIR")]
        from: Option<String>,
        /// Install a release older than this one too.
        #[arg(long)]
        allow_downgrade: bool,
        /// The systemd service to restart.
        #[arg(long, default_value = "toybox-server", value_name = "NAME")]
        service: String,
        /// Where the restarted server is expected to answer.
        #[arg(long, env = "TOYBOX_LISTEN", default_value = "127.0.0.1:8787", value_name = "ADDR:PORT")]
        listen: String,
        /// Seconds to wait for the restarted server before going back to the previous version.
        #[arg(long, default_value_t = 180, value_name = "SECONDS")]
        wait: u64,
    },
    /// Bring the library to this version's format. With --check: only report what that would do.
    Migrate {
        /// Report only: change nothing.
        #[arg(long)]
        check: bool,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
        /// Where a running server would answer (migrating is refused while one does).
        #[arg(long, env = "TOYBOX_LISTEN", default_value = "127.0.0.1:8787", value_name = "ADDR:PORT")]
        listen: String,
    },
    /// The library database copies taken before a migration rewrites entries.
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },
    /// Show the setup and whether a server is running.
    Status {
        /// Where to look for a running server.
        #[arg(long, env = "TOYBOX_LISTEN", default_value = "127.0.0.1:8787", value_name = "ADDR:PORT")]
        listen: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum TokenAction {
    /// Create a token. It is printed once and cannot be shown again.
    Add {
        /// A label so you can tell tokens apart, like "alice laptop".
        #[arg(long)]
        name: String,
        /// read: browse and download. write: also import and delete. admin: also manage tokens.
        #[arg(long, value_enum)]
        role: RoleArg,
    },
    /// List tokens (never their secrets).
    List,
    /// Delete a token by name or id. It stops working immediately.
    Revoke {
        /// The token name or id, as shown by "token list".
        token: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SnapshotAction {
    /// List the snapshots, newest first.
    List,
    /// Put a snapshot's entries back (the server must be stopped; refused when files it names are gone).
    Restore {
        /// The name shown by "snapshot list".
        name: String,
        /// Where a running server would answer (restoring is refused while one does).
        #[arg(long, env = "TOYBOX_LISTEN", default_value = "127.0.0.1:8787", value_name = "ADDR:PORT")]
        listen: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RoleArg {
    Read,
    Write,
    Admin,
}

impl From<RoleArg> for Role {
    fn from(role: RoleArg) -> Role {
        match role {
            RoleArg::Read => Role::Read,
            RoleArg::Write => Role::Write,
            RoleArg::Admin => Role::Admin,
        }
    }
}

/// Runs the command and returns the exit status for success (0, or 10 for `update --check` with an update waiting).
pub fn run(cli: Cli) -> Result<i32, String> {
    let data = DataDir::new(cli.data);
    let done = |result: Result<(), String>| result.map(|()| 0);
    match cli.command {
        Command::Init { name } => done(init(&data, &name)),
        Command::Serve { listen, max_file_mb, max_upload_mb } => done(serve(&data, &listen, max_file_mb, max_upload_mb)),
        Command::Token { action } => done(match action {
            TokenAction::Add { name, role } => token_add(&data, &name, role.into()),
            TokenAction::List => token_list(&data),
            TokenAction::Revoke { token } => token_revoke(&data, &token),
        }),
        Command::Status { listen } => done(status(&data, &listen)),
        Command::Update { check, yes, from, allow_downgrade, service, listen, wait } => update(&data, check, yes, from, allow_downgrade, service, listen, wait),
        Command::Migrate { check, json, listen } => done(migrate(&data, check, json, &listen)),
        Command::Snapshot { action } => done(match action {
            SnapshotAction::List => snapshot_list(&data),
            SnapshotAction::Restore { name, listen } => snapshot_restore(&data, &name, &listen),
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn update(data: &DataDir, check: bool, yes: bool, from: Option<String>, allow_downgrade: bool, service: String, listen: String, wait: u64) -> Result<i32, String> {
    let config = ServerConfig::load(&data.server_json()).ok();
    let from = from.or_else(|| config.and_then(|c| c.update_url));
    let binary = std::env::current_exe().map_err(|e| format!("Cannot tell where this program is installed: {e}"))?;
    let options = crate::update::Options {
        from,
        check,
        yes,
        allow_downgrade,
        data: data.root.clone(),
        binary,
        service,
        listen,
        wait: Duration::from_secs(wait),
        public_key: crate::update::release_public_key(),
        target: crate::update::TARGET.to_string(),
        current_version: crate::SERVER_VERSION.to_string(),
    };
    match crate::update::run(&options, &crate::update::System).map_err(|e| e.to_string())? {
        crate::update::Outcome::Available { .. } => Ok(crate::update::EXIT_UPDATE_AVAILABLE),
        _ => Ok(0),
    }
}

fn mb(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}

fn duration_text(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds} s"),
        60..=3599 => format!("{} min", seconds / 60 + 1),
        _ => format!("{:.1} h", seconds as f64 / 3600.0),
    }
}

/// The report of `migrate --check`, in words.
pub fn print_preflight(report: &toybox_core::infra::preflight::Preflight) {
    println!("Library:        {}", report.root);
    println!("Format version: {} (this program brings it to {})", report.format_version, report.latest_version);
    if report.pending.is_empty() && !report.newer_than_this_build {
        println!("Nothing to migrate: the library is up to date.");
    }
    for step in &report.pending {
        println!("Pending step {} ({}): {}", step.version, step.name, step.summary);
        if let (Some(items), Some(files), Some(bytes)) = (step.items, step.files, step.bytes) {
            println!("    touches {items} items, {files} files, {}", mb(bytes));
        }
    }
    if let Some(seconds) = report.estimate_seconds {
        println!("Time (rough):   about {}", duration_text(seconds));
    }
    if !report.pending.is_empty() {
        match report.free_bytes {
            Some(free) => println!("Disk:           {} free, about {} needed ({})", mb(free), mb(report.needed_bytes), if report.enough_space { "enough" } else { "NOT ENOUGH" }),
            None => println!("Disk:           about {} needed (free space unknown)", mb(report.needed_bytes)),
        }
    }
    if let Some(path) = &report.snapshot {
        println!("Snapshot:       library.db is copied to {path} first (the newest {} are kept)", toybox_core::infra::snapshot::KEEP);
    }
    for note in &report.notes {
        println!("Note:           {note}");
    }
}

fn migrate(data: &DataDir, check: bool, json: bool, listen: &str) -> Result<(), String> {
    if !data.is_initialised() {
        return Err(format!("{} has not been set up yet. Run: toybox-server init --data {}", data.root.display(), data.root.display()));
    }
    let handle = data.handle();
    let report = toybox_core::infra::preflight::check(&handle.root).map_err(|e| e.to_string())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    } else {
        print_preflight(&report);
    }
    if check {
        return Ok(());
    }
    if report.newer_than_this_build {
        return Err("This library was migrated by a newer Toybox. Install that version or newer.".into());
    }
    if report.pending.is_empty() {
        return Ok(());
    }
    if !report.enough_space {
        return Err("There is not enough free disk space for this migration. Nothing was changed.".into());
    }
    if probe(listen).is_some() {
        return Err(format!("A server is running at {listen}. Stop it first (it migrates by itself when it starts, too). Nothing was changed."));
    }
    println!("Migrating ...");
    toybox_core::infra::migrations::run(&handle.root, &handle.cache_path).map_err(|e| e.to_string())?;
    let after = toybox_core::infra::preflight::check(&handle.root).map_err(|e| e.to_string())?;
    println!("Done. Format version {}.", after.format_version);
    Ok(())
}

fn snapshot_list(data: &DataDir) -> Result<(), String> {
    let snapshots = toybox_core::infra::snapshot::list(&data.library());
    if snapshots.is_empty() {
        println!("No snapshots. One is taken before a migration that rewrites entries.");
        return Ok(());
    }
    println!("{:<44}  {:<8}  {:<16}  SIZE", "NAME", "VERSION", "TAKEN (UTC)");
    for snapshot in snapshots {
        println!("{:<44}  {:<8}  {:<16}  {}", snapshot.name, snapshot.from_version, snapshot.taken, mb(snapshot.bytes));
    }
    println!("Restore with: toybox-server snapshot restore <name>   (entries only; files a migration moved are not brought back)");
    Ok(())
}

fn snapshot_restore(data: &DataDir, name: &str, listen: &str) -> Result<(), String> {
    if !data.is_initialised() {
        return Err(format!("{} has not been set up yet.", data.root.display()));
    }
    if probe(listen).is_some() {
        return Err(format!("A server is running at {listen}. Stop it first: systemctl stop toybox-server. Nothing was changed."));
    }
    let restored = toybox_core::infra::snapshot::restore(&data.library(), name).map_err(|e| e.to_string())?;
    println!("Restored the library database from {} (format version {}).", restored.snapshot.name, restored.snapshot.from_version);
    println!("The database that was in use is kept as {}.", restored.previous.display());
    println!("Start the server (the previous program is toybox-server.previous next to this one).");
    Ok(())
}

fn load_config(data: &DataDir) -> Result<ServerConfig, String> {
    if !data.is_initialised() {
        return Err(format!(
            "{} has not been set up yet. Run: toybox-server init --data {}",
            data.root.display(),
            data.root.display()
        ));
    }
    ServerConfig::load(&data.server_json()).map_err(|e| e.to_string())
}

fn print_secret(secret: &str) {
    println!();
    println!("    {secret}");
    println!();
}

pub fn init(data: &DataDir, name: &str) -> Result<(), String> {
    let name = if name.trim().is_empty() { "Toybox" } else { name.trim() };
    println!("Setting up the Toybox server in {} ...", data.root.display());
    let outcome = data::init(data, name)?;
    println!("Done. Server name: {name}");
    println!(
        "Library folder:    {}{}",
        data.library().display(),
        if outcome.library_was_empty { " (empty)" } else { " (existing library adopted)" }
    );
    println!();
    println!("Your admin token (it is shown ONLY THIS ONCE, so store it somewhere safe):");
    print_secret(&outcome.secret);
    println!("Paste it into Toybox together with the server address. Make more tokens for");
    println!("other people with:  toybox-server token add --name <who> --role read|write|admin");
    println!();
    println!("Next: toybox-server serve --data {}", data.root.display());
    Ok(())
}

fn token_add(data: &DataDir, name: &str, role: Role) -> Result<(), String> {
    let mut config = load_config(data)?;
    let (record, secret) = config.add_token(name, role).map_err(|e| e.to_string())?;
    config.save(&data.server_json()).map_err(|e| e.to_string())?;
    println!("Created token \"{}\" with the {} role (id {}).", record.name, record.role.as_str(), record.id);
    println!();
    println!("Its secret, shown ONLY THIS ONCE:");
    print_secret(&secret);
    println!("A running server accepts it right away.");
    Ok(())
}

fn token_list(data: &DataDir) -> Result<(), String> {
    let config = load_config(data)?;
    if config.tokens.is_empty() {
        println!("No tokens yet. Create one with: toybox-server token add --name <who> --role read");
        return Ok(());
    }
    let width = config.tokens.iter().map(|t| t.name.chars().count()).max().unwrap_or(4).max(4);
    println!("{:<8}  {:<width$}  {:<5}  {:<20}  LAST USED", "ID", "NAME", "ROLE", "CREATED");
    for token in &config.tokens {
        println!(
            "{:<8}  {:<width$}  {:<5}  {:<20}  {}",
            token.id,
            token.name,
            token.role.as_str(),
            fmt_utc(token.created_at),
            token.last_used_at.map_or("never".to_string(), fmt_utc),
        );
    }
    Ok(())
}

fn token_revoke(data: &DataDir, key: &str) -> Result<(), String> {
    let mut config = load_config(data)?;
    let removed = config.remove_token(key).map_err(|e| e.to_string())?;
    config.save(&data.server_json()).map_err(|e| e.to_string())?;
    println!("Revoked token \"{}\" ({}). It stops working immediately.", removed.name, removed.id);
    Ok(())
}

fn parse_listen(listen: &str) -> Result<SocketAddr, String> {
    listen
        .parse::<SocketAddr>()
        .or_else(|_| listen.to_socket_addrs().ok().and_then(|mut a| a.next()).ok_or(()))
        .map_err(|_| format!("\"{listen}\" is not an address. Use host:port, like 127.0.0.1:8787."))
}

fn serve(data: &DataDir, listen: &str, max_file_mb: u64, max_upload_mb: u64) -> Result<(), String> {
    let addr = parse_listen(listen)?;
    let mut options = Options::default();
    options.uploads.max_file_bytes = max_file_mb.saturating_mul(1024 * 1024);
    options.uploads.max_upload_bytes = max_upload_mb.saturating_mul(1024 * 1024);
    let state = AppState::open(data.clone(), options)?;
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("Could not start the async runtime: {e}"))?;
    runtime.block_on(async move {
        println!("Opening the library in {} ...", data.library().display());
        let library = state.library.state().await.map_err(|e| e.message)?;
        let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
            format!("Could not listen on {addr}: {e}. Is another program using that port? Choose another with --listen.")
        })?;
        let name = state.config.snapshot().map(|c| c.name.clone()).unwrap_or_default();
        println!("Toybox server \"{name}\" (revision {}) listening on http://{addr}", library.revision);
        if !addr.ip().is_loopback() {
            println!();
            println!("WARNING: {addr} can be reached from other machines. Every request carries an access");
            println!("token in clear text, so serve it over HTTPS: put a reverse proxy such as Caddy in");
            println!("front (see deploy/README.md) and listen on 127.0.0.1 instead.");
            println!();
        }
        let tasks = state.spawn_background_tasks();
        let app = crate::api::router(state.clone());
        let result = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(shutdown_signal())
            .await;
        for task in tasks {
            task.abort();
        }
        // Save the last-used times before leaving.
        let flush = state.clone();
        let _ = tokio::task::spawn_blocking(move || flush.config.flush_usage()).await;
        println!("Stopped.");
        result.map_err(|e| format!("The server stopped unexpectedly: {e}"))
    })
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn count_assets(handle: &toybox_core::infra::registry::LibraryHandle) -> Option<usize> {
    if let Some(catalog) = toybox_core::infra::store::Store::for_handle(handle).load_catalog().ok().flatten() {
        return Some(catalog.assets.len());
    }
    // A data folder from before the databases: the cache file the first start migrates.
    #[derive(serde::Deserialize)]
    struct Counted {
        assets: Vec<serde::de::IgnoredAny>,
    }
    let bytes = std::fs::read(&handle.cache_path).ok()?;
    serde_json::from_slice::<Counted>(&bytes).ok().map(|c| c.assets.len())
}

/// Ask a server on `addr` whether it is up, with a bare HTTP request (no client library needed).
fn probe(listen: &str) -> Option<String> {
    let addr = parse_listen(listen).ok()?;
    let addr = if addr.ip().is_unspecified() { SocketAddr::from(([127, 0, 0, 1], addr.port())) } else { addr };
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream.write_all(b"GET /api/v1/health HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n").ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    (reply.starts_with("HTTP/1.") && reply.contains(" 200 ") && reply.contains("\"ok\"")).then(|| addr.to_string())
}

fn status(data: &DataDir, listen: &str) -> Result<(), String> {
    let config = load_config(data)?;
    let handle = data.handle();
    println!("Toybox server \"{}\" (version {})", config.name, crate::SERVER_VERSION);
    println!("  Data folder: {}", data.root.display());
    let library = match toybox_core::infra::library::validate(&handle.root) {
        Ok(()) => match count_assets(&handle) {
            Some(n) => format!("{} ({n} items)", handle.root.display()),
            None => format!("{} (not indexed yet; the first start does that)", handle.root.display()),
        },
        Err(e) => format!("{} (PROBLEM: {e})", handle.root.display()),
    };
    println!("  Library:     {library}");
    let count = |role: Role| config.tokens.iter().filter(|t| t.role == role).count();
    println!(
        "  Tokens:      {} ({} admin, {} write, {} read)",
        config.tokens.len(),
        count(Role::Admin),
        count(Role::Write),
        count(Role::Read)
    );
    match probe(listen) {
        Some(addr) => println!("  Server:      running at {addr}"),
        None => println!("  Server:      not running (nothing answered on {listen})"),
    }
    Ok(())
}
