//! Shared server state.

use std::sync::Arc;
use std::time::Duration;

use crate::config::ConfigStore;
use crate::data::DataDir;
use crate::library::Library;
use crate::limiter::{Limiter, LimiterConfig};
use crate::uploads::{UploadLimits, Uploads};

#[derive(Debug, Clone)]
pub struct Options {
    /// Pause before answering a wrong token.
    pub fail_delay: Duration,
    pub lockout: LimiterConfig,
    pub uploads: UploadLimits,
    /// Largest accepted thumbnail PNG.
    pub max_thumb_bytes: usize,
    /// Print one log line per request.
    pub log_requests: bool,
    /// The oldest protocol level served. `None`: this server's own level and the one before.
    /// Tests set it to exercise the window without a second protocol level.
    pub min_api_version: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            fail_delay: Duration::from_millis(400),
            lockout: LimiterConfig::default(),
            uploads: UploadLimits::default(),
            max_thumb_bytes: 2 * 1024 * 1024,
            log_requests: true,
            min_api_version: None,
        }
    }
}

pub struct Inner {
    pub library: Library,
    pub config: ConfigStore,
    pub limiter: Limiter,
    pub uploads: Uploads,
    pub options: Options,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    /// Open an initialised data directory. Leftover uploads are discarded.
    pub fn open(data: DataDir, options: Options) -> Result<AppState, String> {
        if !data.is_initialised() {
            return Err(format!(
                "{} has not been set up yet. Run: toybox-server init --data {}",
                data.root.display(),
                data.root.display()
            ));
        }
        toybox_core::infra::library::validate(&data.library()).map_err(|e| format!("The library folder is not usable: {e}"))?;
        for dir in [data.cache(), data.uploads()] {
            std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        }
        let config = ConfigStore::open(&data.server_json()).map_err(|e| e.to_string())?;
        let uploads = Uploads::new(data.uploads(), options.uploads);
        uploads.wipe_all();
        Ok(AppState(Arc::new(Inner {
            library: Library::new(data),
            config,
            limiter: Limiter::new(options.lockout),
            uploads,
            options,
        })))
    }

    /// Periodic housekeeping: stale uploads and token "last used" times.
    pub fn spawn_background_tasks(&self) -> Vec<tokio::task::JoinHandle<()>> {
        let state = self.clone();
        let cleanup = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            loop {
                tick.tick().await;
                let removed = state.uploads.cleanup();
                if removed > 0 {
                    println!("toybox-server: removed {removed} stale import(s)");
                }
            }
        });
        let state = self.clone();
        let usage = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                let state = state.clone();
                let _ = tokio::task::spawn_blocking(move || state.config.flush_usage()).await;
            }
        });
        // Fingerprints of items that have none (a library that was never hashed, or
        // one edited on disk): once at start, then whenever one turns up.
        let state = self.clone();
        let fingerprints = tokio::spawn(async move {
            loop {
                match state.library.fingerprint_pass().await {
                    Ok(report) if report.hashed > 0 || report.failed > 0 => {
                        println!("toybox-server: fingerprinted {} item(s) for duplicate detection, {} could not be read", report.hashed, report.failed)
                    }
                    Ok(_) => {}
                    Err(error) => println!("toybox-server: fingerprinting stopped: {}", error.message),
                }
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
        vec![cleanup, usage, fingerprints]
    }
}
