//! Services a plugin can ask the host for beyond the jailed filesystem, so a
//! plugin that drives an external program can live entirely in its own folder:
//!
//! - **Settings** it declares in its manifest (`settings[]`), which the user
//!   fills in under Settings → Plugins. Stored per plugin in `settings.json`.
//! - **Tools**: start an executable the *user* pointed one of those settings
//!   at. A plugin cannot name a program itself. Needs `permissions.runTools`.
//! - **Local requests**: exchange one JSON message with a service on this
//!   machine (`127.0.0.1` only). Needs `permissions.localNetwork`.
//! - **Scratch**: a private folder for intermediate files.
//!
//! Every call names the calling plugin and checks that plugin's own
//! manifest, read from the plugins folder, for the permission.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use toybox_core::error::{AppError, AppResult};

use crate::host::Host;

/// Programs plugins started, stopped when the app exits.
#[derive(Default)]
pub struct ToolProcesses(Mutex<Vec<Child>>);

impl ToolProcesses {
    pub fn stop_all(&self) {
        if let Ok(mut children) = self.0.lock() {
            for child in children.iter_mut() {
                let _ = child.kill();
            }
            children.clear();
        }
    }
}

/// A plugin id is one folder name: letters, digits, dots, dashes, underscores.
pub(crate) fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.starts_with('.') && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

fn read_manifest(host: &Host, plugin_id: &str) -> AppResult<Value> {
    if !valid_plugin_id(plugin_id) {
        return Err(AppError::msg(format!("'{plugin_id}' is not a plugin id")));
    }
    let path = host.paths.plugins_dir().join(plugin_id).join("manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|_| AppError::msg(format!("plugin '{plugin_id}' is not installed")))?;
    Ok(serde_json::from_str(&text)?)
}

fn require(manifest: &Value, plugin_id: &str, permission: &str) -> AppResult<()> {
    if manifest["permissions"][permission].as_bool() == Some(true) {
        Ok(())
    } else {
        Err(AppError::msg(format!("plugin '{plugin_id}' lacks permission: {permission}")))
    }
}

/// The setting a manifest declares under `key`, if any.
fn declared<'a>(manifest: &'a Value, key: &str) -> Option<&'a Value> {
    manifest["settings"].as_array()?.iter().find(|setting| setting["key"].as_str() == Some(key))
}

/// Replace `{appData}`, `{localAppData}` and `{home}` in a default value, so a
/// manifest can point at a program's usual install location on any machine.
pub fn expand_tokens(text: &str, folders: &[(&str, Option<PathBuf>)]) -> String {
    let mut out = text.to_string();
    for (token, folder) in folders {
        if let Some(folder) = folder {
            out = out.replace(&format!("{{{token}}}"), &folder.to_string_lossy().replace('\\', "/"));
        }
    }
    out
}

/// Stored values over the manifest's defaults, for the keys the manifest declares.
pub fn effective_settings(manifest: &Value, stored: Option<&Map<String, Value>>, folders: &[(&str, Option<PathBuf>)]) -> Map<String, Value> {
    let mut out = Map::new();
    for setting in manifest["settings"].as_array().into_iter().flatten() {
        let Some(key) = setting["key"].as_str() else { continue };
        let value = match stored.and_then(|values| values.get(key)).filter(|value| !value.is_null()) {
            Some(value) => value.clone(),
            None => match &setting["default"] {
                Value::String(text) => Value::String(expand_tokens(text, folders)),
                other => other.clone(),
            },
        };
        if !value.is_null() {
            out.insert(key.to_string(), value);
        }
    }
    out
}

/// The folders a manifest default may name: `{appData}`, `{localAppData}` and `{home}`.
fn folders() -> Vec<(&'static str, Option<PathBuf>)> {
    vec![("appData", dirs::data_dir()), ("localAppData", dirs::data_local_dir()), ("home", dirs::home_dir())]
}

fn settings_of(host: &Host, plugin_id: &str, manifest: &Value) -> AppResult<Map<String, Value>> {
    let stored = host.load_settings()?.plugin_settings;
    Ok(effective_settings(manifest, stored.get(plugin_id), &folders()))
}

/// The program a `file` setting of the plugin points at, after checking the
/// plugin may run tools and the file exists. The path comes only from the
/// user's setting (or the manifest's declared default).
fn tool_program(host: &Host, plugin_id: &str, setting: &str) -> AppResult<PathBuf> {
    let manifest = read_manifest(host, plugin_id)?;
    require(&manifest, plugin_id, "runTools")?;
    if declared(&manifest, setting).and_then(|spec| spec["type"].as_str()) != Some("file") {
        return Err(AppError::msg(format!("plugin '{plugin_id}' has no file setting '{setting}'")));
    }
    let values = settings_of(host, plugin_id, &manifest)?;
    let path = values.get(setting).and_then(Value::as_str).map(str::trim).filter(|path| !path.is_empty())
        .ok_or_else(|| AppError::msg(format!("the '{setting}' setting is empty")))?;
    let program = PathBuf::from(path);
    if !program.is_file() {
        return Err(AppError::msg(format!("no program at {path}")));
    }
    Ok(program)
}

impl Host {
    /// An installed plugin's manifest as written (the permissions are checked against this).
    /// Errors when `plugin_id` is not a plugin id or is not installed.
    pub fn plugin_manifest(&self, plugin_id: &str) -> AppResult<Value> {
        read_manifest(self, plugin_id)
    }

    /// A plugin's settings: what the user chose, else each declared default.
    pub fn plugin_settings(&self, plugin_id: &str) -> AppResult<Map<String, Value>> {
        let manifest = read_manifest(self, plugin_id)?;
        settings_of(self, plugin_id, &manifest)
    }

    /// Store one setting of a plugin; `null` returns it to its default. Only keys
    /// the plugin's manifest declares are accepted.
    pub fn set_plugin_setting(&self, plugin_id: &str, key: &str, value: Value) -> AppResult<()> {
        let manifest = read_manifest(self, plugin_id)?;
        if declared(&manifest, key).is_none() {
            return Err(AppError::msg(format!("plugin '{plugin_id}' has no setting '{key}'")));
        }
        let mut all = self.load_settings()?;
        all.set_plugin_setting(plugin_id, key, value);
        self.save_settings(&all)
    }
}

/// A command for `program` with `args`, run in the program's own folder and
/// without a console window on Windows.
fn tool_command(program: &Path, args: &[String]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(folder) = program.parent().filter(|folder| !folder.as_os_str().is_empty()) {
        command.current_dir(folder);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

/// What a finished program printed and how it ended.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    /// The exit code; -1 when the program was ended by a signal.
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The most output kept from each stream of a program.
const OUTPUT_LIMIT: usize = 1024 * 1024;

/// Run the program a `file` setting of the plugin points at, with `args`, in
/// that program's own folder, and wait for it. A program still running after
/// `timeout_ms` (two minutes when 0, at most 30 minutes) is killed and the call
/// fails. A non-zero exit code is a normal result.
impl Host {
    pub fn run_plugin_tool(&self, plugin_id: &str, setting: &str, args: &[String], timeout_ms: u64) -> AppResult<ToolResult> {
        let program = tool_program(self, plugin_id, setting)?;
        let timeout = Duration::from_millis(if timeout_ms == 0 { 120_000 } else { timeout_ms.min(30 * 60 * 1000) });
        run_program(&program, args, timeout)
    }
}

/// Read a stream to its end on its own thread, keeping the last `OUTPUT_LIMIT` bytes.
fn drain(mut stream: impl Read + Send + 'static) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut kept: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 64 * 1024];
        while let Ok(count) = stream.read(&mut chunk) {
            if count == 0 { break; }
            kept.extend_from_slice(&chunk[..count]);
            if kept.len() > OUTPUT_LIMIT * 2 {
                kept.drain(..kept.len() - OUTPUT_LIMIT);
            }
        }
        if kept.len() > OUTPUT_LIMIT {
            kept.drain(..kept.len() - OUTPUT_LIMIT);
        }
        let _ = sender.send(kept);
    });
    receiver
}

/// Run `program` with `args` to completion or `timeout`, capturing its output.
fn run_program(program: &Path, args: &[String], timeout: Duration) -> AppResult<ToolResult> {
    let mut command = tool_command(program, args);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| AppError::msg(format!("could not start {}: {error}", program.display())))?;
    let stdout = drain(child.stdout.take().ok_or_else(|| AppError::msg("no output pipe"))?);
    let stderr = drain(child.stderr.take().ok_or_else(|| AppError::msg("no output pipe"))?);
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::msg(format!("{} did not finish within {} seconds", program.display(), timeout.as_secs())));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // A program it started may keep the pipes open; wait only briefly for the rest.
    let text = |receiver: std::sync::mpsc::Receiver<Vec<u8>>| String::from_utf8_lossy(&receiver.recv_timeout(Duration::from_secs(5)).unwrap_or_default()).into_owned();
    Ok(ToolResult { code: status.code().unwrap_or(-1), stdout: text(stdout), stderr: text(stderr) })
}

/// Start the program a `file` setting of the plugin points at, with `args`, in
/// that program's own folder. It runs in the background until it exits or the
/// app closes. The path always comes from the user's setting (or the default
/// the manifest declares), never from the call.
impl Host {
    pub fn launch_plugin_tool(&self, plugin_id: &str, setting: &str, args: &[String]) -> AppResult<()> {
        let program = tool_program(self, plugin_id, setting)?;
        let mut command = tool_command(&program, args);
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let child = command.spawn().map_err(|error| AppError::msg(format!("could not start {}: {error}", program.display())))?;
        if let Ok(mut children) = self.tools.0.lock() {
            // Forget the ones that have already exited.
            children.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
            children.push(child);
        }
        Ok(())
    }
}

/// Toybox's own process id. A program a plugin starts may want to know which
/// process it is working for, to stop when that process goes away.
pub fn host_process_id() -> u32 {
    std::process::id()
}

fn local(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

impl Host {
    /// Whether something on this machine is listening on `port` (needs `permissions.localNetwork`).
    pub fn plugin_local_port_open(&self, plugin_id: &str, port: u16) -> AppResult<bool> {
        require(&read_manifest(self, plugin_id)?, plugin_id, "localNetwork")?;
        Ok(TcpStream::connect_timeout(&local(port), Duration::from_millis(800)).is_ok())
    }

    /// Send one JSON message to a service on this machine and return its JSON
    /// reply. One connection per message; the reply ends when it parses or the
    /// service closes the connection (needs `permissions.localNetwork`).
    pub fn plugin_local_json(&self, plugin_id: &str, port: u16, message: &Value, timeout_ms: u64) -> AppResult<Value> {
        require(&read_manifest(self, plugin_id)?, plugin_id, "localNetwork")?;
        let timeout = Duration::from_millis(timeout_ms.clamp(100, 600_000));
        exchange_json(local(port), message, timeout)
    }
}

fn exchange_json(address: SocketAddr, message: &Value, timeout: Duration) -> AppResult<Value> {
    let deadline = Instant::now() + timeout;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3).min(timeout))
        .map_err(|error| AppError::msg(format!("nothing is listening on port {}: {error}", address.port())))?;
    stream.write_all(serde_json::to_string(message)?.as_bytes())?;
    stream.flush()?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    let mut reply: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                reply.extend_from_slice(&chunk[..count]);
                if let Ok(value) = serde_json::from_slice::<Value>(&reply) {
                    return Ok(value);
                }
            }
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(error) => return Err(error.into()),
        }
        if Instant::now() >= deadline {
            return Err(AppError::msg(format!("no reply from port {} within {} ms", address.port(), timeout.as_millis())));
        }
    }
    serde_json::from_slice(&reply).map_err(|_| AppError::msg(format!("port {} did not reply with JSON", address.port())))
}

impl Host {
    /// The plugin's private folder for intermediate files, created on demand.
    /// `clear` empties it first. Nothing here is kept: the host may empty it too.
    pub fn plugin_scratch_dir(&self, plugin_id: &str, clear: bool) -> AppResult<String> {
        read_manifest(self, plugin_id)?;
        let dir = self.paths.plugin_scratch_root().join(plugin_id);
        if clear && dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir.to_string_lossy().replace('\\', "/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::TcpListener;

    #[test]
    fn settings_fall_back_to_declared_defaults_with_folder_tokens() {
        let manifest = json!({ "settings": [
            { "key": "engine", "type": "file", "default": "{appData}/Adobe/engine.exe" },
            { "key": "size", "type": "select", "default": "1024" },
            { "key": "notes", "type": "text" },
        ] });
        let folders = [("appData", Some(PathBuf::from("C:\\Users\\me\\AppData\\Roaming"))), ("home", None)];
        let defaults = effective_settings(&manifest, None, &folders);
        assert_eq!(defaults["engine"], "C:/Users/me/AppData/Roaming/Adobe/engine.exe");
        assert_eq!(defaults["size"], "1024");
        assert!(!defaults.contains_key("notes"));

        let stored = json!({ "engine": "D:/tools/engine.exe", "size": null, "undeclared": "ignored" });
        let chosen = effective_settings(&manifest, stored.as_object(), &folders);
        assert_eq!(chosen["engine"], "D:/tools/engine.exe");
        assert_eq!(chosen["size"], "1024");
        assert!(!chosen.contains_key("undeclared"));
    }

    #[test]
    fn permissions_and_plugin_ids_are_checked() {
        let manifest = json!({ "permissions": { "runTools": true }, "settings": [{ "key": "engine", "type": "file" }] });
        assert!(require(&manifest, "p", "runTools").is_ok());
        assert!(require(&manifest, "p", "localNetwork").is_err());
        assert!(declared(&manifest, "engine").is_some());
        assert!(declared(&manifest, "other").is_none());
        assert!(valid_plugin_id("com.toybox.material-sbsar"));
        for id in ["", "..", "../x", "a/b", "a\\b", ".hidden", "a b"] {
            assert!(!valid_plugin_id(id), "{id}");
        }
    }

    #[test]
    fn a_json_message_gets_its_json_reply_even_when_the_service_keeps_the_connection_open() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 1024];
            let count = stream.read(&mut buffer).unwrap();
            let request: Value = serde_json::from_slice(&buffer[..count]).unwrap();
            // Reply in two pieces, then hold the connection open.
            stream.write_all(b"{\"echo\":").unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(60));
            stream.write_all(format!("{}}}", request["operation"]).as_bytes()).unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(400));
        });
        let reply = exchange_json(address, &json!({ "operation": 20 }), Duration::from_secs(5)).unwrap();
        assert_eq!(reply, json!({ "echo": 20 }));
        server.join().unwrap();
    }

    #[cfg(windows)]
    fn cmd() -> PathBuf {
        PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32").join("cmd.exe")
    }

    #[cfg(windows)]
    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[cfg(windows)]
    #[test]
    fn a_program_runs_and_its_output_is_captured() {
        let result = run_program(&cmd(), &words(&["/C", "echo hello"]), Duration::from_secs(30)).unwrap();
        assert_eq!(result.code, 0);
        assert_eq!(result.stdout.trim(), "hello");
        assert!(result.stderr.is_empty());
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("code").is_some() && json.get("stdout").is_some() && json.get("stderr").is_some());
    }

    #[cfg(windows)]
    #[test]
    fn a_failing_exit_code_is_a_result_not_an_error() {
        let result = run_program(&cmd(), &words(&["/C", "exit 3"]), Duration::from_secs(30)).unwrap();
        assert_eq!(result.code, 3);
    }

    #[cfg(windows)]
    #[test]
    fn a_program_that_outlives_its_timeout_is_killed() {
        let started = Instant::now();
        let error = run_program(&cmd(), &words(&["/C", "ping -n 30 127.0.0.1"]), Duration::from_millis(300)).unwrap_err().to_string();
        assert!(error.contains("did not finish"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_missing_program_is_reported() {
        assert!(run_program(Path::new("definitely-not-a-program.exe"), &[], Duration::from_secs(1)).is_err());
    }

    #[test]
    fn a_closed_port_is_reported_plainly() {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        let error = exchange_json(local(port), &json!({}), Duration::from_secs(2)).unwrap_err().to_string();
        assert!(error.contains("nothing is listening"), "{error}");
    }
}
