//! Module resolution and loading with a host-controlled jail.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{Ctx, Exception, Module};

/// What a bare specifier (`@scope/pkg`) maps to.
#[derive(Debug, Clone)]
pub(crate) enum Bare {
    File { path: PathBuf, root: PathBuf },
    Virtual(String),
}

#[derive(Default)]
pub(crate) struct ModState {
    /// In-memory modules: name -> source.
    pub(crate) virtuals: HashMap<String, String>,
    pub(crate) bare: HashMap<String, Bare>,
    /// Roots that code without a root of its own (the host's bootstrap) may import absolute paths from.
    pub(crate) roots: Vec<PathBuf>,
    /// Every module name seen so far: file modules carry their allowed root, in-memory ones `None`.
    pub(crate) modules: HashMap<String, Option<PathBuf>>,
    pub(crate) hint: String,
}

pub(crate) type SharedState = Rc<RefCell<ModState>>;

/// Strips the Windows verbatim prefix that `canonicalize` adds.
pub(crate) fn simplify(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy().into_owned();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    p
}

pub(crate) fn canonical(p: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(p).map(simplify)
}

/// Resolves `.` and `..` without touching the disk. Leading `..` that cannot be
/// resolved are kept, so the result never silently climbs above a root.
pub(crate) fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }
    out.iter().collect()
}

fn is_relative_spec(s: &str) -> bool {
    s == "." || s == ".." || s.starts_with("./") || s.starts_with("../") || s.starts_with(".\\") || s.starts_with("..\\")
}

fn percent_decode(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = std::str::from_utf8(&b[i + 1..i + 3]).map_err(|_| "bad percent escape")?;
            out.push(u8::from_str_radix(h, 16).map_err(|_| "bad percent escape".to_string())?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "file URL is not valid UTF-8".to_string())
}

/// `file:` URL or absolute path to a path; `None` when the specifier is neither.
fn path_from_spec(spec: &str) -> Option<Result<PathBuf, String>> {
    if let Some(rest) = spec.strip_prefix("file:") {
        let rest = rest.split(['?', '#']).next().unwrap_or("");
        let path_part = if let Some(after) = rest.strip_prefix("//") {
            let (host, p) = match after.find('/') {
                Some(i) => (&after[..i], &after[i..]),
                None => (after, ""),
            };
            if !host.is_empty() && host != "localhost" {
                return Some(Err(format!("file URL host \"{host}\" is not supported")));
            }
            p
        } else {
            rest
        };
        let decoded = match percent_decode(path_part) {
            Ok(d) => d,
            Err(e) => return Some(Err(e)),
        };
        #[cfg(windows)]
        let decoded = {
            let b = decoded.as_bytes();
            if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
                decoded[1..].to_string()
            } else {
                decoded
            }
        };
        return Some(Ok(PathBuf::from(decoded)));
    }
    let b = spec.as_bytes();
    let abs = spec.starts_with('/')
        || spec.starts_with('\\')
        || (cfg!(windows) && b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\'));
    if abs {
        Some(Ok(PathBuf::from(spec)))
    } else {
        None
    }
}

pub(crate) fn path_to_file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl ModState {
    pub(crate) fn resolve(&mut self, base: &str, name: &str) -> Result<String, String> {
        let base_kind = self.modules.get(base).cloned();
        let shown_base = if base.is_empty() { "<host>" } else { base };
        if is_relative_spec(name) {
            return match base_kind {
                Some(Some(root)) => self.resolve_file_relative(base, name, &root),
                Some(None) => self.resolve_virtual_relative(base, name),
                None => Err(format!(
                    "Cannot resolve \"{name}\" from \"{shown_base}\": relative imports are only available inside a module"
                )),
            };
        }
        if let Some(path) = path_from_spec(name) {
            let path = path.map_err(|e| format!("Cannot import \"{name}\" from \"{shown_base}\": {e}"))?;
            let own_root = base_kind.and_then(|k| k);
            return self.resolve_absolute(shown_base, name, &path, own_root);
        }
        match self.bare.get(name).cloned() {
            Some(Bare::Virtual(vname)) => {
                if !self.virtuals.contains_key(&vname) {
                    return Err(format!("Cannot import \"{name}\" from \"{shown_base}\": the built-in module \"{vname}\" is missing"));
                }
                self.modules.entry(vname.clone()).or_insert(None);
                Ok(vname)
            }
            Some(Bare::File { path, root }) => {
                let canon = canonical(&path).map_err(|e| {
                    format!("Cannot import \"{name}\" from \"{shown_base}\": its file {} cannot be read ({e})", path.display())
                })?;
                if !canon.starts_with(&root) {
                    return Err(format!(
                        "Cannot import \"{name}\" from \"{shown_base}\": its file {} is outside its allowed folder {}",
                        canon.display(),
                        root.display()
                    ));
                }
                let key = canon.to_string_lossy().into_owned();
                self.modules.entry(key.clone()).or_insert(Some(root));
                Ok(key)
            }
            None => Err(format!(
                "Cannot import \"{name}\" from \"{shown_base}\": the module \"{name}\" is not available outside the app window \
                 (on the command line only the plugin's own files and the modules the host provides can be imported).{}",
                if self.hint.is_empty() { String::new() } else { format!(" {}", self.hint) }
            )),
        }
    }

    fn resolve_file_relative(&mut self, base: &str, name: &str, root: &Path) -> Result<String, String> {
        let dir = Path::new(base).parent().unwrap_or(Path::new(""));
        let joined = lexical_normalize(&dir.join(name));
        let outside = || {
            format!(
                "Cannot import \"{name}\" from \"{base}\": the path leaves the plugin folder {}",
                root.display()
            )
        };
        if !joined.starts_with(root) {
            return Err(outside());
        }
        let canon = canonical(&joined)
            .map_err(|_| format!("Cannot find module \"{name}\" imported from \"{base}\" (looked for {})", joined.display()))?;
        if !canon.starts_with(root) {
            return Err(outside());
        }
        if !canon.is_file() {
            return Err(format!("Cannot find module \"{name}\" imported from \"{base}\": {} is not a file", canon.display()));
        }
        let key = canon.to_string_lossy().into_owned();
        self.modules.entry(key.clone()).or_insert(Some(root.to_path_buf()));
        Ok(key)
    }

    fn resolve_virtual_relative(&mut self, base: &str, name: &str) -> Result<String, String> {
        let mut parts: Vec<&str> = base.split('/').collect();
        parts.pop(); // file name of the importer
        for seg in name.split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err(format!("Cannot import \"{name}\" from \"{base}\": the path leaves its built-in module folder"));
                    }
                }
                s => parts.push(s),
            }
        }
        let resolved = parts.join("/");
        if !self.virtuals.contains_key(&resolved) {
            return Err(format!("Cannot find module \"{name}\" imported from \"{base}\" (no built-in module \"{resolved}\")"));
        }
        self.modules.entry(resolved.clone()).or_insert(None);
        Ok(resolved)
    }

    fn resolve_absolute(&mut self, base: &str, name: &str, path: &Path, own_root: Option<PathBuf>) -> Result<String, String> {
        let lexical = lexical_normalize(path);
        let canon = match canonical(&lexical) {
            Ok(c) => c,
            Err(_) => {
                let allowed = match &own_root {
                    Some(r) => lexical.starts_with(r),
                    None => self.roots.iter().any(|r| lexical.starts_with(r)),
                };
                return Err(if allowed {
                    format!("Cannot find module \"{name}\" imported from \"{base}\"")
                } else {
                    format!("Cannot import \"{name}\" from \"{base}\": the path is outside the allowed folders")
                });
            }
        };
        let key = canon.to_string_lossy().into_owned();
        let root = match own_root {
            Some(r) => canon.starts_with(&r).then_some(r),
            None => match self.modules.get(&key) {
                Some(Some(r)) => Some(r.clone()),
                _ => self
                    .roots
                    .iter()
                    .filter(|r| canon.starts_with(r))
                    .max_by_key(|r| r.components().count())
                    .cloned(),
            },
        };
        let Some(root) = root else {
            return Err(format!("Cannot import \"{name}\" from \"{base}\": the path is outside the allowed folders"));
        };
        if !canon.is_file() {
            return Err(format!("Cannot find module \"{name}\" imported from \"{base}\": {} is not a file", canon.display()));
        }
        self.modules.entry(key.clone()).or_insert(Some(root));
        Ok(key)
    }
}

pub(crate) struct JailResolver(pub(crate) SharedState);

impl Resolver for JailResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let r = self.0.borrow_mut().resolve(base, name);
        r.map_err(|m| Exception::throw_message(ctx, &m))
    }
}

pub(crate) struct DiskLoader(pub(crate) SharedState);

impl Loader for DiskLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, Declared>> {
        let (source, url) = {
            let st = self.0.borrow();
            if let Some(src) = st.virtuals.get(name) {
                (src.clone(), name.to_string())
            } else if matches!(st.modules.get(name), Some(Some(_))) {
                let bytes = std::fs::read(name)
                    .map_err(|e| Exception::throw_message(ctx, &format!("Cannot read module \"{name}\": {e}")))?;
                let text = String::from_utf8(bytes)
                    .map_err(|_| Exception::throw_message(ctx, &format!("Module \"{name}\" is not valid UTF-8")))?;
                let text = text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text);
                let text = if name.ends_with(".json") {
                    let lit = serde_json::to_string(&text).unwrap_or_else(|_| "\"\"".into());
                    format!("export default JSON.parse({lit});")
                } else {
                    text
                };
                (text, path_to_file_url(Path::new(name)))
            } else {
                return Err(Exception::throw_message(ctx, &format!("Module \"{name}\" was not resolved")));
            }
        };
        let module = Module::declare(ctx.clone(), name, source)?;
        module.meta()?.set("url", url)?;
        Ok(module)
    }
}
