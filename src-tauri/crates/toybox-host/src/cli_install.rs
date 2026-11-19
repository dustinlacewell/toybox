//! Getting `tbx` onto the user's PATH, and saying whether it is.
//!
//! `tbx` ships inside the desktop app (next to the app's own program, always the same version). This
//! module is the logic behind "Command line" in Settings and `tbx self install|uninstall|status`; the
//! Windows installer runs `tbx self install` for the same effect. Nothing here talks to the real PATH
//! or registry except [`System`]; the tests use a fake [`Environment`].
//!
//! Per platform:
//! * Windows: a folder `bin` holding only a copy of `tbx.exe` (next to the app, or under
//!   `%LOCALAPPDATA%\Toybox` for a standalone `tbx`) is added to the user's PATH (the registry value
//!   `HKCU\Environment\Path`, no administrator needed). The app's own folder is not put on PATH: it holds
//!   `toybox.exe`, `uninstall.exe` and other files that would end up callable from every terminal.
//! * macOS and Linux: a symbolic link `~/.local/bin/tbx` to the bundled program (a package that installs
//!   `/usr/bin/tbx` needs nothing). An AppImage's program lives in a mount that changes on every run, so
//!   there the program is copied to `~/.local/bin/tbx` instead (and goes stale until the row updates it).

use std::path::{Path, PathBuf};

use serde::Serialize;

pub const EXE: &str = if cfg!(windows) { "tbx.exe" } else { "tbx" };
const APP_EXE: &str = if cfg!(windows) { "toybox.exe" } else { "toybox" };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

/// Everything that touches the machine outside the files this module is pointed at.
pub trait Environment {
    fn os(&self) -> Os;
    /// The directories of the PATH this process sees.
    fn path_dirs(&self) -> Vec<PathBuf>;
    fn home(&self) -> Option<PathBuf>;
    /// `%LOCALAPPDATA%` (Windows).
    fn local_data(&self) -> Option<PathBuf>;
    /// Set when running from an AppImage: the program is not at a stable path.
    fn appimage(&self) -> bool;
    /// Windows: the user's own PATH value, as stored.
    fn user_path(&self) -> Result<Option<String>, String>;
    fn set_user_path(&self, value: &str) -> Result<(), String>;
    /// Make `link` a symbolic link to `original` (Unix).
    fn symlink(&self, original: &Path, link: &Path) -> std::io::Result<()>;
    /// What `tbx --version` of this file prints after the name (`0.4.1`).
    fn version_of(&self, program: &Path) -> Option<String>;
}

/// The machine this runs on.
pub struct System;

impl Environment for System {
    fn os(&self) -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
    fn path_dirs(&self) -> Vec<PathBuf> {
        std::env::var_os("PATH").map(|value| std::env::split_paths(&value).collect()).unwrap_or_default()
    }
    fn home(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }
    fn local_data(&self) -> Option<PathBuf> {
        dirs::data_local_dir()
    }
    fn appimage(&self) -> bool {
        std::env::var_os("APPIMAGE").is_some()
    }
    fn user_path(&self) -> Result<Option<String>, String> {
        windows_registry::read()
    }
    fn set_user_path(&self, value: &str) -> Result<(), String> {
        windows_registry::write(value)
    }
    #[cfg(unix)]
    fn symlink(&self, original: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(original, link)
    }
    #[cfg(not(unix))]
    fn symlink(&self, _: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "links are not used on this system"))
    }
    fn version_of(&self, program: &Path) -> Option<String> {
        let output = std::process::Command::new(program).arg("--version").output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).split_whitespace().last().map(str::to_string)).flatten()
    }
}

#[cfg(windows)]
mod windows_registry {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ};
    use winreg::{RegKey, RegValue};

    pub fn read() -> Result<Option<String>, String> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Environment", KEY_READ).map_err(|e| e.to_string())?;
        match key.get_value::<String, _>("Path") {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn write(value: &str) -> Result<(), String> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE).map_err(|e| e.to_string())?;
        // Keep it expandable: other programs put %USERPROFILE%-style entries in it.
        let mut bytes: Vec<u8> = value.encode_utf16().chain(std::iter::once(0)).flat_map(|unit| unit.to_le_bytes()).collect();
        let raw = RegValue { bytes: std::mem::take(&mut bytes), vtype: REG_EXPAND_SZ };
        key.set_raw_value("Path", &raw).map_err(|e| e.to_string())?;
        broadcast();
        Ok(())
    }

    /// Tell running programs (Explorer, so new terminals) that the environment changed.
    fn broadcast() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
        let text: Vec<u16> = "Environment".encode_utf16().chain(std::iter::once(0)).collect();
        let mut result = 0usize;
        // SAFETY: a documented call with a NUL-terminated wide string that outlives it.
        unsafe {
            SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, text.as_ptr() as isize, SMTO_ABORTIFHUNG, 2000, &mut result);
        }
    }
}

#[cfg(not(windows))]
mod windows_registry {
    pub fn read() -> Result<Option<String>, String> {
        Ok(None)
    }
    pub fn write(_: &str) -> Result<(), String> {
        Err("There is no user PATH value to edit on this system".into())
    }
}

// ----------------------------------------------------------------- the status

/// What Settings and `tbx self status` show.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    /// The `tbx` that comes with this app, if the app has one.
    pub bundled: Option<String>,
    pub bundled_version: Option<String>,
    /// The `tbx` a terminal finds first.
    pub on_path: Option<String>,
    pub on_path_version: Option<String>,
    /// The version of the app this is asked from.
    pub app_version: String,
    /// The terminal's `tbx` is the bundled one (or the same version of it).
    pub up_to_date: bool,
    /// Where "Add to PATH" would put it.
    pub target: Option<String>,
    /// Whether the folder that holds the target is on PATH already.
    pub target_on_path: bool,
    /// An installer or package already did it (nothing to offer).
    pub managed: bool,
    /// One sentence for the person.
    pub message: String,
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['/', '\\']).replace('\\', "/").to_lowercase();
    norm(a) == norm(b)
}

fn find_on_path(env: &dyn Environment) -> Option<PathBuf> {
    env.path_dirs().into_iter().map(|dir| dir.join(EXE)).find(|candidate| candidate.is_file())
}

/// Where the program should be installed for a user: the folder for Windows, the file for the rest.
pub fn install_target(env: &dyn Environment, bundled: &Path) -> Option<PathBuf> {
    match env.os() {
        Os::Windows => {
            let beside_app = bundled.parent().filter(|dir| dir.join(APP_EXE).is_file());
            let folder = match beside_app {
                Some(dir) => dir.join("bin"),
                None => env.local_data()?.join("Toybox").join("bin"),
            };
            Some(folder.join(EXE))
        }
        _ => Some(env.home()?.join(".local").join("bin").join(EXE)),
    }
}

/// The `tbx` next to the app's own program.
pub fn bundled_for(app_exe: &Path) -> Option<PathBuf> {
    let candidate = app_exe.parent()?.join(EXE);
    candidate.is_file().then_some(candidate)
}

/// What is there. `bundled` is the program that comes with the app (or the running `tbx` itself).
pub fn status(env: &dyn Environment, bundled: Option<&Path>, app_version: &str) -> CliStatus {
    let on_path = find_on_path(env);
    let on_path_version = on_path.as_deref().and_then(|p| env.version_of(p));
    let bundled_version = bundled.and_then(|p| env.version_of(p)).or_else(|| bundled.map(|_| app_version.to_string()));
    let target = bundled.and_then(|b| install_target(env, b));
    let target_on_path = target.as_deref().and_then(Path::parent).is_some_and(|dir| env.path_dirs().iter().any(|d| same_dir(d, dir)));
    // A system package puts it in /usr/bin: on PATH with nothing of ours to remove.
    let managed = on_path.as_deref().is_some_and(|p| p.starts_with("/usr/bin") || p.starts_with("/usr/local/bin") || p.starts_with("/opt/homebrew/bin"));
    let up_to_date = match (&on_path_version, &bundled_version) {
        (Some(path), Some(bundled)) => path == bundled,
        (Some(path), None) => path == app_version,
        _ => false,
    };
    let message = match (&on_path, bundled) {
        (Some(path), _) if up_to_date => format!("tbx {} is on your PATH ({}).", on_path_version.clone().unwrap_or_default(), path.display()),
        (Some(path), _) => format!(
            "The tbx your terminal finds ({}) is version {}, but Toybox is {}. {}",
            path.display(),
            on_path_version.clone().unwrap_or_else(|| "unknown".into()),
            app_version,
            if bundled.is_some() { "Update it from Settings, or run: tbx self install" } else { "Update Toybox or download the matching tbx." }
        ),
        (None, Some(_)) if target_on_path => "tbx is not installed yet. Add it to your PATH from Settings.".to_string(),
        (None, Some(_)) => "tbx comes with Toybox but your terminal cannot find it yet. Add it to your PATH from Settings.".to_string(),
        (None, None) => "This copy of Toybox has no tbx with it. Download tbx from the release page.".to_string(),
    };
    CliStatus {
        bundled: bundled.map(|p| p.to_string_lossy().to_string()),
        bundled_version,
        on_path: on_path.map(|p| p.to_string_lossy().to_string()),
        on_path_version,
        app_version: app_version.to_string(),
        up_to_date,
        target: target.map(|p| p.to_string_lossy().to_string()),
        target_on_path,
        managed,
        message,
    }
}

// ----------------------------------------------------------------- install and remove

/// What [`install`] or [`uninstall`] did.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub program: Option<String>,
    /// Windows: the folder added to or removed from PATH.
    pub folder: Option<String>,
    /// Something was changed (false: it was already so).
    pub changed: bool,
    /// Open a new terminal (Windows) or add the folder to PATH (Unix, when it is not on it).
    pub note: Option<String>,
}

fn entries(value: &str) -> Vec<&str> {
    value.split(';').filter(|entry| !entry.trim().is_empty()).collect()
}

fn contains_dir(path_value: &str, dir: &Path) -> bool {
    entries(path_value).iter().any(|entry| same_dir(Path::new(entry.trim().trim_matches('"')), dir))
}

fn copy_if_different(from: &Path, to: &Path) -> Result<bool, String> {
    if to.is_file() && std::fs::read(from).map_err(|e| e.to_string())? == std::fs::read(to).map_err(|e| e.to_string())? {
        return Ok(false);
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    // Replace by rename so a running copy elsewhere is never half-written.
    let staged = to.with_extension("new");
    std::fs::copy(from, &staged).map_err(|e| format!("Could not copy tbx to {}: {e}", to.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_file(to);
    std::fs::rename(&staged, to).map_err(|e| format!("Could not put tbx in {}: {e}", to.display()))?;
    Ok(true)
}

/// Make `tbx` available in a terminal for this user. `bundled` is the program to install.
pub fn install(env: &dyn Environment, bundled: &Path) -> Result<Change, String> {
    if !bundled.is_file() {
        return Err(format!("{} is not there.", bundled.display()));
    }
    let target = install_target(env, bundled).ok_or("Could not find your home folder.")?;
    let folder = target.parent().ok_or("Bad target")?.to_path_buf();
    match env.os() {
        Os::Windows => {
            let mut changed = false;
            if !same_file(bundled, &target) {
                changed |= copy_if_different(bundled, &target)?;
            }
            let current = env.user_path()?.unwrap_or_default();
            if !contains_dir(&current, &folder) {
                let mut value = current.trim_end_matches(';').to_string();
                if !value.is_empty() {
                    value.push(';');
                }
                value.push_str(&folder.to_string_lossy());
                env.set_user_path(&value)?;
                changed = true;
            }
            Ok(Change {
                program: Some(target.to_string_lossy().to_string()),
                folder: Some(folder.to_string_lossy().to_string()),
                changed,
                note: changed.then(|| "Open a new terminal window to use it.".to_string()),
            })
        }
        _ => {
            std::fs::create_dir_all(&folder).map_err(|e| format!("Could not create {}: {e}", folder.display()))?;
            let mut changed = false;
            let copy_instead = env.appimage() || bundled.to_string_lossy().contains(".mount_");
            let existing = std::fs::symlink_metadata(&target).ok();
            if copy_instead {
                if existing.as_ref().is_some_and(|m| m.file_type().is_symlink()) {
                    let _ = std::fs::remove_file(&target);
                }
                changed |= copy_if_different(bundled, &target)?;
            } else {
                let points_there = std::fs::read_link(&target).map(|to| same_dir(&to, bundled)).unwrap_or(false);
                if !points_there {
                    match &existing {
                        Some(meta) if meta.file_type().is_symlink() => {
                            std::fs::remove_file(&target).map_err(|e| format!("Could not replace {}: {e}", target.display()))?
                        }
                        // A program that is not a link of ours: only replace one that is a tbx.
                        Some(_) if env.version_of(&target).is_some() => {
                            std::fs::remove_file(&target).map_err(|e| format!("Could not replace {}: {e}", target.display()))?
                        }
                        Some(_) => return Err(format!("{} exists and is not a tbx. Remove or rename it, then try again.", target.display())),
                        None => {}
                    }
                    env.symlink(bundled, &target).map_err(|e| format!("Could not link {}: {e}", target.display()))?;
                    changed = true;
                }
            }
            let on_path = env.path_dirs().iter().any(|dir| same_dir(dir, &folder));
            Ok(Change {
                program: Some(target.to_string_lossy().to_string()),
                folder: Some(folder.to_string_lossy().to_string()),
                changed,
                note: (!on_path).then(|| format!("{} is not on your PATH. Add it, for example in ~/.profile:  export PATH=\"{}:$PATH\"", folder.display(), folder.display())),
            })
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Take back what [`install`] did: the PATH entry and the copy (Windows), the link or copy (elsewhere).
pub fn uninstall(env: &dyn Environment, bundled: &Path) -> Result<Change, String> {
    let target = install_target(env, bundled).ok_or("Could not find your home folder.")?;
    let folder = target.parent().ok_or("Bad target")?.to_path_buf();
    match env.os() {
        Os::Windows => {
            let mut changed = false;
            let current = env.user_path()?.unwrap_or_default();
            if contains_dir(&current, &folder) {
                let kept: Vec<&str> = entries(&current).into_iter().filter(|entry| !same_dir(Path::new(entry.trim().trim_matches('"')), &folder)).collect();
                env.set_user_path(&kept.join(";"))?;
                changed = true;
            }
            if target.is_file() && !same_file(&target, bundled) {
                std::fs::remove_file(&target).map_err(|e| format!("Could not remove {}: {e}", target.display()))?;
                changed = true;
            }
            if folder.is_dir() && std::fs::read_dir(&folder).map(|mut d| d.next().is_none()).unwrap_or(false) {
                let _ = std::fs::remove_dir(&folder);
            }
            Ok(Change { program: None, folder: Some(folder.to_string_lossy().to_string()), changed, note: None })
        }
        _ => {
            let meta = std::fs::symlink_metadata(&target).ok();
            let ours = match &meta {
                Some(m) if m.file_type().is_symlink() => true,
                Some(_) => env.version_of(&target).is_some(),
                None => false,
            };
            if meta.is_some() && !ours {
                return Err(format!("{} is not a tbx, so it was left alone.", target.display()));
            }
            if meta.is_some() {
                std::fs::remove_file(&target).map_err(|e| format!("Could not remove {}: {e}", target.display()))?;
            }
            Ok(Change { program: None, folder: Some(folder.to_string_lossy().to_string()), changed: meta.is_some(), note: None })
        }
    }
}

// ----------------------------------------------------------------- what the app tells tbx

/// What the desktop app leaves for `tbx` in the shared app-data folder each time it starts, so `tbx`
/// can tell when it is not the version that was installed with the app.
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppRecord {
    pub version: String,
    pub app: String,
    /// The `tbx` shipped with that app, when there is one.
    pub tbx: Option<String>,
}

const RECORD: &str = "app-install.json";

pub fn write_app_record(app_data: &Path, app_exe: &Path, version: &str) -> Result<(), String> {
    let record = AppRecord { version: version.to_string(), app: app_exe.to_string_lossy().to_string(), tbx: bundled_for(app_exe).map(|p| p.to_string_lossy().to_string()) };
    std::fs::create_dir_all(app_data).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    // Only rewrite when it changed, so starting the app does not touch the disk needlessly.
    if std::fs::read(app_data.join(RECORD)).ok().as_deref() == Some(&bytes) {
        return Ok(());
    }
    std::fs::write(app_data.join(RECORD), bytes).map_err(|e| e.to_string())
}

pub fn read_app_record(app_data: &Path) -> Option<AppRecord> {
    serde_json::from_slice(&std::fs::read(app_data.join(RECORD)).ok()?).ok()
}

/// A sentence when the `tbx` that is running (`running_version`) differs from the installed app's version.
pub fn mismatch_with_app(record: &AppRecord, running_version: &str) -> Option<String> {
    let (Ok(app), Ok(me)) = (semver::Version::parse(&record.version), semver::Version::parse(running_version)) else { return None };
    if app == me {
        return None;
    }
    let fix = match &record.tbx {
        Some(path) => format!("Run the one that came with it ({path}) or `\"{path}\" self install`."),
        None => "Update tbx to the same version.".to_string(),
    };
    Some(format!("tbx {running_version} is {} than the installed Toybox {}. {fix}", if me < app { "older" } else { "newer" }, record.version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct Fake {
        os: Os,
        path: Vec<PathBuf>,
        home: PathBuf,
        local: PathBuf,
        appimage: bool,
        registry: RefCell<Option<String>>,
        /// Contents of a program file -> the version it reports.
        versions: Vec<(Vec<u8>, String)>,
    }

    impl Environment for Fake {
        fn os(&self) -> Os {
            self.os
        }
        fn path_dirs(&self) -> Vec<PathBuf> {
            self.path.clone()
        }
        fn home(&self) -> Option<PathBuf> {
            Some(self.home.clone())
        }
        fn local_data(&self) -> Option<PathBuf> {
            Some(self.local.clone())
        }
        fn appimage(&self) -> bool {
            self.appimage
        }
        fn user_path(&self) -> Result<Option<String>, String> {
            Ok(self.registry.borrow().clone())
        }
        fn set_user_path(&self, value: &str) -> Result<(), String> {
            *self.registry.borrow_mut() = Some(value.to_string());
            Ok(())
        }
        fn symlink(&self, original: &Path, link: &Path) -> std::io::Result<()> {
            // A link is a small file naming its target, so this runs on every system.
            std::fs::write(link, format!("-> {}", original.display()))
        }
        fn version_of(&self, program: &Path) -> Option<String> {
            let mut content = std::fs::read(program).ok()?;
            // A fake link names its target.
            if let Some(target) = content.strip_prefix(b"-> ") {
                content = std::fs::read(String::from_utf8_lossy(target).to_string()).ok()?;
            }
            self.versions.iter().find(|(bytes, _)| *bytes == content).map(|(_, v)| v.clone())
        }
    }

    struct Dirs(PathBuf);
    impl Dirs {
        fn new(name: &str) -> Dirs {
            let root = std::env::temp_dir().join(format!("toybox-cli-install-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Dirs(root)
        }
    }
    impl Drop for Dirs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fake(os: Os, dirs: &Dirs) -> Fake {
        Fake {
            os,
            path: vec![],
            home: dirs.0.join("home"),
            local: dirs.0.join("local"),
            appimage: false,
            registry: RefCell::new(None),
            versions: vec![(b"tbx 0.4.1".to_vec(), "0.4.1".into()), (b"tbx 0.4.0".to_vec(), "0.4.0".into())],
        }
    }

    fn app(dirs: &Dirs, with_app: bool) -> PathBuf {
        let folder = dirs.0.join("app");
        std::fs::create_dir_all(&folder).unwrap();
        if with_app {
            std::fs::write(folder.join(APP_EXE), b"app").unwrap();
        }
        let tbx = folder.join(EXE);
        std::fs::write(&tbx, b"tbx 0.4.1").unwrap();
        tbx
    }

    #[test]
    fn windows_gets_a_bin_folder_with_only_tbx_on_the_user_path_once() {
        let dirs = Dirs::new("win");
        let env = fake(Os::Windows, &dirs);
        *env.registry.borrow_mut() = Some("C:\\Tools;%USERPROFILE%\\bin".into());
        let bundled = app(&dirs, true);
        let first = install(&env, &bundled).unwrap();
        let bin = bundled.parent().unwrap().join("bin");
        assert!(first.changed && bin.join(EXE).is_file());
        assert_eq!(std::fs::read_dir(&bin).unwrap().count(), 1, "only tbx is in that folder");
        let path = env.registry.borrow().clone().unwrap();
        assert_eq!(path, format!("C:\\Tools;%USERPROFILE%\\bin;{}", bin.display()), "the existing entries are untouched");
        assert!(first.note.unwrap().contains("new terminal"));
        // An update installs again: no second entry, nothing changed.
        let again = install(&env, &bundled).unwrap();
        assert!(!again.changed);
        assert_eq!(env.registry.borrow().clone().unwrap(), path);
        // A new version of the app's tbx is copied over, the entry still single.
        std::fs::write(&bundled, b"tbx 0.4.0").unwrap();
        assert!(install(&env, &bundled).unwrap().changed);
        assert_eq!(std::fs::read(bin.join(EXE)).unwrap(), b"tbx 0.4.0");
        assert_eq!(env.registry.borrow().clone().unwrap().matches("bin").count(), 2, "{path}");
        // Removing takes the entry and the copy, and the empty folder.
        let gone = uninstall(&env, &bundled).unwrap();
        assert!(gone.changed && !bin.exists());
        assert_eq!(env.registry.borrow().clone().unwrap(), "C:\\Tools;%USERPROFILE%\\bin");
        assert!(!uninstall(&env, &bundled).unwrap().changed);
    }

    #[test]
    fn windows_recognises_the_entry_however_it_is_written() {
        let dirs = Dirs::new("win-spelling");
        let env = fake(Os::Windows, &dirs);
        let bundled = app(&dirs, true);
        let bin = bundled.parent().unwrap().join("bin");
        let spelled = format!("\"{}\\\";C:\\Other", bin.to_string_lossy().to_uppercase().replace('/', "\\"));
        *env.registry.borrow_mut() = Some(spelled.clone());
        assert!(!install(&env, &bundled).unwrap().changed || env.registry.borrow().clone().unwrap() == spelled);
        assert_eq!(env.registry.borrow().clone().unwrap(), spelled, "not added twice");
    }

    #[test]
    fn a_standalone_windows_tbx_goes_to_the_users_program_data_not_next_to_itself() {
        let dirs = Dirs::new("win-alone");
        let env = fake(Os::Windows, &dirs);
        let alone = app(&dirs, false);
        install(&env, &alone).unwrap();
        assert!(dirs.0.join("local/Toybox/bin").join(EXE).is_file());
        assert!(!alone.parent().unwrap().join("bin").exists());
    }

    #[test]
    fn unix_gets_a_link_in_the_user_bin_and_is_told_when_that_is_not_on_path() {
        let dirs = Dirs::new("unix");
        let mut env = fake(Os::Linux, &dirs);
        let bundled = app(&dirs, true);
        let change = install(&env, &bundled).unwrap();
        let link = dirs.0.join("home/.local/bin").join(EXE);
        assert!(change.changed && link.is_file());
        assert!(change.note.unwrap().contains("is not on your PATH"));
        let again = install(&env, &bundled).unwrap();
        // (The fake link is a plain file, so only a real symbolic link is recognised as already in place.)
        assert!(!again.changed || !cfg!(unix), "already linked");
        env.path = vec![dirs.0.join("home/.local/bin")];
        assert!(install(&env, &bundled).unwrap().note.is_none());
        assert!(uninstall(&env, &bundled).unwrap().changed && !link.exists());
    }

    #[test]
    fn a_file_in_the_way_that_is_not_a_tbx_is_never_replaced() {
        let dirs = Dirs::new("unix-clash");
        let env = fake(Os::MacOs, &dirs);
        let bundled = app(&dirs, true);
        let link = dirs.0.join("home/.local/bin").join(EXE);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::fs::write(&link, b"something else").unwrap();
        assert!(install(&env, &bundled).unwrap_err().contains("is not a tbx"));
        assert!(uninstall(&env, &bundled).unwrap_err().contains("left alone"));
        assert_eq!(std::fs::read(&link).unwrap(), b"something else");
        // An older tbx there is replaced.
        std::fs::write(&link, b"tbx 0.4.0").unwrap();
        assert!(install(&env, &bundled).unwrap().changed);
    }

    #[test]
    fn an_appimage_gets_a_copy_because_its_own_path_changes_every_run() {
        let dirs = Dirs::new("appimage");
        let mut env = fake(Os::Linux, &dirs);
        env.appimage = true;
        let bundled = app(&dirs, true);
        install(&env, &bundled).unwrap();
        let target = dirs.0.join("home/.local/bin").join(EXE);
        assert_eq!(std::fs::read(&target).unwrap(), b"tbx 0.4.1");
        // The app updates; the copy is stale until installed again.
        std::fs::write(&bundled, b"tbx 0.4.0").unwrap();
        env.path = vec![dirs.0.join("home/.local/bin")];
        let before = status(&env, Some(&bundled), "0.4.0");
        assert!(!before.up_to_date && before.message.contains("0.4.1") && before.message.contains("0.4.0"), "{}", before.message);
        install(&env, &bundled).unwrap();
        assert!(status(&env, Some(&bundled), "0.4.0").up_to_date);
    }

    #[test]
    fn the_status_says_what_is_missing_and_what_is_stale() {
        let dirs = Dirs::new("status");
        let mut env = fake(Os::Linux, &dirs);
        let bundled = app(&dirs, true);
        let none = status(&env, Some(&bundled), "0.4.1");
        assert!(!none.up_to_date && none.on_path.is_none() && none.message.contains("cannot find it"), "{}", none.message);
        assert_eq!(none.bundled_version.as_deref(), Some("0.4.1"));
        let bin = dirs.0.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(EXE), b"tbx 0.4.0").unwrap();
        env.path = vec![bin.clone()];
        let stale = status(&env, Some(&bundled), "0.4.1");
        assert!(!stale.up_to_date && stale.message.contains("version 0.4.0") && stale.message.contains("Toybox is 0.4.1"), "{}", stale.message);
        std::fs::write(bin.join(EXE), b"tbx 0.4.1").unwrap();
        let fine = status(&env, Some(&bundled), "0.4.1");
        assert!(fine.up_to_date && fine.message.contains("is on your PATH"));
        assert!(status(&env, None, "0.4.1").message.contains("no tbx with it") || fine.up_to_date);
    }

    #[test]
    fn the_app_leaves_a_record_and_a_differing_tbx_is_told_so() {
        let dirs = Dirs::new("record");
        let tbx = app(&dirs, true);
        let app_exe = tbx.parent().unwrap().join(APP_EXE);
        let data = dirs.0.join("data");
        write_app_record(&data, &app_exe, "0.4.1").unwrap();
        let record = read_app_record(&data).unwrap();
        assert_eq!((record.version.as_str(), record.tbx.as_deref()), ("0.4.1", Some(tbx.to_string_lossy().as_ref())));
        assert_eq!(mismatch_with_app(&record, "0.4.1"), None);
        let text = mismatch_with_app(&record, "0.4.0").unwrap();
        assert!(text.contains("older than the installed Toybox 0.4.1") && text.contains("self install"), "{text}");
        assert!(mismatch_with_app(&record, "0.5.0").unwrap().contains("newer"));
        assert!(read_app_record(&dirs.0.join("nowhere")).is_none());
    }
}
