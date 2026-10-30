use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::fabric::{self, FabricProfile};
use crate::forge;
use crate::instances::{self, Instance, InstanceError, Loader};
use crate::mcmeta;
use crate::paths;

pub(crate) const RESOURCES_BASE: &str = "https://resources.download.minecraft.net";
const DEFAULT_LIBRARY_BASE: &str = "https://libraries.minecraft.net/";
const ASSET_THREADS: usize = 8;
const EMIT_INTERVAL: Duration = Duration::from_millis(100);

pub const PROGRESS_EVENT: &str = "instance-download-progress";

#[derive(Default)]
pub struct InstanceState {
    downloading: Mutex<Option<String>>,
    cancel: Arc<AtomicBool>,
}

impl InstanceState {
    pub(crate) fn is_downloading(&self, id: &str) -> bool {
        self.downloading
            .lock()
            .unwrap()
            .as_deref()
            .is_some_and(|current| current == id)
    }

    fn begin(&self, id: &str) -> Result<(), InstanceError> {
        let mut guard = self.downloading.lock().unwrap();
        if let Some(current) = guard.as_deref() {
            return Err(InstanceError::busy(current));
        }
        *guard = Some(id.to_owned());
        self.cancel.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn request_cancel(&self, id: &str) -> bool {
        if self.is_downloading(id) {
            self.cancel.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

struct DownloadGuard<'a>(&'a InstanceState);

impl Drop for DownloadGuard<'_> {
    fn drop(&mut self) {
        *self.0.downloading.lock().unwrap() = None;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub instance_id: String,
    pub stage: &'static str,
    pub done: u32,
    pub total: u32,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct FilePlan {
    pub(crate) url: String,
    pub(crate) dest: PathBuf,
    pub(crate) sha1: Option<String>,
    pub(crate) size: u64,
}

struct Plan {
    jar: FilePlan,
    libraries: Vec<FilePlan>,
    asset_index_id: String,
    asset_index_url: String,
    asset_index_sha1: Option<String>,
    assets_total_bytes: u64,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct VersionJson {
    #[serde(default)]
    pub(crate) libraries: Vec<Library>,
    #[serde(rename = "assetIndex")]
    pub(crate) asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub(crate) downloads: ClientDownloads,
    #[serde(rename = "javaVersion", default)]
    pub(crate) java_version: Option<JavaVersionRef>,
    #[serde(rename = "mainClass", default)]
    pub(crate) main_class: Option<String>,
    #[serde(rename = "type", default)]
    pub(crate) version_type: Option<String>,
    #[serde(rename = "minecraftArguments", default)]
    pub(crate) minecraft_arguments: Option<String>,
    #[serde(default)]
    pub(crate) arguments: Option<Arguments>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct JavaVersionRef {
    #[serde(default)]
    pub(crate) component: Option<String>,
    #[serde(rename = "majorVersion", default)]
    pub(crate) major_version: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct ClientDownloads {
    #[serde(rename = "client", default)]
    pub(crate) client: Option<DownloadEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DownloadEntry {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) sha1: Option<String>,
    #[serde(default)]
    pub(crate) size: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AssetIndexRef {
    #[serde(default)]
    pub(crate) id: String,
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) sha1: Option<String>,
    #[serde(rename = "totalSize", default)]
    pub(crate) total_size: u64,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct Library {
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub(crate) rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct LibraryDownloads {
    #[serde(default)]
    pub(crate) artifact: Option<DownloadEntry>,
    #[serde(default)]
    pub(crate) classifiers: BTreeMap<String, DownloadEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Rule {
    pub(crate) action: String,
    #[serde(rename = "os")]
    pub(crate) os: Option<OsRule>,
    /// Feature-gated rules never apply: the launcher sets no features.
    #[serde(default)]
    pub(crate) features: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsRule {
    #[serde(rename = "name")]
    pub(crate) name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct Arguments {
    #[serde(default)]
    pub(crate) game: Vec<ArgEntry>,
    #[serde(default)]
    pub(crate) jvm: Vec<ArgEntry>,
}

/// A game/jvm argument: either a plain string or a `{rules, value}` object.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ArgEntry {
    Plain(String),
    Structured {
        #[serde(default)]
        rules: Vec<Rule>,
        #[serde(default)]
        value: ArgValue,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ArgValue {
    Single(String),
    Multiple(Vec<String>),
}

impl Default for ArgValue {
    fn default() -> Self {
        Self::Single(String::new())
    }
}

#[derive(Debug, Deserialize)]
struct AssetIndexJson {
    objects: BTreeMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
    #[serde(default)]
    size: u64,
}

/// A library destined for the shared `libraries/` cache; `natives` marks
/// classifier jars (they are extracted at launch, not put on the classpath).
#[derive(Debug, Clone)]
pub(crate) struct LibraryPlan {
    pub(crate) url: String,
    pub(crate) dest: PathBuf,
    pub(crate) sha1: Option<String>,
    pub(crate) size: u64,
    pub(crate) natives: bool,
}

pub(crate) fn rules_allowed(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule.features.is_some() {
            continue;
        }
        let applies = match &rule.os {
            Some(os) => os.name.as_deref() == Some("windows"),
            None => true,
        };
        if applies {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn url_file_path(url: &str) -> Option<String> {
    let without_scheme = url.split("://").nth(1)?;
    let (_, path) = without_scheme.split_once('/')?;
    if path.is_empty() {
        None
    } else {
        Some(path.to_owned())
    }
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

fn file_sha1(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(sha1_hex(&bytes))
}

pub(crate) fn parse_version_json(
    bytes: &[u8],
) -> Result<VersionJson, InstanceError> {
    serde_json::from_slice(bytes)
        .map_err(|error| InstanceError::storage(format!("не удалось разобрать описание версии: {error}")))
}

/// Version JSONs are immutable per version id; cache them for offline launches.
pub(crate) fn fetch_version_json_cached(
    app: &AppHandle,
    version_id: &str,
) -> Result<Vec<u8>, InstanceError> {
    if !version_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(InstanceError::storage(format!(
            "некорректный идентификатор версии: {version_id}"
        )));
    }
    let home = paths::launcher_home(app).ok_or_else(|| {
        InstanceError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    let cache = home
        .join("cache")
        .join("version_json")
        .join(format!("{version_id}.json"));
    if let Ok(bytes) = fs::read(&cache) {
        return Ok(bytes);
    }
    let versions = mcmeta::fetch_version_list(app)?;
    let version = versions
        .iter()
        .find(|version| version.id == version_id)
        .ok_or_else(|| {
            InstanceError::download(format!("версия {version_id} не найдена в манифесте"))
        })?;
    let bytes = mcmeta::http_get_bytes(&version.url)?;
    if let Some(parent) = cache.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&cache, &bytes)?;
    Ok(bytes)
}

/// Extracts `(component, majorVersion)` from a version JSON, e.g.
/// `("java-runtime-gamma", 17)`.
pub(crate) fn extract_java_version(
    bytes: &[u8],
) -> Result<Option<(String, u32)>, InstanceError> {
    let json = parse_version_json(bytes)?;
    Ok(json.java_version.and_then(|java| {
        let component = java.component?;
        let major = java.major_version?;
        Some((component, major))
    }))
}

fn parse_asset_index(bytes: &[u8]) -> Result<AssetIndexJson, InstanceError> {
    serde_json::from_slice(bytes)
        .map_err(|error| InstanceError::storage(format!("не удалось разобрать индекс ассетов: {error}")))
}

fn fabric_file_plan(home: &Path, library: &fabric::FabricLibrary) -> Option<FilePlan> {
    let relative = fabric::maven_relative_path(&library.name)?;
    let base = library
        .url
        .as_deref()
        .unwrap_or(fabric::FABRIC_MAVEN_DEFAULT)
        .trim_end_matches('/');
    Some(FilePlan {
        url: format!("{base}/{relative}"),
        dest: home.join("libraries").join(relative),
        sha1: None,
        size: 0,
    })
}

/// Allowed-for-Windows vanilla libraries: artifact jars plus
/// `natives-windows` classifier jars.
pub(crate) fn library_plans(json: &VersionJson, home: &Path) -> Vec<LibraryPlan> {
    let mut plans = Vec::new();
    for library in &json.libraries {
        if !rules_allowed(&library.rules) {
            continue;
        }
        if let Some(downloads) = &library.downloads {
            if let Some(artifact) = &downloads.artifact {
                if let Some((url, relative, sha1, size)) =
                    library_target(library.name.as_deref(), artifact)
                {
                    plans.push(LibraryPlan {
                        url,
                        dest: home.join("libraries").join(relative),
                        sha1,
                        size,
                        natives: false,
                    });
                }
            }
            if let Some(natives) = downloads.classifiers.get("natives-windows") {
                if let Some((url, relative, sha1, size)) =
                    library_target(library.name.as_deref(), natives)
                {
                    plans.push(LibraryPlan {
                        url,
                        dest: home.join("libraries").join(relative),
                        sha1,
                        size,
                        natives: true,
                    });
                }
            }
        } else if let Some(name) = library.name.as_deref() {
            if let Some(relative) = fabric::maven_relative_path(name) {
                plans.push(LibraryPlan {
                    url: format!("{DEFAULT_LIBRARY_BASE}{relative}"),
                    dest: home.join("libraries").join(relative),
                    sha1: None,
                    size: 0,
                    natives: false,
                });
            }
        }
    }
    plans
}

fn library_target(
    name: Option<&str>,
    entry: &DownloadEntry,
) -> Option<(String, String, Option<String>, u64)> {
    let relative = url_file_path(&entry.url).or_else(|| name.and_then(fabric::maven_relative_path))?;
    Some((
        entry.url.clone(),
        relative,
        entry.sha1.clone(),
        entry.size.unwrap_or(0),
    ))
}

fn file_plan_of(plan: LibraryPlan) -> FilePlan {
    FilePlan {
        url: plan.url,
        dest: plan.dest,
        sha1: plan.sha1,
        size: plan.size,
    }
}

/// Every library of one version JSON (vanilla, Fabric or a Forge overlay
/// profile — all share the `Library` shape) as plain file plans.
fn library_file_plans(json: &VersionJson, home: &Path) -> Vec<FilePlan> {
    library_plans(json, home)
        .into_iter()
        .map(file_plan_of)
        .collect()
}

fn build_plan(
    home: &Path,
    instance: &Instance,
    version_json: &VersionJson,
    fabric_profile: Option<&FabricProfile>,
) -> Result<Plan, InstanceError> {
    let client = version_json
        .downloads
        .client
        .as_ref()
        .ok_or_else(|| InstanceError::download("в описании версии нет клиентского jar"))?;
    let jar = FilePlan {
        url: client.url.clone(),
        dest: home
            .join("versions")
            .join(&instance.version_id)
            .join(format!("{}.jar", instance.version_id)),
        sha1: client.sha1.clone(),
        size: client.size.unwrap_or(0),
    };

    let mut libraries = library_file_plans(version_json, home);
    if let Some(profile) = fabric_profile {
        for library in &profile.libraries {
            if let Some(plan) = fabric_file_plan(home, library) {
                libraries.push(plan);
            }
        }
    }

    let asset_index = version_json
        .asset_index
        .as_ref()
        .ok_or_else(|| InstanceError::download("в описании версии нет индекса ассетов"))?;

    Ok(Plan {
        jar,
        libraries,
        asset_index_id: asset_index.id.clone(),
        asset_index_url: asset_index.url.clone(),
        asset_index_sha1: asset_index.sha1.clone(),
        assets_total_bytes: asset_index.total_size,
    })
}

fn verify_or_skip(plan: &FilePlan) -> Result<bool, InstanceError> {
    if !plan.dest.exists() {
        return Ok(false);
    }
    match &plan.sha1 {
        Some(expected) => Ok(file_sha1(&plan.dest).is_some_and(|actual| &actual == expected)),
        None => {
            if plan.size > 0 {
                let metadata = fs::metadata(&plan.dest)?;
                Ok(metadata.len() == plan.size)
            } else {
                Ok(true)
            }
        }
    }
}

pub(crate) fn download_one(plan: &FilePlan) -> Result<(), InstanceError> {
    if verify_or_skip(plan)? {
        return Ok(());
    }
    let bytes = mcmeta::http_get_bytes(&plan.url)?;
    if let Some(expected) = &plan.sha1 {
        let actual = sha1_hex(&bytes);
        if &actual != expected {
            return Err(InstanceError::download(format!(
                "контрольная сумма не совпала: {url}",
                url = plan.url
            )));
        }
    }
    if let Some(parent) = plan.dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&plan.dest, &bytes)?;
    Ok(())
}

/// The asset index is cached in the standard Mojang location
/// (`assets/indexes/<id>.json`) — the game reads it from there at launch too.
fn fetch_asset_index_bytes(home: &Path, plan: &Plan) -> Result<Vec<u8>, InstanceError> {
    let id = plan.asset_index_id.as_str();
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(InstanceError::download(format!(
            "некорректный идентификатор индекса ассетов: {id}"
        )));
    }
    let cache = home
        .join("assets")
        .join("indexes")
        .join(format!("{id}.json"));
    if let Some(actual) = file_sha1(&cache) {
        let matches = match &plan.asset_index_sha1 {
            Some(expected) => &actual == expected,
            None => true,
        };
        if matches {
            return fs::read(&cache).map_err(InstanceError::from);
        }
    }
    let bytes = mcmeta::http_get_bytes(&plan.asset_index_url)?;
    if let Some(expected) = &plan.asset_index_sha1 {
        if &sha1_hex(&bytes) != expected {
            return Err(InstanceError::download(
                "контрольная сумма индекса ассетов не совпала",
            ));
        }
    }
    if let Some(parent) = cache.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&cache, &bytes)?;
    Ok(bytes)
}

/// True when every file of the plan is already on disk and passes its
/// sha1/size check — the download can be skipped entirely (caches are shared
/// across instances, so a second instance of the same version needs nothing).
fn download_already_complete(
    jar: &FilePlan,
    libraries: &[FilePlan],
    assets: &[FilePlan],
) -> bool {
    let present = |file: &FilePlan| verify_or_skip(file).unwrap_or(false);
    present(jar) && libraries.iter().all(present) && assets.iter().all(present)
}

struct ProgressEmitter<'a> {
    app: &'a AppHandle,
    instance_id: String,
    last_emit: Mutex<Instant>,
}

impl<'a> ProgressEmitter<'a> {
    fn new(app: &'a AppHandle, instance_id: &str) -> Self {
        Self {
            app,
            instance_id: instance_id.to_owned(),
            last_emit: Mutex::new(Instant::now() - EMIT_INTERVAL),
        }
    }

    fn emit(&self, stage: &'static str, done: u32, total: u32, bytes_done: u64, bytes_total: u64) {
        let payload = DownloadProgress {
            instance_id: self.instance_id.clone(),
            stage,
            done,
            total,
            bytes_done,
            bytes_total,
        };
        let _ = self
            .app
            .emit(PROGRESS_EVENT, payload);
    }

    fn emit_throttled(
        &self,
        stage: &'static str,
        done: u32,
        total: u32,
        bytes_done: u64,
        bytes_total: u64,
    ) {
        let mut last = self.last_emit.lock().unwrap();
        if last.elapsed() >= EMIT_INTERVAL {
            *last = Instant::now();
            self.emit(stage, done, total, bytes_done, bytes_total);
        }
    }
}

fn run_download(
    app: &AppHandle,
    state: &InstanceState,
    id: &str,
) -> Result<(), InstanceError> {
    let home = paths::launcher_home(app).ok_or_else(|| {
        InstanceError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    let dir = instances::instance_dir(app, id)?;
    let instance = instances::read_instance(&dir)?.ok_or_else(|| InstanceError::not_found(id))?;
    state.begin(id)?;
    let _guard = DownloadGuard(state);

    let version_bytes = fetch_version_json_cached(app, &instance.version_id)?;
    let version_json = parse_version_json(&version_bytes)?;
    let java_info = version_json.java_version.as_ref().and_then(|java| {
        let component = java.component.clone()?;
        let major = java.major_version?;
        Some((component, major))
    });

    let fabric_profile = match instance.loader {
        Loader::Vanilla => None,
        Loader::Forge => None,
        Loader::Fabric => {
            let loader_version = instance
                .loader_version
                .as_deref()
                .ok_or_else(|| InstanceError::download("не указана версия Fabric"))?;
            let url = fabric::profile_url(&instance.version_id, loader_version);
            let bytes = mcmeta::http_get_bytes(&url)?;
            let profile = fabric::parse_profile(&bytes)?;
            let profile_path = dir.join("fabric.json");
            fs::write(&profile_path, serde_json::to_vec_pretty(&profile)?)?;
            Some(profile)
        }
    };

    // For Forge the installer-generated profile is only read when it is
    // already in the shared home caches (no installer run here); its
    // libraries then join the plan and the early-completion check.
    let forge_build = match instance.loader {
        Loader::Forge => instance
            .loader_version
            .clone()
            .ok_or_else(|| InstanceError::download("не указана версия Forge"))?,
        _ => String::new(),
    };
    let forge_raw: Option<Vec<u8>> = match instance.loader {
        Loader::Forge => forge::cached_profile(&home, &instance.version_id, &forge_build),
        _ => None,
    };
    let forge_overlay: Option<VersionJson> = forge_raw
        .as_deref()
        .and_then(|bytes| parse_version_json(bytes).ok());

    let mut plan = build_plan(&home, &instance, &version_json, fabric_profile.as_ref())?;
    if let Some(overlay) = &forge_overlay {
        plan.libraries.extend(library_file_plans(overlay, &home));
    }
    let emitter = ProgressEmitter::new(app, id);

    if state.cancelled() {
        return Err(InstanceError::Cancelled);
    }

    // Asset index (cached on disk under assets/indexes/).
    let index_bytes = fetch_asset_index_bytes(&home, &plan)?;
    let index = parse_asset_index(&index_bytes)?;
    let asset_plans: Vec<FilePlan> = index
        .objects
        .values()
        .map(|object| {
            let prefix = &object.hash[..2];
            FilePlan {
                url: format!("{RESOURCES_BASE}/{prefix}/{}", object.hash),
                dest: home
                    .join("assets")
                    .join("objects")
                    .join(prefix)
                    .join(&object.hash),
                sha1: Some(object.hash.clone()),
                size: object.size,
            }
        })
        .collect();
    let assets_total = asset_plans.len() as u32;
    let assets_bytes_total = if plan.assets_total_bytes > 0 {
        plan.assets_total_bytes
    } else {
        asset_plans.iter().map(|plan| plan.size).sum()
    };

    // Everything for this version is already on disk (caches are shared
    // between instances): skip the download entirely. Forge instances also
    // require the installer-generated profile to exist in the home cache.
    let forge_ready = instance.loader != Loader::Forge || forge_overlay.is_some();
    if forge_ready && download_already_complete(&plan.jar, &plan.libraries, &asset_plans) {
        instances::mark_downloaded(app, id, true)?;
        if instance.loader == Loader::Forge {
            let forge_path = dir.join("forge.json");
            if !forge_path.is_file() {
                if let Some(raw) = &forge_raw {
                    fs::write(&forge_path, raw)?;
                }
            }
        }
        if let Some((component, major)) = java_info {
            instances::set_java_info(app, id, &component, major)?;
        }
        return Ok(());
    }

    // Client jar
    let jar_total = plan.jar.size;
    download_one(&plan.jar)?;
    emitter.emit("jar", 1, 1, jar_total, jar_total);

    if state.cancelled() {
        return Err(InstanceError::Cancelled);
    }

    // Libraries (sequential: a handful of mid-size files)
    let libs_total = plan.libraries.len() as u32;
    let libs_bytes_total: u64 = plan.libraries.iter().map(|plan| plan.size).sum();
    let mut libs_done: u32 = 0;
    let mut libs_bytes: u64 = 0;
    for plan in &plan.libraries {
        if state.cancelled() {
            return Err(InstanceError::Cancelled);
        }
        download_one(plan)?;
        libs_done += 1;
        libs_bytes += plan.size;
        emitter.emit_throttled("libraries", libs_done, libs_total, libs_bytes, libs_bytes_total);
    }
    emitter.emit("libraries", libs_done, libs_total, libs_bytes, libs_bytes_total);

    if state.cancelled() {
        return Err(InstanceError::Cancelled);
    }

    // Assets
    let done = AtomicU32::new(0);
    let bytes_done = AtomicU64::new(0);
    let first_error: Mutex<Option<InstanceError>> = Mutex::new(None);
    let cancelled = AtomicBool::new(false);

    std::thread::scope(|scope| {
        let emitter = &emitter;
        let done = &done;
        let bytes_done = &bytes_done;
        let first_error = &first_error;
        let cancelled = &cancelled;
        let chunk_size = asset_plans.len().div_ceil(ASSET_THREADS).max(1);
        for chunk in asset_plans.chunks(chunk_size) {
            scope.spawn(move || {
                for plan in chunk {
                    if state.cancelled() {
                        cancelled.store(true, Ordering::Relaxed);
                        return;
                    }
                    match download_one(plan) {
                        Ok(()) => {
                            done.fetch_add(1, Ordering::Relaxed);
                            bytes_done.fetch_add(plan.size, Ordering::Relaxed);
                            emitter.emit_throttled(
                                "assets",
                                done.load(Ordering::Relaxed),
                                assets_total,
                                bytes_done.load(Ordering::Relaxed),
                                assets_bytes_total,
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

    if cancelled.load(Ordering::Relaxed) {
        return Err(InstanceError::Cancelled);
    }
    if let Some(error) = first_error.into_inner().unwrap() {
        return Err(error);
    }
    emitter.emit(
        "assets",
        done.load(Ordering::Relaxed),
        assets_total,
        bytes_done.load(Ordering::Relaxed),
        assets_bytes_total,
    );

    if instance.loader == Loader::Forge {
        run_forge_stage(app, state, &emitter, &home, &dir, &instance, &forge_build)?;
    }

    instances::mark_downloaded(app, id, true)?;
    if let Some((component, major)) = java_info {
        instances::set_java_info(app, id, &component, major)?;
    }
    Ok(())
}

/// Installs Forge into the shared launcher home (running the official
/// installer when needed), stores its generated profile per instance and
/// downloads/verifies the overlay libraries that have not landed on disk yet.
fn run_forge_stage(
    app: &AppHandle,
    state: &InstanceState,
    emitter: &ProgressEmitter,
    home: &Path,
    dir: &Path,
    instance: &Instance,
    build: &str,
) -> Result<(), InstanceError> {
    emitter.emit("forge", 0, 1, 0, 0);
    let bytes = forge::ensure_profile(app, home, &instance.version_id, build)?;
    let overlay = parse_version_json(&bytes)?;
    fs::write(dir.join("forge.json"), &bytes)?;
    let plans = library_file_plans(&overlay, home);
    let total = plans.len() as u32;
    let mut done: u32 = 0;
    for lib in &plans {
        if state.cancelled() {
            return Err(InstanceError::Cancelled);
        }
        download_one(lib)?;
        done += 1;
        emitter.emit_throttled("forge", done, total, 0, 0);
    }
    emitter.emit("forge", total, total, 0, 0);
    Ok(())
}

#[tauri::command]
pub async fn instance_download(app: AppHandle, id: String) -> Result<(), InstanceError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<InstanceState>();
        run_download(&app, &state, &id)
    })
    .await
    .map_err(|error| InstanceError::storage(error.to_string()))?
}

#[tauri::command]
pub fn instance_cancel_download(
    state: State<'_, InstanceState>,
    id: String,
) -> Result<(), InstanceError> {
    state.request_cancel(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_allowed_no_rules_means_allowed() {
        assert!(rules_allowed(&[]));
    }

    #[test]
    fn library_allowed_windows_only_rule() {
        let rules = vec![Rule {
            action: "allow".to_owned(),
            os: Some(OsRule {
                name: Some("windows".to_owned()),
            }),
            features: None,
        }];
        assert!(rules_allowed(&rules));
        let linux = vec![Rule {
            action: "allow".to_owned(),
            os: Some(OsRule {
                name: Some("linux".to_owned()),
            }),
            features: None,
        }];
        assert!(!rules_allowed(&linux));
    }

    #[test]
    fn rules_allowed_ignores_feature_rules() {
        let rules = vec![Rule {
            action: "disallow".to_owned(),
            os: None,
            features: Some(serde_json::json!({ "is_demo_user": true })),
        }];
        assert!(!rules_allowed(&rules));
    }

    #[test]
    fn library_allowed_last_matching_rule_wins() {
        let rules = vec![
            Rule {
                action: "deny".to_owned(),
                os: None,
                features: None,
            },
            Rule {
                action: "allow".to_owned(),
                os: Some(OsRule {
                    name: Some("windows".to_owned()),
                }),
                features: None,
            },
        ];
        assert!(rules_allowed(&rules));
    }

    #[test]
    fn url_file_path_extracts_repo_path() {
        assert_eq!(
            url_file_path("https://libraries.minecraft.net/org/ow2/asm/asm/9.6/asm-9.6.jar")
                .as_deref(),
            Some("org/ow2/asm/asm/9.6/asm-9.6.jar")
        );
        assert_eq!(
            url_file_path("https://example.com").is_none(),
            true
        );
    }

    #[test]
    fn sha1_hex_known_vector() {
        assert_eq!(
            sha1_hex(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    const SAMPLE_VERSION_JSON: &str = r#"{
        "id": "1.20.1",
        "mainClass": "net.minecraft.client.main.Main",
        "downloads": {
            "client": {
                "url": "https://piston-data.mojang.com/v1/objects/aa/client.jar",
                "sha1": "aaaa",
                "size": 12345
            }
        },
        "assetIndex": {
            "id": "8",
            "url": "https://piston-meta.mojang.com/v1/packages/bb/8.json",
            "sha1": "bbbb",
            "totalSize": 370000000,
            "size": 400000
        },
        "javaVersion": {
            "component": "java-runtime-gamma",
            "majorVersion": 17
        },
        "libraries": [
            {
                "name": "com.mojang:brigadier:1.0.18",
                "downloads": {
                    "artifact": {
                        "url": "https://libraries.minecraft.net/com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar",
                        "sha1": "ccc",
                        "size": 1000
                    }
                }
            },
            {
                "name": "org.lwjgl:lwjgl:3.3.1",
                "rules": [{ "action": "allow", "os": { "name": "linux" } }],
                "downloads": {
                    "artifact": {
                        "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
                        "sha1": "ddd",
                        "size": 1000
                    }
                }
            },
            {
                "name": "org.lwjgl.lwjgl:lwjgl-platform:2.9.4",
                "natives": { "natives-windows": "natives-windows" },
                "downloads": {
                    "artifact": {
                        "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl-platform/2.9.4/lwjgl-platform-2.9.4.jar",
                        "sha1": "eee",
                        "size": 1000
                    },
                    "classifiers": {
                        "natives-windows": {
                            "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl-platform/2.9.4/lwjgl-platform-2.9.4-natives-windows.jar",
                            "sha1": "fff",
                            "size": 2000
                        }
                    }
                }
            }
        ]
    }"#;

    fn sample_instance() -> Instance {
        Instance {
            id: "inst".to_owned(),
            name: "Test".to_owned(),
            version_id: "1.20.1".to_owned(),
            loader: Loader::Vanilla,
            loader_version: None,
            created_at: 0,
            downloaded: false,
            java_component: None,
            java_major: None,
        }
    }

    #[test]
    fn parse_version_json_reads_sections() {
        let json = parse_version_json(SAMPLE_VERSION_JSON.as_bytes()).unwrap();
        assert_eq!(json.libraries.len(), 3);
        let client = json.downloads.client.unwrap();
        assert_eq!(client.sha1.as_deref(), Some("aaaa"));
        assert_eq!(client.size, Some(12345));
        let index = json.asset_index.unwrap();
        assert_eq!(index.total_size, 370_000_000);
        let java = json.java_version.unwrap();
        assert_eq!(java.component.as_deref(), Some("java-runtime-gamma"));
        assert_eq!(java.major_version, Some(17));
    }

    #[test]
    fn extract_java_version_from_bytes() {
        let extracted = extract_java_version(SAMPLE_VERSION_JSON.as_bytes()).unwrap();
        assert_eq!(
            extracted,
            Some(("java-runtime-gamma".to_owned(), 17))
        );
        assert_eq!(extract_java_version(br#"{ "id": "old" }"#).unwrap(), None);
    }

    #[test]
    fn build_plan_skips_non_windows_and_includes_natives() {
        let home = std::env::temp_dir().join("ligone-downloader-plan-test");
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let json = parse_version_json(SAMPLE_VERSION_JSON.as_bytes()).unwrap();
        let plan = build_plan(&home, &sample_instance(), &json, None).unwrap();
        assert_eq!(
            plan.jar.dest,
            home.join("versions").join("1.20.1").join("1.20.1.jar")
        );
        // brigadier + lwjgl-platform jar + natives-windows classifier; lwjgl (linux-only) excluded
        assert_eq!(plan.libraries.len(), 3);
        let urls: Vec<&str> = plan.libraries.iter().map(|plan| plan.url.as_str()).collect();
        assert!(urls.iter().any(|url| url.contains("brigadier")));
        assert!(urls.iter().any(|url| url.contains("natives-windows")));
        assert!(!urls.iter().any(|url| url.contains("lwjgl/3.3.1")));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn build_plan_appends_fabric_libraries() {
        let home = std::env::temp_dir().join("ligone-downloader-fabric-test");
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let json = parse_version_json(SAMPLE_VERSION_JSON.as_bytes()).unwrap();
        let profile: FabricProfile = serde_json::from_str(
            r#"{
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "libraries": [
                    { "name": "net.fabricmc:fabric-loader:0.15.11", "url": "https://maven.fabricmc.net/" }
                ]
            }"#,
        )
        .unwrap();
        let plan = build_plan(&home, &sample_instance(), &json, Some(&profile)).unwrap();
        assert_eq!(plan.libraries.len(), 4);
        assert!(plan
            .libraries
            .iter()
            .any(|plan| plan.url.ends_with("net/fabricmc/fabric-loader/0.15.11/fabric-loader-0.15.11.jar")));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn parse_asset_index_reads_objects() {
        let index = parse_asset_index(
            br#"{
                "objects": {
                    "minecraft/sounds/ambience/loop1.ogg": { "hash": "abcdef1234567890abcdef1234567890abcdef123", "size": 100 },
                    "pack.png": { "hash": "1111222233334444555566667777888899990000", "size": 200 }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(index.objects.len(), 2);
        let pack = index.objects.get("pack.png").unwrap();
        assert_eq!(pack.hash, "1111222233334444555566667777888899990000");
        assert_eq!(pack.size, 200);
    }

    #[test]
    fn verify_or_skip_valid_existing_file() {
        let dir = std::env::temp_dir().join("ligone-downloader-skip-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("file.jar");
        fs::write(&dest, b"abc").unwrap();
        let plan = FilePlan {
            url: "https://example.com/file.jar".to_owned(),
            dest: dest.clone(),
            sha1: Some("a9993e364706816aba3e25717850c26c9cd0d89d".to_owned()),
            size: 3,
        };
        assert!(verify_or_skip(&plan).unwrap());
        let plan_bad = FilePlan {
            sha1: Some("0000000000000000000000000000000000000000".to_owned()),
            ..plan
        };
        assert!(!verify_or_skip(&plan_bad).unwrap());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn download_one_skips_without_network_when_valid() {
        let dir = std::env::temp_dir().join("ligone-downloader-nonet-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("file.jar");
        fs::write(&dest, b"abc").unwrap();
        let plan = FilePlan {
            url: "https://invalid.invalid/file.jar".to_owned(),
            dest,
            sha1: Some("a9993e364706816aba3e25717850c26c9cd0d89d".to_owned()),
            size: 3,
        };
        assert!(download_one(&plan).is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

    fn sample_file_plan(dest: PathBuf) -> FilePlan {
        FilePlan {
            url: "https://invalid.invalid/file".to_owned(),
            dest,
            sha1: Some(ABC_SHA1.to_owned()),
            size: 3,
        }
    }

    #[test]
    fn download_already_complete_true_only_when_all_files_valid() {
        let dir = std::env::temp_dir().join("ligone-downloader-complete-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let jar = sample_file_plan(dir.join("client.jar"));
        let libs = vec![sample_file_plan(dir.join("lib.jar"))];
        let assets = vec![
            sample_file_plan(dir.join("objects/aa/aaaa")),
            sample_file_plan(dir.join("objects/bb/bbbb")),
        ];
        assert!(!download_already_complete(&jar, &libs, &assets));

        for file in [&jar.dest, &libs[0].dest, &assets[0].dest, &assets[1].dest] {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
        }
        fs::write(&jar.dest, b"abc").unwrap();
        fs::write(&libs[0].dest, b"abc").unwrap();
        // Asset present but corrupt → still needs a download.
        fs::write(&assets[0].dest, b"wrong").unwrap();
        assert!(!download_already_complete(&jar, &libs, &assets));

        fs::write(&assets[0].dest, b"abc").unwrap();
        assert!(!download_already_complete(&jar, &libs, &assets));

        fs::write(&assets[1].dest, b"abc").unwrap();
        assert!(download_already_complete(&jar, &libs, &assets));

        // A single missing file forces the download.
        fs::remove_file(&libs[0].dest).unwrap();
        assert!(!download_already_complete(&jar, &libs, &assets));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fetch_asset_index_reads_valid_disk_cache_without_network() {
        let dir = std::env::temp_dir().join("ligone-downloader-index-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("assets").join("indexes").join("8.json");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, br#"{"objects":{}}"#).unwrap();
        let plan = Plan {
            jar: sample_file_plan(dir.join("client.jar")),
            libraries: Vec::new(),
            asset_index_id: "8".to_owned(),
            asset_index_url: "https://invalid.invalid/8.json".to_owned(),
            asset_index_sha1: Some(sha1_hex(br#"{"objects":{}}"#)),
            assets_total_bytes: 0,
        };
        let bytes = fetch_asset_index_bytes(&dir, &plan).unwrap();
        assert_eq!(bytes, br#"{"objects":{}}"#);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fetch_asset_index_rejects_bad_ids() {
        let dir = std::env::temp_dir().join("ligone-downloader-index-bad-id-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for id in ["", "../evil", "a/b", "a\\b"] {
            let plan = Plan {
                jar: sample_file_plan(dir.join("client.jar")),
                libraries: Vec::new(),
                asset_index_id: id.to_owned(),
                asset_index_url: "https://invalid.invalid/8.json".to_owned(),
                asset_index_sha1: None,
                assets_total_bytes: 0,
            };
            assert!(fetch_asset_index_bytes(&dir, &plan).is_err(), "{id}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
