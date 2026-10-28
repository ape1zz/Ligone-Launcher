use std::fmt;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::paths;

const MIN_RAM_MB: u32 = 256;
const MAX_RAM_MB: u32 = 65_536;
const DEFAULT_RAM_MIN_MB: u32 = 1024;
const DEFAULT_RAM_MAX_MB: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Ru,
    En,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub java_path: String,
    pub ram_min_mb: u32,
    pub ram_max_mb: u32,
    pub theme: Theme,
    pub language: Language,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            java_path: String::new(),
            ram_min_mb: DEFAULT_RAM_MIN_MB,
            ram_max_mb: DEFAULT_RAM_MAX_MB,
            theme: Theme::Light,
            language: Language::Ru,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettingsError {
    Storage { message: String },
}

impl SettingsError {
    fn storage(message: impl Into<String>) -> Self {
        Self::Storage {
            message: message.into(),
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage { message } => {
                write!(formatter, "ошибка хранилища: {message}")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

impl From<std::io::Error> for SettingsError {
    fn from(error: std::io::Error) -> Self {
        Self::storage(error.to_string())
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, SettingsError> {
    let home = paths::launcher_home(app).ok_or_else(|| {
        SettingsError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    Ok(home.join("settings.json"))
}

pub(crate) fn normalize(mut settings: Settings) -> Settings {
    settings.java_path = settings.java_path.trim().to_owned();
    settings.ram_min_mb = settings.ram_min_mb.clamp(MIN_RAM_MB, MAX_RAM_MB);
    settings.ram_max_mb = settings.ram_max_mb.clamp(MIN_RAM_MB, MAX_RAM_MB);
    if settings.ram_min_mb > settings.ram_max_mb {
        std::mem::swap(&mut settings.ram_min_mb, &mut settings.ram_max_mb);
    }
    settings
}

pub(crate) fn load(app: &AppHandle) -> Result<Settings, SettingsError> {
    let path = settings_path(app)?;
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            SettingsError::storage(format!(
                "не удалось разобрать {}: {error}",
                path.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn save(app: &AppHandle, settings: &Settings) -> Result<(), SettingsError> {
    let path = settings_path(app)?;
    let parent = path.parent().ok_or_else(|| {
        SettingsError::storage(format!(
            "нет родительского каталога: {}",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(settings).map_err(|error| {
        SettingsError::storage(error.to_string())
    })?;
    fs::write(&path, bytes)?;
    Ok(())
}

#[tauri::command]
pub fn settings_get(app: AppHandle) -> Result<Settings, SettingsError> {
    load(&app)
}

#[tauri::command]
pub fn settings_set(app: AppHandle, settings: Settings) -> Result<Settings, SettingsError> {
    let normalized = normalize(settings);
    save(&app, &normalized)?;
    Ok(normalized)
}

#[tauri::command]
pub fn settings_open_home(app: AppHandle) -> Result<(), SettingsError> {
    let home = paths::launcher_home(&app).ok_or_else(|| {
        SettingsError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    std::fs::create_dir_all(&home)?;
    app.opener()
        .open_path(home.as_os_str().to_string_lossy(), None::<&str>)
        .map_err(|error| SettingsError::storage(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{normalize, Language, Settings, Theme, DEFAULT_RAM_MAX_MB, DEFAULT_RAM_MIN_MB, MAX_RAM_MB, MIN_RAM_MB};

    #[test]
    fn defaults_are_sane() {
        let settings = Settings::default();
        assert_eq!(settings.java_path, "");
        assert_eq!(settings.ram_min_mb, DEFAULT_RAM_MIN_MB);
        assert_eq!(settings.ram_max_mb, DEFAULT_RAM_MAX_MB);
        assert_eq!(settings.theme, Theme::Light);
        assert_eq!(settings.language, Language::Ru);
    }

    #[test]
    fn normalize_trims_java_path() {
        let mut settings = Settings::default();
        settings.java_path = "  C:\\Java\\bin\\javaw.exe  ".to_owned();
        let normalized = normalize(settings);
        assert_eq!(normalized.java_path, "C:\\Java\\bin\\javaw.exe");
    }

    #[test]
    fn normalize_clamps_ram_into_bounds() {
        let mut settings = Settings::default();
        settings.ram_min_mb = 1;
        settings.ram_max_mb = MAX_RAM_MB * 10;
        let normalized = normalize(settings);
        assert_eq!(normalized.ram_min_mb, MIN_RAM_MB);
        assert_eq!(normalized.ram_max_mb, MAX_RAM_MB);
    }

    #[test]
    fn normalize_swaps_inverted_ram() {
        let mut settings = Settings::default();
        settings.ram_min_mb = 8192;
        settings.ram_max_mb = 2048;
        let normalized = normalize(settings);
        assert_eq!(normalized.ram_min_mb, 2048);
        assert_eq!(normalized.ram_max_mb, 8192);
    }

    #[test]
    fn settings_serde_camel_case_and_theme_lowercase() {
        let settings = Settings::default();
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["javaPath"], "");
        assert_eq!(json["ramMinMb"], DEFAULT_RAM_MIN_MB);
        assert_eq!(json["ramMaxMb"], DEFAULT_RAM_MAX_MB);
        assert_eq!(json["theme"], "light");
        assert_eq!(json["language"], "ru");
        let parsed: Settings = serde_json::from_value(serde_json::json!({
            "javaPath": "java",
            "ramMinMb": 512,
            "ramMaxMb": 1024,
            "theme": "dark",
            "language": "en"
        }))
        .unwrap();
        assert_eq!(parsed.theme, Theme::Dark);
        assert_eq!(parsed.language, Language::En);
    }
}
