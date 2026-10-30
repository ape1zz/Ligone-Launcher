use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::auth::{self, Account};
use crate::downloader::{self, ArgValue, LibraryPlan, VersionJson};
use crate::fabric::{self, FabricProfile};
use crate::instances::{self, Instance, InstanceError, Loader};
use crate::jre;
use crate::mcmeta;
use crate::paths;
use crate::settings;

const RUN_STATE_EVENT: &str = "instance-run-state";
const AUTHLIB_API_URL: &str =
    "https://api.github.com/repos/yushijinhun/authlib-injector/releases/latest";
const AUTHLIB_FALLBACK_URL: &str = "https://github.com/yushijinhun/authlib-injector/releases/download/v1.2.8/authlib-injector-1.2.8.jar";
const ELY_AUTHLIB_BASE: &str = "https://authserver.ely.by/api/authlib-injector";
const LAUNCHER_NAME: &str = "Ligone Launcher";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Pids of running game processes keyed by instance id.
#[derive(Default)]
pub struct LaunchState {
    running: Mutex<BTreeMap<String, u32>>,
}

impl LaunchState {
    fn register(&self, id: &str, pid: u32) {
        self.running.lock().unwrap().insert(id.to_string(), pid);
    }

    fn unregister(&self, id: &str) {
        self.running.lock().unwrap().remove(id);
    }

    pub(crate) fn is_running(&self, id: &str) -> bool {
        self.running.lock().unwrap().contains_key(id)
    }

    fn running_ids(&self) -> Vec<String> {
        self.running.lock().unwrap().keys().cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub instance_id: String,
    pub running: bool,
    /// Set when the game exited with a non-zero code; shown in the UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[tauri::command]
pub async fn instance_launch(app: AppHandle, id: String) -> Result<u32, InstanceError> {
    tauri::async_runtime::spawn_blocking(move || launch(&app, &id))
        .await
        .map_err(|error| InstanceError::launch(error.to_string()))?
}

#[tauri::command]
pub fn instance_running(state: tauri::State<'_, LaunchState>) -> Vec<String> {
    state.running_ids()
}

fn launch(app: &AppHandle, id: &str) -> Result<u32, InstanceError> {
    let launch_state = app.state::<LaunchState>();
    if launch_state.is_running(id) {
        return Err(InstanceError::launch("сборка уже запущена"));
    }
    if app
        .state::<downloader::InstanceState>()
        .is_downloading(id)
    {
        return Err(InstanceError::busy(id));
    }

    let home = paths::launcher_home(app).ok_or_else(|| {
        InstanceError::storage("не удалось определить домашнюю папку лаунчера")
    })?;
    let dir = instances::instance_dir(app, id)?;
    let instance = instances::read_instance(&dir)?.ok_or_else(|| InstanceError::not_found(id))?;
    if !instance.downloaded {
        return Err(InstanceError::launch("сборка ещё не скачана"));
    }

    let version_bytes = downloader::fetch_version_json_cached(app, &instance.version_id)?;
    let version_json = downloader::parse_version_json(&version_bytes)?;
    let fabric_profile = load_fabric_profile(&dir, &instance)?;
    let forge_profile = load_forge_profile(&home, &dir, &instance)?;

    let account = auth::current_account(app)
        .map_err(|error| InstanceError::launch(error.to_string()))?;
    let account = auth::refresh_ely_if_expired(app, &account)
        .map_err(|error| InstanceError::launch(error.to_string()))?;

    let java = jre::resolve_launch_java(app)?;
    let settings = settings::load(app).map_err(|error| InstanceError::storage(error.to_string()))?;

    let natives_dir = dir.join("natives");
    extract_natives(&home, &version_json, &natives_dir)?;
    let classpath = build_classpath(
        &home,
        &version_json,
        fabric_profile.as_ref(),
        forge_profile.as_ref(),
        &instance,
    );
    let authlib_jar = ensure_authlib_injector(&home)?;
    let mut values = substitution_values(&home, &dir, &instance, &account, &version_json, &classpath);
    // Forge JVM args reference the merged version id (`-DignoreList` etc.),
    // not the vanilla one underlying the overridden profile.
    if instance.loader == Loader::Forge {
        if let Some(build) = instance
            .loader_version
            .as_deref()
            .filter(|build| !build.is_empty())
        {
            values.insert(
                "version_name",
                format!("{}-forge-{}", instance.version_id, build),
            );
        }
    }

    let main_class = main_class(&version_json, fabric_profile.as_ref(), forge_profile.as_ref())?;
    let jvm = build_jvm_args(
        &version_json,
        fabric_profile.as_ref(),
        forge_profile.as_ref(),
        &values,
        settings.ram_min_mb,
        settings.ram_max_mb,
        &natives_dir,
        &classpath,
        &authlib_jar,
    );
    let game = build_game_args(
        &version_json,
        fabric_profile.as_ref(),
        forge_profile.as_ref(),
        &values,
    );

    append_log_line(&dir, &format!("=== запуск {} ({}) ===", instance.name, instance.version_id));

    let mut command = Command::new(&java);
    command
        .args(&jvm)
        .arg(&main_class)
        .args(&game)
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|error| InstanceError::launch(format!("не удалось запустить процесс: {error}")))?;
    let pid = child.id();

    launch_state.register(id, pid);
    let _ = app.emit(
        RUN_STATE_EVENT,
        RunState {
            instance_id: id.to_string(),
            running: true,
            error: None,
        },
    );

    spawn_log_threads(&mut child, &dir);
    spawn_exit_watcher(app.clone(), child, id.to_string(), dir.join("launcher.log"));
    Ok(pid)
}

fn main_class(
    version_json: &VersionJson,
    fabric_profile: Option<&FabricProfile>,
    forge_profile: Option<&VersionJson>,
) -> Result<String, InstanceError> {
    if let Some(forge) = forge_profile {
        if let Some(main_class) = &forge.main_class {
            return Ok(main_class.clone());
        }
    }
    if let Some(profile) = fabric_profile {
        return Ok(profile.main_class.clone());
    }
    version_json
        .main_class
        .clone()
        .ok_or_else(|| InstanceError::launch("в описании версии нет mainClass"))
}

fn load_forge_profile(
    home: &Path,
    dir: &Path,
    instance: &Instance,
) -> Result<Option<VersionJson>, InstanceError> {
    if instance.loader != Loader::Forge {
        return Ok(None);
    }
    let bytes = match fs::read(dir.join("forge.json")) {
        Ok(bytes) => bytes,
        Err(_) => {
            let build = instance
                .loader_version
                .as_deref()
                .ok_or_else(|| InstanceError::launch("не указана версия Forge"))?;
            crate::forge::cached_profile(home, &instance.version_id, build).ok_or_else(|| {
                InstanceError::launch(
                    "сборка Forge не установлена — запустите скачивание сборки ещё раз",
                )
            })?
        }
    };
    downloader::parse_version_json(&bytes).map(Some)
}

fn load_fabric_profile(
    dir: &Path,
    instance: &Instance,
) -> Result<Option<FabricProfile>, InstanceError> {
    if instance.loader != Loader::Fabric {
        return Ok(None);
    }
    let path = dir.join("fabric.json");
    if let Ok(bytes) = fs::read(&path) {
        return serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| InstanceError::storage(format!("не удалось разобрать fabric.json: {error}")));
    }
    let loader_version = instance
        .loader_version
        .as_deref()
        .ok_or_else(|| InstanceError::launch("не указана версия Fabric"))?;
    let bytes = mcmeta::http_get_bytes(&fabric::profile_url(
        &instance.version_id,
        loader_version,
    ))?;
    let profile = fabric::parse_profile(&bytes)?;
    let _ = fs::write(&path, serde_json::to_vec_pretty(&profile)?);
    Ok(Some(profile))
}

fn ensure_authlib_injector(home: &Path) -> Result<PathBuf, InstanceError> {
    let jar = home.join("authlib-injector.jar");
    if jar.is_file() {
        return Ok(jar);
    }
    let bytes = match latest_authlib_jar_url().and_then(|url| mcmeta::http_get_bytes(&url)) {
        Ok(bytes) => bytes,
        Err(_) => mcmeta::http_get_bytes(AUTHLIB_FALLBACK_URL)?,
    };
    fs::write(&jar, &bytes)?;
    Ok(jar)
}

/// Resolves the jar asset URL of the latest authlib-injector release via the
/// GitHub API (release assets are named `authlib-injector-<version>.jar`).
fn latest_authlib_jar_url() -> Result<String, InstanceError> {
    let bytes = mcmeta::http_get_bytes(AUTHLIB_API_URL)?;
    pick_authlib_jar(&bytes)
}

fn pick_authlib_jar(bytes: &[u8]) -> Result<String, InstanceError> {
    let release: ReleaseAssets = serde_json::from_slice(bytes).map_err(|error| {
        InstanceError::launch(format!("не удалось разобрать ответ GitHub: {error}"))
    })?;
    release
        .assets
        .iter()
        .find(|asset| asset.name.starts_with("authlib-injector-") && asset.name.ends_with(".jar"))
        .map(|asset| asset.browser_download_url.clone())
        .ok_or_else(|| {
            InstanceError::launch("в последнем релизе authlib-injector нет jar-ассета")
        })
}

#[derive(Deserialize)]
struct ReleaseAssets {
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    #[serde(rename = "browser_download_url")]
    browser_download_url: String,
}

fn substitution_values(
    home: &Path,
    dir: &Path,
    instance: &Instance,
    account: &Account,
    version_json: &VersionJson,
    classpath: &str,
) -> BTreeMap<&'static str, String> {
    let access_token = account
        .ely
        .as_ref()
        .map(|ely| ely.access_token.clone())
        .unwrap_or_else(|| account.uuid.clone());
    let user_type = if account.ely.is_some() {
        "mojang"
    } else {
        "legacy"
    };
    let assets_index_name = version_json
        .asset_index
        .as_ref()
        .map(|index| index.id.clone())
        .unwrap_or_default();
    let mut values: BTreeMap<&'static str, String> = BTreeMap::new();
    values.insert("auth_player_name", account.username.clone());
    values.insert("auth_uuid", account.uuid.clone());
    values.insert("auth_access_token", access_token.clone());
    values.insert("auth_session", format!("token:{access_token}:{}", account.uuid));
    values.insert("auth_xuid", String::new());
    values.insert("auth_clientid", String::new());
    values.insert("clientid", String::new());
    values.insert("user_type", user_type.to_string());
    values.insert("version_name", instance.version_id.clone());
    values.insert(
        "version_type",
        version_json.version_type.clone().unwrap_or_default(),
    );
    values.insert("profile_name", instance.name.clone());
    values.insert("game_directory", dir.to_string_lossy().to_string());
    values.insert("game_dir", dir.to_string_lossy().to_string());
    values.insert("assets_root", home.join("assets").to_string_lossy().to_string());
    values.insert("assets_index_name", assets_index_name);
    values.insert(
        "library_directory",
        home.join("libraries").to_string_lossy().to_string(),
    );
    values.insert("classpath_separator", ";".to_string());
    values.insert("classpath", classpath.to_string());
    values.insert("natives_directory", dir.join("natives").to_string_lossy().to_string());
    values.insert("launcher_name", LAUNCHER_NAME.to_string());
    values.insert(
        "launcher_version",
        env!("CARGO_PKG_VERSION").to_string(),
    );
    values
}

fn substitute(arg: &str, values: &BTreeMap<&'static str, String>) -> String {
    let mut result = arg.to_string();
    for (key, value) in values {
        let pattern = format!("${{{key}}}");
        if result.contains(&pattern) {
            result = result.replace(&pattern, value);
        }
    }
    result
}

fn push_entry(args: &mut Vec<String>, entry: &downloader::ArgEntry, values: &BTreeMap<&'static str, String>) {
    match entry {
        downloader::ArgEntry::Plain(value) => args.push(substitute(value, values)),
        downloader::ArgEntry::Structured { rules, value } => {
            if !downloader::rules_allowed(rules) {
                return;
            }
            match value {
                ArgValue::Single(value) => args.push(substitute(value, values)),
                ArgValue::Multiple(items) => {
                    args.extend(items.iter().map(|value| substitute(value, values)));
                }
            }
        }
    }
}

fn build_jvm_args(
    version_json: &VersionJson,
    fabric_profile: Option<&FabricProfile>,
    forge_profile: Option<&VersionJson>,
    values: &BTreeMap<&'static str, String>,
    ram_min_mb: u32,
    ram_max_mb: u32,
    natives_dir: &Path,
    classpath: &str,
    authlib_jar: &Path,
) -> Vec<String> {
    let mut args = vec![
        format!("-Xms{ram_min_mb}M"),
        format!("-Xmx{ram_max_mb}M"),
    ];
    if let Some(arguments) = &version_json.arguments {
        for entry in &arguments.jvm {
            push_entry(&mut args, entry, values);
        }
    }
    if let Some(overlay) = forge_profile {
        if let Some(arguments) = &overlay.arguments {
            for entry in &arguments.jvm {
                push_entry(&mut args, entry, values);
            }
        }
    }
    if let Some(profile) = fabric_profile {
        if let Some(arguments) = &profile.arguments {
            for value in arguments.flat_jvm() {
                args.push(substitute(&value, values));
            }
        }
    }
    if !args
        .iter()
        .any(|arg| arg.starts_with("-Djava.library.path="))
    {
        args.push(format!(
            "-Djava.library.path={}",
            natives_dir.to_string_lossy()
        ));
    }
    if !args.iter().any(|arg| arg == "-cp" || arg == "-classpath") {
        args.push("-cp".to_string());
        args.push(classpath.to_string());
    }
    args.push(format!(
        "-javaagent:{}={ELY_AUTHLIB_BASE}",
        authlib_jar.to_string_lossy()
    ));
    args
}

fn build_game_args(
    version_json: &VersionJson,
    fabric_profile: Option<&FabricProfile>,
    forge_profile: Option<&VersionJson>,
    values: &BTreeMap<&'static str, String>,
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(arguments) = &version_json.arguments {
        for entry in &arguments.game {
            push_entry(&mut args, entry, values);
        }
    } else if let Some(legacy) = &version_json.minecraft_arguments {
        args.extend(
            legacy
                .split_whitespace()
                .map(|arg| substitute(arg, values)),
        );
    }
    if let Some(overlay) = forge_profile {
        if let Some(arguments) = &overlay.arguments {
            for entry in &arguments.game {
                push_entry(&mut args, entry, values);
            }
        }
    }
    if let Some(profile) = fabric_profile {
        if let Some(arguments) = &profile.arguments {
            for value in arguments.flat_game() {
                args.push(substitute(&value, values));
            }
        }
    }
    args
}

fn build_classpath(
    home: &Path,
    version_json: &VersionJson,
    fabric_profile: Option<&FabricProfile>,
    forge_profile: Option<&VersionJson>,
    instance: &Instance,
) -> String {
    let mut entries = vec![home
        .join("versions")
        .join(&instance.version_id)
        .join(format!("{}.jar", instance.version_id))];
    for plan in downloader::library_plans(version_json, home) {
        if !plan.natives {
            entries.push(plan.dest);
        }
    }
    if let Some(overlay) = forge_profile {
        for plan in downloader::library_plans(overlay, home) {
            if !plan.natives {
                entries.push(plan.dest);
            }
        }
    }
    if let Some(profile) = fabric_profile {
        for library in &profile.libraries {
            if let Some(relative) = fabric::maven_relative_path(&library.name) {
                entries.push(home.join("libraries").join(relative));
            }
        }
    }
    entries
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(";")
}

/// Extracts native libraries (`natives-windows` classifier jars) into a fresh
/// per-launch directory; duplicate file names are flattened.
fn extract_natives(
    home: &Path,
    version_json: &VersionJson,
    natives_dir: &Path,
) -> Result<(), InstanceError> {
    let plans: Vec<LibraryPlan> = downloader::library_plans(version_json, home)
        .into_iter()
        .filter(|plan| plan.natives)
        .collect();
    if plans.is_empty() {
        return Ok(());
    }
    if natives_dir.exists() {
        fs::remove_dir_all(natives_dir)?;
    }
    fs::create_dir_all(natives_dir)?;
    for plan in plans {
        if !plan.dest.is_file() {
            continue;
        }
        let file = fs::File::open(&plan.dest)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|error| InstanceError::launch(format!("не удалось открыть natives: {error}")))?;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| InstanceError::launch(format!("повреждён natives-архив: {error}")))?;
            let name = entry.name().to_string();
            if !(name.ends_with(".dll") || name.ends_with(".so") || name.ends_with(".jnilib")) {
                continue;
            }
            let file_name = name.rsplit('/').next().unwrap_or(&name).to_string();
            if file_name.is_empty() {
                continue;
            }
            let mut out = fs::File::create(natives_dir.join(&file_name))?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    Ok(())
}

fn append_log_line(dir: &Path, line: &str) {
    let log = dir.join("launcher.log");
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log) {
        let _ = writeln!(file, "{line}");
    }
}

fn spawn_log_threads(child: &mut Child, dir: &Path) {
    let log = dir.join("launcher.log");
    if let Some(stdout) = child.stdout.take() {
        let log = log.clone();
        thread::spawn(move || pipe_to_log(stdout, log));
    }
    if let Some(stderr) = child.stderr.take() {
        thread::spawn(move || pipe_to_log(stderr, log));
    }
}

fn pipe_to_log<R: std::io::Read>(stream: R, log: PathBuf) {
    let reader = BufReader::new(stream);
    for line in reader.lines().map_while(Result::ok) {
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(file, "{line}");
        }
    }
}

fn spawn_exit_watcher(app: AppHandle, mut child: Child, instance_id: String, log: PathBuf) {
    thread::spawn(move || {
        let status = child.wait();
        let state = app.state::<LaunchState>();
        state.unregister(&instance_id);
        let failure = status
            .as_ref()
            .ok()
            .and_then(|status| status.code())
            .filter(|code| *code != 0)
            .map(|code| crash_message(code, &log));
        let _ = app.emit(
            RUN_STATE_EVENT,
            RunState {
                instance_id: instance_id.clone(),
                running: false,
                error: failure,
            },
        );
        if let Ok(status) = status {
            if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log) {
                let _ = writeln!(file, "процесс завершился: {status}");
            }
        }
    });
}

/// Builds the crash notice shown in the UI: exit code plus the last
/// meaningful lines of the game log.
fn crash_message(exit_code: i32, log: &Path) -> String {
    let tail = log_tail(log, 3);
    if tail.is_empty() {
        format!("Игра завершилась с ошибкой (код {exit_code}); подробности в launcher.log")
    } else {
        format!(
            "Игра завершилась с ошибкой (код {exit_code}): {}",
            tail.join(" | ")
        )
    }
}

fn log_tail(log: &Path, max_lines: usize) -> Vec<String> {
    let Ok(content) = fs::read_to_string(log) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|line| {
            !line.trim().is_empty()
                && !line.starts_with("===")
                && !line.starts_with("процесс завершился")
        })
        .map(|line| line.chars().take(200).collect::<String>())
        .rev()
        .take(max_lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODERN_JSON: &str = r#"{
        "mainClass": "net.minecraft.client.main.Main",
        "arguments": {
            "game": [
                "--username ${auth_player_name}",
                "--version ${version_name}",
                { "rules": [{ "action": "disallow", "os": { "name": "windows" } }], "value": "--linux-only" },
                { "rules": [{ "action": "allow" }], "value": ["--demo", "${auth_player_name}"] }
            ],
            "jvm": [
                { "rules": [{ "action": "allow", "os": { "name": "windows" } }], "value": "-Dos.name=Windows" },
                "-Djava.library.path=${natives_directory}",
                "-cp",
                "${classpath}"
            ]
        },
        "assetIndex": { "id": "5", "url": "https://example.com/5.json" }
    }"#;

    const LEGACY_JSON: &str = r#"{
        "mainClass": "net.minecraft.client.main.Main",
        "minecraftArguments": "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory}",
        "assetIndex": { "id": "legacy", "url": "https://example.com/legacy.json" }
    }"#;

    fn sample_instance() -> Instance {
        Instance {
            id: "abc".to_string(),
            name: "Test".to_string(),
            version_id: "1.20.1".to_string(),
            loader: Loader::Vanilla,
            loader_version: None,
            created_at: 0,
            downloaded: true,
            java_component: None,
            java_major: None,
        }
    }

    fn sample_account() -> Account {
        Account {
            username: "Steve".to_string(),
            uuid: "uuid-1".to_string(),
            is_offline: true,
            logged_in_at: 0,
            ely: None,
        }
    }

    #[test]
    fn pick_authlib_jar_selects_versioned_jar() {
        let response = r#"{
            "tag_name": "v1.2.8",
            "assets": [
                {
                    "name": "authlib-injector-1.2.8.jar",
                    "browser_download_url": "https://github.com/yushijinhun/authlib-injector/releases/download/v1.2.8/authlib-injector-1.2.8.jar"
                },
                {
                    "name": "authlib-injector-1.2.8.jar.sha256",
                    "browser_download_url": "https://github.com/yushijinhun/authlib-injector/releases/download/v1.2.8/authlib-injector-1.2.8.jar.sha256"
                },
                {
                    "name": "source.tar.gz",
                    "browser_download_url": "https://example.com/source.tar.gz"
                }
            ]
        }"#;
        assert_eq!(
            pick_authlib_jar(response.as_bytes()).unwrap(),
            "https://github.com/yushijinhun/authlib-injector/releases/download/v1.2.8/authlib-injector-1.2.8.jar"
        );
    }

    #[test]
    fn pick_authlib_jar_errors_without_jar_asset() {
        assert!(pick_authlib_jar(br#"{ "assets": [] }"#).is_err());
        assert!(pick_authlib_jar(br#"not json"#).is_err());
    }

    #[test]
    fn substitute_replaces_known_vars() {
        let mut values = BTreeMap::new();
        values.insert("auth_player_name", "Steve".to_string());
        values.insert("version_name", "1.20.1".to_string());
        assert_eq!(
            substitute("--username ${auth_player_name} v=${version_name}", &values),
            "--username Steve v=1.20.1"
        );
        assert_eq!(substitute("plain", &values), "plain");
    }

    #[test]
    fn build_game_args_modern_filters_rules() {
        let json: VersionJson = serde_json::from_str(MODERN_JSON).unwrap();
        let values = BTreeMap::new();
        let args = build_game_args(&json, None, None, &values);
        assert!(args.contains(&"--username ${auth_player_name}".to_string()));
        assert!(!args.contains(&"--linux-only".to_string()));
        assert!(args.contains(&"--demo".to_string()));
    }

    #[test]
    fn build_game_args_legacy_splits() {
        let json: VersionJson = serde_json::from_str(LEGACY_JSON).unwrap();
        let mut values = BTreeMap::new();
        values.insert("auth_player_name", "Steve".to_string());
        values.insert("game_directory", "C:\\game".to_string());
        let args = build_game_args(&json, None, None, &values);
        assert_eq!(
            args,
            vec![
                "--username",
                "Steve",
                "--version",
                "${version_name}",
                "--gameDir",
                "C:\\game"
            ]
        );
    }

    #[test]
    fn build_jvm_args_appends_classpath_and_agent() {
        let json: VersionJson = serde_json::from_str(MODERN_JSON).unwrap();
        let mut values = BTreeMap::new();
        values.insert("classpath", "C:\\cp".to_string());
        let args = build_jvm_args(
            &json,
            None,
            None,
            &values,
            1024,
            4096,
            Path::new("C:\\natives"),
            "C:\\cp",
            Path::new("C:\\agent.jar"),
        );
        assert!(args.contains(&"-Xms1024M".to_string()));
        assert!(args.contains(&"-Xmx4096M".to_string()));
        assert!(args.contains(&"-Dos.name=Windows".to_string()));
        assert!(args.contains(&"C:\\cp".to_string()));
        assert!(!args.iter().any(|arg| arg.contains("${classpath}")));
        assert!(args
            .iter()
            .any(|arg| arg.starts_with("-javaagent:C:\\agent.jar=")));
    }

    #[test]
    fn build_jvm_args_legacy_adds_natives_and_cp() {
        let json: VersionJson = serde_json::from_str(LEGACY_JSON).unwrap();
        let values = BTreeMap::new();
        let args = build_jvm_args(
            &json,
            None,
            None,
            &values,
            1024,
            1024,
            Path::new("N"),
            "CP",
            Path::new("A"),
        );
        assert!(args.contains(&"-Djava.library.path=N".to_string()));
        let cp_pos = args.iter().position(|arg| arg == "-cp").unwrap();
        assert_eq!(args[cp_pos + 1], "CP");
    }

    #[test]
    fn substitution_values_fills_offline_account() {
        let json: VersionJson = serde_json::from_str(LEGACY_JSON).unwrap();
        let home = Path::new("H");
        let dir = Path::new("I");
        let values = substitution_values(home, dir, &sample_instance(), &sample_account(), &json, "CP");
        assert_eq!(values["auth_player_name"], "Steve");
        assert_eq!(values["auth_uuid"], "uuid-1");
        assert_eq!(values["auth_access_token"], "uuid-1");
        assert_eq!(values["user_type"], "legacy");
        assert_eq!(values["assets_index_name"], "legacy");
        assert_eq!(values["classpath_separator"], ";");
        assert_eq!(values["classpath"], "CP");
    }

    #[test]
    fn substitution_values_uses_ely_token() {
        let json: VersionJson = serde_json::from_str(LEGACY_JSON).unwrap();
        let mut account = sample_account();
        account.is_offline = false;
        account.ely = Some(serde_json::from_str(r#"{"accessToken":"tok"}"#).unwrap());
        let values = substitution_values(Path::new("H"), Path::new("I"), &sample_instance(), &account, &json, "CP");
        assert_eq!(values["auth_access_token"], "tok");
        assert_eq!(values["user_type"], "mojang");
    }

    #[test]
    fn crash_message_includes_log_tail() {
        let dir = std::env::temp_dir().join(format!("ligone-launcher-crash-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("launcher.log");
        fs::write(
            &log,
            "=== запуск ===\n\nError: Could not find or load main class net.minecraft.client.main.Main\nCaused by: java.lang.ClassNotFoundException\nпроцесс завершился: exit code: 1\n",
        )
        .unwrap();
        let message = crash_message(1, &log);
        assert!(message.contains("код 1"));
        assert!(message.contains("ClassNotFoundException"));
        assert!(!message.contains("запуск"));
        assert!(!message.contains("процесс завершился"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn crash_message_without_log_mentions_log_file() {
        let message = crash_message(1, Path::new("Z:/missing/launcher.log"));
        assert!(message.contains("код 1"));
        assert!(message.contains("launcher.log"));
    }

    #[test]
    fn extract_natives_pulls_dlls_flattened() {
        let base = std::env::temp_dir().join(format!("ligone-natives-test-{}", std::process::id()));
        let natives_dir = base.join("natives");
        let jar_path = base
            .join("libraries")
            .join("org")
            .join("lwjgl")
            .join("lwjgl")
            .join("3.3.1")
            .join("lwjgl-3.3.1-natives-windows.jar");
        fs::create_dir_all(jar_path.parent().unwrap()).unwrap();
        let file = fs::File::create(&jar_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("org/lwjgl/natives/lwjgl.dll", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, b"dll-bytes").unwrap();
        writer
            .start_file("META-INF/MANIFEST.MF", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, b"manifest").unwrap();
        writer.finish().unwrap();

        let json: VersionJson = serde_json::from_str(
            r#"{
            "libraries": [
                {
                    "name": "org.lwjgl:lwjgl:3.3.1",
                    "downloads": {
                        "artifact": { "url": "https://example.com/lwjgl.jar", "size": 1 },
                        "classifiers": {
                            "natives-windows": { "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar", "size": 2 }
                        }
                    }
                }
            ]
        }"#,
        )
        .unwrap();
        extract_natives(&base, &json, &natives_dir).unwrap();
        let extracted = fs::read(natives_dir.join("lwjgl.dll")).unwrap();
        assert_eq!(extracted, b"dll-bytes");
        assert!(!natives_dir.join("MANIFEST.MF").exists());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn build_classpath_joins_with_semicolon() {
        let json: VersionJson = serde_json::from_str(LEGACY_JSON).unwrap();
        let home = Path::new("H");
        let instance = sample_instance();
        let classpath = build_classpath(home, &json, None, None, &instance);
        assert!(classpath.starts_with("H\\versions\\1.20.1\\1.20.1.jar"));
    }
}
