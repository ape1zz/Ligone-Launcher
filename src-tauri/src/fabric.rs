use serde::{Deserialize, Serialize};

use crate::instances::InstanceError;
use crate::mcmeta::http_get_bytes;

pub(crate) const FABRIC_META_BASE: &str = "https://meta.fabricmc.net/v2";
pub(crate) const FABRIC_MAVEN_DEFAULT: &str = "https://maven.fabricmc.net/";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricLoaderVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Deserialize)]
struct RawLoaderEntry {
    loader: RawLoader,
}

#[derive(Debug, Deserialize)]
struct RawLoader {
    version: String,
    stable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricProfile {
    pub main_class: String,
    #[serde(default)]
    pub libraries: Vec<FabricLibrary>,
    /// Raw `arguments` from the meta profile; shapes vary, so entries are
    /// kept as values and flattened to strings at launch time.
    #[serde(default)]
    pub arguments: Option<FabricArguments>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricArguments {
    #[serde(default)]
    pub game: Vec<serde_json::Value>,
    #[serde(default)]
    pub jvm: Vec<serde_json::Value>,
}

impl FabricArguments {
    /// Flattens entries to plain strings, dropping objects (rules/features).
    pub(crate) fn flat_game(&self) -> Vec<String> {
        self.game.iter().flat_map(string_values).collect()
    }

    pub(crate) fn flat_jvm(&self) -> Vec<String> {
        self.jvm.iter().flat_map(string_values).collect()
    }
}

fn string_values(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(text) => vec![text.clone()],
        serde_json::Value::Array(items) => items.iter().flat_map(string_values).collect(),
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricLibrary {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
}

fn loader_versions_url(game_version: &str) -> String {
    format!("{FABRIC_META_BASE}/versions/loader/{game_version}")
}

pub(crate) fn profile_url(game_version: &str, loader_version: &str) -> String {
    format!("{FABRIC_META_BASE}/versions/loader/{game_version}/{loader_version}/profile/json")
}

fn parse_loader_versions(bytes: &[u8]) -> Result<Vec<FabricLoaderVersion>, InstanceError> {
    let raw: Vec<RawLoaderEntry> = serde_json::from_slice(bytes).map_err(|error| {
        InstanceError::storage(format!(
            "не удалось разобрать список версий Fabric: {error}"
        ))
    })?;
    Ok(raw
        .into_iter()
        .map(|entry| FabricLoaderVersion {
            version: entry.loader.version,
            stable: entry.loader.stable,
        })
        .collect())
}

pub(crate) fn parse_profile(bytes: &[u8]) -> Result<FabricProfile, InstanceError> {
    serde_json::from_slice(bytes)
        .map_err(|error| InstanceError::storage(format!("не удалось разобрать профиль Fabric: {error}")))
}

pub(crate) fn fetch_loader_versions(
    game_version: &str,
) -> Result<Vec<FabricLoaderVersion>, InstanceError> {
    let bytes = http_get_bytes(&loader_versions_url(game_version))?;
    parse_loader_versions(&bytes)
}

#[tauri::command]
pub async fn fabric_loader_versions(
    game_version: String,
) -> Result<Vec<FabricLoaderVersion>, InstanceError> {
    tauri::async_runtime::spawn_blocking(move || fetch_loader_versions(&game_version))
        .await
        .map_err(|error| InstanceError::storage(error.to_string()))?
}

/// Converts a maven coordinate (`group:artifact:version`) into a relative
/// repository path (`group/artifact/version/artifact-version.jar`).
pub(crate) fn maven_relative_path(coordinate: &str) -> Option<String> {
    let parts: Vec<&str> = coordinate.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0];
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts.get(3).copied().unwrap_or("");
    let extension = parts.get(4).copied().unwrap_or("jar");
    let file = if classifier.is_empty() {
        format!("{artifact}-{version}.{extension}")
    } else {
        format!("{artifact}-{version}-{classifier}.{extension}")
    };
    Some(format!(
        "{}/{}/{}/{}",
        group.replace('.', "/"),
        artifact,
        version,
        file
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOADERS: &str = r#"[
        { "loader": { "version": "0.16.5", "stable": true }, "intermediary": { "version": "1" } },
        { "loader": { "version": "0.16.0-beta", "stable": false }, "intermediary": { "version": "1" } }
    ]"#;

    const SAMPLE_PROFILE: &str = r#"{
        "id": "1.20.1-fabric0.15.11",
        "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
        "libraries": [
            { "name": "net.fabricmc:fabric-loader:0.15.11", "url": "https://maven.fabricmc.net/" },
            { "name": "org.ow2.asm:asm:9.6" }
        ]
    }"#;

    #[test]
    fn parse_loader_versions_reads_entries() {
        let versions = parse_loader_versions(SAMPLE_LOADERS.as_bytes()).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, "0.16.5");
        assert!(versions[0].stable);
        assert!(!versions[1].stable);
    }

    #[test]
    fn loader_versions_serde_camel_case() {
        let versions = parse_loader_versions(SAMPLE_LOADERS.as_bytes()).unwrap();
        let json = serde_json::to_value(&versions).unwrap();
        assert_eq!(json[0]["version"], "0.16.5");
        assert_eq!(json[0]["stable"], true);
    }

    const SAMPLE_PROFILE_WITH_ARGS: &str = r#"{
        "id": "1.20.1-fabric0.15.11",
        "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
        "arguments": {
            "game": ["--username", "${auth_player_name}", { "rules": [] }],
            "jvm": ["-Dfabric.test=1", { "value": "-Xss" }]
        },
        "libraries": [
            { "name": "net.fabricmc:fabric-loader:0.15.11", "url": "https://maven.fabricmc.net/" }
        ]
    }"#;

    #[test]
    fn parse_profile_reads_libraries_and_main_class() {
        let profile = parse_profile(SAMPLE_PROFILE.as_bytes()).unwrap();
        assert_eq!(
            profile.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(profile.libraries.len(), 2);
        assert_eq!(profile.libraries[0].name, "net.fabricmc:fabric-loader:0.15.11");
        assert_eq!(
            profile.libraries[0].url.as_deref(),
            Some("https://maven.fabricmc.net/")
        );
        assert!(profile.libraries[1].url.is_none());
    }

    #[test]
    fn parse_profile_reads_arguments_flat() {
        let profile = parse_profile(SAMPLE_PROFILE_WITH_ARGS.as_bytes()).unwrap();
        let arguments = profile.arguments.expect("arguments present");
        assert_eq!(arguments.flat_game(), ["--username", "${auth_player_name}"]);
        assert_eq!(arguments.flat_jvm(), ["-Dfabric.test=1"]);
    }

    #[test]
    fn maven_relative_path_builds_expected_paths() {
        assert_eq!(
            maven_relative_path("net.fabricmc:fabric-loader:0.15.11").as_deref(),
            Some("net/fabricmc/fabric-loader/0.15.11/fabric-loader-0.15.11.jar")
        );
        assert_eq!(
            maven_relative_path("org.ow2.asm:asm:9.6").as_deref(),
            Some("org/ow2/asm/asm/9.6/asm-9.6.jar")
        );
        assert_eq!(
            maven_relative_path("org.lwjgl.lwjgl:lwjgl-platform:2.9.4-nightly-20150210:natives-windows:jar").as_deref(),
            Some("org/lwjgl/lwjgl/lwjgl-platform/2.9.4-nightly-20150210/lwjgl-platform-2.9.4-nightly-20150210-natives-windows.jar")
        );
        assert!(maven_relative_path("broken").is_none());
    }
}
