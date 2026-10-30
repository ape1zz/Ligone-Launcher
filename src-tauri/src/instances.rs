use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::downloader::InstanceState;
use crate::paths;

const MAX_NAME_LEN: usize = 64;
const FORBIDDEN_NAME_CHARS: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Vanilla,
    Fabric,
    Forge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub version_id: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub created_at: u64,
    pub downloaded: bool,
    #[serde(default)]
    pub java_component: Option<String>,
    #[serde(default)]
    pub java_major: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InstanceError {
    Storage { message: String },
    NotFound { id: String },
    InvalidName { message: String },
    Busy { id: String },
    Network { message: String },
    Download { message: String },
    Launch { message: String },
    Cancelled,
}

impl InstanceError {
    pub(crate) fn storage(message: impl Into<String>) -> Self {
        Self::Storage {
            message: message.into(),
        }
    }

    pub(crate) fn not_found(id: impl Into<String>) -> Self {
        Self::NotFound { id: id.into() }
    }

    pub(crate) fn busy(id: impl Into<String>) -> Self {
        Self::Busy { id: id.into() }
    }

    pub(crate) fn download(message: impl Into<String>) -> Self {
        Self::Download {
            message: message.into(),
        }
    }

    pub(crate) fn launch(message: impl Into<String>) -> Self {
        Self::Launch {
            message: message.into(),
        }
    }
}

impl fmt::Display for InstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage { message } => {
                write!(formatter, "ошибка хранилища: {message}")
            }
            Self::NotFound { id } => {
                write!(formatter, "сборка не найдена: {id}")
            }
            Self::InvalidName { message } => {
                write!(formatter, "некорректное имя сборки: {message}")
            }
            Self::Busy { id } => {
                write!(formatter, "сборка занята: {id}")
            }
            Self::Network { message } => {
                write!(formatter, "ошибка сети: {message}")
            }
            Self::Download { message } => {
                write!(formatter, "ошибка загрузки: {message}")
            }
            Self::Launch { message } => {
                write!(formatter, "ошибка запуска: {message}")
            }
            Self::Cancelled => {
                write!(formatter, "загрузка отменена")
            }
        }
    }
}

impl std::error::Error for InstanceError {}

impl From<std::io::Error> for InstanceError {
    fn from(error: std::io::Error) -> Self {
        Self::storage(error.to_string())
    }
}

impl From<serde_json::Error> for InstanceError {
    fn from(error: serde_json::Error) -> Self {
        Self::storage(error.to_string())
    }
}

fn now_millis() -> Result<u64, InstanceError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .map_err(|error| InstanceError::storage(error.to_string()))
}

pub(crate) fn validate_name(name: &str) -> Result<String, InstanceError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(InstanceError::InvalidName {
            message: "имя не может быть пустым".to_owned(),
        });
    }
    if trimmed.len() > MAX_NAME_LEN {
        return Err(InstanceError::InvalidName {
            message: format!("имя длиннее {MAX_NAME_LEN} символов"),
        });
    }
    if let Some(forbidden) = trimmed
        .chars()
        .find(|ch| FORBIDDEN_NAME_CHARS.contains(ch) || ch.is_control())
    {
        return Err(InstanceError::InvalidName {
            message: format!("недопустимый символ: {forbidden}"),
        });
    }
    Ok(trimmed.to_owned())
}

pub(crate) fn validate_id(id: &str) -> Result<(), InstanceError> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
    if valid {
        Ok(())
    } else {
        Err(InstanceError::storage(format!(
            "некорректный идентификатор сборки: {id}"
        )))
    }
}

pub(crate) fn instances_dir_at(home: &Path) -> PathBuf {
    home.join("instances")
}

fn instances_dir(app: &AppHandle) -> Result<PathBuf, InstanceError> {
    let home = paths::launcher_home(app).ok_or_else(|| {
        InstanceError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    Ok(instances_dir_at(&home))
}

fn instance_meta_path(dir: &Path) -> PathBuf {
    dir.join("instance.json")
}

pub(crate) fn instance_dir_at(home: &Path, id: &str) -> PathBuf {
    instances_dir_at(home).join(id)
}

pub(crate) fn instance_dir(app: &AppHandle, id: &str) -> Result<PathBuf, InstanceError> {
    let home = paths::launcher_home(app).ok_or_else(|| {
        InstanceError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    Ok(instance_dir_at(&home, id))
}

pub(crate) fn read_instance(dir: &Path) -> Result<Option<Instance>, InstanceError> {
    match fs::read(instance_meta_path(dir)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn write_instance(dir: &Path, instance: &Instance) -> Result<(), InstanceError> {
    fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec_pretty(instance)?;
    fs::write(instance_meta_path(dir), bytes)?;
    Ok(())
}

pub(crate) fn load_all_at(dir: &Path) -> Result<Vec<Instance>, InstanceError> {
    let mut instances = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(instances),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match read_instance(&path) {
            Ok(Some(instance)) => instances.push(instance),
            Ok(None) | Err(_) => continue,
        }
    }
    instances.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(instances)
}

fn load_one_at(dir: &Path, id: &str) -> Result<Instance, InstanceError> {
    read_instance(dir)?.ok_or_else(|| InstanceError::not_found(id))
}

fn create_instance_dir(dir: &Path) -> Result<(), InstanceError> {
    fs::create_dir_all(dir)?;
    for sub in ["mods", "resourcepacks", "shaderpacks", "saves"] {
        fs::create_dir_all(dir.join(sub))?;
    }
    let settings = dir.join("settings.txt");
    if !settings.exists() {
        fs::write(&settings, "")?;
    }
    Ok(())
}

fn instance_exists(dir: &Path) -> bool {
    instance_meta_path(dir).exists()
}

#[tauri::command]
pub fn instance_list(app: AppHandle) -> Result<Vec<Instance>, InstanceError> {
    load_all_at(&instances_dir(&app)?)
}

#[tauri::command]
pub fn instance_create(
    app: AppHandle,
    name: String,
    version_id: String,
    loader: Loader,
    loader_version: Option<String>,
) -> Result<Instance, InstanceError> {
    let name = validate_name(&name)?;
    let version_id = version_id.trim().to_owned();
    if version_id.is_empty() {
        return Err(InstanceError::storage("не указана версия игры"));
    }
    let loader_version = match loader {
        Loader::Vanilla => None,
        Loader::Fabric | Loader::Forge => {
            let label = match loader {
                Loader::Fabric => "Fabric",
                _ => "Forge",
            };
            match loader_version.as_deref().map(str::trim) {
                Some(version) if !version.is_empty() => Some(version.to_owned()),
                _ => {
                    return Err(InstanceError::storage(format!(
                        "для {label} необходимо указать версию загрузчика"
                    )))
                }
            }
        }
    };
    let dir = instances_dir(&app)?;
    let instance = Instance {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        version_id,
        loader,
        loader_version,
        created_at: now_millis()?,
        downloaded: false,
        java_component: None,
        java_major: None,
    };
    let target = dir.join(&instance.id);
    if instance_exists(&target) {
        return Err(InstanceError::storage(
            "конфликт идентификаторов сборки, попробуйте ещё раз",
        ));
    }
    create_instance_dir(&target)?;
    write_instance(&target, &instance)?;
    Ok(instance)
}

#[tauri::command]
pub fn instance_remove(app: AppHandle, id: String) -> Result<(), InstanceError> {
    validate_id(&id)?;
    if app.state::<InstanceState>().is_downloading(&id) {
        return Err(InstanceError::busy(id));
    }
    if app.state::<crate::launch::LaunchState>().is_running(&id) {
        return Err(InstanceError::busy(id));
    }
    let dir = instance_dir(&app, &id)?;
    if !instance_exists(&dir) {
        return Err(InstanceError::not_found(id));
    }
    fs::remove_dir_all(&dir)?;
    Ok(())
}

#[tauri::command]
pub fn instance_rename(app: AppHandle, id: String, name: String) -> Result<Instance, InstanceError> {
    validate_id(&id)?;
    let name = validate_name(&name)?;
    let dir = instance_dir(&app, &id)?;
    let mut instance = load_one_at(&dir, &id)?;
    instance.name = name;
    write_instance(&dir, &instance)?;
    Ok(instance)
}

pub(crate) fn validate_sub(sub: Option<&str>) -> Result<Option<String>, InstanceError> {
    match sub.map(str::trim) {
        None | Some("") => Ok(None),
        Some(name)
            if matches!(name, "mods" | "resourcepacks" | "shaderpacks" | "saves") =>
        {
            Ok(Some(name.to_owned()))
        }
        Some(other) => Err(InstanceError::storage(format!(
            "неизвестная папка сборки: {other}"
        ))),
    }
}

#[tauri::command]
pub fn instance_open_folder(
    app: AppHandle,
    id: String,
    sub: Option<String>,
) -> Result<(), InstanceError> {
    validate_id(&id)?;
    let sub = validate_sub(sub.as_deref())?;
    let dir = instance_dir(&app, &id)?;
    if !instance_exists(&dir) {
        return Err(InstanceError::not_found(id));
    }
    let target = match sub {
        Some(name) => dir.join(name),
        None => dir,
    };
    fs::create_dir_all(&target)?;
    app.opener()
        .open_path(target.as_os_str().to_string_lossy(), None::<&str>)
        .map_err(|error| InstanceError::storage(error.to_string()))
}

pub(crate) fn mark_downloaded(
    app: &AppHandle,
    id: &str,
    downloaded: bool,
) -> Result<Instance, InstanceError> {
    let dir = instance_dir(app, id)?;
    let mut instance = load_one_at(&dir, id)?;
    instance.downloaded = downloaded;
    write_instance(&dir, &instance)?;
    Ok(instance)
}

pub(crate) fn set_java_info(
    app: &AppHandle,
    id: &str,
    component: &str,
    major: u32,
) -> Result<Instance, InstanceError> {
    let home = paths::launcher_home(app).ok_or_else(|| {
        InstanceError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    set_java_info_at(&home, id, component, major)
}

pub(crate) fn set_java_info_at(
    home: &Path,
    id: &str,
    component: &str,
    major: u32,
) -> Result<Instance, InstanceError> {
    let dir = instance_dir_at(home, id);
    let mut instance = load_one_at(&dir, id)?;
    instance.java_component = Some(component.to_owned());
    instance.java_major = Some(major);
    write_instance(&dir, &instance)?;
    Ok(instance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ligone-instances-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(id: &str, name: &str) -> Instance {
        Instance {
            id: id.to_owned(),
            name: name.to_owned(),
            version_id: "1.20.1".to_owned(),
            loader: Loader::Vanilla,
            loader_version: None,
            created_at: 1_700_000_000_000,
            downloaded: false,
            java_component: None,
            java_major: None,
        }
    }

    #[test]
    fn validate_name_trims_and_accepts() {
        assert_eq!(validate_name("  My Pack  ").unwrap(), "My Pack");
        assert_eq!(validate_name("1.20-тест").unwrap(), "1.20-тест");
    }

    #[test]
    fn validate_name_rejects_empty_and_long() {
        assert!(matches!(
            validate_name("   "),
            Err(InstanceError::InvalidName { .. })
        ));
        let long = "x".repeat(MAX_NAME_LEN + 1);
        assert!(matches!(
            validate_name(&long),
            Err(InstanceError::InvalidName { .. })
        ));
    }

    #[test]
    fn validate_name_rejects_forbidden_chars() {
        for ch in FORBIDDEN_NAME_CHARS {
            assert!(matches!(
                validate_name(&format!("bad{ch}name")),
                Err(InstanceError::InvalidName { .. })
            ));
        }
    }

    #[test]
    fn validate_id_rejects_traversal() {
        assert!(validate_id("../etc").is_err());
        assert!(validate_id("a/b").is_err());
        assert!(validate_id("").is_err());
        assert!(validate_id("a".repeat(65).as_str()).is_err());
        assert!(validate_id("0123abcd-ABCD").is_ok());
    }

    #[test]
    fn create_and_list_roundtrip() {
        let home = temp_dir("roundtrip");
        let dir = instances_dir_at(&home);
        let instance = sample("abc", "Test");
        create_instance_dir(&dir.join("abc")).unwrap();
        write_instance(&dir.join("abc"), &instance).unwrap();
        let listed = load_all_at(&dir).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "abc");
        assert_eq!(listed[0].name, "Test");
        std::thread::spawn(move || {
            let _ = fs::remove_dir_all(home);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn list_skips_corrupt_and_files() {
        let home = temp_dir("corrupt");
        let dir = instances_dir_at(&home);
        fs::create_dir_all(dir.join("good")).unwrap();
        write_instance(&dir.join("good"), &sample("good", "Good")).unwrap();
        fs::create_dir_all(dir.join("bad")).unwrap();
        fs::write(dir.join("bad").join("instance.json"), "{not json").unwrap();
        fs::create_dir_all(dir.join("nometadata")).unwrap();
        fs::write(dir.join("stray.json"), "{}").unwrap();
        let listed = load_all_at(&dir).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "good");
        std::thread::spawn(move || {
            let _ = fs::remove_dir_all(home);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn create_instance_dir_makes_game_folders() {
        let home = temp_dir("subdirs");
        let target = instances_dir_at(&home).join("abc");
        create_instance_dir(&target).unwrap();
        for sub in ["mods", "resourcepacks", "shaderpacks", "saves"] {
            assert!(target.join(sub).is_dir(), "missing {sub}");
        }
        assert!(target.join("settings.txt").is_file());
        std::thread::spawn(move || {
            let _ = fs::remove_dir_all(home);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn read_instance_missing_is_none() {
        let home = temp_dir("missing");
        let result = read_instance(&instances_dir_at(&home).join("nope")).unwrap();
        assert!(result.is_none());
        std::thread::spawn(move || {
            let _ = fs::remove_dir_all(home);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn instance_serde_camel_case() {
        let instance = Instance {
            id: "i".to_owned(),
            name: "n".to_owned(),
            version_id: "1.20.1".to_owned(),
            loader: Loader::Fabric,
            loader_version: Some("0.14.21".to_owned()),
            created_at: 123,
            downloaded: true,
            java_component: Some("java-runtime-gamma".to_owned()),
            java_major: Some(17),
        };
        let json = serde_json::to_value(&instance).unwrap();
        assert_eq!(json["versionId"], "1.20.1");
        assert_eq!(json["loaderVersion"], "0.14.21");
        assert_eq!(json["loader"], "fabric");
        assert_eq!(json["createdAt"], 123);
        assert_eq!(json["downloaded"], true);
        assert_eq!(json["javaComponent"], "java-runtime-gamma");
        assert_eq!(json["javaMajor"], 17);
        let parsed: Instance = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.loader, Loader::Fabric);
        assert_eq!(parsed.java_major, Some(17));
    }

    #[test]
    fn instance_serde_backfills_missing_java_fields() {
        let parsed: Instance = serde_json::from_value(serde_json::json!({
            "id": "i",
            "name": "n",
            "versionId": "1.20.1",
            "loader": "vanilla",
            "loaderVersion": null,
            "createdAt": 1,
            "downloaded": false
        }))
        .unwrap();
        assert_eq!(parsed.java_component, None);
        assert_eq!(parsed.java_major, None);
    }

    #[test]
    fn set_java_info_updates_fields() {
        let home = temp_dir("java-info");
        let dir = instances_dir_at(&home).join("abc");
        create_instance_dir(&dir).unwrap();
        write_instance(&dir, &sample("abc", "Test")).unwrap();
        let updated = set_java_info_at(&home, "abc", "java-runtime-gamma", 17).unwrap();
        assert_eq!(updated.java_component.as_deref(), Some("java-runtime-gamma"));
        assert_eq!(updated.java_major, Some(17));
        let reread = load_one_at(&dir, "abc").unwrap();
        assert_eq!(reread.java_major, Some(17));
        std::thread::spawn(move || {
            let _ = fs::remove_dir_all(home);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn validate_sub_whitelists_game_folders() {
        assert_eq!(validate_sub(None).unwrap(), None);
        assert_eq!(validate_sub(Some("")).unwrap(), None);
        assert_eq!(validate_sub(Some("  mods ")).unwrap(), Some("mods".to_owned()));
        for name in ["resourcepacks", "shaderpacks", "saves"] {
            assert_eq!(
                validate_sub(Some(name)).unwrap(),
                Some(name.to_owned())
            );
        }
        assert!(validate_sub(Some("../escape")).is_err());
        assert!(validate_sub(Some("versions")).is_err());
    }

    #[test]
    fn mark_downloaded_updates_flag() {
        let home = temp_dir("mark");
        let dir = instances_dir_at(&home).join("abc");
        create_instance_dir(&dir).unwrap();
        write_instance(&dir, &sample("abc", "Test")).unwrap();
        let updated = mark_downloaded_at(&home, "abc", true).unwrap();
        assert!(updated.downloaded);
        let reread = load_one_at(&dir, "abc").unwrap();
        assert!(reread.downloaded);
        std::thread::spawn(move || {
            let _ = fs::remove_dir_all(home);
        })
        .join()
        .unwrap();
    }
}

#[cfg(test)]
pub(crate) fn mark_downloaded_at(home: &Path, id: &str, downloaded: bool) -> Result<Instance, InstanceError> {
    let dir = instance_dir_at(home, id);
    let mut instance = load_one_at(&dir, id)?;
    instance.downloaded = downloaded;
    write_instance(&dir, &instance)?;
    Ok(instance)
}
