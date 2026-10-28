use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::instances::InstanceError;
use crate::paths;

pub(crate) const MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const CACHE_TTL_MILLIS: u64 = 30 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedManifest {
    fetched_at: u64,
    versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    versions: Vec<VersionInfo>,
}

pub(crate) fn now_millis() -> Result<u64, InstanceError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .map_err(|error| InstanceError::storage(error.to_string()))
}

pub(crate) fn http_get_bytes(url: &str) -> Result<Vec<u8>, InstanceError> {
    let response = ureq::get(url)
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|error| InstanceError::Network {
            message: format!("не удалось скачать {url}: {error}"),
        })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(512 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|error| InstanceError::Network {
            message: format!("не удалось дочитать {url}: {error}"),
        })?;
    Ok(bytes)
}

fn parse_manifest(bytes: &[u8]) -> Result<Vec<VersionInfo>, InstanceError> {
    let raw: RawManifest = serde_json::from_slice(bytes).map_err(|error| {
        InstanceError::storage(format!("не удалось разобрать манифест версий: {error}"))
    })?;
    Ok(raw.versions)
}

fn cache_path_at(home: &Path) -> PathBuf {
    home.join("cache").join("version_manifest.json")
}

fn read_cache_at(path: &Path) -> Option<CachedManifest> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_cache_at(path: &Path, versions: &[VersionInfo]) -> Result<(), InstanceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let cached = CachedManifest {
        fetched_at: now_millis()?,
        versions: versions.to_vec(),
    };
    fs::write(path, serde_json::to_vec_pretty(&cached)?)?;
    Ok(())
}

fn is_fresh(fetched_at: u64, now: u64) -> bool {
    now.saturating_sub(fetched_at) < CACHE_TTL_MILLIS
}

pub(crate) fn fetch_version_list(app: &AppHandle) -> Result<Vec<VersionInfo>, InstanceError> {
    let home = paths::launcher_home(app).ok_or_else(|| {
        InstanceError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    let cache_path = cache_path_at(&home);
    let now = now_millis()?;
    if let Some(cached) = read_cache_at(&cache_path) {
        if is_fresh(cached.fetched_at, now) {
            return Ok(cached.versions);
        }
    }
    match http_get_bytes(MANIFEST_URL) {
        Ok(bytes) => {
            let versions = parse_manifest(&bytes)?;
            write_cache_at(&cache_path, &versions)?;
            Ok(versions)
        }
        Err(network_error) => {
            if let Some(cached) = read_cache_at(&cache_path) {
                return Ok(cached.versions);
            }
            Err(network_error)
        }
    }
}

#[tauri::command]
pub async fn version_list(app: AppHandle) -> Result<Vec<VersionInfo>, InstanceError> {
    tauri::async_runtime::spawn_blocking(move || fetch_version_list(&app))
        .await
        .map_err(|error| InstanceError::storage(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MANIFEST: &str = r#"{
        "latest": { "release": "1.21.4", "snapshot": "25w02a" },
        "versions": [
            {
                "id": "1.21.4",
                "type": "release",
                "url": "https://piston-meta.mojang.com/v1/packages/aa/1.21.4.json",
                "time": "2024-12-03T11:24:36+00:00",
                "releaseTime": "2024-12-03T10:12:00+00:00",
                "sha1": "aaaa1111bbbb2222cccc3333dddd4444eeee5555"
            },
            {
                "id": "25w02a",
                "type": "snapshot",
                "url": "https://piston-meta.mojang.com/v1/packages/bb/25w02a.json",
                "time": "2025-01-08T15:00:00+00:00",
                "releaseTime": "2025-01-08T14:30:00+00:00"
            }
        ]
    }"#;

    #[test]
    fn parse_manifest_reads_versions() {
        let versions = parse_manifest(SAMPLE_MANIFEST.as_bytes()).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].id, "1.21.4");
        assert_eq!(versions[0].kind, "release");
        assert_eq!(
            versions[0].sha1.as_deref(),
            Some("aaaa1111bbbb2222cccc3333dddd4444eeee5555")
        );
        assert_eq!(versions[1].kind, "snapshot");
        assert!(versions[1].sha1.is_none());
    }

    #[test]
    fn parse_manifest_rejects_garbage() {
        assert!(parse_manifest(b"{ nope").is_err());
    }

    #[test]
    fn version_info_serde_camel_case_roundtrip() {
        let versions = parse_manifest(SAMPLE_MANIFEST.as_bytes()).unwrap();
        let json = serde_json::to_value(&versions).unwrap();
        assert_eq!(json[0]["releaseTime"], "2024-12-03T10:12:00+00:00");
        assert_eq!(json[0]["id"], "1.21.4");
        let parsed: Vec<VersionInfo> = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn cache_roundtrip_and_ttl() {
        let dir = std::env::temp_dir().join("ligone-mcmeta-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("version_manifest.json");
        let versions = parse_manifest(SAMPLE_MANIFEST.as_bytes()).unwrap();
        write_cache_at(&path, &versions).unwrap();
        let cached = read_cache_at(&path).unwrap();
        assert_eq!(cached.versions.len(), 2);
        assert!(is_fresh(cached.fetched_at, cached.fetched_at + 1000));
        assert!(!is_fresh(cached.fetched_at, cached.fetched_at + CACHE_TTL_MILLIS + 1));
        assert!(read_cache_at(&dir.join("missing.json")).is_none());
        fs::remove_dir_all(&dir).unwrap();
    }
}
