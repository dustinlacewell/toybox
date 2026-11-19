//! An embedded JavaScript engine (QuickJS-NG through `rquickjs`) that loads ES modules from disk behind a
//! host-controlled jail and lets Rust call into them. It knows nothing about Toybox.
//!
//! ```no_run
//! use toybox_script::{Config, Engine, Value};
//! # fn main() -> Result<(), toybox_script::Error> {
//! let engine = Engine::new(Config::default())?;
//! engine.register_host_fn("double", |args| match args.into_iter().next() {
//!     Some(Value::Bytes(mut b)) => { b.iter_mut().for_each(|x| *x = x.wrapping_mul(2)); Ok(Value::Bytes(b)) }
//!     _ => Err("expected bytes".into()),
//! });
//! let boot = engine.eval_module("bootstrap", r#"
//!     const { host } = import.meta;
//!     export const double = async (b) => host.call("double", b);
//! "#)?;
//! let f = engine.get(&boot, "double")?;
//! let out = engine.call(&f, vec![Value::bytes(vec![1, 2, 3])])?;
//! assert_eq!(out, Value::Bytes(vec![2, 4, 6]));
//! # Ok(()) }
//! ```

mod engine;
mod error;
mod modules;
mod value;

pub use engine::{Config, Engine, LogLevel};
pub use error::{Error, Result};
pub use value::{Handle, Value};
