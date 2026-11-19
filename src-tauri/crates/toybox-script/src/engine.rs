use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rquickjs as rq;
use rquickjs::function::Rest;
use rquickjs::promise::PromiseState;
use rquickjs::{Ctx, Exception, Persistent};

use crate::error::{Error, Result};
use crate::modules::{canonical, Bare, DiskLoader, JailResolver, ModState, SharedState};
use crate::value::{from_js, make_args, to_js, with_bytes, Handle, Slots, Value};

type Pers = Persistent<rq::Value<'static>>;
type HostFn = Rc<dyn Fn(Vec<Value>) -> std::result::Result<Value, String>>;
type HostMap = Rc<RefCell<HashMap<String, HostFn>>>;

const PRELUDE: &str = include_str!("prelude.js");
static NEXT_ENGINE: AtomicU64 = AtomicU64::new(1);

/// Severity of a console message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Log,
    Info,
    Warn,
    Error,
    Debug,
}

impl LogLevel {
    fn from_i32(n: i32) -> LogLevel {
        match n {
            1 => LogLevel::Info,
            2 => LogLevel::Warn,
            3 => LogLevel::Error,
            4 => LogLevel::Debug,
            _ => LogLevel::Log,
        }
    }
}

/// Engine settings.
pub struct Config {
    /// JavaScript heap limit in bytes (byte buffers are allocated by Rust and not counted). Default 4 GiB.
    pub memory_limit: usize,
    /// Native stack the interpreter may use before it throws `RangeError`. Default 512 KiB, which is safe on
    /// every thread (a Windows main thread has 1 MiB); raise it only when the engine runs on a thread you
    /// gave a bigger stack.
    pub max_stack_size: usize,
    /// Set this from any thread to abort running script with an interrupt error.
    pub interrupt: Arc<AtomicBool>,
    /// Receives `console.*` output. Default: discarded.
    pub console: Option<Box<dyn Fn(LogLevel, &str)>>,
    /// Appended to the error for an import that is not available (a place for the host to say which
    /// modules it does provide).
    pub import_hint: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            memory_limit: if usize::BITS >= 64 { 4 << 30 } else { 1 << 30 },
            max_stack_size: 512 * 1024,
            interrupt: Arc::new(AtomicBool::new(false)),
            console: None,
            import_hint: String::new(),
        }
    }
}

/// A JavaScript runtime with Toybox-agnostic plugin plumbing. Not `Send`: keep it on one thread.
///
/// Host closures must not call back into the engine that is calling them.
pub struct Engine {
    // Drop order matters: the context goes before the runtime.
    tick: Option<Persistent<rq::Function<'static>>>,
    ctx: rq::Context,
    rt: rq::Runtime,
    slots: Rc<Slots>,
    hosts: HostMap,
    modules: SharedState,
    interrupt: Arc<AtomicBool>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.slots.clear();
        self.tick = None;
        self.rt.run_gc();
    }
}

fn error_from_value<'js>(ctx: &Ctx<'js>, v: rq::Value<'js>) -> Error {
    if let Some(obj) = v.as_object() {
        let get = |k: &str| obj.get::<_, Option<std::string::String>>(k).ok().flatten();
        let message = get("message");
        let name = get("name").unwrap_or_default();
        let stack = get("stack").filter(|s| !s.trim().is_empty());
        if let Some(message) = message {
            return Error { name, message, stack };
        }
        return Error { name, message: "an object was thrown".into(), stack };
    }
    let _ = ctx;
    let message = if let Some(s) = v.as_string() {
        s.to_string().unwrap_or_default()
    } else if let Some(b) = v.as_bool() {
        b.to_string()
    } else if let Some(n) = v.as_number() {
        n.to_string()
    } else if v.is_null() {
        "null".into()
    } else if v.is_undefined() {
        "undefined".into()
    } else {
        "a non-Error value was thrown".into()
    };
    Error::new(message)
}

fn error_from_exception<'js>(ctx: &Ctx<'js>, e: rq::Error) -> Error {
    match e {
        rq::Error::Exception => {
            let v = ctx.catch();
            error_from_value(ctx, v)
        }
        other => Error::new(other.to_string()),
    }
}

fn install<'js>(
    ctx: &Ctx<'js>,
    sink: Rc<dyn Fn(LogLevel, &str)>,
) -> rq::Result<Persistent<rq::Function<'static>>> {
    let native = rq::Object::new(ctx.clone())?;
    let start = Instant::now();
    native.set(
        "log",
        rq::Function::new(ctx.clone(), move |level: i32, text: std::string::String| {
            sink(LogLevel::from_i32(level), &text);
        })?,
    )?;
    native.set(
        "now",
        rq::Function::new(ctx.clone(), move || start.elapsed().as_secs_f64() * 1000.0)?,
    )?;
    native.set(
        "utf8Encode",
        rq::Function::new(ctx.clone(), |ctx: Ctx<'js>, s: std::string::String| {
            rq::TypedArray::<u8>::new(ctx, s.into_bytes())
        })?,
    )?;
    native.set(
        "utf8Decode",
        rq::Function::new(ctx.clone(), |v: rq::Value<'js>, fatal: bool| -> Option<std::string::String> {
            with_bytes(&v, |b| {
                if fatal {
                    std::str::from_utf8(b).ok().map(str::to_string)
                } else {
                    Some(std::string::String::from_utf8_lossy(b).into_owned())
                }
            })
            .flatten()
        })?,
    )?;
    let f: rq::Function = ctx.eval(PRELUDE)?;
    let ret: rq::Object = f.call((native,))?;
    let tick: rq::Function = ret.get("tick")?;
    Ok(Persistent::save(ctx, tick))
}

fn make_host_object<'js>(ctx: &Ctx<'js>, slots: Rc<Slots>, hosts: HostMap) -> rq::Result<rq::Object<'js>> {
    let obj = rq::Object::new(ctx.clone())?;
    let hosts2 = hosts.clone();
    obj.set(
        "call",
        rq::Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<rq::Value<'js>>| -> rq::Result<rq::Value<'js>> {
            let mut it = args.0.into_iter();
            let name: std::string::String = match it.next() {
                Some(n) => n.get()?,
                None => return Err(Exception::throw_message(&ctx, "host.call needs a function name")),
            };
            let f = hosts.borrow().get(&name).cloned();
            let Some(f) = f else {
                return Err(Exception::throw_message(&ctx, &format!("no host function named \"{name}\"")));
            };
            let mut vals = Vec::new();
            for a in it {
                vals.push(from_js(&ctx, &slots, a, 0)?);
            }
            match f(vals) {
                Ok(v) => to_js(&ctx, &slots, v),
                Err(msg) => Err(Exception::throw_message(&ctx, &msg)),
            }
        })?,
    )?;
    obj.set(
        "names",
        rq::Function::new(ctx.clone(), move |ctx: Ctx<'js>| -> rq::Result<rq::Array<'js>> {
            let arr = rq::Array::new(ctx.clone())?;
            let mut names: Vec<std::string::String> = hosts2.borrow().keys().cloned().collect();
            names.sort();
            for (i, n) in names.into_iter().enumerate() {
                arr.set(i, n)?;
            }
            Ok(arr)
        })?,
    )?;
    Ok(obj)
}

impl Engine {
    /// Creates a runtime with the standard globals installed.
    pub fn new(config: Config) -> Result<Engine> {
        let rt = rq::Runtime::new().map_err(|e| Error::new(e.to_string()))?;
        rt.set_memory_limit(config.memory_limit);
        rt.set_max_stack_size(config.max_stack_size);
        let flag = config.interrupt.clone();
        rt.set_interrupt_handler(Some(Box::new(move || flag.load(Ordering::Relaxed))));
        let modules: SharedState = Rc::new(RefCell::new(ModState { hint: config.import_hint, ..Default::default() }));
        rt.set_loader(JailResolver(modules.clone()), DiskLoader(modules.clone()));
        let ctx = rq::Context::full(&rt).map_err(|e| Error::new(e.to_string()))?;
        let sink: Rc<dyn Fn(LogLevel, &str)> = match config.console {
            Some(f) => Rc::from(f),
            None => Rc::new(|_, _| {}),
        };
        let tick = ctx
            .with(|ctx| install(&ctx, sink).map_err(|e| error_from_exception(&ctx, e)))?;
        Ok(Engine {
            tick: Some(tick),
            ctx,
            rt,
            slots: Rc::new(Slots::new(NEXT_ENGINE.fetch_add(1, Ordering::Relaxed))),
            hosts: Rc::default(),
            modules,
            interrupt: config.interrupt,
        })
    }

    /// The flag passed in [`Config::interrupt`].
    pub fn interrupt_flag(&self) -> Arc<AtomicBool> {
        self.interrupt.clone()
    }

    // ------------------------------------------------------------ setup

    /// Registers a host function. JavaScript reaches it as
    /// `import.meta.host.call(name, ...args)` from a module given to [`Engine::eval_module`];
    /// `import.meta.host.names()` lists the registered names. `Err(message)` is thrown as a JS `Error`.
    pub fn register_host_fn<F>(&self, name: &str, f: F)
    where
        F: Fn(Vec<Value>) -> std::result::Result<Value, String> + 'static,
    {
        self.hosts.borrow_mut().insert(name.to_string(), Rc::new(f));
    }

    /// Lets code that has no root of its own (modules from [`eval_module`](Engine::eval_module)) import
    /// absolute paths and `file:` URLs below `root`.
    pub fn allow_root(&self, root: &Path) -> Result<()> {
        let root = canonical(root).map_err(|e| Error::new(format!("{}: {e}", root.display())))?;
        let mut st = self.modules.borrow_mut();
        if !st.roots.contains(&root) {
            st.roots.push(root);
        }
        Ok(())
    }

    /// Makes `specifier` importable and resolve to `path`; `path` must be inside `root`, and the module's
    /// own relative imports are confined to `root`.
    pub fn map_bare_file(&self, specifier: &str, path: &Path, root: &Path) -> Result<()> {
        let root = canonical(root).map_err(|e| Error::new(format!("{}: {e}", root.display())))?;
        self.modules
            .borrow_mut()
            .bare
            .insert(specifier.to_string(), Bare::File { path: path.to_path_buf(), root });
        Ok(())
    }

    /// Makes `specifier` importable and resolve to the in-memory module `name` (see [`register_module`](Engine::register_module)).
    pub fn map_bare_module(&self, specifier: &str, name: &str) {
        self.modules.borrow_mut().bare.insert(specifier.to_string(), Bare::Virtual(name.to_string()));
    }

    /// Registers in-memory module source under a virtual name. A name like `sdk/index.js` lets modules
    /// registered as `sdk/util.js` import it as `./util.js`; `../` may not climb above the first segment.
    pub fn register_module(&self, name: &str, source: &str) {
        self.modules.borrow_mut().virtuals.insert(name.to_string(), source.to_string());
    }

    /// Registers a folder of in-memory modules: `files` are `(relative path, source)` pairs, registered as
    /// `{prefix}/{relative path}`. Returns nothing; map a bare specifier to `{prefix}/{entry}` with
    /// [`map_bare_module`](Engine::map_bare_module).
    pub fn register_folder<'a>(&self, prefix: &str, files: impl IntoIterator<Item = (&'a str, &'a str)>) {
        for (rel, src) in files {
            self.register_module(&format!("{}/{}", prefix.trim_end_matches('/'), rel.trim_start_matches("./")), src);
        }
    }

    // ---------------------------------------------------------- loading

    /// Imports the module at `path` (an ES module on disk), evaluating it and everything it imports once.
    /// Relative imports must stay inside `root`. Returns the module namespace; top-level await is awaited.
    pub fn load_module(&self, path: &Path, root: &Path) -> Result<Handle> {
        self.sweep();
        let root = canonical(root).map_err(|e| Error::new(format!("{}: {e}", root.display())))?;
        let canon = canonical(path).map_err(|e| Error::new(format!("Cannot find module {}: {e}", path.display())))?;
        if !canon.starts_with(&root) {
            return Err(Error::new(format!(
                "Cannot import {}: the file is outside the allowed folder {}",
                canon.display(),
                root.display()
            )));
        }
        let key = canon.to_string_lossy().into_owned();
        self.modules.borrow_mut().modules.insert(key.clone(), Some(root));
        let promise = self
            .with(|ctx| {
                let p = rq::Module::import(ctx, key)?;
                Ok(Persistent::save(ctx, p.into_value()))
            })
            .map_err(|e| self.fix(e))?;
        let ns = self.settle(promise).map_err(|e| self.fix(e))?;
        self.to_handle_p(ns)
    }

    /// Evaluates in-memory module source under a virtual name (the host's bootstrap). It may use
    /// `import.meta.host` (see [`register_host_fn`](Engine::register_host_fn)) and dynamic `import()` of
    /// absolute paths below [`allow_root`](Engine::allow_root) roots. Returns the module namespace.
    pub fn eval_module(&self, name: &str, source: &str) -> Result<Handle> {
        self.sweep();
        self.modules.borrow_mut().modules.insert(name.to_string(), None);
        let (promise, ns) = self
            .with(|ctx| {
                let module = rq::Module::declare(ctx.clone(), name, source)?;
                let meta = module.meta()?;
                meta.set("url", name)?;
                meta.set("host", make_host_object(ctx, self.slots.clone(), self.hosts.clone())?)?;
                let (module, promise) = module.eval()?;
                let ns = self.slots.store(ctx, module.namespace()?.into_value());
                Ok((Persistent::save(ctx, promise.into_value()), ns))
            })
            .map_err(|e| self.fix(e))?;
        self.settle(promise).map_err(|e| self.fix(e))?;
        Ok(ns)
    }

    /// Evaluates a classic script in the global scope and returns its completion value (a promise is awaited).
    pub fn eval(&self, source: &str) -> Result<Value> {
        self.sweep();
        let p = self
            .with(|ctx| {
                let v: rq::Value = ctx.eval(source)?;
                Ok(Persistent::save(ctx, v))
            })
            .map_err(|e| self.fix(e))?;
        let p = self.await_value(p).map_err(|e| self.fix(e))?;
        self.to_value_p(p)
    }

    // ---------------------------------------------------------- calling

    /// Calls a function handle with no `this` and awaits a returned promise.
    pub fn call(&self, func: &Handle, args: Vec<Value>) -> Result<Value> {
        let p = self.invoke(func, None, None, args)?;
        self.to_value_p(p)
    }

    /// Like [`call`](Engine::call) but keeps the result as a handle (use it for an object with methods).
    pub fn call_handle(&self, func: &Handle, args: Vec<Value>) -> Result<Handle> {
        let p = self.invoke(func, None, None, args)?;
        self.to_handle_p(p)
    }

    /// Calls `obj[method](...args)` with `this = obj`, awaiting a returned promise.
    pub fn call_method(&self, obj: &Handle, method: &str, args: Vec<Value>) -> Result<Value> {
        let p = self.invoke(obj, Some(method), None, args)?;
        self.to_value_p(p)
    }

    /// Like [`call_method`](Engine::call_method) but keeps the result as a handle.
    pub fn call_method_handle(&self, obj: &Handle, method: &str, args: Vec<Value>) -> Result<Handle> {
        let p = self.invoke(obj, Some(method), None, args)?;
        self.to_handle_p(p)
    }

    /// Calls a function handle with an explicit `this`.
    pub fn call_with_this(&self, func: &Handle, this: &Handle, args: Vec<Value>) -> Result<Value> {
        let p = self.invoke(func, None, Some(this), args)?;
        self.to_value_p(p)
    }

    /// Property of an object handle (module namespace, returned object, ...) as a handle.
    pub fn get(&self, obj: &Handle, key: &str) -> Result<Handle> {
        self.sweep();
        self.with(|ctx| {
            let o = self.object_of(ctx, obj)?;
            let v: rq::Value = o.get(key)?;
            Ok(self.slots.store(ctx, v))
        })
        .map_err(|e| self.fix(e))
    }

    /// Property of an object handle converted to a [`Value`].
    pub fn get_value(&self, obj: &Handle, key: &str) -> Result<Value> {
        self.sweep();
        self.with(|ctx| {
            let o = self.object_of(ctx, obj)?;
            let v: rq::Value = o.get(key)?;
            from_js(ctx, &self.slots, v, 0)
        })
        .map_err(|e| self.fix(e))
    }

    /// Own enumerable string keys of an object handle (the export names of a module namespace).
    pub fn keys(&self, obj: &Handle) -> Result<Vec<String>> {
        self.sweep();
        self.with(|ctx| {
            let o = self.object_of(ctx, obj)?;
            o.keys::<std::string::String>().collect::<rq::Result<Vec<_>>>()
        })
        .map_err(|e| self.fix(e))
    }

    /// Converts the value behind a handle to a [`Value`] (functions inside become handles).
    pub fn to_value(&self, h: &Handle) -> Result<Value> {
        self.sweep();
        self.with(|ctx| {
            let v = self.slots.restore(ctx, h)?;
            from_js(ctx, &self.slots, v, 0)
        })
        .map_err(|e| self.fix(e))
    }

    /// Creates a JavaScript value from a [`Value`] and returns a handle to it.
    pub fn to_handle(&self, v: Value) -> Result<Handle> {
        self.sweep();
        self.with(|ctx| {
            let j = to_js(ctx, &self.slots, v)?;
            Ok(self.slots.store(ctx, j))
        })
        .map_err(|e| self.fix(e))
    }

    /// `typeof` of the value behind a handle.
    pub fn type_of(&self, h: &Handle) -> Result<&'static str> {
        self.sweep();
        self.with(|ctx| {
            let v = self.slots.restore(ctx, h)?;
            Ok(if v.is_function() {
                "function"
            } else if v.is_undefined() {
                "undefined"
            } else if v.is_bool() {
                "boolean"
            } else if v.is_number() {
                "number"
            } else if v.is_string() {
                "string"
            } else if v.is_symbol() {
                "symbol"
            } else if v.is_big_int() {
                "bigint"
            } else {
                "object"
            })
        })
        .map_err(|e| self.fix(e))
    }

    /// A global variable (for example a constructor) as a handle.
    pub fn global(&self, name: &str) -> Result<Handle> {
        self.sweep();
        self.with(|ctx| {
            let v: rq::Value = ctx.globals().get(name)?;
            Ok(self.slots.store(ctx, v))
        })
        .map_err(|e| self.fix(e))
    }

    // ------------------------------------------------------- event loop

    /// Runs queued jobs and timers until nothing is left to run, waiting for timers that are not due yet.
    /// A `setInterval` that is never cleared keeps this running until the interrupt flag is raised.
    pub fn run_until_idle(&self) -> Result<()> {
        self.sweep();
        loop {
            self.check_interrupt()?;
            self.run_jobs().map_err(|e| self.fix(e))?;
            match self.tick().map_err(|e| self.fix(e))? {
                None if self.rt.is_job_pending() => {}
                None => return Ok(()),
                Some(ms) if ms <= 0.0 => {}
                Some(ms) => self.sleep(ms)?,
            }
        }
    }

    /// Runs queued promise jobs and returns immediately (no timers).
    pub fn run_jobs_now(&self) -> Result<()> {
        self.sweep();
        self.run_jobs().map_err(|e| self.fix(e))
    }

    // --------------------------------------------------------- internals

    fn with<R>(&self, f: impl for<'js> FnOnce(&Ctx<'js>) -> rq::Result<R>) -> Result<R> {
        self.ctx.with(|ctx| f(&ctx).map_err(|e| error_from_exception(&ctx, e)))
    }

    fn sweep(&self) {
        self.slots.sweep();
    }

    fn object_of<'js>(&self, ctx: &Ctx<'js>, h: &Handle) -> rq::Result<rq::Object<'js>> {
        let v = self.slots.restore(ctx, h)?;
        v.into_object().ok_or_else(|| Exception::throw_type(ctx, "handle does not refer to an object"))
    }

    fn fix(&self, e: Error) -> Error {
        if self.interrupt.load(Ordering::Relaxed) && !e.is_interrupt() {
            Error::interrupted(Some(e))
        } else {
            e
        }
    }

    fn check_interrupt(&self) -> Result<()> {
        if self.interrupt.load(Ordering::Relaxed) {
            Err(Error::interrupted(None))
        } else {
            Ok(())
        }
    }

    fn invoke(&self, target: &Handle, method: Option<&str>, this: Option<&Handle>, args: Vec<Value>) -> Result<Pers> {
        self.sweep();
        let p = self
            .with(|ctx| {
                let tv = self.slots.restore(ctx, target)?;
                let (func, this) = match method {
                    Some(name) => {
                        let obj = tv
                            .clone()
                            .into_object()
                            .ok_or_else(|| Exception::throw_type(ctx, "handle does not refer to an object"))?;
                        let f: rq::Value = obj.get(name)?;
                        let f = f
                            .into_function()
                            .ok_or_else(|| Exception::throw_type(ctx, &format!("\"{name}\" is not a function")))?;
                        (f, Some(tv))
                    }
                    None => {
                        let f = tv
                            .into_function()
                            .ok_or_else(|| Exception::throw_type(ctx, "handle does not refer to a function"))?;
                        let this = match this {
                            Some(t) => Some(self.slots.restore(ctx, t)?),
                            None => None,
                        };
                        (f, this)
                    }
                };
                let args = make_args(ctx, &self.slots, args, this)?;
                let r: rq::Value = args.apply(&func)?;
                Ok(Persistent::save(ctx, r))
            })
            .map_err(|e| self.fix(e))?;
        self.await_value(p).map_err(|e| self.fix(e))
    }

    fn await_value(&self, p: Pers) -> Result<Pers> {
        let is_promise = self.with(|ctx| Ok(p.clone().restore(ctx)?.is_promise()))?;
        if is_promise {
            self.settle(p)
        } else {
            Ok(p)
        }
    }

    fn to_value_p(&self, p: Pers) -> Result<Value> {
        self.with(|ctx| {
            let v = p.restore(ctx)?;
            from_js(ctx, &self.slots, v, 0)
        })
        .map_err(|e| self.fix(e))
    }

    fn to_handle_p(&self, p: Pers) -> Result<Handle> {
        self.with(|ctx| {
            let v = p.restore(ctx)?;
            Ok(self.slots.store(ctx, v))
        })
    }

    fn run_jobs(&self) -> Result<()> {
        loop {
            self.check_interrupt()?;
            match self.rt.execute_pending_job() {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(job) => return Err(job.0.with(|ctx| error_from_value(&ctx, ctx.catch()))),
            }
        }
    }

    /// `Some(ms until the next timer)`, `Some(0)` when more is due now, `None` when no timers exist.
    fn tick(&self) -> Result<Option<f64>> {
        let tick = self.tick.clone().expect("engine is alive");
        self.with(|ctx| {
            let f = tick.restore(ctx)?;
            let ms: f64 = f.call(())?;
            Ok(if ms < 0.0 { None } else { Some(ms) })
        })
    }

    fn sleep(&self, ms: f64) -> Result<()> {
        let end = Instant::now() + Duration::from_secs_f64(ms / 1000.0);
        loop {
            self.check_interrupt()?;
            let now = Instant::now();
            if now >= end {
                return Ok(());
            }
            std::thread::sleep((end - now).min(Duration::from_millis(5)));
        }
    }

    /// Drives jobs and timers until the promise settles.
    fn settle(&self, promise: Pers) -> Result<Pers> {
        loop {
            self.check_interrupt()?;
            self.run_jobs()?;
            let done = self.with(|ctx| {
                let v = promise.clone().restore(ctx)?;
                let p = rq::Promise::from_value(v)?;
                match p.state() {
                    PromiseState::Pending => Ok(None),
                    _ => {
                        let r: rq::Value = p.result().expect("settled")?;
                        Ok(Some(Persistent::save(ctx, r)))
                    }
                }
            })?;
            if let Some(r) = done {
                return Ok(r);
            }
            match self.tick()? {
                None if self.rt.is_job_pending() => {}
                None => {
                    return Err(Error::new(
                        "The promise can never settle: nothing is left to run (no queued jobs and no pending timers)",
                    ))
                }
                Some(ms) if ms <= 0.0 => {}
                Some(ms) => self.sleep(ms)?,
            }
        }
    }
}
