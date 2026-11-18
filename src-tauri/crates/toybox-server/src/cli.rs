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

pub fn run(cli: Cli) -> Result<(), String> {
    let data = DataDir::new(cli.data);
    match cli.command {
        Command::Init { name } => init(&data, &name),
        Command::Serve { listen, max_file_mb, max_upload_mb } => serve(&data, &listen, max_file_mb, max_upload_mb),
        Command::Token { action } => match action {
            TokenAction::Add { name, role } => token_add(&data, &name, role.into()),
            TokenAction::List => token_list(&data),
            TokenAction::Revoke { token } => token_revoke(&data, &token),
        },
        Command::Status { listen } => status(&data, &listen),
    }
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
