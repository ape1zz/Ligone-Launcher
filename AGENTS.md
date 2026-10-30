# AGENTS.md

Minecraft launcher «Ligone Launcher»: Tauri 2 desktop app — React 19 + Vite 8 + TypeScript frontend in `src/`, Rust backend in `src-tauri/`. Implemented: offline + Ely.by auth (OAuth and password fallback), persistent accounts, settings, instance system with version downloading (vanilla / Fabric / Forge loaders), content browser (Modrinth + CurseForge), Java auto-resolution, and game launch (JVM args, natives, authlib-injector).

## Commands

- `npm run tauri dev` — run the desktop app (repo root; there is no nested project dir). Starts Vite via `beforeDevCommand` on fixed port `1420` (`strictPort` — a busy port fails); HMR port `1421` only when `TAURI_DEV_HOST` is set.
- `npm run dev` — frontend only in a browser. Tauri `invoke` calls will not work; use for pure UI work only.
- `npm run build` — `tsc && vite build`; this is also the frontend typecheck. Fails on unused locals/params (`noUnusedLocals`/`noUnusedParameters`).
- `cargo test --manifest-path src-tauri/Cargo.toml` — Rust unit tests (in `#[cfg(test)]` blocks across almost every module).
- `cargo check --manifest-path src-tauri/Cargo.toml` — Rust-only check without the frontend.
- `npm run tauri build` — native bundle (slow first run; compiles Rust deps). EXE-only without the NSIS/MSI installers: `cargo build --release` inside `src-tauri/` (tauri's `--no-bundle` flag routes to cargo, not the CLI). Windows prerequisites: MSVC toolchain + WebView2 (standard Tauri 2 set).
- No lint or formatter scripts are configured. Do not invent commands; verify with the above.

## Wiring

- Frontend entry: `index.html` → `src/main.tsx` → `src/App.tsx`; views/modals live in `src/components/`, styles in `src/App.css`.
- Rust entry: `src-tauri/src/main.rs` calls `ligone_launcher_lib::run()` in `src-tauri/src/lib.rs`. The `_lib` suffix is intentional (Windows bin/lib name collision).
- Every `#[tauri::command]` must be listed in `tauri::generate_handler![...]` in `lib.rs` or `invoke` fails at runtime.
- Plugin/API permissions go in `src-tauri/capabilities/default.json` (window `main`); missing permissions fail at runtime, not compile time.
- Managed state structs (`AuthState`, `InstanceState`, `JavaState`, `LaunchState`) are registered with `.manage(...)` in `run()`; note `InstanceState` lives in `downloader.rs`, not `instances.rs`.
- `README.md` / `README_en.md` are real project docs (Russian/English docs, not boilerplate); OAuth details: `docs/ely-by-oauth.md`.

## Tauri command conventions (both sides)

- Commands are named `<module>_<verb>` in snake_case (`auth_login_offline`, `instance_create`, ...); frontend `invoke()` args use camelCase keys.
- Errors are typed, not strings: commands return `Result<T, SomeError>` where the error is a serde internally-tagged enum (`#[serde(tag = "type", rename_all = "snake_case")]`). `invoke` rejects with that object; narrow it with the per-module `parse*Error()` guard in `src/lib/*.ts`. New error type = Rust tagged enum + TS mirror type + parse guard.
- Long-running commands (network, servers, processes) must be `async fn` + `tauri::async_runtime::spawn_blocking` (see the `run_blocking` helper in `src-tauri/src/auth/mod.rs`): plain sync commands execute on the **main thread** and freeze the UI. In async commands don't take `State<'_, T>` params — get state via `app.state::<T>()` inside the closure. `run_blocking` is private to the `auth` module and typed to `AuthError` — other modules inline the same pattern: `tauri::async_runtime::spawn_blocking(move || ...).await`.
- All `invoke` calls live in typed services under `src/lib/*.ts` (`auth.ts`, `instances.ts`, `settings.ts`, `content.ts`, `java.ts`); components never call `invoke` directly, they call `<service><ErrorMessage>(error, t)` helpers.
- Long-running cancellable operations get a `*_cancel_*` companion command (`auth_cancel_ely`, `instance_cancel_download`); both sides should be wired together when touching those flows.
- Progress/state flows over Tauri events (`instance-download-progress`, `instance-run-state`, `java-status`); services expose `on*`/`use*` subscriptions (e.g. `onDownloadProgress()`, `useJavaStatus()`). `instance-run-state` carries an optional `error` string (shown in the UI) when the game exits with a non-zero code.

## Auth

- Offline UUID = MD5 of `OfflinePlayer:{username}` bit-tweaked to a v3 UUID (matches Java's `nameUUIDFromBytes`); pinned test vector in `auth/offline.rs`. Username rules: 3–16 chars, ASCII letters/digits/`_`.
- Launcher home = sibling `.ligone` dir of `app_data_dir` (`paths::launcher_home`, i.e. `%APPDATA%\.ligone` on Windows) — instances, caches, settings, runtime all live there. Accounts persist to `<home>/auth/accounts.json` (camelCase, `{ accounts, last }`) but Ely tokens do NOT live there: the file keeps a tokenless `ely: {}` marker (`Stored`/`StoredAccount` in `storage.rs`) and full tokens are stored in the OS keyring (`keyring` crate, `windows-native` feature; service `com.ligone.launcher`, user `ely@<username>`, blob = serialized `ElyAccount`). `storage::load` rehydrates tokens from the keyring; a one-time migration on load moves inline tokens from legacy files into the keyring, and entries missing from the keyring load as synthetic expired tokens (any refresh then errors "сессия требует повторного входа"). `auth_remove_saved` deletes the keyring entry. `restore_last_session` runs at startup. First read migrates legacy files from `%APPDATA%\com.ligone.launcher\auth\`.
- Ely.by OAuth in `auth/ely.rs`: callback server binds `127.0.0.1:8921`; the Vite dev server proxies `/ely/callback` to it, so the Ely.by app redirect URI is `http://localhost:1420/ely/callback` in dev; release builds have no dev server, so the callback server binds 8921 itself and the URI is `http://localhost:8921/ely/callback` (both must be registered in the Ely.by application; `ELYBY_REDIRECT_URI` overrides the default in either mode). `credentials()` uses embedded defaults (`EMBEDDED_CLIENT_ID`/`EMBEDDED_CLIENT_SECRET` in `ely.rs`, app `ligone-launcher`) so OAuth works out of the box; `ELYBY_CLIENT_ID`/`ELYBY_CLIENT_SECRET` (`real env vars win, dotenvy`; `.env` in the launcher home is loaded best-effort at startup via `auth::load_env` in setup) override the embedded pair — each env value is used only if non-blank. The Russian "не настроен" credential error is effectively unreachable.
- `auth_ely_refresh` re-refreshes the current account's tokens on demand using the same token-kind rules. Password fallback (`auth_login_ely_password`) hits `authserver.ely.by`; profile UUID arrives without dashes — normalize it. Passwords are never stored (and never logged — no println/logging crates in the backend); launcher-wide non-secret `clientToken` in `<home>/auth/client_token.json`. Token kind decides the login method: `refreshToken` → OAuth, `clientToken` → password flow. 2FA is deliberately unsupported in the password flow (error directs to OAuth).

## Instances, downloads, launch, Java

- Instance = `<home>/instances/<id>/` with `instance.json` + `mods/`, `resourcepacks/`, `shaderpacks/`, `saves/`, `settings.txt`. Shared caches (do NOT duplicate per instance): `versions/`, `libraries/`, `assets/objects/<hash2>/<hash>`, `assets/indexes/<id>.json` (asset index — the game also reads it from there at launch), `cache/`. Re-downloads are cheap — valid files are skipped by sha1/size; if every file (jar, libraries, all assets) is already valid, `instance_download` short-circuits before the download loop — no network transfers, no progress events, just marks the instance downloaded. Version list is cached 30 min in `<home>/cache/version_manifest.json`; on network failure a stale cache is served instead of erroring.
- Modules: `instances.rs` (CRUD + rename, name rules: 1..=64 chars, no `\/:*?"<>|`), `mcmeta.rs`, `fabric.rs`, `forge.rs`, `downloader.rs`, `launch.rs`, `jre.rs`, `content.rs`, `paths.rs`, `settings.rs`. Each has its own tagged `*Error` enum; `instance_remove` rejects with `busy` while downloading or while the game runs. Loader enum lives in `instances.rs` (`Loader::Vanilla|Fabric|Forge`); the chosen build/version is stored in `instance.json` as `loaderVersion`.
- Launch (`launch.rs`): cached version JSON → `<instance>/fabric.json` or `<instance>/forge.json` → account (OAuth sessions auto-refresh when expired; legacy `clientToken` sessions refresh best-effort on every launch — a failing refresh never blocks the launch) → Java → spawn. Natives re-extracted to `<instance>/natives` every launch. Game spawns hidden (`CREATE_NO_WINDOW` on Windows), working dir = instance dir, output piped to `<instance>/launcher.log`. authlib-injector is injected for BOTH Ely and offline accounts (`ely.by` authlib base — skins by nick work offline).
- Java (`jre.rs`): auto-runs at startup (`setup` hot-tasks `ensure_java`). Order: custom `javaPath` setting → Mojang runtime `<home>/runtime/<component>/` (downloaded, no UAC/MSI involved) → system Java probed with required major version. Required version comes from the version JSON's `javaVersion`, stored in `instance.json` (`serde(default)` fields — old files stay loadable). `jre::resolve_launch_java` is also what runs the Forge installer.
- Forge (`forge.rs`): installs by running the official installer jar headlessly (`--installClient <home>`, via `resolve_launch_java` — prefers `java.exe` over `javaw.exe` so errors are captured). It requires the marker file `launcher_profiles.json` in the launcher home (written with `{"profiles":{}}` if absent) and generates a shared profile at `versions/<mc>-forge-<build>/<id>.json`; the installer jar is cached in `<home>/cache/forge/`. The downloader copies this overlay into the instance as `<instance>/forge.json`; for Forge instances the short-circuit logic also requires that overlay to exist.

## Content browser (`content.rs`)

- Merged Modrinth + CurseForge search (`content_search`), always filtered by instance game version (+ `fabric`/`forge` loader facet for mods on loader instances). CurseForge uses `CURSEFORGE_API_KEY` env var (constant in `content.rs` is an empty placeholder); a missing key must not hide Modrinth results, so `content_search` swallows CurseForge Api errors — but `content_versions` and `content_install` on a Curseforge source still error out with «CURSEFORGE_API_KEY не настроен».
- `content_install` auto-installs required dependencies (cycle-safe); `content_remove` is path-traversal guarded; `content_versions`/`content_installed` serve the version list and per-instance installed list.

## Repo style

- NEVER make git commits (or amend/push) — the user does version control manually; finish work with uncommitted changes and just report what changed.

- UI strings and error messages are Russian; backend error messages stay Russian (wire format, not localized). All static UI strings go through `t()` from `src/lib/i18n.tsx` (`LanguageProvider` + `useI18n()`; ru/en dictionaries inline).
- CSS class names are BEM-flavored (`welcome__title`, `account--modifier`) in `src/App.css`; dark theme via `[data-theme="dark"]` CSS variables (set by `applyTheme`).
- Keep version and app identity in sync across `package.json`, `src-tauri/Cargo.toml` (`com.ligone.launcher`), and `src-tauri/tauri.conf.json`. Human-visible name everywhere is `Ligone Launcher` (window/UI/docs/EXE): `productName` + `mainBinaryName` are set to it in `tauri.conf.json` — **`mainBinaryName` must stay explicit**, otherwise the installed EXE is named after the cargo binary (`ligone-launcher.exe`). Machine identifiers stay fixed: npm `ligone-launcher`, payload id `com.ligone.launcher` (keyring service, appdata dir), Ely.by client id `ligone-launcher` (registered remotely — renaming requires re-registration).
- Rust edition is 2024. Do not edit generated/ignored outputs: `dist/`, `src-tauri/target/`, `src-tauri/gen/schemas/`.
- Keep `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` in `main.rs` — hides the console in release builds.
