//! "Command line" in Settings: whether `tbx` (shipped inside the app) is on the PATH, and putting it
//! there or taking it off for this user. The logic is `toybox_host::cli_install`.

use toybox_host::cli_install::{self, Change, CliStatus, System};

use crate::commands::remote::blocking;
use crate::error::{AppError, AppResult};

fn bundled() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok().and_then(|exe| cli_install::bundled_for(&exe))
}

#[tauri::command]
pub async fn cli_status() -> AppResult<CliStatus> {
    blocking(|| Ok(cli_install::status(&System, bundled().as_deref(), env!("CARGO_PKG_VERSION")))).await
}

/// Put the bundled `tbx` on the PATH for this user (or refresh a stale copy).
#[tauri::command]
pub async fn cli_install() -> AppResult<Change> {
    blocking(|| {
        let program = bundled().ok_or_else(|| AppError::msg("This copy of Toybox has no tbx with it."))?;
        cli_install::install(&System, &program).map_err(AppError::msg)
    })
    .await
}

#[tauri::command]
pub async fn cli_uninstall() -> AppResult<Change> {
    blocking(|| {
        let program = bundled().ok_or_else(|| AppError::msg("This copy of Toybox has no tbx with it."))?;
        cli_install::uninstall(&System, &program).map_err(AppError::msg)
    })
    .await
}
