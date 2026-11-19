use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use toybox_script::{Config, Engine, Handle, LogLevel, Value};

type Log = Rc<RefCell<Vec<(LogLevel, String)>>>;

fn engine() -> (Engine, Log) {
    let log: Log = Rc::default();
    let sink = log.clone();
    let engine = Engine::new(Config {
        console: Some(Box::new(move |l, t| sink.borrow_mut().push((l, t.to_string())))),
        ..Config::default()
    })
    .unwrap();
    (engine, log)
}

fn write(dir: &Path, rel: &str, text: &str) -> PathBuf {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, text).unwrap();
    p
}

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// Evaluates `source` as a bootstrap module and returns its namespace.
fn boot(e: &Engine, source: &str) -> Handle {
    e.eval_module("bootstrap", source).unwrap()
}

fn run(e: &Engine, ns: &Handle, f: &str, args: Vec<Value>) -> Value {
    let f = e.get(ns, f).unwrap();
    e.call(&f, args).unwrap()
}

fn run_err(e: &Engine, ns: &Handle, f: &str, args: Vec<Value>) -> toybox_script::Error {
    let f = e.get(ns, f).unwrap();
    e.call(&f, args).unwrap_err()
}

// ---------------------------------------------------------------- 1. runtime

#[test]
fn console_levels_and_formatting() {
    let (e, log) = engine();
    e.eval(
        r#"console.log("a", 1, {x: [1, "two"]}); console.info("i"); console.warn("w"); console.error("e");
           console.debug("%s is %d", "x", 5); console.log(new Error("boom").message);"#,
    )
    .unwrap();
    let log = log.borrow();
    assert_eq!(log[0], (LogLevel::Log, "a 1 { x: [ 1, \"two\" ] }".into()));
    assert_eq!(log[1], (LogLevel::Info, "i".into()));
    assert_eq!(log[2], (LogLevel::Warn, "w".into()));
    assert_eq!(log[3], (LogLevel::Error, "e".into()));
    assert_eq!(log[4], (LogLevel::Debug, "x is 5".into()));
    assert_eq!(log[5].1, "boom");
}

#[test]
fn interrupt_flag_stops_an_infinite_loop() {
    let (e, _) = engine();
    let flag = e.interrupt_flag();
    let ns = boot(&e, "export const spin = () => { for (;;) {} };");
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        flag.store(true, Ordering::Relaxed);
    });
    let start = Instant::now();
    let err = run_err(&e, &ns, "spin", vec![]);
    t.join().unwrap();
    assert!(err.is_interrupt(), "{err}");
    assert!(err.message.contains("interrupt"), "{err}");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn interrupt_flag_stops_a_timer_wait() {
    let (e, _) = engine();
    let flag = e.interrupt_flag();
    let ns = boot(&e, "export const wait = () => new Promise(r => setTimeout(r, 60000));");
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        flag.store(true, Ordering::Relaxed);
    });
    let err = run_err(&e, &ns, "wait", vec![]);
    t.join().unwrap();
    assert!(err.is_interrupt(), "{err}");
}

#[test]
fn memory_limit_is_enforced_and_stack_overflow_is_an_error() {
    let e = Engine::new(Config { memory_limit: 32 << 20, ..Config::default() }).unwrap();
    let err = e.eval("const a = []; for (;;) a.push(new Array(10000).fill(1));").unwrap_err();
    assert!(err.message.to_lowercase().contains("memory"), "{err}");
    let (e, _) = engine();
    let err = e.eval("function f() { return f() + 1; } f();").unwrap_err();
    assert!(err.message.contains("stack"), "{err}");
}

// --------------------------------------------------------------- 2. modules

#[test]
fn relative_imports_cache_and_top_level_await() {
    let dir = tmp();
    write(dir.path(), "lib/counter.js", "globalThis.__loads = (globalThis.__loads || 0) + 1; export const n = globalThis.__loads;");
    write(dir.path(), "a.js", "import { n } from './lib/counter.js'; export const a = n;");
    write(dir.path(), "b.js", "import { n } from './lib/counter.js'; import { a } from './a.js'; export const b = n + a * 10;");
    write(
        dir.path(),
        "main.js",
        "import { b } from './b.js'; await new Promise(r => setTimeout(r, 20)); export const result = b; export default function () { return 'ok'; }",
    );
    let (e, _) = engine();
    let ns = e.load_module(&dir.path().join("main.js"), dir.path()).unwrap();
    // counter evaluated once: a = 1, b = 1 + 10.
    assert_eq!(e.get_value(&ns, "result").unwrap(), Value::Number(11.0));
    // A second load of the same file is cached.
    let again = e.load_module(&dir.path().join("main.js"), dir.path()).unwrap();
    assert_eq!(e.get_value(&again, "result").unwrap(), Value::Number(11.0));
    assert_eq!(e.eval("globalThis.__loads").unwrap(), Value::Number(1.0));
    assert!(e.keys(&ns).unwrap().contains(&"default".to_string()));
}

#[test]
fn import_cycle() {
    let dir = tmp();
    write(dir.path(), "x.js", "import { y } from './y.js'; export function x() { return 'x' + y(); } export function tail() { return 't'; }");
    write(dir.path(), "y.js", "import { tail } from './x.js'; export function y() { return 'y' + tail(); }");
    let (e, _) = engine();
    let ns = e.load_module(&dir.path().join("x.js"), dir.path()).unwrap();
    assert_eq!(run(&e, &ns, "x", vec![]), Value::String("xyt".into()));
}

#[test]
fn module_that_throws_at_top_level() {
    let dir = tmp();
    write(dir.path(), "boom.js", "const a = 1;\nthrow new TypeError('top level boom');\n");
    write(dir.path(), "uses.js", "import './boom.js';");
    let (e, _) = engine();
    let err = e.load_module(&dir.path().join("uses.js"), dir.path()).unwrap_err();
    assert_eq!(err.message, "top level boom");
    assert_eq!(err.name, "TypeError");
    let stack = err.stack.clone().unwrap();
    assert!(stack.contains("boom.js:2"), "stack was: {stack}");
    assert!(format!("{err}").contains("TypeError: top level boom"));
}

#[test]
fn syntax_error_names_the_file() {
    let dir = tmp();
    write(dir.path(), "bad.js", "export const = ;");
    let (e, _) = engine();
    let err = e.load_module(&dir.path().join("bad.js"), dir.path()).unwrap_err();
    assert!(err.name.contains("SyntaxError") || err.message.contains("expecting"), "{err}");
}

#[test]
fn rejected_promise_and_thrown_values() {
    let (e, _) = engine();
    let ns = boot(
        &e,
        r#"export const reject = async () => { await null; throw new RangeError("late failure"); };
           export const rejectPlain = () => Promise.reject("just a string");
           export const throwSync = () => { throw new Error("sync"); };"#,
    );
    let err = run_err(&e, &ns, "reject", vec![]);
    assert_eq!((err.name.as_str(), err.message.as_str()), ("RangeError", "late failure"));
    assert!(err.stack.unwrap().contains("bootstrap:1"));
    assert_eq!(run_err(&e, &ns, "rejectPlain", vec![]).message, "just a string");
    assert_eq!(run_err(&e, &ns, "throwSync", vec![]).message, "sync");
}

#[test]
fn json_modules_and_import_meta_url() {
    let dir = tmp();
    write(dir.path(), "data.json", r#"{"a": [1, 2, 3]}"#);
    write(dir.path(), "m.js", "import data from './data.json' with { type: 'json' }; export const sum = data.a.reduce((x, y) => x + y); export const url = import.meta.url;");
    let (e, _) = engine();
    let ns = e.load_module(&dir.path().join("m.js"), dir.path()).unwrap();
    assert_eq!(e.get_value(&ns, "sum").unwrap(), Value::Number(6.0));
    let url = e.get_value(&ns, "url").unwrap();
    assert!(url.as_str().unwrap().starts_with("file:///") && url.as_str().unwrap().ends_with("/m.js"), "{url:?}");
}

#[test]
fn bare_specifiers_map_to_files_and_in_memory_folders() {
    let dir = tmp();
    let sdk_dir = tmp();
    write(sdk_dir.path(), "dist/index.js", "import { helper } from './util.js'; export const sdk = () => helper() + '!';");
    write(sdk_dir.path(), "dist/util.js", "export const helper = () => 'sdk-file';");
    write(
        dir.path(),
        "main.js",
        "import { sdk } from '@scope/sdk'; import { mem } from 'memlib'; export const out = () => sdk() + mem();",
    );
    let (e, _) = engine();
    e.map_bare_file("@scope/sdk", &sdk_dir.path().join("dist/index.js"), sdk_dir.path()).unwrap();
    e.register_folder(
        "memlib",
        [
            ("index.js", "import { a } from './deep/a.js'; export const mem = () => a + b;\nimport { b } from './b.js';"),
            ("b.js", "export const b = '-b';"),
            ("deep/a.js", "import { c } from '../c.js'; export const a = 'a' + c;"),
            ("c.js", "export const c = '-c';"),
        ],
    );
    e.map_bare_module("memlib", "memlib/index.js");
    let ns = e.load_module(&dir.path().join("main.js"), dir.path()).unwrap();
    assert_eq!(run(&e, &ns, "out", vec![]), Value::String("sdk-file!a-c-b".into()));
}

// ------------------------------------------------------------------ 3. eval

#[test]
fn dynamic_import_by_path_and_file_url() {
    let dir = tmp();
    let plugin = write(dir.path(), "plugin/p.js", "import { v } from './dep.js'; export const get = () => v; export default { name: 'p' };");
    write(dir.path(), "plugin/dep.js", "export const v = 42;");
    let (e, _) = engine();
    e.allow_root(&dir.path().join("plugin")).unwrap();
    let ns = boot(&e, "export const load = async (spec) => { const m = await import(spec); return m.get(); };");
    let canon = plugin.canonicalize().unwrap();
    let as_path = canon.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    assert_eq!(run(&e, &ns, "load", vec![Value::String(as_path.clone())]), Value::Number(42.0));
    let url = format!("file:///{}", as_path.replace('\\', "/").trim_start_matches('/'));
    assert_eq!(run(&e, &ns, "load", vec![Value::String(url)]), Value::Number(42.0));
}

#[test]
fn bootstrap_can_import_relative_in_memory_modules_only() {
    let (e, _) = engine();
    let err = e.eval_module("bootstrap", "import './x.js';").unwrap_err();
    assert!(err.message.contains("./x.js"), "{err}");
}

// ------------------------------------------------------------ 4. host funcs

#[test]
fn host_functions_values_errors_and_names() {
    let (e, _) = engine();
    e.register_host_fn("echo", |mut a| Ok(a.pop().unwrap_or(Value::Null)));
    e.register_host_fn("fail", |_| Err("host said no".into()));
    e.register_host_fn("sum", |a| Ok(Value::Number(a.iter().filter_map(|v| v.as_f64()).sum())));
    let ns = boot(
        &e,
        r#"const { host } = import.meta;
           export const names = () => host.names();
           export const echo = (v) => host.call("echo", v);
           export const echoAsync = async (v) => host.call("echo", v);
           export const sum = (...n) => host.call("sum", ...n);
           export const caught = () => { try { host.call("fail"); return "not thrown"; } catch (e) { return e instanceof Error && e.message; } };
           export const uncaught = () => host.call("fail");
           export const unknown = () => host.call("nope");"#,
    );
    assert_eq!(
        run(&e, &ns, "names", vec![]),
        Value::Array(vec!["echo".into(), "fail".into(), "sum".into()])
    );
    assert_eq!(run(&e, &ns, "sum", vec![1.into(), 2.5.into()]), Value::Number(3.5));
    assert_eq!(run(&e, &ns, "caught", vec![]), Value::String("host said no".into()));
    assert_eq!(run_err(&e, &ns, "uncaught", vec![]).message, "host said no");
    assert!(run_err(&e, &ns, "unknown", vec![]).message.contains("nope"));
    let v = Value::object([
        ("null", Value::Null),
        ("t", true.into()),
        ("n", (-1.5).into()),
        ("s", "héllo \u{1F600}".into()),
        ("arr", Value::Array(vec![1.into(), Value::Array(vec![]), Value::object([("k", "v".into())])])),
        ("bytes", Value::bytes(vec![0, 1, 255])),
    ]);
    assert_eq!(run(&e, &ns, "echo", vec![v.clone()]), v);
    assert_eq!(run(&e, &ns, "echoAsync", vec![v.clone()]), v);
}

#[test]
fn byte_buffers_in_all_shapes() {
    let (e, _) = engine();
    e.register_host_fn("take", |a| Ok(Value::Array(a.iter().map(|v| Value::Number(v.as_bytes().map(|b| b.len() as f64).unwrap_or(-1.0))).collect())));
    e.register_host_fn("first", |a| Ok(a.into_iter().next().unwrap()));
    let ns = boot(
        &e,
        r#"const { host } = import.meta;
           export const shapes = () => {
             const buf = new Uint8Array([1,2,3,4,5,6,7,8]).buffer;
             return host.call("take", new Uint8Array(buf, 2, 4), buf, new Float32Array(2), new DataView(buf, 1, 3), "not bytes");
           };
           export const type = () => { const r = host.call("first", new Uint8Array([9, 8])); return [r instanceof Uint8Array, r.length, r[0], r[1]]; };
           export const sub = () => host.call("first", new Uint8Array([1,2,3,4,5]).subarray(1, 4));"#,
    );
    assert_eq!(
        run(&e, &ns, "shapes", vec![]),
        Value::Array(vec![4.into(), 8.into(), 8.into(), 3.into(), (-1).into()])
    );
    assert_eq!(
        run(&e, &ns, "type", vec![]),
        Value::Array(vec![true.into(), 2.into(), 9.into(), 8.into()])
    );
    assert_eq!(run(&e, &ns, "sub", vec![]), Value::Bytes(vec![2, 3, 4]));
}

#[test]
fn bytes_round_trip_50_mb() {
    const N: usize = 50 * 1024 * 1024;
    let (e, _) = engine();
    e.register_host_fn("make", |_| {
        let mut v = vec![0u8; N];
        v[0] = 1;
        v[N - 1] = 2;
        Ok(Value::Bytes(v))
    });
    e.register_host_fn("check", |a| {
        let b = a[0].as_bytes().ok_or("bytes expected")?;
        Ok(Value::Bool(b.len() == N && b[0] == 1 && b[N / 2] == 9 && b[N - 1] == 2))
    });
    let ns = boot(
        &e,
        r#"const { host } = import.meta;
           export const through = async (input) => {
             if (!(input instanceof Uint8Array) || input.length !== input.byteLength) throw new Error("not a Uint8Array");
             const made = host.call("make");
             made[made.length >> 1] = 9;                 // touch the middle
             const copy = host.call("echo", made);        // JS -> Rust -> JS
             return [copy.length, host.call("check", copy), input[0], input[input.length - 1]];
           };"#,
    );
    e.register_host_fn("echo", |mut a| Ok(a.pop().unwrap()));
    let mut input = vec![0u8; N];
    input[0] = 7;
    input[N - 1] = 8;
    let start = Instant::now();
    let out = run(&e, &ns, "through", vec![Value::Bytes(input)]);
    let took = start.elapsed();
    println!("50 MB Rust->JS->Rust->JS->Rust round trip (input + make + echo + check): {took:?}");
    assert_eq!(out, Value::Array(vec![(N as f64).into(), true.into(), 7.into(), 8.into()]));
    assert!(took < Duration::from_secs(10), "too slow: {took:?}");
}

#[test]
fn handles_survive_across_calls() {
    let dir = tmp();
    write(
        dir.path(),
        "plugin.js",
        r#"export default function create(options) {
             let count = options.start;
             return { name: "counter", inc(by) { count += by; return count; }, async later(x) { await new Promise(r => setTimeout(r, 5)); return x * 2; }, callback: (v) => "cb:" + v };
           }"#,
    );
    let (e, _) = engine();
    e.register_host_fn("note", |a| Ok(a.into_iter().next().unwrap_or(Value::Null)));
    let ns = e.load_module(&dir.path().join("plugin.js"), dir.path()).unwrap();
    let create = e.get(&ns, "default").unwrap();
    assert_eq!(e.type_of(&create).unwrap(), "function");
    let obj = e.call_handle(&create, vec![Value::object([("start", 10.into())])]).unwrap();
    assert_eq!(e.call_method(&obj, "inc", vec![5.into()]).unwrap(), Value::Number(15.0));
    assert_eq!(e.call_method(&obj, "inc", vec![1.into()]).unwrap(), Value::Number(16.0));
    assert_eq!(e.call_method(&obj, "later", vec![21.into()]).unwrap(), Value::Number(42.0));
    // The object converted to a Value carries function members as handles; pass one back.
    let as_value = e.to_value(&obj).unwrap();
    assert_eq!(as_value.get("name"), Some(&Value::String("counter".into())));
    let cb = as_value.get("callback").unwrap().as_handle().unwrap().clone();
    assert_eq!(e.call(&cb, vec!["x".into()]).unwrap(), Value::String("cb:x".into()));
    // A handle as an argument of another call.
    let apply = e.eval("(f, v) => f(v) + '!'").unwrap();
    let apply = apply.as_handle().unwrap().clone();
    assert_eq!(e.call(&apply, vec![Value::Handle(cb), "y".into()]).unwrap(), Value::String("cb:y!".into()));
    // Released handles do not break later calls.
    drop(apply);
    drop(as_value);
    assert_eq!(e.call_method(&obj, "inc", vec![0.into()]).unwrap(), Value::Number(16.0));
    assert!(e.call_method(&obj, "missing", vec![]).unwrap_err().message.contains("not a function"));
}

// ------------------------------------------------------------------ 5. await

#[test]
fn timer_driven_async_function() {
    let (e, _) = engine();
    let ns = boot(
        &e,
        r#"export const slow = async () => {
             const t0 = performance.now();
             const order = [];
             const a = new Promise(r => setTimeout(() => { order.push("a60"); r(); }, 60));
             const b = new Promise(r => setTimeout(() => { order.push("b20"); r(); }, 20));
             queueMicrotask(() => order.push("micro"));
             setTimeout((x, y) => order.push("args" + x + y), 0, 1, 2);
             const cancelled = setTimeout(() => order.push("never"), 10);
             clearTimeout(cancelled);
             let ticks = 0;
             await new Promise(r => { const id = setInterval(() => { if (++ticks === 3) { clearInterval(id); r(); } }, 10); });
             await Promise.all([a, b]);
             return { order, ticks, elapsed: performance.now() - t0 };
           };"#,
    );
    let start = Instant::now();
    let out = run(&e, &ns, "slow", vec![]);
    assert!(start.elapsed() >= Duration::from_millis(55));
    assert_eq!(
        out.get("order").unwrap(),
        &Value::Array(vec!["micro".into(), "args12".into(), "b20".into(), "a60".into()])
    );
    assert_eq!(out.get("ticks").unwrap(), &Value::Number(3.0));
    let elapsed = out.get("elapsed").unwrap().as_f64().unwrap();
    assert!((55.0..2000.0).contains(&elapsed), "{elapsed}");
}

#[test]
fn a_promise_that_can_never_settle_is_an_error() {
    let (e, _) = engine();
    let ns = boot(&e, "export const hang = () => new Promise(() => {});");
    let err = run_err(&e, &ns, "hang", vec![]);
    assert!(err.message.contains("never settle"), "{err}");
}

#[test]
fn run_until_idle_waits_for_timers() {
    let (e, log) = engine();
    e.eval("setTimeout(() => console.log('late'), 30); Promise.resolve().then(() => console.log('micro')); 0;").unwrap();
    assert!(log.borrow().is_empty());
    e.run_until_idle().unwrap();
    let texts: Vec<_> = log.borrow().iter().map(|l| l.1.clone()).collect();
    assert_eq!(texts, ["micro", "late"]);
}

#[test]
fn a_throwing_timer_callback_reaches_the_awaiting_call() {
    let (e, _) = engine();
    let ns = boot(&e, "export const f = () => new Promise(() => { setTimeout(() => { throw new Error('timer boom'); }, 5); });");
    assert_eq!(run_err(&e, &ns, "f", vec![]).message, "timer boom");
}

// ------------------------------------------------------------------- jail

#[test]
fn jail_relative_escape_unmapped_bare_and_absolute() {
    let dir = tmp();
    let secret = write(dir.path(), "secret/secret.js", "export const s = 1;");
    write(dir.path(), "plugin/escape.js", "import '../secret/secret.js';");
    write(dir.path(), "plugin/deep/escape2.js", "import '../../secret/secret.js';");
    write(dir.path(), "plugin/bare.js", "import fs from 'node:fs';");
    write(dir.path(), "plugin/bare2.js", "import x from 'lodash';");
    write(dir.path(), "plugin/abs.js", &format!("import {:?};", secret.to_string_lossy()));
    write(dir.path(), "plugin/dyn.js", &format!("export const f = () => import({:?});", secret.to_string_lossy()));
    write(dir.path(), "plugin/missing.js", "import './nope.js';");
    write(dir.path(), "plugin/ok.js", "export const fine = true;");
    let root = dir.path().join("plugin");
    let (e, _) = engine();

    let err = e.load_module(&root.join("escape.js"), &root).unwrap_err();
    assert!(err.message.contains("../secret/secret.js") && err.message.contains("plugin folder"), "{err}");
    let err = e.load_module(&root.join("deep/escape2.js"), &root).unwrap_err();
    assert!(err.message.contains("../../secret/secret.js"), "{err}");

    let err = e.load_module(&root.join("bare.js"), &root).unwrap_err();
    assert!(err.message.contains("node:fs") && err.message.contains("bare.js") && err.message.contains("not available outside the app window"), "{err}");
    let err = e.load_module(&root.join("bare2.js"), &root).unwrap_err();
    assert!(err.message.contains("\"lodash\"") && err.message.contains("bare2.js"), "{err}");

    let err = e.load_module(&root.join("abs.js"), &root).unwrap_err();
    assert!(err.message.contains("outside the allowed"), "{err}");

    let ns = e.load_module(&root.join("dyn.js"), &root).unwrap();
    let f = e.get(&ns, "f").unwrap();
    let err = e.call(&f, vec![]).unwrap_err();
    assert!(err.message.contains("outside the allowed"), "{err}");

    let err = e.load_module(&root.join("missing.js"), &root).unwrap_err();
    assert!(err.message.contains("./nope.js"), "{err}");

    // load_module itself refuses a file outside the root.
    let err = e.load_module(&secret, &root).unwrap_err();
    assert!(err.message.contains("outside the allowed folder"), "{err}");

    // The bootstrap's dynamic import is confined to the allowed roots too.
    e.allow_root(&root).unwrap();
    let ns = boot(&e, "export const load = (p) => import(p).then(m => Object.keys(m));");
    let err = run_err(&e, &ns, "load", vec![Value::String(secret.to_string_lossy().into())]);
    assert!(err.message.contains("outside the allowed"), "{err}");
    let ok = root.join("ok.js");
    assert_eq!(run(&e, &ns, "load", vec![Value::String(ok.to_string_lossy().into())]), Value::Array(vec!["fine".into()]));
    // A relative specifier typed in the bootstrap has no folder to be relative to.
    let err = run_err(&e, &ns, "load", vec![Value::String("./ok.js".into())]);
    assert!(err.message.contains("./ok.js"), "{err}");
}

#[test]
fn jail_blocks_a_symlink_that_leaves_the_folder() {
    let dir = tmp();
    write(dir.path(), "outside/o.js", "export const o = 1;");
    let root = dir.path().join("plugin");
    fs::create_dir_all(&root).unwrap();
    let link = root.join("link");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(dir.path().join("outside"), &link).is_ok();
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(dir.path().join("outside"), &link).is_ok();
    if !made {
        eprintln!("skipped: cannot create symlinks here");
        return;
    }
    write(&root, "main.js", "import './link/o.js';");
    let (e, _) = engine();
    let err = e.load_module(&root.join("main.js"), &root).unwrap_err();
    assert!(err.message.contains("./link/o.js"), "{err}");
}

// ------------------------------------------------------------------ 6. globals

#[test]
fn text_encoder_and_decoder() {
    let (e, _) = engine();
    let ns = boot(
        &e,
        r#"export const t = () => {
             const out = {};
             const enc = new TextEncoder();
             out.encoding = enc.encoding;
             out.bytes = Array.from(enc.encode("héllo €😀"));
             out.lone = Array.from(enc.encode("a\uD800b"));
             out.round = new TextDecoder().decode(enc.encode("héllo €😀"));
             out.bom = new TextDecoder().decode(new Uint8Array([0xEF, 0xBB, 0xBF, 0x68, 0x69]));
             out.keepBom = new TextDecoder("utf-8", { ignoreBOM: true }).decode(new Uint8Array([0xEF, 0xBB, 0xBF, 0x68])).length;
             out.bad = new TextDecoder().decode(new Uint8Array([0x68, 0xFF, 0x69]));
             try { new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array([0xFF])); out.fatal = "no throw"; }
             catch (e) { out.fatal = e.constructor.name; }
             const d = new TextDecoder();
             const euro = enc.encode("€");
             out.stream = d.decode(euro.subarray(0, 1), { stream: true }) + "|" + d.decode(euro.subarray(1, 2), { stream: true }) + "|" + d.decode(euro.subarray(2));
             out.utf16 = new TextDecoder("utf-16le").decode(new Uint8Array([0x68, 0, 0x3D, 0xD8, 0x00, 0xDE]));
             out.utf16bom = new TextDecoder("utf-16le").decode(new Uint8Array([0xFF, 0xFE, 0x68, 0]));
             out.label = new TextDecoder("UTF8").encoding;
             try { new TextDecoder("latin9"); out.unsupported = "no throw"; } catch (e) { out.unsupported = e.constructor.name; }
             out.fromBuffer = new TextDecoder().decode(enc.encode("buf").buffer);
             const dest = new Uint8Array(4);
             out.into = enc.encodeInto("a€b", dest);
             out.intoBytes = Array.from(dest);
             return out;
           };"#,
    );
    let v = run(&e, &ns, "t", vec![]);
    let get = |k: &str| v.get(k).unwrap().clone();
    let nums = |a: &[u8]| Value::Array(a.iter().map(|&b| Value::from(b as i32)).collect());
    assert_eq!(get("encoding"), "utf-8".into());
    assert_eq!(get("bytes"), nums("héllo €😀".as_bytes()));
    assert_eq!(get("lone"), nums("a\u{FFFD}b".as_bytes()));
    assert_eq!(get("round"), "héllo €😀".into());
    assert_eq!(get("bom"), "hi".into());
    assert_eq!(get("keepBom"), 2.into());
    assert_eq!(get("bad"), "h\u{FFFD}i".into());
    assert_eq!(get("fatal"), "TypeError".into());
    assert_eq!(get("stream"), "||\u{20AC}".into());
    assert_eq!(get("utf16"), "h\u{1F600}".into());
    assert_eq!(get("utf16bom"), "h".into());
    assert_eq!(get("label"), "utf-8".into());
    assert_eq!(get("unsupported"), "RangeError".into());
    assert_eq!(get("fromBuffer"), "buf".into());
    let into = get("into");
    assert_eq!(into.get("read"), Some(&Value::Number(2.0)));
    assert_eq!(into.get("written"), Some(&Value::Number(4.0)));
    assert_eq!(get("intoBytes"), nums("a€".as_bytes()));
}

#[test]
fn abort_controller_and_signal() {
    let (e, _) = engine();
    let ns = boot(
        &e,
        r#"export const t = async () => {
             const out = [];
             const c = new AbortController();
             out.push(c.signal.aborted, c.signal.reason);
             c.signal.throwIfAborted();
             c.signal.onabort = (ev) => out.push("onabort:" + ev.type);
             c.signal.addEventListener("abort", () => out.push("listener"));
             const removed = () => out.push("removed");
             c.signal.addEventListener("abort", removed); c.signal.removeEventListener("abort", removed);
             c.abort(); c.abort();
             out.push(c.signal.aborted, c.signal.reason.name, c.signal.reason instanceof Error);
             try { c.signal.throwIfAborted(); } catch (e) { out.push("threw:" + e.name); }
             const c2 = new AbortController(); c2.abort("why"); out.push(c2.signal.reason);
             out.push(AbortSignal.abort().aborted);
             const t = AbortSignal.timeout(10);
             await new Promise(r => t.addEventListener("abort", r));
             out.push(t.reason.name);
             try { new AbortSignal(); } catch (e) { out.push("illegal"); }
             return out;
           };"#,
    );
    let v = run(&e, &ns, "t", vec![]);
    let a = v.as_array().unwrap();
    let texts: Vec<_> = a.iter().map(|x| format!("{x:?}")).collect();
    assert_eq!(
        texts,
        [
            "Bool(false)", "Null", "String(\"onabort:abort\")", "String(\"listener\")", "Bool(true)",
            "String(\"AbortError\")", "Bool(true)", "String(\"threw:AbortError\")", "String(\"why\")", "Bool(true)",
            "String(\"TimeoutError\")", "String(\"illegal\")"
        ]
    );
}

#[test]
fn structured_clone() {
    let (e, _) = engine();
    let ns = boot(
        &e,
        r#"export const t = () => {
             const out = {};
             const src = { n: 1, s: "x", d: new Date(1700000000000), m: new Map([[1, { a: 1 }]]), set: new Set([1, 2]),
                           u8: new Uint8Array([1, 2, 3]), f32: new Float32Array([1.5]), arr: [1, [2]], big: 10n, re: /a+/gi, nul: null, und: undefined };
             src.self = src;
             const c = structuredClone(src);
             out.distinct = c !== src && c.m !== src.m && c.u8 !== src.u8 && c.arr[1] !== src.arr[1];
             out.cycle = c.self === c;
             out.date = c.d instanceof Date && c.d.getTime() === 1700000000000;
             out.map = c.m instanceof Map && c.m.get(1).a === 1 && c.m.get(1) !== src.m.get(1);
             out.set = c.set instanceof Set && c.set.has(2);
             out.typed = c.u8 instanceof Uint8Array && Array.from(c.u8).join() === "1,2,3" && c.f32[0] === 1.5;
             c.u8[0] = 99; out.isolated = src.u8[0] === 1;
             out.misc = [c.big === 10n, c.re.source, c.re.flags, c.nul, "und" in c, c.n];
             const buf = new ArrayBuffer(8); const a = new Uint8Array(buf, 2, 4);
             const cb = structuredClone({ a, b: buf });
             out.sharedBuffer = cb.a.buffer === cb.b && cb.a.byteOffset === 2 && cb.a.length === 4;
             const err = structuredClone(new RangeError("bad"));
             out.err = [err instanceof RangeError, err.message];
             try { structuredClone({ f() {} }); out.fn = "no throw"; } catch (e) { out.fn = e.name; }
             try { structuredClone(Symbol("s")); out.sym = "no throw"; } catch (e) { out.sym = e.name; }
             class P { constructor() { this.x = 1; } get y() { return 2; } }
             const p = structuredClone(new P());
             out.cls = [p instanceof P, p.x, p.y];
             return out;
           };"#,
    );
    let v = run(&e, &ns, "t", vec![]);
    let get = |k: &str| v.get(k).unwrap().clone();
    for k in ["distinct", "cycle", "date", "map", "set", "typed", "isolated", "sharedBuffer"] {
        assert_eq!(get(k), Value::Bool(true), "{k}");
    }
    // 10n converts to a handle, so compare the rest.
    let misc = get("misc");
    let misc = misc.as_array().unwrap();
    assert_eq!(misc[1], "a+".into());
    assert_eq!(misc[2], "gi".into());
    assert_eq!(misc[3], Value::Null);
    assert_eq!(misc[4], true.into());
    assert_eq!(get("err"), Value::Array(vec![true.into(), "bad".into()]));
    assert_eq!(get("fn"), "DataCloneError".into());
    assert_eq!(get("sym"), "DataCloneError".into());
    assert_eq!(get("cls"), Value::Array(vec![false.into(), 1.into(), Value::Null]));
}

#[test]
fn base64_and_performance_and_missing_globals() {
    let (e, _) = engine();
    let ns = boot(
        &e,
        r#"export const t = () => {
             const out = {};
             out.b = [btoa(""), btoa("f"), btoa("fo"), btoa("foo"), btoa("foob"), btoa("\xff\x00\x80")];
             out.a = [atob("Zm9v"), atob("Zm9vYg=="), atob("Zm9vYg"), atob(" Zm 9v\n"), atob("")];
             out.bin = atob(btoa("\x00\x01\xfe\xff")).split("").map(c => c.charCodeAt(0));
             try { btoa("€"); out.bad = "no throw"; } catch (e) { out.bad = e.name; }
             try { atob("@@@@"); out.bad2 = "no throw"; } catch (e) { out.bad2 = e.name; }
             const t0 = performance.now(); let x = 0; for (let i = 0; i < 1e5; i++) x += i;
             out.perf = performance.now() >= t0 && typeof t0 === "number";
             out.missing = [typeof fetch, typeof require, typeof process, typeof window, typeof document, typeof Buffer, typeof setImmediate, typeof XMLHttpRequest, typeof WebAssembly === "object" ? "wasm" : "nowasm"];
             try { fetch("http://x"); out.fetch = "no throw"; } catch (e) { out.fetch = e.name; }
             out.enumerableGlobals = Object.keys(globalThis).filter(k => ["console","TextEncoder","setTimeout","structuredClone","AbortController","atob","performance"].includes(k));
             return out;
           };"#,
    );
    let v = run(&e, &ns, "t", vec![]);
    let get = |k: &str| v.get(k).unwrap().clone();
    let strs = |a: &[&str]| Value::Array(a.iter().map(|s| Value::from(*s)).collect());
    assert_eq!(get("b"), strs(&["", "Zg==", "Zm8=", "Zm9v", "Zm9vYg==", "/wCA"]));
    assert_eq!(get("a"), strs(&["foo", "foob", "foob", "foo", ""]));
    assert_eq!(get("bin"), Value::Array(vec![0.into(), 1.into(), 254.into(), 255.into()]));
    assert_eq!(get("bad"), "InvalidCharacterError".into());
    assert_eq!(get("bad2"), "InvalidCharacterError".into());
    assert_eq!(get("perf"), true.into());
    let missing = get("missing");
    let m: Vec<_> = missing.as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert_eq!(&m[..8], ["undefined"; 8], "{m:?}");
    assert_eq!(get("fetch"), "ReferenceError".into());
    assert_eq!(get("enumerableGlobals"), Value::Array(vec![]));
}

#[test]
fn value_conversion_edge_cases() {
    let (e, _) = engine();
    let ns = boot(
        &e,
        r#"export const t = () => ({ u: undefined, n: null, d: new Date(0), nested: { deep: [undefined, { z: 1 }] }, big: 2 ** 40, neg: -0.5, str: "lone\uD800" });
           export const cyc = () => { const o = {}; o.o = o; return o; };
           export const fnval = () => ({ f() {} });"#,
    );
    let v = run(&e, &ns, "t", vec![]);
    assert_eq!(v.get("u"), None);
    assert_eq!(v.get("n"), Some(&Value::Null));
    assert_eq!(v.get("d"), Some(&Value::String("1970-01-01T00:00:00.000Z".into())));
    assert_eq!(v.get("nested").unwrap().get("deep").unwrap().as_array().unwrap()[0], Value::Null);
    assert_eq!(v.get("big").unwrap().as_i64(), Some(1 << 40));
    assert_eq!(v.get("neg"), Some(&Value::Number(-0.5)));
    assert_eq!(v.get("str"), Some(&Value::String("lone\u{FFFD}".into())));
    assert!(run_err(&e, &ns, "cyc", vec![]).message.contains("too deeply"));
    assert!(matches!(run(&e, &ns, "fnval", vec![]).get("f"), Some(Value::Handle(_))));
    // JSON interop.
    let j: serde_json::Value = serde_json::json!({"a": [1, 2.5, "x", null, true], "b": {"c": {}}});
    let back = Value::from(j.clone()).to_json().unwrap();
    assert_eq!(back, j);
    assert!(Value::bytes(vec![1]).to_json().is_err());
}

// --------------------------------------------------------- real plugin module

#[test]
fn loads_the_real_placer_plugin_when_built() {
    let plugin = Path::new(r"D:\dev\toybox\plugins\com.toybox.placer\dist\index.js");
    let sdk = Path::new(r"D:\dev\toybox\packages\toybox-sdk\dist\index.js");
    if !plugin.exists() {
        eprintln!("skipped: {} does not exist", plugin.display());
        return;
    }
    let (e, _) = engine();
    if sdk.exists() {
        e.map_bare_file("@ldlework/toybox-sdk", sdk, sdk.parent().unwrap()).unwrap();
    } else {
        eprintln!("note: {} does not exist; importing the plugin without the SDK mapping", sdk.display());
    }
    let result = e.load_module(plugin, plugin.parent().unwrap());
    match result {
        Ok(ns) => {
            let keys = e.keys(&ns).unwrap();
            println!("placer exports: {keys:?}");
            assert!(!keys.is_empty());
        }
        Err(err) if !sdk.exists() => {
            println!("without the SDK the import fails as expected: {}", err.message);
            assert!(err.message.contains("@ldlework/toybox-sdk"), "{err}");
        }
        Err(err) => panic!("placer failed to load: {err}"),
    }
}
