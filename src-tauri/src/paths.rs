use std::path::PathBuf;

use tauri::{AppHandle, Manager};

pub(crate) fn launcher_home(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    dir.parent().map(|parent| parent.join(".ligone"))
}
