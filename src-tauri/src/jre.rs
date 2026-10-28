use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::downloader::{self, FilePlan};
use crate::instances::{self, InstanceError};
use crate::mcmeta;
use crate::paths;

/// Products manifest of Mojang's bundled java runtimes (windows-x64).
pub(crate) const RUNTIME_PRODUCTS_URL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7ddd3927d302e379a95/windows-x64.json";
pub(crate) const STATUS_EVENT: &str = "java-status";
const EMIT_INTERVAL: Duration = Duration::from_millis(100);
const ASSET_THREADS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JavaSource {
    System,
    Bundled,
    Custom,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JavaStatus {
    #[default]
    Idle,
    Checking,
    Ready {
        source: JavaSource,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    Installing {
        component: String,
        done: u32,
        total: u32,
        bytes_done: u64,
        bytes_total: u64,
    },
    Error {
        message: String,
    },
}

#[derive(Default)]
pub struct JavaState {
    status: Mutex<JavaStatus>,
}

impl JavaState {
    fn set(&self, app: &AppHandle, status: JavaStatus) {
        *self.status.lock().unwrap() = status.clone();
        let _ = app.emit(STATUS_EVENT, status);
    }

    fn set_throttled(
        &self,
        app: &AppHandle,
        status: JavaStatus,
        clock: &Mutex<Instant>,
    ) {
        *self.status.lock().unwrap() = status.clone();
        let emit = {
            let mut last = clock.lock().unwrap();
            if last.elapsed() >= EMIT_INTERVAL {
                *last = Instant::now();
                true
            } else {
                false
            }
        };
        if emit {
            let _ = app.emit(STATUS_EVENT, status);
        }
    }
}

#[tauri::command]
pub fn java_status(state: State<'_, JavaState>) -> JavaStatus {
    state.status.lock().unwrap().clone()
}

/// Resolves the java executable for launching a game: the ready status path
/// (preferring `javaw.exe` next to it), or a fresh resolution run.
pub(crate) fn resolve_launch_java(app: &AppHandle) -> Result<PathBuf, InstanceError> {
    if let Some(path) = ready_java_path(app) {
        return Ok(path);
    }
    let state = app.state::<JavaState>();
    run_ensure(app, &state)?;
    ready_java_path(app).ok_or_else(|| {
        InstanceError::launch("не удалось найти подходящую Java для запуска")
    })
}

fn ready_java_path(app: &AppHandle) -> Option<PathBuf> {
    let status = app.state::<JavaState>().status.lock().unwrap().clone();
    let JavaStatus::Ready { path, .. } = status else {
        return None;
    };
    let path = PathBuf::from(&path);
    if path.file_name().is_some_and(|name| name == "java.exe") {
        if let Some(parent) = path.parent() {
            let javaw = parent.join("javaw.exe");
            if javaw.is_file() {
                return Some(javaw);
            }
        }
    }
    Some(path)
}

struct RequiredJava {
    component: String,
    major: u32,
}

/// Startup routine: resolves Java for the installed instances —
/// custom path → bundled runtime → system installation → download.
pub(crate) fn ensure_java(app: &AppHandle) {
    let state = app.state::<JavaState>();
    if let Err(error) = run_ensure(app, &state) {
        state.set(
            app,
            JavaStatus::Error {
                message: error.to_string(),
            },
        );
    }
}

fn run_ensure(app: &AppHandle, state: &JavaState) -> Result<(), InstanceError> {
    state.set(app, JavaStatus::Checking);

    let settings = crate::settings::load(app)
        .map_err(|error| InstanceError::storage(error.to_string()))?;
    if !settings.java_path.trim().is_empty() {
        let custom = PathBuf::from(settings.java_path.trim());
        if custom.is_file() {
            state.set(
                app,
                JavaStatus::Ready {
                    source: JavaSource::Custom,
                    path: settings.java_path.trim().to_owned(),
                    version: None,
                },
            );
            return Ok(());
        }
    }

    let Some(required) = required_java(app)? else {
        state.set(app, JavaStatus::Idle);
        return Ok(());
    };

    let home = paths::launcher_home(app).ok_or_else(|| {
        InstanceError::storage("не удалось определить домашнюю папку лаунчера")
    })?;

    let bundled_dir = home.join("runtime").join(&required.component);
    let bundled_exe = bundled_dir.join("bin").join("javaw.exe");
    if bundled_exe.is_file() {
        state.set(
            app,
            JavaStatus::Ready {
                source: JavaSource::Bundled,
                path: bundled_exe.to_string_lossy().to_string(),
                version: None,
            },
        );
        return Ok(());
    }

    if let Some(found) = find_system_java(required.major) {
        state.set(
            app,
            JavaStatus::Ready {
                source: JavaSource::System,
                path: found.path,
                version: found.version,
            },
        );
        return Ok(());
    }

    let version = install_runtime(app, state, &home, &required.component)?;
    let installed_exe = bundled_dir.join("bin").join("javaw.exe");
    if !installed_exe.is_file() {
        return Err(InstanceError::download(
            "рантайм скачан, но javaw.exe не найден",
        ));
    }
    state.set(
        app,
        JavaStatus::Ready {
            source: JavaSource::Bundled,
            path: installed_exe.to_string_lossy().to_string(),
            version,
        },
    );
    Ok(())
}

/// The strictest requirement among downloaded instances (max major version).
/// Instances downloaded before java tracking have their info backfilled
/// from the version JSON.
fn required_java(app: &AppHandle) -> Result<Option<RequiredJava>, InstanceError> {
    let home = paths::launcher_home(app).ok_or_else(|| {
        InstanceError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    let instances = instances::load_all_at(&instances::instances_dir_at(&home))?;
    let mut best: Option<(u32, String)> = None;
    for instance in instances {
        if !instance.downloaded {
            continue;
        }
        let info = match (instance.java_component.clone(), instance.java_major) {
            (Some(component), Some(major)) => Some((component, major)),
            _ => backfill_java_info(app, &home, &instance)?,
        };
        if let Some((component, major)) = info {
            if best.as_ref().is_none_or(|(best_major, _)| major > *best_major) {
                best = Some((major, component));
            }
        }
    }
    Ok(best.map(|(major, component)| RequiredJava { component, major }))
}

fn backfill_java_info(
    app: &AppHandle,
    home: &Path,
    instance: &instances::Instance,
) -> Result<Option<(String, u32)>, InstanceError> {
    let versions = mcmeta::fetch_version_list(app)?;
    let Some(version) = versions.iter().find(|v| v.id == instance.version_id) else {
        return Ok(None);
    };
    let bytes = match mcmeta::http_get_bytes(&version.url) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    let Ok(info) = downloader::extract_java_version(&bytes) else {
        return Ok(None);
    };
    if let Some((component, major)) = &info {
        let _ = instances::set_java_info_at(home, &instance.id, component, *major);
    }
    Ok(info)
}

struct FoundJava {
    path: String,
    version: Option<String>,
}

fn program_files_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(value) = env::var(variable) {
            if !value.trim().is_empty() {
                let base = PathBuf::from(value.trim());
                bases.push(base.join("Java"));
                bases.push(base.join("Eclipse Adoptium"));
            }
        }
    }
    bases
}

fn candidate_java_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(home) = env::var("JAVA_HOME") {
        let home = home.trim();
        if !home.is_empty() {
            candidates.push(PathBuf::from(home).join("bin").join("java.exe"));
        }
    }
    if let Ok(output) = Command::new("where").arg("java").output() {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    candidates.push(PathBuf::from(trimmed));
                }
            }
        }
    }
    for base in program_files_bases() {
        let entries = match fs::read_dir(&base) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if dir.is_dir() {
                candidates.push(dir.join("bin").join("java.exe"));
            }
        }
    }
    candidates
}

/// Parses the major version out of `java -version` output, e.g.
/// `openjdk version "17.0.9"` → 17, `java version "1.8.0_392"` → 8.
fn parse_java_version(output: &str) -> Option<u32> {
    let index = output.find("version \"")?;
    let rest = &output[index + "version \"".len()..];
    let end = rest.find('"')?;
    let version = &rest[..end];
    let mut parts = version.split('.');
    let first = leading_number(parts.next()?)?;
    if first == 1 {
        leading_number(parts.next()?)
    } else {
        Some(first)
    }
}

fn leading_number(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn probe_java(path: &Path) -> Option<(u32, Option<String>)> {
    let output = Command::new(path).arg("-version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    let major = parse_java_version(&combined)?;
    let version = extract_version_token(&combined);
    Some((major, version))
}

fn extract_version_token(output: &str) -> Option<String> {
    let index = output.find("version \"")?;
    let rest = &output[index + "version \"".len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn find_system_java(required_major: u32) -> Option<FoundJava> {
    for candidate in candidate_java_paths() {
        if !candidate.is_file() {
            continue;
        }
        if let Some((major, version)) = probe_java(&candidate) {
            if major == required_major {
                return Some(FoundJava {
                    path: candidate.to_string_lossy().to_string(),
                    version,
                });
            }
        }
    }
    None
}

#[derive(Debug, Deserialize)]
struct RuntimeEntry {
    #[serde(rename = "manifest")]
    manifest: RuntimeManifestRef,
    #[serde(default)]
    version: Option<RuntimeVersionRef>,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifestRef {
    #[serde(rename = "downloadUrl")]
    download_url: String,
    #[serde(default)]
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RuntimeVersionRef {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: BTreeMap<String, RuntimeFile>,
}

#[derive(Debug, Deserialize)]
struct RuntimeFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    downloads: Option<RuntimeDownloads>,
}

#[derive(Debug, Deserialize)]
struct RuntimeDownloads {
    #[serde(default)]
    raw: Option<RuntimeDownloadRef>,
}

#[derive(Debug, Deserialize)]
struct RuntimeDownloadRef {
    url: String,
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

fn parse_runtime_products(
    bytes: &[u8],
) -> Result<BTreeMap<String, RuntimeEntry>, InstanceError> {
    // The manifest is `{ "<component>": {...} }` on newer revisions and
    // `{ "<platform>": { "<component>": {...} } }` on older ones.
    if let Ok(flat) = serde_json::from_slice::<BTreeMap<String, RuntimeEntry>>(bytes) {
        if !flat.is_empty() {
            return Ok(flat);
        }
    }
    let nested: BTreeMap<String, BTreeMap<String, RuntimeEntry>> =
        serde_json::from_slice(bytes).map_err(|error| {
            InstanceError::storage(format!(
                "не удалось разобрать манифест рантаймов Java: {error}"
            ))
        })?;
    let mut flat = BTreeMap::new();
    for components in nested.into_values() {
        flat.extend(components);
    }
    Ok(flat)
}

fn parse_runtime_manifest(
    bytes: &[u8],
) -> Result<Vec<FilePlan>, InstanceError> {
    let manifest: RuntimeManifest = serde_json::from_slice(bytes).map_err(|error| {
        InstanceError::storage(format!(
            "не удалось разобрать манифест рантайма: {error}"
        ))
    })?;
    let mut plans = Vec::new();
    for (relative, file) in manifest.files {
        if file.kind != "file" {
            continue;
        }
        let Some(raw) = file.downloads.and_then(|downloads| downloads.raw) else {
            continue;
        };
        plans.push(FilePlan {
            url: raw.url,
            dest: PathBuf::from(&relative),
            sha1: raw.sha1,
            size: raw.size.unwrap_or(0),
        });
    }
    Ok(plans)
}

fn install_runtime(
    app: &AppHandle,
    state: &JavaState,
    home: &Path,
    component: &str,
) -> Result<Option<String>, InstanceError> {
    let products_bytes = mcmeta::http_get_bytes(RUNTIME_PRODUCTS_URL)?;
    let products = parse_runtime_products(&products_bytes)?;
    let entry = products.get(component).ok_or_else(|| {
        InstanceError::download(format!(
            "рантайм {component} отсутствует в манифесте Mojang"
        ))
    })?;
    let version = entry.version.as_ref().and_then(|v| v.name.clone());

    let manifest_bytes = mcmeta::http_get_bytes(&entry.manifest.download_url)?;
    if let Some(expected) = &entry.manifest.sha1 {
        if &sha1_hex(&manifest_bytes) != expected {
            return Err(InstanceError::download(
                "контрольная сумма манифеста рантайма не совпала",
            ));
        }
    }
    let plans = parse_runtime_manifest(&manifest_bytes)?;
    if plans.is_empty() {
        return Err(InstanceError::download(
            "манифест рантайма не содержит файлов",
        ));
    }

    let base = home.join("runtime").join(component);
    let total_plans: Vec<FilePlan> = plans
        .into_iter()
        .map(|plan| FilePlan {
            dest: base.join(&plan.dest),
            ..plan
        })
        .collect();
    let total = total_plans.len() as u32;
    let bytes_total: u64 = total_plans.iter().map(|plan| plan.size).sum();

    state.set(
        app,
        JavaStatus::Installing {
            component: component.to_owned(),
            done: 0,
            total,
            bytes_done: 0,
            bytes_total,
        },
    );

    let clock = Mutex::new(Instant::now() - EMIT_INTERVAL);
    let done = AtomicU32::new(0);
    let bytes_done = AtomicU64::new(0);
    let first_error: Mutex<Option<InstanceError>> = Mutex::new(None);

    std::thread::scope(|scope| {
        let clock = &clock;
        let done = &done;
        let bytes_done = &bytes_done;
        let first_error = &first_error;
        let chunk_size = total_plans.len().div_ceil(ASSET_THREADS).max(1);
        for chunk in total_plans.chunks(chunk_size) {
            scope.spawn(move || {
                for plan in chunk {
                    match downloader::download_one(plan) {
                        Ok(()) => {
                            done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            bytes_done
                                .fetch_add(plan.size, std::sync::atomic::Ordering::Relaxed);
                            state.set_throttled(
                                app,
                                JavaStatus::Installing {
                                    component: component.to_owned(),
                                    done: done.load(std::sync::atomic::Ordering::Relaxed),
                                    total,
                                    bytes_done: bytes_done
                                        .load(std::sync::atomic::Ordering::Relaxed),
                                    bytes_total,
                                },
                                &clock,
                            );
                        }
                        Err(error) => {
                            let mut guard = first_error.lock().unwrap();
                            if guard.is_none() {
                                *guard = Some(error);
                            }
                            return;
                        }
                    }
                }
            });
        }
    });

    if let Some(error) = first_error.into_inner().unwrap() {
        return Err(error);
    }
    state.set(
        app,
        JavaStatus::Installing {
            component: component.to_owned(),
            done: total,
            total,
            bytes_done: bytes_done.load(std::sync::atomic::Ordering::Relaxed),
            bytes_total,
        },
    );
    Ok(version)
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_java_version_known_outputs() {
        assert_eq!(
            parse_java_version("openjdk version \"17.0.9\" 2023-10-17"),
            Some(17)
        );
        assert_eq!(
            parse_java_version("java version \"1.8.0_392\""),
            Some(8)
        );
        assert_eq!(
            parse_java_version("openjdk version \"21.0.1\" 2023-10-17"),
            Some(21)
        );
        assert_eq!(
            parse_java_version("openjdk version \"25-ea\" 2025-03-18"),
            Some(25)
        );
        assert_eq!(parse_java_version("no version here"), None);
    }

    #[test]
    fn extract_version_token_reads_quoted_value() {
        assert_eq!(
            extract_version_token("openjdk version \"17.0.9\" 2023-10-17").as_deref(),
            Some("17.0.9")
        );
        assert_eq!(extract_version_token("nothing"), None);
    }

    const FLAT_PRODUCTS: &str = r#"{
        "java-runtime-gamma": {
            "version": { "name": "17.0.3" },
            "manifest": { "downloadUrl": "https://example.com/gamma.json", "size": 1, "sha1": "aa" }
        },
        "jre-legacy": {
            "version": { "name": "1.8.0_402" },
            "manifest": { "downloadUrl": "https://example.com/legacy.json", "size": 1, "sha1": "bb" }
        }
    }"#;

    const NESTED_PRODUCTS: &str = r#"{
        "windows-x64": {
            "java-runtime-delta": {
                "version": { "name": "21.0.2" },
                "manifest": { "downloadUrl": "https://example.com/delta.json", "size": 1, "sha1": "cc" }
            }
        }
    }"#;

    #[test]
    fn parse_runtime_products_flat() {
        let products = parse_runtime_products(FLAT_PRODUCTS.as_bytes()).unwrap();
        assert_eq!(products.len(), 2);
        assert_eq!(
            products.get("java-runtime-gamma").unwrap().manifest.download_url,
            "https://example.com/gamma.json"
        );
        assert_eq!(
            products.get("jre-legacy").unwrap().version.as_ref().unwrap().name.as_deref(),
            Some("1.8.0_402")
        );
    }

    #[test]
    fn parse_runtime_products_nested_platform() {
        let products = parse_runtime_products(NESTED_PRODUCTS.as_bytes()).unwrap();
        assert_eq!(products.len(), 1);
        let delta = products.get("java-runtime-delta").unwrap();
        assert_eq!(delta.manifest.download_url, "https://example.com/delta.json");
        assert_eq!(
            delta.version.as_ref().unwrap().name.as_deref(),
            Some("21.0.2")
        );
    }

    #[test]
    fn parse_runtime_manifest_keeps_files_with_raw_downloads() {
        let manifest = parse_runtime_manifest(
            br#"{
                "files": {
                    "bin/javaw.exe": {
                        "type": "file",
                        "downloads": { "raw": { "url": "https://example.com/javaw.exe", "sha1": "aa", "size": 100 } },
                        "executable": true
                    },
                    "lib/modules": {
                        "type": "file",
                        "downloads": { "raw": { "url": "https://example.com/modules", "size": 50 } }
                    },
                    "conf/": { "type": "directory" },
                    "broken": { "type": "file" }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(manifest.len(), 2);
        let javaw = manifest
            .iter()
            .find(|plan| plan.dest == PathBuf::from("bin/javaw.exe"))
            .unwrap();
        assert_eq!(javaw.url, "https://example.com/javaw.exe");
        assert_eq!(javaw.sha1.as_deref(), Some("aa"));
        assert_eq!(javaw.size, 100);
        assert!(manifest
            .iter()
            .all(|plan| plan.dest != PathBuf::from("conf/")
                && plan.dest != PathBuf::from("broken")));
    }

    #[test]
    fn sha1_hex_known_vector() {
        assert_eq!(
            sha1_hex(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }
}
