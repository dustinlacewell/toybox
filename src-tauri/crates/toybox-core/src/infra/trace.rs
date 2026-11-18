//! Phase timings on stderr, switched on by the environment variable `TOYBOX_TRACE`:
//! where an operation spends its time. Costs one environment lookup when off.

use std::time::Instant;

pub(crate) struct Trace {
    label: &'static str,
    last: Option<Instant>,
}

impl Trace {
    pub(crate) fn new(label: &'static str) -> Trace {
        Trace { label, last: std::env::var_os("TOYBOX_TRACE").map(|_| Instant::now()) }
    }

    /// Print how long the phase that just ended took.
    pub(crate) fn phase(&mut self, name: &str) {
        if let Some(last) = &mut self.last {
            eprintln!("{} {name}: {:.1} ms", self.label, last.elapsed().as_secs_f64() * 1000.0);
            *last = Instant::now();
        }
    }
}
