use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tauri::AppHandle;

use crate::instances::{self, Instance, Loader};
use crate::mcmeta;
use crate::paths;

const MODRINTH_API_BASE: &str = "https://api.modrinth.com/v2";
const CURSEFORGE_API_BASE: &str = "https://api.curseforge.com/v1";
/// CurseForge game id for Minecraft.
const CURSEFORGE_GAME_ID: u32 = 432;
/// CurseForge section ids: mods / resource packs / shader packs.
const CURSEFORGE_CLASS_MODS: u32 = 6;
const CURSEFORGE_CLASS_RESOURCEPACKS: u32 = 12;
const CURSEFORGE_CLASS_SHADERPACKS: u32 = 4546;
/// Placeholder, overridable via the `CURSEFORGE_API_KEY` env var
/// (the official CurseForge API requires a key, see docs).
const CURSEFORGE_API_KEY: &str = "";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentCategory {
    Mods,
    Resourcepacks,
    Shaderpacks,
}

impl ContentCategory {
    fn dir_name(self) -> &'static str {
        match self {
            Self::Mods => "mods",
            Self::Resourcepacks => "resourcepacks",
            Self::Shaderpacks => "shaderpacks",
        }
    }

    fn modrinth_project_type(self) -> &'static str {
        match self {
            Self::Mods => "mod",
            Self::Resourcepacks => "resourcepack",
            Self::Shaderpacks => "shader",
        }
    }

    fn curseforge_class_id(self) -> u32 {
        match self {
            Self::Mods => CURSEFORGE_CLASS_MODS,
            Self::Resourcepacks => CURSEFORGE_CLASS_RESOURCEPACKS,
            Self::Shaderpacks => CURSEFORGE_CLASS_SHADERPACKS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentSource {
    Modrinth,
    Curseforge,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentItem {
    pub source: ContentSource,
    pub project_id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    pub icon_url: Option<String>,
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentVersion {
    pub source: ContentSource,
    pub version_id: String,
    pub name: String,
    pub file_name: String,
    pub file_url: String,
    pub file_size: u64,
    pub sha1: Option<String>,
    pub date_published: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledContent {
    pub file_name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentError {
    Storage { message: String },
    NotFound { id: String },
    Network { message: String },
    Api { message: String },
}

impl ContentError {
    fn storage(message: impl Into<String>) -> Self {
        Self::Storage {
            message: message.into(),
        }
    }

    fn not_found(id: impl Into<String>) -> Self {
        Self::NotFound { id: id.into() }
    }

    fn network(message: impl Into<String>) -> Self {
        Self::Network {
            message: message.into(),
        }
    }

    fn api(message: impl Into<String>) -> Self {
        Self::Api {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ContentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage { message } => write!(formatter, "ошибка хранилища: {message}"),
            Self::NotFound { id } => write!(formatter, "не найдено: {id}"),
            Self::Network { message } => write!(formatter, "ошибка сети: {message}"),
            Self::Api { message } => write!(formatter, "ошибка API: {message}"),
        }
    }
}

impl std::error::Error for ContentError {}

impl From<std::io::Error> for ContentError {
    fn from(error: std::io::Error) -> Self {
        Self::storage(error.to_string())
    }
}

// ---------- HTTP helpers ----------

fn http_get(url: &str, api_key: Option<&str>) -> Result<Vec<u8>, ContentError> {
    let mut request = ureq::get(url).timeout(Duration::from_secs(60));
    if let Some(key) = api_key {
        request = request.set("x-api-key", key);
    }
    let response = request
        .call()
        .map_err(|error| ContentError::network(format!("не удалось скачать {url}: {error}")))?;
    read_body(url, response)
}

fn http_post_json(
    url: &str,
    body: &str,
    api_key: &str,
) -> Result<Vec<u8>, ContentError> {
    let response = ureq::post(url)
        .timeout(Duration::from_secs(60))
        .set("x-api-key", api_key)
        .send_string(body)
        .map_err(|error| ContentError::network(format!("не удалось отправить запрос {url}: {error}")))?;
    read_body(url, response)
}

fn read_body(
    url: &str,
    response: ureq::Response,
) -> Result<Vec<u8>, ContentError> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(512 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|error| ContentError::network(format!("не удалось дочитать {url}: {error}")))?;
    Ok(bytes)
}

/// Percent-encodes everything except unreserved URL characters.
fn url_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char);
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

// ---------- Modrinth ----------

#[derive(Debug, Deserialize)]
struct ModrinthSearch {
    #[serde(default)]
    hits: Vec<ModrinthHit>,
}

#[derive(Debug, Deserialize)]
struct ModrinthHit {
    project_id: String,
    #[serde(default)]
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthVersionRaw {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version_number: Option<String>,
    #[serde(default)]
    date_published: Option<String>,
    #[serde(default)]
    files: Vec<ModrinthFile>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    dependencies: Vec<ModrinthDependency>,
}

#[derive(Debug, Deserialize)]
struct ModrinthFile {
    url: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    hashes: Option<ModrinthHashes>,
}

#[derive(Debug, Deserialize)]
struct ModrinthHashes {
    #[serde(default)]
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthDependency {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    dependency_type: Option<String>,
}

/// Modrinth facet query: outer groups are AND'd, inner ones OR'd, so every
/// filter must live in its own group — a single mixed group would be an OR
/// and would match any project regardless of type or version.
fn modrinth_facets_json(
    game_version: &str,
    project_type: &str,
    loader: Option<&str>,
) -> Result<String, ContentError> {
    let mut facets = vec![
        vec![format!("versions:{game_version}")],
        vec![format!("project_type:{project_type}")],
    ];
    if let Some(loader) = loader {
        facets.push(vec![format!("categories:{loader}")]);
    }
    serde_json::to_string(&facets)
        .map_err(|error| ContentError::storage(error.to_string()))
}

fn modrinth_search(
    query: &str,
    category: ContentCategory,
    game_version: &str,
    loader: Option<&str>,
) -> Result<Vec<ContentItem>, ContentError> {
    let facets_json = modrinth_facets_json(game_version, category.modrinth_project_type(), loader)?;
    let url = format!(
        "{MODRINTH_API_BASE}/search?limit=20&index=downloads&query={query}&facets={facets}",
        query = url_encode(query),
        facets = url_encode(&facets_json),
    );
    let bytes = http_get(&url, None)?;
    let parsed: ModrinthSearch = serde_json::from_slice(&bytes)
        .map_err(|error| ContentError::api(format!("не удалось разобрать ответ Modrinth: {error}")))?;
    Ok(parsed
        .hits
        .into_iter()
        .map(|hit| ContentItem {
            source: ContentSource::Modrinth,
            project_id: hit.project_id,
            slug: hit.slug,
            name: hit.title,
            description: hit.description,
            author: hit.author,
            downloads: hit.downloads,
            icon_url: hit.icon_url,
            website_url: None,
        })
        .collect())
}

fn modrinth_project_versions(
    project_id: &str,
    game_version: &str,
    loader: Option<&str>,
) -> Result<Vec<ModrinthVersionRaw>, ContentError> {
    let game_versions = serde_json::to_string(&[game_version])
        .map_err(|error| ContentError::storage(error.to_string()))?;
    let mut url = format!(
        "{MODRINTH_API_BASE}/project/{project_id}/version?game_versions={}",
        url_encode(&game_versions)
    );
    if let Some(loader) = loader {
        let loaders = serde_json::to_string(&[loader])
            .map_err(|error| ContentError::storage(error.to_string()))?;
        url.push_str(&format!("&loaders={}", url_encode(&loaders)));
    }
    let bytes = http_get(&url, None)?;
    let mut versions: Vec<ModrinthVersionRaw> = serde_json::from_slice(&bytes)
        .map_err(|error| ContentError::api(format!("не удалось разобрать версии Modrinth: {error}")))?;
    // The API already filters, but keep a defensive client-side pass.
    versions.retain(|version| {
        version.game_versions.iter().any(|item| item == game_version)
            && loader.is_none_or(|wanted| {
                version
                    .loaders
                    .iter()
                    .any(|item| item.eq_ignore_ascii_case(wanted))
            })
    });
    Ok(versions)
}

fn modrinth_version_by_id(version_id: &str) -> Result<ModrinthVersionRaw, ContentError> {
    let bytes = http_get(&format!("{MODRINTH_API_BASE}/version/{version_id}"), None)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| ContentError::api(format!("не удалось разобрать версию Modrinth: {error}")))
}

impl ModrinthVersionRaw {
    fn primary_file(&self) -> Option<&ModrinthFile> {
        self.files
            .iter()
            .find(|file| file.primary)
            .or_else(|| self.files.first())
    }

    fn to_content_version(&self, source: ContentSource) -> Option<ContentVersion> {
        let file = self.primary_file()?;
        Some(ContentVersion {
            source,
            version_id: self.id.clone(),
            name: self
                .name
                .clone()
                .or_else(|| self.version_number.clone())
                .unwrap_or_else(|| self.id.clone()),
            file_name: file.filename.clone().unwrap_or_default(),
            file_url: file.url.clone(),
            file_size: file.size,
            sha1: file.hashes.as_ref().and_then(|hashes| hashes.sha1.clone()),
            date_published: self.date_published.clone(),
        })
    }

    fn required_dependency_ids(&self) -> Vec<String> {
        self.dependencies
            .iter()
            .filter(|dependency| {
                dependency
                    .dependency_type
                    .as_deref()
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("required"))
            })
            .filter_map(|dependency| dependency.project_id.clone())
            .collect()
    }
}

// ---------- CurseForge ----------

#[derive(Debug, Deserialize)]
struct CfSearch {
    #[serde(default)]
    data: Vec<CfMod>,
}

#[derive(Debug, Deserialize)]
struct CfMod {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default, rename = "downloadCount")]
    download_count: u64,
    #[serde(default)]
    authors: Vec<CfAuthor>,
    #[serde(default)]
    logo: Option<CfLogo>,
    #[serde(default)]
    links: Option<CfLinks>,
}

#[derive(Debug, Deserialize)]
struct CfAuthor {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CfLogo {
    #[serde(default, rename = "thumbnailUrl")]
    thumbnail_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CfLinks {
    #[serde(default, rename = "websiteUrl")]
    website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CfFiles {
    #[serde(default)]
    data: Vec<CfFile>,
}

#[derive(Debug, Deserialize)]
struct CfFile {
    id: u64,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
    #[serde(default, rename = "fileName")]
    file_name: Option<String>,
    #[serde(default, rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(default, rename = "fileSize")]
    file_size: u64,
    #[serde(default, rename = "fileDate")]
    file_date: Option<String>,
    #[serde(default, rename = "gameVersions")]
    game_versions: Vec<String>,
    #[serde(default)]
    relations: Option<CfRelations>,
}

#[derive(Debug, Deserialize)]
struct CfRelations {
    #[serde(default, rename = "type")]
    relation_type: Option<String>,
    #[serde(default)]
    ids: Vec<u64>,
}

fn curseforge_api_key() -> Result<String, ContentError> {
    let from_env = std::env::var("CURSEFORGE_API_KEY").ok();
    let key = from_env
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| CURSEFORGE_API_KEY.to_string());
    if key.trim().is_empty() {
        return Err(ContentError::api(
            "CurseForge API key не настроен — задайте CURSEFORGE_API_KEY",
        ));
    }
    Ok(key)
}

fn curseforge_search(
    query: &str,
    category: ContentCategory,
    game_version: &str,
) -> Result<Vec<ContentItem>, ContentError> {
    let key = curseforge_api_key()?;
    let body = serde_json::json!({
        "gameId": CURSEFORGE_GAME_ID,
        "classId": category.curseforge_class_id(),
        "searchFilter": query,
        "gameVersion": game_version,
        "sortField": 2,
        "sortOrder": "desc",
        "pageSize": 20,
        "index": 0,
    });
    let bytes = http_post_json(
        &format!("{CURSEFORGE_API_BASE}/mods/search"),
        &body.to_string(),
        &key,
    )?;
    let parsed: CfSearch = serde_json::from_slice(&bytes)
        .map_err(|error| ContentError::api(format!("не удалось разобрать ответ CurseForge: {error}")))?;
    Ok(parsed
        .data
        .into_iter()
        .map(|mod_item| ContentItem {
            source: ContentSource::Curseforge,
            project_id: mod_item.id.to_string(),
            slug: mod_item.slug.unwrap_or_default(),
            name: mod_item.name,
            description: mod_item.summary,
            author: mod_item
                .authors
                .first()
                .and_then(|author| author.name.clone())
                .unwrap_or_default(),
            downloads: mod_item.download_count,
            icon_url: mod_item.logo.and_then(|logo| logo.thumbnail_url),
            website_url: mod_item.links.and_then(|links| links.website_url),
        })
        .collect())
}

fn curseforge_files(mod_id: &str, game_version: Option<&str>) -> Result<Vec<CfFile>, ContentError> {
    let key = curseforge_api_key()?;
    let mut url = format!("{CURSEFORGE_API_BASE}/mods/{mod_id}/files");
    if let Some(game_version) = game_version {
        url.push_str(&format!("?gameVersion={}", url_encode(game_version)));
    }
    let bytes = http_get(&url, Some(&key))?;
    let parsed: CfFiles = serde_json::from_slice(&bytes)
        .map_err(|error| ContentError::api(format!("не удалось разобрать файлы CurseForge: {error}")))?;
    Ok(parsed.data)
}

fn curseforge_file_by_id(mod_id: &str, file_id: &str) -> Result<CfFile, ContentError> {
    let key = curseforge_api_key()?;
    let bytes = http_get(
        &format!("{CURSEFORGE_API_BASE}/mods/{mod_id}/file/{file_id}"),
        Some(&key),
    )?;
    serde_json::from_slice(&bytes)
        .map_err(|error| ContentError::api(format!("не удалось разобрать файл CurseForge: {error}")))
}

impl CfFile {
    fn matches_loader(&self, loader: Option<&str>) -> bool {
        let Some(wanted) = loader else {
            return true;
        };
        self.game_versions
            .iter()
            .any(|item| item.eq_ignore_ascii_case(wanted))
    }

    fn to_content_version(&self, source: ContentSource) -> Option<ContentVersion> {
        Some(ContentVersion {
            source,
            version_id: self.id.to_string(),
            name: self
                .display_name
                .clone()
                .or_else(|| self.file_name.clone())
                .unwrap_or_else(|| self.id.to_string()),
            file_name: self.file_name.clone().unwrap_or_default(),
            file_url: self.download_url.clone()?,
            file_size: self.file_size,
            sha1: None,
            date_published: self.file_date.clone(),
        })
    }
}

// ---------- Commands ----------

fn load_instance(app: &AppHandle, instance_id: &str) -> Result<Instance, ContentError> {
    let dir = instances::instance_dir(app, instance_id)
        .map_err(|error| ContentError::storage(error.to_string()))?;
    instances::read_instance(&dir)
        .map_err(|error| ContentError::storage(error.to_string()))?
        .ok_or_else(|| ContentError::not_found(instance_id))
}

fn category_dir(home: &Path, instance: &Instance, category: ContentCategory) -> PathBuf {
    instances::instance_dir_at(
        &home,
        &instance.id,
    )
    .join(category.dir_name())
}

/// Loader facet for content search: mods on loader instances are filtered by
/// the matching loader (`fabric`/`forge`); everything else searches without
/// a loader facet.
fn loader_filter(instance: &Instance, category: ContentCategory) -> Option<&'static str> {
    if category != ContentCategory::Mods {
        return None;
    }
    match instance.loader {
        Loader::Fabric => Some("fabric"),
        Loader::Forge => Some("forge"),
        Loader::Vanilla => None,
    }
}

#[tauri::command]
pub async fn content_search(
    app: AppHandle,
    instance_id: String,
    category: ContentCategory,
    query: String,
) -> Result<Vec<ContentItem>, ContentError> {
    tauri::async_runtime::spawn_blocking(move || search(&app, &instance_id, category, &query))
        .await
        .map_err(|error| ContentError::network(error.to_string()))?
}

fn search(
    app: &AppHandle,
    instance_id: &str,
    category: ContentCategory,
    query: &str,
) -> Result<Vec<ContentItem>, ContentError> {
    let instance = load_instance(app, instance_id)?;
    let loader = loader_filter(&instance, category);
    let mut items = Vec::new();
    let mut first_error: Option<ContentError> = None;
    match modrinth_search(query, category, &instance.version_id, loader) {
        Ok(found) => items.extend(found),
        Err(error) => {
            first_error.get_or_insert(error);
        }
    }
    match curseforge_search(query, category, &instance.version_id) {
        Ok(found) => items.extend(found),
        // A missing CurseForge key must not hide Modrinth results.
        Err(ContentError::Api { .. }) => {}
        Err(error) => {
            first_error.get_or_insert(error);
        }
    }
    if items.is_empty() {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    Ok(items)
}

#[tauri::command]
pub async fn content_versions(
    app: AppHandle,
    instance_id: String,
    category: ContentCategory,
    source: ContentSource,
    project_id: String,
) -> Result<Vec<ContentVersion>, ContentError> {
    tauri::async_runtime::spawn_blocking(move || {
        versions(&app, &instance_id, category, source, &project_id)
    })
    .await
    .map_err(|error| ContentError::network(error.to_string()))?
}

fn versions(
    app: &AppHandle,
    instance_id: &str,
    category: ContentCategory,
    source: ContentSource,
    project_id: &str,
) -> Result<Vec<ContentVersion>, ContentError> {
    let instance = load_instance(app, instance_id)?;
    let loader = loader_filter(&instance, category);
    match source {
        ContentSource::Modrinth => {
            let raw = modrinth_project_versions(project_id, &instance.version_id, loader)?;
            Ok(raw
                .iter()
                .filter_map(|version| version.to_content_version(source))
                .collect())
        }
        ContentSource::Curseforge => {
            let files = curseforge_files(project_id, Some(&instance.version_id))?;
            Ok(files
                .iter()
                .filter(|file| file.matches_loader(loader))
                .filter_map(|file| file.to_content_version(source))
                .collect())
        }
    }
}

#[tauri::command]
pub async fn content_install(
    app: AppHandle,
    instance_id: String,
    category: ContentCategory,
    source: ContentSource,
    project_id: String,
    version_id: String,
) -> Result<Vec<InstalledContent>, ContentError> {
    tauri::async_runtime::spawn_blocking(move || {
        install(&app, &instance_id, category, source, &project_id, &version_id)
    })
    .await
    .map_err(|error| ContentError::network(error.to_string()))?
}

fn install(
    app: &AppHandle,
    instance_id: &str,
    category: ContentCategory,
    source: ContentSource,
    project_id: &str,
    version_id: &str,
) -> Result<Vec<InstalledContent>, ContentError> {
    let home = paths::launcher_home(app)
        .ok_or_else(|| ContentError::storage("не удалось определить домашнюю папку лаунчера"))?;
    let instance = load_instance(app, instance_id)?;
    let dir = category_dir(&home, &instance, category);
    fs::create_dir_all(&dir)?;
    // Dependency resolution shares the category's loader filter (mods only).
    let loader = loader_filter(&instance, category);
    let game_version = instance.version_id.clone();
    let mut visited = HashSet::new();
    let mut installed = Vec::new();
    match source {
        ContentSource::Modrinth => {
            visited.insert(format!("modrinth:{project_id}"));
            let version = modrinth_version_by_id(version_id)?;
            install_modrinth(
                &version,
                &dir,
                &game_version,
                loader,
                &mut visited,
                &mut installed,
            )?;
        }
        ContentSource::Curseforge => {
            visited.insert(format!("curseforge:{project_id}"));
            let file = curseforge_file_by_id(project_id, version_id)?;
            install_curseforge(
                &file,
                &dir,
                &game_version,
                loader,
                &mut visited,
                &mut installed,
            )?;
        }
    }
    Ok(installed)
}

fn install_modrinth(
    version: &ModrinthVersionRaw,
    dir: &Path,
    game_version: &str,
    loader: Option<&str>,
    visited: &mut HashSet<String>,
    installed: &mut Vec<InstalledContent>,
) -> Result<(), ContentError> {
    let Some(file) = version.primary_file() else {
        return Err(ContentError::api("у версии нет файлов для скачивания"));
    };
    let file_name = file.filename.clone().unwrap_or_default();
    if file_name.is_empty() {
        return Err(ContentError::api("у файла нет имени"));
    }
    download_into_dir(&file.url, &file_name, file.hashes.as_ref().and_then(|h| h.sha1.clone()), dir)?;
    installed.push(InstalledContent {
        file_name,
        size_bytes: file.size,
    });

    for dependency_id in version.required_dependency_ids() {
        if !visited.insert(format!("modrinth:{dependency_id}")) {
            continue;
        }
        let dependency_versions =
            modrinth_project_versions(&dependency_id, game_version, loader)?;
        let Some(dependency) = dependency_versions.first() else {
            continue;
        };
        install_modrinth(dependency, dir, game_version, loader, visited, installed)?;
    }
    Ok(())
}

fn install_curseforge(
    file: &CfFile,
    dir: &Path,
    game_version: &str,
    loader: Option<&str>,
    visited: &mut HashSet<String>,
    installed: &mut Vec<InstalledContent>,
) -> Result<(), ContentError> {
    let download_url = file.download_url.clone().ok_or_else(|| {
        ContentError::api("CurseForge не предоставил ссылку на файл")
    })?;
    let file_name = file.file_name.clone().unwrap_or_default();
    if file_name.is_empty() {
        return Err(ContentError::api("у файла нет имени"));
    }
    download_into_dir(&download_url, &file_name, None, dir)?;
    installed.push(InstalledContent {
        file_name,
        size_bytes: file.file_size,
    });

    let Some(relations) = &file.relations else {
        return Ok(());
    };
    let required = relations
        .relation_type
        .as_deref()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("required"));
    if !required {
        return Ok(());
    }
    for dependency_id in &relations.ids {
        let key = format!("curseforge:{dependency_id}");
        if !visited.insert(key) {
            continue;
        }
        let files = curseforge_files(&dependency_id.to_string(), Some(game_version))?;
        let Some(dependency) = files
            .iter()
            .filter(|candidate| candidate.matches_loader(loader))
            .find(|candidate| candidate.download_url.is_some())
        else {
            continue;
        };
        install_curseforge(
            dependency,
            dir,
            game_version,
            loader,
            visited,
            installed,
        )?;
    }
    Ok(())
}

fn download_into_dir(
    url: &str,
    file_name: &str,
    sha1: Option<String>,
    dir: &Path,
) -> Result<(), ContentError> {
    let bytes = mcmeta::http_get_bytes(url)
        .map_err(|error| ContentError::network(error.to_string()))?;
    if let Some(expected) = sha1 {
        if sha1_hex(&bytes) != expected {
            return Err(ContentError::api(format!(
                "контрольная сумма не совпала: {file_name}"
            )));
        }
    }
    fs::write(dir.join(file_name), &bytes)?;
    Ok(())
}

#[tauri::command]
pub fn content_installed(
    app: AppHandle,
    instance_id: String,
    category: ContentCategory,
) -> Result<Vec<InstalledContent>, ContentError> {
    let home = paths::launcher_home(&app)
        .ok_or_else(|| ContentError::storage("не удалось определить домашнюю папку лаунчера"))?;
    let instance = load_instance(&app, &instance_id)?;
    let dir = category_dir(&home, &instance, category);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut installed = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        installed.push(InstalledContent {
            size_bytes: entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            file_name: name,
        });
    }
    installed.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(installed)
}

#[tauri::command]
pub async fn content_remove(
    app: AppHandle,
    instance_id: String,
    category: ContentCategory,
    file_name: String,
) -> Result<(), ContentError> {
    tauri::async_runtime::spawn_blocking(move || remove(&app, &instance_id, category, &file_name))
        .await
        .map_err(|error| ContentError::network(error.to_string()))?
}

fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && name != "."
}

fn remove(
    app: &AppHandle,
    instance_id: &str,
    category: ContentCategory,
    file_name: &str,
) -> Result<(), ContentError> {
    if !valid_file_name(file_name) {
        return Err(ContentError::storage(format!(
            "некорректное имя файла: {file_name}"
        )));
    }
    let home = paths::launcher_home(app)
        .ok_or_else(|| ContentError::storage("не удалось определить домашнюю папку лаунчера"))?;
    let instance = load_instance(app, instance_id)?;
    let dir = category_dir(&home, &instance, category);
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err(ContentError::not_found(file_name));
    }
    fs::remove_file(&path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODRINTH_SEARCH: &str = r#"{
        "hits": [
            {
                "project_id": "AABBCC",
                "slug": "sodium",
                "title": "Sodium",
                "description": "Modern rendering engine",
                "author": "jellysquid",
                "downloads": 1000,
                "icon_url": "https://cdn.modrinth.com/icon.png"
            }
        ]
    }"#;

    const MODRINTH_VERSIONS: &str = r#"[
        {
            "id": "ver2",
            "name": "Sodium 0.5.3",
            "version_number": "0.5.3",
            "date_published": "2024-01-02T00:00:00Z",
            "game_versions": ["1.20.1"],
            "loaders": ["fabric"],
            "files": [
                { "url": "https://cdn.modrinth.com/sodium.jar", "filename": "sodium-0.5.3.jar", "size": 100, "primary": true, "hashes": { "sha1": "abc" } }
            ],
            "dependencies": [
                { "project_id": "DEP1", "dependency_type": "required" },
                { "project_id": "DEP2", "dependency_type": "optional" }
            ]
        },
        {
            "id": "ver1",
            "version_number": "0.5.1",
            "game_versions": ["1.19.4"],
            "loaders": ["fabric"],
            "files": []
        }
    ]"#;

    const CF_SEARCH: &str = r#"{
        "data": [
            {
                "id": 123,
                "name": "JEI",
                "summary": "Just Enough Items",
                "slug": "jei",
                "downloadCount": 500,
                "authors": [{ "name": "mezz" }],
                "logo": { "thumbnailUrl": "https://media.cf.com/64.png" },
                "links": { "websiteUrl": "https://curseforge.com/jei" }
            }
        ]
    }"#;

    const CF_FILES: &str = r#"{
        "data": [
            {
                "id": 777,
                "displayName": "JEI 1.20.1",
                "fileName": "jei-1.20.1-fabric.jar",
                "downloadUrl": "https://media.cf.com/jei.jar",
                "fileSize": 2048,
                "fileDate": "2024-02-01T00:00:00.000Z",
                "gameVersions": ["1.20.1", "Fabric"],
                "relations": { "type": "required", "ids": [555] }
            },
            {
                "id": 556,
                "fileName": "jei-forge.jar",
                "downloadUrl": "https://media.cf.com/jei-forge.jar",
                "fileSize": 1,
                "gameVersions": ["1.20.1", "Forge"]
            }
        ]
    }"#;

    #[test]
    fn parse_modrinth_search() {
        let parsed: ModrinthSearch = serde_json::from_str(MODRINTH_SEARCH).unwrap();
        assert_eq!(parsed.hits.len(), 1);
        assert_eq!(parsed.hits[0].project_id, "AABBCC");
        assert_eq!(parsed.hits[0].title, "Sodium");
    }

    #[test]
    fn modrinth_versions_filtered_by_game_and_loader() {
        let mut versions: Vec<ModrinthVersionRaw> =
            serde_json::from_str(MODRINTH_VERSIONS).unwrap();
        versions.retain(|version| {
            version.game_versions.iter().any(|item| item == "1.20.1")
                && version
                    .loaders
                    .iter()
                    .any(|item| item.eq_ignore_ascii_case("fabric"))
        });
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "ver2");
        let content = versions[0].to_content_version(ContentSource::Modrinth).unwrap();
        assert_eq!(content.file_name, "sodium-0.5.3.jar");
        assert_eq!(content.sha1.as_deref(), Some("abc"));
        assert_eq!(versions[0].required_dependency_ids(), vec!["DEP1"]);
    }

    #[test]
    fn parse_curseforge_search() {
        let parsed: CfSearch = serde_json::from_str(CF_SEARCH).unwrap();
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].id, 123);
        assert_eq!(parsed.data[0].slug.as_deref(), Some("jei"));
    }

    #[test]
    fn curseforge_files_filtered_by_loader() {
        let parsed: CfFiles = serde_json::from_str(CF_FILES).unwrap();
        let fabric: Vec<&CfFile> = parsed
            .data
            .iter()
            .filter(|file| file.matches_loader(Some("fabric")))
            .collect();
        assert_eq!(fabric.len(), 1);
        assert_eq!(fabric[0].id, 777);
        let version = fabric[0]
            .to_content_version(ContentSource::Curseforge)
            .unwrap();
        assert_eq!(version.file_name, "jei-1.20.1-fabric.jar");
        assert_eq!(version.file_url, "https://media.cf.com/jei.jar");
    }

    #[test]
    fn category_dirs() {
        assert_eq!(ContentCategory::Mods.dir_name(), "mods");
        assert_eq!(ContentCategory::Resourcepacks.dir_name(), "resourcepacks");
        assert_eq!(ContentCategory::Shaderpacks.dir_name(), "shaderpacks");
    }

    /// Only mod searches/installs are scoped by the instance loader; packs and
    /// shaders must stay unfiltered regardless of the instance loader.
    #[test]
    fn loader_filter_applies_only_to_mods() {
        use crate::instances::{Instance, Loader};
        let fabric = Instance {
            id: "abc".to_owned(),
            name: "test".to_owned(),
            version_id: "1.20.1".to_owned(),
            loader: Loader::Fabric,
            loader_version: Some("0.14.21".to_owned()),
            created_at: 0,
            downloaded: true,
            java_component: None,
            java_major: None,
        };
        assert_eq!(loader_filter(&fabric, ContentCategory::Mods), Some("fabric"));
        assert_eq!(loader_filter(&fabric, ContentCategory::Resourcepacks), None);
        assert_eq!(loader_filter(&fabric, ContentCategory::Shaderpacks), None);
    }

    #[test]
    fn modrinth_facets_are_separate_and_groups() {
        let facets = modrinth_facets_json("1.20.1", "mod", Some("fabric")).unwrap();
        assert_eq!(
            facets,
            r#"[["versions:1.20.1"],["project_type:mod"],["categories:fabric"]]"#
        );
        let no_loader = modrinth_facets_json("1.20.1", "resourcepack", None).unwrap();
        assert_eq!(
            no_loader,
            r#"[["versions:1.20.1"],["project_type:resourcepack"]]"#
        );
    }

    #[test]
    fn url_encode_keeps_unreserved() {
        assert_eq!(url_encode("abc-1_2.~"), "abc-1_2.~");
        assert_eq!(url_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(url_encode("[[\"versions:1.20.1\"]]"), "%5B%5B%22versions%3A1.20.1%22%5D%5D");
    }

    #[test]
    fn valid_file_name_rejects_traversal() {
        assert!(valid_file_name("sodium-0.5.3.jar"));
        assert!(!valid_file_name("../evil.jar"));
        assert!(!valid_file_name("a/b.jar"));
        assert!(!valid_file_name("a\\b.jar"));
        assert!(!valid_file_name(".."));
        assert!(!valid_file_name(""));
    }
}
