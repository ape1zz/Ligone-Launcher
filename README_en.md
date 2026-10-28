# Ligone Launcher

A Minecraft launcher with offline login and Ely.by support. Desktop app built with Tauri 2: React 19 + TypeScript (Vite) frontend, Rust backend.

> Ваш язык не русский? Читайте [русский README](README.md).

## Download

Ready-made builds (EXE and installers) are available on the **Releases** tab of this repository. Download the archive, unpack it and run `ligone-launcher.exe`. For Ely.by login you only need to configure the credentials (see [below](#elyby-setup-required-for-oauthpassword-login)).

The sections below are for those who want to build the launcher manually from source.

## Features

- **Accounts**: offline login (UUID matching Java's `nameUUIDFromBytes`) and Ely.by — OAuth (the recommended way) plus a password fallback. Accounts persist between runs; Ely.by tokens are stored in the OS keyring (Windows Credential Manager), so no secrets ever touch disk files.
- **Instances**: create, rename, delete; downloading of the selected Minecraft version and Fabric (jar, libraries, assets) with sha1 verification — repeated downloads skip files that are already valid. Each instance has its own `mods/`, `resourcepacks/`, `shaderpacks/`, `saves/`.
- **Content browser**: search and installation of mods and resource packs from Modrinth and CurseForge (always filtered by the instance's game version; for mods on Fabric — by loader as well). Mod dependencies are installed automatically. CurseForge is used when `CURSEFORGE_API_KEY` is set, otherwise it degrades to Modrinth only.
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
| `src-tauri/target/release/ligone-launcher.exe` | ready-to-use portable EXE |
| `src-tauri/target/release/bundle/nsis/ligone-launcher_0.1.0_x64-setup.exe` | NSIS installer |
| `src-tauri/target/release/bundle/msi/ligone-launcher_0.1.0_x64_en-US.msi` | MSI installer |

If you only need the EXE without installers:

```bash
npm run tauri build -- --no-bundle
```

Important differences between release builds and dev:

- **OAuth redirect URI**: in dev the browser lands on `http://localhost:1420/ely/callback` through the Vite proxy; in release there is no proxy, so `http://localhost:8921/ely/callback` must be registered in your Ely.by application, and the environment variable `ELYBY_REDIRECT_URI=http://localhost:8921/ely/callback` must be set before running the EXE.
- **Ely.by credentials** are still read from `%APPDATA%\.ligone\.env` or from environment variables — put `.env` next to the launcher's data, not next to the EXE.
- The console is hidden in release builds (`windows_subsystem = "windows"`); the game log is at `<instance>/launcher.log`.

## Checks

```bash
npm run build                                       # frontend: tsc (typecheck) + vite build
cargo test  --manifest-path src-tauri/Cargo.toml    # Rust unit tests
cargo check --manifest-path src-tauri/Cargo.toml    # Rust only, without the frontend
```

## Ely.by setup (required for OAuth/password login)

1. Register an application at <https://account.ely.by/dev/applications/new> (type **Website**), Redirect URI — `http://localhost:1420/ely/callback` (dev; for release — `http://localhost:8921/ely/callback` or your own `ELYBY_REDIRECT_URI`).
2. Put the `clientId`/`clientSecret` into a `.env` file in the launcher home (`%APPDATA%\.ligone\.env`) or set environment variables:

   ```
   ELYBY_CLIENT_ID=...
   ELYBY_CLIENT_SECRET=...
   ```

   Environment variables take priority over `.env`; no secrets are hardcoded anywhere.

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
| `cache/` | version manifest cache |

## Project structure

```
src/                 frontend: App.tsx, components/, lib/ (typed invoke services, i18n)
src-tauri/src/       backend: lib.rs (entry point), auth/, downloader.rs, launch.rs, jre.rs,
                     instances.rs, content.rs, mcmeta.rs, fabric.rs, settings.rs, paths.rs
docs/ely-by-oauth.md how Ely.by login works
AGENTS.md            repository conventions and agent hints
```

The version and app identifier are kept in sync between `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` (`com.ligone.launcher`).
