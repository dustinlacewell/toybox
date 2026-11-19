use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use rquickjs::function::Args;
use rquickjs::{ArrayBuffer, Ctx, Exception, Persistent};

type Pers = Persistent<rquickjs::Value<'static>>;

/// A value crossing the Rust/JavaScript boundary: JSON plus byte buffers plus
/// opaque handles to JavaScript values that have no JSON form.
///
/// JS to Rust: `undefined` and `null` become `Null`; numbers become `Number`;
/// arrays and plain objects are converted deeply (keys with `undefined` values
/// are dropped, as `JSON.stringify` does; an object with a `toJSON` method is
/// converted from its result, so a `Date` becomes an ISO string);
/// `ArrayBuffer` and every typed array or `DataView` become `Bytes` (the bytes
/// the view covers); functions, promises, symbols and bigints become `Handle`s.
///
/// Rust to JS: `Bytes` becomes a `Uint8Array` (no copy); `Handle` becomes the
/// original value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    /// Own enumerable string keys, in order.
    Object(Vec<(String, Value)>),
    Bytes(Vec<u8>),
    Handle(Handle),
}

impl Value {
    pub fn bytes(data: impl Into<Vec<u8>>) -> Value {
        Value::Bytes(data.into())
    }

    pub fn object<K: Into<String>>(entries: impl IntoIterator<Item = (K, Value)>) -> Value {
        Value::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self { Some(*b) } else { None }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let Value::Number(n) = self { Some(*n) } else { None }
    }
    /// The number as an integer, when it is one.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(n) if n.fract() == 0.0 && n.abs() <= 9007199254740992.0 => Some(*n as i64),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Value::String(s) = self { Some(s) } else { None }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Value::Bytes(b) = self { Some(b) } else { None }
    }
    pub fn into_bytes(self) -> Option<Vec<u8>> {
        if let Value::Bytes(b) = self { Some(b) } else { None }
    }
    pub fn as_handle(&self) -> Option<&Handle> {
        if let Value::Handle(h) = self { Some(h) } else { None }
    }
    /// Member of an `Object` by key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        if let Value::Object(entries) = self {
            entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        } else {
            None
        }
    }

    /// Converts to `serde_json::Value`. Fails on `Bytes`, `Handle` and non-finite numbers.
    pub fn to_json(&self) -> Result<serde_json::Value, String> {
        Ok(match self {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => (*b).into(),
            Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 9007199254740992.0 {
                    serde_json::Value::from(*n as i64)
                } else {
                    serde_json::Number::from_f64(*n)
                        .ok_or_else(|| format!("number {n} has no JSON form"))?
                        .into()
                }
            }
            Value::String(s) => s.clone().into(),
            Value::Array(a) => {
                serde_json::Value::Array(a.iter().map(Value::to_json).collect::<Result<_, _>>()?)
            }
            Value::Object(o) => {
                let mut map = serde_json::Map::new();
                for (k, v) in o {
                    map.insert(k.clone(), v.to_json()?);
                }
                serde_json::Value::Object(map)
            }
            Value::Bytes(_) => return Err("byte buffers have no JSON form".into()),
            Value::Handle(_) => return Err("JavaScript handles have no JSON form".into()),
        })
    }
}

impl From<serde_json::Value> for Value {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Array(a) => Value::Array(a.into_iter().map(Value::from).collect()),
            serde_json::Value::Object(o) => {
                Value::Object(o.into_iter().map(|(k, v)| (k, Value::from(v))).collect())
            }
        }
    }
}

macro_rules! from_num {
    ($($t:ty),*) => {$(
        impl From<$t> for Value { fn from(n: $t) -> Self { Value::Number(n as f64) } }
    )*};
}
from_num!(i32, u32, i64, u64, usize, f64);

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.into())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

/// A reference to a JavaScript value held by an [`Engine`](crate::Engine): a
/// module namespace, a function, the object a function returned. Cloning is
/// cheap; the value is released when the last clone is dropped (the engine
/// reclaims it at its next call). A handle is only valid with the engine that
/// made it and is not `Send`.
#[derive(Clone, Debug)]
pub struct Handle(pub(crate) Rc<HandleInner>);

#[derive(Debug)]
pub(crate) struct HandleInner {
    pub(crate) id: u64,
    pub(crate) engine: u64,
    pub(crate) free: Rc<RefCell<Vec<u64>>>,
}

impl Drop for HandleInner {
    fn drop(&mut self) {
        if let Ok(mut q) = self.free.try_borrow_mut() {
            q.push(self.id);
        }
    }
}

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// The engine-side table behind [`Handle`]s.
pub(crate) struct Slots {
    pub(crate) engine: u64,
    map: RefCell<HashMap<u64, Pers>>,
    next: Cell<u64>,
    free: Rc<RefCell<Vec<u64>>>,
}

impl Slots {
    pub(crate) fn new(engine: u64) -> Self {
        Slots { engine, map: RefCell::default(), next: Cell::new(1), free: Rc::default() }
    }

    pub(crate) fn store<'js>(&self, ctx: &Ctx<'js>, v: rquickjs::Value<'js>) -> Handle {
        let id = self.next.get();
        self.next.set(id + 1);
        self.map.borrow_mut().insert(id, Persistent::save(ctx, v));
        Handle(Rc::new(HandleInner { id, engine: self.engine, free: self.free.clone() }))
    }

    pub(crate) fn restore<'js>(&self, ctx: &Ctx<'js>, h: &Handle) -> rquickjs::Result<rquickjs::Value<'js>> {
        if h.0.engine != self.engine {
            return Err(Exception::throw_message(ctx, "handle belongs to a different engine"));
        }
        let p = self.map.borrow().get(&h.0.id).cloned();
        match p {
            Some(p) => p.restore(ctx),
            None => Err(Exception::throw_message(ctx, "handle was already released")),
        }
    }

    /// Drops the values of handles that no longer exist.
    pub(crate) fn sweep(&self) {
        let ids: Vec<u64> = std::mem::take(&mut *self.free.borrow_mut());
        if ids.is_empty() {
            return;
        }
        let mut map = self.map.borrow_mut();
        let dead: Vec<Pers> = ids.iter().filter_map(|id| map.remove(id)).collect();
        drop(map);
        drop(dead);
    }

    pub(crate) fn clear(&self) {
        let all: Vec<Pers> = self.map.borrow_mut().drain().map(|(_, v)| v).collect();
        drop(all);
    }
}

const MAX_DEPTH: usize = 256;

fn throw<'js>(ctx: &Ctx<'js>, msg: &str) -> rquickjs::Error {
    Exception::throw_message(ctx, msg)
}

/// Bytes covered by an ArrayBuffer or an ArrayBuffer view, without copying.
/// The closure must not run JavaScript.
pub(crate) fn with_bytes<'js, R>(v: &rquickjs::Value<'js>, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    let obj = v.as_object()?;
    if let Some(ab) = ArrayBuffer::from_object(obj.clone()) {
        // SAFETY: no JavaScript runs while the slice is alive.
        let bytes = unsafe { ab.as_bytes() }?;
        return Some(f(bytes));
    }
    let buffer: rquickjs::Value = obj.get("buffer").ok()?;
    let ab = ArrayBuffer::from_object(buffer.into_object()?)?;
    let off: usize = obj.get::<_, f64>("byteOffset").ok()? as usize;
    let len: usize = obj.get::<_, f64>("byteLength").ok()? as usize;
    // SAFETY: as above.
    let bytes = unsafe { ab.as_bytes() }?;
    let end = off.checked_add(len)?;
    if end > bytes.len() {
        return None;
    }
    Some(f(&bytes[off..end]))
}

/// A JS string as UTF-8; lone surrogates become U+FFFD.
fn js_string<'js>(ctx: &Ctx<'js>, v: rquickjs::Value<'js>) -> rquickjs::Result<std::string::String> {
    match v.get::<std::string::String>() {
        Ok(s) => Ok(s),
        Err(_) => {
            let fix: rquickjs::Function = ctx.eval(
                r"(s) => typeof s.toWellFormed === 'function' ? s.toWellFormed()
                    : s.replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, '\uFFFD')",
            )?;
            fix.call((v,))
        }
    }
}

pub(crate) fn from_js<'js>(
    ctx: &Ctx<'js>,
    slots: &Slots,
    v: rquickjs::Value<'js>,
    depth: usize,
) -> rquickjs::Result<Value> {
    if depth > MAX_DEPTH {
        return Err(throw(ctx, "value is nested too deeply or refers to itself"));
    }
    if v.is_undefined() || v.is_null() {
        return Ok(Value::Null);
    }
    if let Some(b) = v.as_bool() {
        return Ok(Value::Bool(b));
    }
    if let Some(i) = v.as_int() {
        return Ok(Value::Number(i as f64));
    }
    if let Some(f) = v.as_float() {
        return Ok(Value::Number(f));
    }
    if v.is_string() {
        return Ok(Value::String(js_string(ctx, v)?));
    }
    if v.is_function() || v.is_promise() || v.is_symbol() || v.is_big_int() {
        return Ok(Value::Handle(slots.store(ctx, v)));
    }
    if v.is_array() {
        let arr = v.into_array().ok_or_else(|| throw(ctx, "not an array"))?;
        let mut out = Vec::with_capacity(arr.len());
        for i in 0..arr.len() {
            out.push(from_js(ctx, slots, arr.get(i)?, depth + 1)?);
        }
        return Ok(Value::Array(out));
    }
    if let Some(obj) = v.as_object() {
        if let Some(bytes) = with_bytes(&v, |b| b.to_vec()) {
            return Ok(Value::Bytes(bytes));
        }
        if let Ok(to_json) = obj.get::<_, rquickjs::Function>("toJSON") {
            let r: rquickjs::Value = to_json.call((rquickjs::function::This(obj.clone()),))?;
            if r.as_object().map(|o| o == obj).unwrap_or(false) {
                return Err(throw(ctx, "toJSON returned its own receiver"));
            }
            return from_js(ctx, slots, r, depth + 1);
        }
        let mut out = Vec::new();
        for prop in obj.props::<std::string::String, rquickjs::Value>() {
            let (k, val) = prop?;
            if val.is_undefined() {
                continue;
            }
            out.push((k, from_js(ctx, slots, val, depth + 1)?));
        }
        return Ok(Value::Object(out));
    }
    Ok(Value::Handle(slots.store(ctx, v)))
}

pub(crate) fn to_js<'js>(ctx: &Ctx<'js>, slots: &Slots, v: Value) -> rquickjs::Result<rquickjs::Value<'js>> {
    use rquickjs::IntoJs;
    Ok(match v {
        Value::Null => rquickjs::Value::new_null(ctx.clone()),
        Value::Bool(b) => rquickjs::Value::new_bool(ctx.clone(), b),
        Value::Number(n) => n.into_js(ctx)?,
        Value::String(s) => rquickjs::String::from_str(ctx.clone(), &s)?.into_value(),
        Value::Array(items) => {
            let arr = rquickjs::Array::new(ctx.clone())?;
            for (i, item) in items.into_iter().enumerate() {
                arr.set(i, to_js(ctx, slots, item)?)?;
            }
            arr.into_value()
        }
        Value::Object(entries) => {
            let obj = rquickjs::Object::new(ctx.clone())?;
            for (k, item) in entries {
                obj.set(k.as_str(), to_js(ctx, slots, item)?)?;
            }
            obj.into_value()
        }
        Value::Bytes(data) => rquickjs::TypedArray::<u8>::new(ctx.clone(), data)?.into_value(),
        Value::Handle(h) => slots.restore(ctx, &h)?,
    })
}

/// Builds an `Args` list (with optional `this`) for a call.
pub(crate) fn make_args<'js>(
    ctx: &Ctx<'js>,
    slots: &Slots,
    args: Vec<Value>,
    this: Option<rquickjs::Value<'js>>,
) -> rquickjs::Result<Args<'js>> {
    let mut a = Args::new(ctx.clone(), args.len());
    for v in args {
        a.push_arg(to_js(ctx, slots, v)?)?;
    }
    if let Some(t) = this {
        a.this(t)?;
    }
    Ok(a)
}
