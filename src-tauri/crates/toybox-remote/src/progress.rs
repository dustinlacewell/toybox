//! Progress of long transfers, reported to whoever listens (the app turns these
//! into window events).

use std::sync::Arc;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub library: String,
    /// `download`, `upload`.
    pub operation: &'static str,
    pub done_files: u64,
    pub total_files: u64,
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// The file being worked on, when that is useful to show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

pub type Sink = Arc<dyn Fn(&Progress) + Send + Sync>;

/// A reader that counts what passes through it and reports every so often.
pub struct Counting<'a, R> {
    pub inner: R,
    pub on_bytes: &'a mut dyn FnMut(u64),
}

impl<R: std::io::Read> std::io::Read for Counting<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buf)?;
        if count > 0 {
            (self.on_bytes)(count as u64);
        }
        Ok(count)
    }
}
