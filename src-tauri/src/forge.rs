use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::instances::InstanceError;
use crate::jre;
use crate::mcmeta::http_get_bytes;

pub(crate) const FORGE_MAVEN_BASE: &str = "https://maven.minecraftforge.net";
const METADATA_URL: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const PROMOTIONS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";

/// The official installer refuses to run without this marker file in the
/// target directory ("There is no minecraft launcher profile..."); custom
/// launchers work around it with an empty profile set.
const LAUNCHER_MARKER: &str = "{\"profiles\":{}}";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeLoaderVersion {
    pub version: String,
    pub recommended: bool,
}

/// Extracts `<version>…</version>` values from maven-metadata.xml without an
/// XML dependency; the format is fixed upstream.
fn extract_versions_xml(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut versions = Vec::new();
    let mut rest = text.as_ref();
    const OPEN: &str = "<version>";
    const CLOSE: &str = "</version>";
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        match after.find(CLOSE) {
            Some(end) => {
                versions.push(after[..end].to_owned());
                rest = &after[end + CLOSE.len()..];
            }
            None => break,
        }
    }
    versions
}

/// Sort key for Forge build numbers (`47.2.0`), compared numerically.
fn build_sort_key(build: &str) -> Vec<u64> {
    build
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

/// All Forge builds for one game version, newest first, with the
/// recommended build (from promotions) flagged.
pub(crate) fn fetch_loader_versions(game_version: &str) -> Result<Vec<ForgeLoaderVersion>, InstanceError> {
    if !game_version
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(InstanceError::download(format!(
            "некорректная версия игры: {game_version}"
        )));
    }
    let bytes = http_get_bytes(METADATA_URL)?;
    let prefix = format!("{game_version}-");
    let mut builds: Vec<String> = extract_versions_xml(&bytes)
        .into_iter()
        .filter_map(|entry| {
            entry
                .strip_prefix(&prefix)
                .filter(|build| !build.is_empty())
                .map(str::to_owned)
        })
        .collect();
    builds.sort_by(|left, right| build_sort_key(right).cmp(&build_sort_key(left)));
    builds.dedup();
    let recommended = fetch_recommended(game_version)
        .map_err(|error| {
            InstanceError::download(format!(
                "не удалось определить рекомендуемую сборку Forge: {error}"
            ))
        })
        .unwrap_or(None);
    let versions: Vec<ForgeLoaderVersion> = builds
        .into_iter()
        .map(|build| ForgeLoaderVersion {
            recommended: recommended.as_deref() == Some(build.as_str()),
            version: build,
        })
        .collect();
    if versions.is_empty() {
        return Err(InstanceError::download(format!(
            "сборок Forge для {game_version} нет в списке maven"
        )));
    }
    Ok(versions)
}

#[derive(Deserialize, Default)]
struct Promotions {
    #[serde(default)]
    promos: BTreeMap<String, String>,
}

/// Best effort: the recommended build or None (some versions have none).
fn fetch_recommended(game_version: &str) -> Result<Option<String>, InstanceError> {
    let bytes = http_get_bytes(PROMOTIONS_URL)?;
    let promos: Promotions = serde_json::from_slice(&bytes)
        .map_err(|error| {
            InstanceError::download(format!("не удалось разобрать promotions Forge: {error}"))
        })?;
    let value = promos.promos.get(&format!("{game_version}-recommended"));
    Ok(value.cloned())
}

/// The installer-generated profile lives at `versions/<mc>-forge-<build>/…`,
/// mirroring the official launcher layout.
pub(crate) fn forge_version_id(game_version: &str, build: &str) -> String {
    format!("{game_version}-forge-{build}")
}

pub(crate) fn installer_url(game_version: &str, build: &str) -> String {
    format!("{FORGE_MAVEN_BASE}/net/minecraftforge/forge/{game_version}-{build}/forge-{game_version}-{build}-installer.jar")
}

pub(crate) fn installer_dest(home: &Path, game_version: &str, build: &str) -> PathBuf {
    home.join("cache")
        .join("forge")
        .join(format!("forge-{game_version}-{build}-installer.jar"))
}

fn generated_profile_path(home: &Path, game_version: &str, build: &str) -> PathBuf {
    let id = forge_version_id(game_version, build);
    home.join("versions").join(&id).join(format!("{id}.json"))
}

/// The installer profile as generated into the shared home caches — None
/// when no installer has run for this version yet.
pub(crate) fn cached_profile(
    home: &Path,
    game_version: &str,
    build: &str,
) -> Option<Vec<u8>> {
    let path = generated_profile_path(home, game_version, build);
    fs::read(path).ok()
}

/// Makes sure the installer-generated Forge profile exists (`versions/<id>/…`
/// in the launcher home), running the official installer headlessly when it
/// does not. Returns the raw profile JSON bytes.
pub(crate) fn ensure_profile(
    app: &tauri::AppHandle,
    home: &Path,
    game_version: &str,
    build: &str,
) -> Result<Vec<u8>, InstanceError> {
    if let Some(bytes) = cached_profile(home, game_version, build) {
        return Ok(bytes);
    }
    let installer = ensure_installer(home, game_version, build)?;
    run_installer(app, home, &installer)?;
    cached_profile(home, game_version, build).ok_or_else(|| {
        InstanceError::download(
            "установщик Forge не создал описание версии — попробуйте ещё раз",
        )
    })
}

fn ensure_installer(
    home: &Path,
    game_version: &str,
    build: &str,
) -> Result<PathBuf, InstanceError> {
    let dest = installer_dest(home, game_version, build);
    if dest.is_file() {
        return Ok(dest);
    }
    let bytes = http_get_bytes(&installer_url(game_version, build))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, &bytes)?;
    Ok(dest)
}

/// Tail of installer output for error messages; installed logs are ASCII.
fn tail_text(bytes: &[u8], max_chars: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_owned();
    }
    let skip = trimmed.chars().count() - max_chars;
    trimmed.chars().skip(skip).collect()
}

fn run_installer(
    app: &tauri::AppHandle,
    home: &Path,
    installer: &Path,
) -> Result<(), InstanceError> {
    // The marker only has to exist; the official launcher keeps real profiles
    // here, but the installer only checks the file's presence.
    let marker = home.join("launcher_profiles.json");
    if !marker.is_file() {
        fs::write(&marker, LAUNCHER_MARKER)?;
    }

    // Prefer java.exe over javaw.exe so installer errors reach our log.
    let mut java = jre::resolve_launch_java(app)?;
    let is_javaw = java
        .file_name()
        .is_some_and(|name| name == "javaw.exe" || name == "javaw");
    if is_javaw {
        if let Some(parent) = java.parent() {
            let console = parent.join("java.exe");
            if console.is_file() {
                java = console;
            }
        }
    }

    let output = Command::new(&java)
        .arg("-jar")
        .arg(installer)
        .arg("--installClient")
        .arg(home)
        .current_dir(home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            InstanceError::download(format!("не удалось запустить установщик Forge: {error}"))
        })?
        .wait_with_output()
        .map_err(|error| {
            InstanceError::download(format!(
                "не удалось дождаться установщика Forge: {error}"
            ))
        })?;

    if !output.status.success() {
        let mut combined = output.stdout.clone();
        combined.extend_from_slice(&output.stderr);
        let tail = tail_text(&combined, 400);
        let code = output.status.code().unwrap_or(-1);
        return Err(InstanceError::download(format!(
            "установщик Forge завершился с ошибкой (код {code}): {tail}"
        )));
    }
    Ok(())
}

#[tauri::command]
pub async fn forge_loader_versions(
    game_version: String,
) -> Result<Vec<ForgeLoaderVersion>, InstanceError> {
    tauri::async_runtime::spawn_blocking(move || fetch_loader_versions(&game_version))
        .await
        .map_err(|error| InstanceError::storage(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_METADATA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>net.minecraftforge</groupId>
  <artifactId>forge</artifactId>
  <versioning>
    <versions>
      <version>1.20.1-47.2.0</version>
      <version>1.20.1-47.0.3</version>
      <version>1.20.1-47.4.28</version>
      <version>1.16.5-36.2.39</version>
      <version>broken</version>
    </versions>
  </versioning>
</metadata>"#;

    #[test]
    fn extract_versions_scans_straight_rows() {
        let versions = extract_versions_xml(SAMPLE_METADATA.as_bytes());
        assert_eq!(
            versions,
            vec![
                "1.20.1-47.2.0".to_owned(),
                "1.20.1-47.0.3".to_owned(),
                "1.20.1-47.4.28".to_owned(),
                "1.16.5-36.2.39".to_owned(),
                "broken".to_owned(),
            ]
        );
    }

    #[test]
    fn build_sort_key_compares_numerically() {
        assert!(build_sort_key("47.4.28") > build_sort_key("47.2.0"));
        assert!(build_sort_key("47.2.0") > build_sort_key("47.0.3"));
        // Missing segments count as zero.
        assert!(build_sort_key("36.2.0") > build_sort_key("36.0"));
        // Unparsable segments are zeros, so they never panic.
        assert!(build_sort_key("x") == build_sort_key("y"));
    }

    #[test]
    fn forge_version_id_and_urls_follow_maven_naming() {
        assert_eq!(
            forge_version_id("1.20.1", "47.2.0"),
            "1.20.1-forge-47.2.0"
        );
        assert_eq!(
            installer_url("1.20.1", "47.2.0"),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-installer.jar"
        );
    }

    #[test]
    fn fetch_recommended_reads_promotions() {
        let promos: Promotions = serde_json::from_str(
            r#"{"homepage":"https://files.minecraftforge.net","promos":{"1.20.1-recommended":"47.4.28","1.13.2-latest":"25.0.63"}}"#,
        )
        .unwrap();
        assert_eq!(
            promos.promos.get("1.20.1-recommended").map(String::as_str),
            Some("47.4.28")
        );
        assert!(promos.promos.get("1.13.2-recommended").is_none());
    }

    #[test]
    fn tail_text_keeps_last_n_chars() {
        let long = (0..500).map(|i| (b'a' + (i % 26) as u8) as char).collect::<String>();
        let tail = tail_text(long.as_bytes(), 10);
        assert_eq!(tail.chars().count(), 10);
        assert_eq!(tail, long.chars().skip(490).collect::<String>());
    }

    #[test]
    fn tail_text_passes_short_text_through() {
        assert_eq!(tail_text(b"ok", 10), "ok");
    }
}
