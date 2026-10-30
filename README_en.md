# Ligone Launcher

A Minecraft launcher with offline login and Ely.by support. Desktop app built with Tauri 2: React 19 + TypeScript (Vite) frontend, Rust backend.

> Ваш язык не русский? Читайте [русский README](README.md).

## Download

Ready-made builds (EXE and installers) are available on the **Releases** tab of this repository. Download the archive, unpack it and run `Ligone Launcher.exe`. Ely.by login works out of the box, no configuration needed (see [below](#ely-by-login)).

The sections below are for those who want to build the launcher manually from source.

## Features

- **Accounts**: offline login (UUID matching Java's `nameUUIDFromBytes`) and Ely.by — OAuth (the recommended way) plus a password fallback. Accounts persist between runs; Ely.by tokens are stored in the OS keyring (Windows Credential Manager), so no secrets ever touch disk files.
- **Instances**: create, rename, delete; downloading of the selected Minecraft version with vanilla / Fabric / Forge (jar, libraries, assets) and sha1 verification — repeated downloads skip files that are already valid. Forge is installed through the official installer (available for releases 1.13+; for the new year-based versions 26.1+ Forge does not exist — the launcher warns about this when creating an instance). Each instance has its own `mods/`, `resourcepacks/`, `shaderpacks/`, `saves/`.
- **Content browser**: search and installation of mods, resource packs and shader packs from Modrinth and CurseForge (always filtered by the instance's game version; for mods — by the Fabric/Forge loader as well). Mod dependencies are installed automatically. CurseForge requires `CURSEFORGE_API_KEY`: without a key, search returns Modrinth results only.
- **Java**: automatic version resolution — custom path → Mojang runtime (downloaded without UAC/MSI) → system Java.
- **Game launch**: JVM arguments, natives extraction, authlib-injector (skins by nickname work offline too). The game log is written to `<instance>/launcher.log`; a crash shows the error in the UI.
- **Interface**: dark/light theme, Russian and English languages.

## Requirements (for building from source)

- Node.js + npm
- Rust (stable); on Windows — the MSVC toolchain and WebView2 (standard Tauri 2 requirements)

## Development

```bash
npm install
npm run tauri dev
```

`npm run tauri dev` starts Vite on the fixed port `1420` (a busy port fails) and builds the Rust part. For frontend-only work in a browser, use `npm run dev` (Tauri `invoke` calls will not work).

## Building the EXE from source

```bash
npm run tauri build
```

The first build is slow (Rust dependencies are compiled in release mode; later builds are fast). The frontend is built automatically (`tsc` + `vite build`) and embedded into the binary. On Windows the results are:

| File | What it is |
| --- | --- |
| `src-tauri/target/release/Ligone Launcher.exe` | ready-to-use portable EXE |
| `src-tauri/target/release/bundle/nsis/Ligone Launcher_0.1.0_x64-setup.exe` | NSIS installer |
| `src-tauri/target/release/bundle/msi/Ligone Launcher_0.1.0_x64_en-US.msi` | MSI installer |

If you only need the EXE without installers:

```bash
npm run tauri build -- --no-bundle
```

Important differences between release builds and dev:

- **OAuth redirect URI** is chosen automatically: in dev the browser lands on `http://localhost:1420/ely/callback` through the Vite proxy (this URI must also be registered in the Ely.by application); in release there is no proxy — the launcher's callback server binds port 8921 itself, and the browser lands on `http://localhost:8921/ely/callback`. No environment variables are required (optionally, everything can be overridden via `ELYBY_REDIRECT_URI`).
- **Ely.by credentials** (clientId/clientSecret) are embedded into the build, so login works right after installation — no configuration is needed.
- The console is hidden in release builds (`windows_subsystem = "windows"`); the game log is at `<instance>/launcher.log`.

## Checks

```bash
npm run build                                       # frontend: tsc (typecheck) + vite build
cargo test  --manifest-path src-tauri/Cargo.toml    # Rust unit tests
cargo check --manifest-path src-tauri/Cargo.toml    # Rust only, without the frontend
```

## Ely.by login

Ely.by OAuth (and the password fallback) work without any configuration: the application credentials are embedded into the build, and the redirect URIs (`http://localhost:8921/ely/callback` for release, `http://localhost:1420/ely/callback` for dev runs) are registered in the `ligone-launcher` application on the Ely.by side.

Override (optional) — to keep the callback on a non-standard port:

```
ELYBY_REDIRECT_URI=...   # your redirect URI
```

Full details of the OAuth flow and password login are in [docs/ely-by-oauth.md](docs/ely-by-oauth.md) (Russian only).

## Where the data lives

Everything is stored in `%APPDATA%\.ligone\`:

| Path | Contents |
| --- | --- |
| `auth/accounts.json` | saved accounts (no tokens) |
| `auth/client_token.json` | launcher-wide `clientToken` (not a secret) |
| `instances/<id>/` | instances (`instance.json`, `mods/`, `saves/`, …) |
| `versions/`, `libraries/`, `assets/` | shared game caches |
| `runtime/` | Java runtimes |
| `cache/` | version manifest cache, Forge installer jars |

## Project structure

```
src/                 frontend: App.tsx, components/, lib/ (typed invoke services, i18n)
src-tauri/src/       backend: lib.rs (entry point), auth/, downloader.rs, launch.rs, jre.rs,
                     instances.rs, content.rs, mcmeta.rs, fabric.rs, forge.rs, settings.rs, paths.rs
docs/ely-by-oauth.md how Ely.by login works
AGENTS.md            repository conventions and agent hints
```

The version and app identifier are kept in sync between `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` (`com.ligone.launcher`).
