# Ligone Launcher

Лаунчер Minecraft с поддержкой офлайн-входа и Ely.by. Десктоп-приложение на Tauri 2: фронтенд — React 19 + TypeScript (Vite), бэкенд — Rust.

> Your language is not Russian? Check the [English README](README_en.md).

## Скачать

Готовые сборки (EXE и установщики) — на вкладке **Releases** этого репозитория. Скачайте архив, распакуйте и запустите `Ligone Launcher.exe`. Вход через Ely.by работает из коробки, без настройки (см. [ниже](#вход-через-elyby)).

Разделы ниже — для тех, кто хочет собрать лаунчер вручную из исходного кода.

## Возможности

- **Аккаунты**: офлайн-вход (UUID как `nameUUIDFromBytes` в Java) и Ely.by — OAuth (рекомендуемый способ) и запасной вход по паролю. Аккаунты сохраняются между запусками; токены Ely.by хранятся в OS keyring (Windows Credential Manager), в файлах на диске секретов нет.
- **Инстансы**: создание, переименование, удаление; скачивание выбранной версии Minecraft с vanilla / Fabric / Forge (jar, библиотеки, ассеты) с проверкой по sha1 — повторные скачивания пропускают валидные файлы. Forge ставится через официальный установщик (доступен для релизов 1.13+; для новых годовых версий 26.1+ Forge не существует — о чём лаунчер предупреждает при создании инстанса). У каждого инстанса свои `mods/`, `resourcepacks/`, `shaderpacks/`, `saves/`.
- **Браузер контента**: поиск и установка модов/ресурспаков/шейдерпаков с Modrinth и CurseForge (всегда отфильтровано по версии игры; для модов — ещё и по загрузчику Fabric/Forge). Зависимости модов ставятся автоматически. CurseForge требует `CURSEFORGE_API_KEY`: поиск без ключа отдаёт только Modrinth.
- **Java**: автоматический подбор нужной версии — пользовательский путь → рантайм Mojang (скачивается без UAC/MSI) → системная Java.
- **Запуск игры**: JVM-аргументы, распаковка нативных библиотек, authlib-injector - скины по нику работают и офлайн (может не работать). Лог игры пишется в `<инстанс>/launcher.log`, при аварийном выходе ошибка показывается в UI.
- **Интерфейс**: тёмная/светлая тема, русский и английский языки.

## Требования (для сборки из исходников)

- Node.js + npm
- Rust (stable); на Windows — toolchain MSVC и WebView2 (стандартные требования Tauri 2)

## Запуск в разработке

```bash
npm install
npm run tauri dev
```

`npm run tauri dev` поднимает Vite на фиксированном порту `1420` (занятый порт приведёт к ошибке) и собирает Rust-часть. Для чисто фронтендовой работы в браузере — `npm run dev` (вызовы `invoke` к Rust работать не будут).

## Сборка EXE из исходников

```bash
npm run tauri build
```

Первая сборка долгая (компилируются Rust-зависимости в release-режиме, дальше — быстро). Фронтенд собирается автоматически (`tsc` + `vite build`) и встраивается в бинарник. Результат на Windows:

| Файл | Что это |
| --- | --- |
| `src-tauri/target/release/Ligone Launcher.exe` | готовый портативный EXE |
| `src-tauri/target/release/bundle/nsis/Ligone Launcher_0.1.0_x64-setup.exe` | установщик (NSIS) |
| `src-tauri/target/release/bundle/msi/Ligone Launcher_0.1.0_x64_en-US.msi` | установщик (MSI) |

Нужен только EXE без установщиков:

```bash
npm run tauri build -- --no-bundle
```

Важные отличия release-сборки от dev:

- **Redirect URI для OAuth** выбирается автоматически: в dev браузер попадает на `http://localhost:1420/ely/callback` через прокси Vite (в приложении Ely.by должен быть зарегистрирован и этот URI); в release прокси нет — callback-сервер лаунчера сам биндит порт 8921, и браузер попадает на `http://localhost:8921/ely/callback`. Ничего задавать переменными не нужно (при желании можно переопределить значением `ELYBY_REDIRECT_URI`).
- **Учётные данные Ely.by** (clientId/clientSecret) вшиты в сборку, поэтому вход работает сразу после установки — настраивать ничего не нужно.
- Консоль в release-сборке скрыта (`windows_subsystem = "windows"`); лог игры — в `<инстанс>/launcher.log`.

## Проверки

```bash
npm run build                                       # фронтенд: tsc (typecheck) + vite build
cargo test  --manifest-path src-tauri/Cargo.toml    # юнит-тесты Rust
cargo check --manifest-path src-tauri/Cargo.toml    # Rust без фронтенда
```

## Вход через Ely.by

Ely.by OAuth (и запасной вход по паролю) работают без какой-либо настройки: учётные данные приложения вшиты в сборку, а redirect URI (`http://localhost:8921/ely/callback` для release, `http://localhost:1420/ely/callback` для dev-запуска) зарегистрированы в приложении `ligone-launcher` на стороне Ely.by.

Переопределение (необязательно), если хотите держать callback на нестандартном порту:

```
ELYBY_REDIRECT_URI=...   # свой redirect URI
```

Подробности OAuth-флоу и парольного входа — [docs/ely-by-oauth.md](docs/ely-by-oauth.md).

## Где лежат данные

Всё в `%APPDATA%\.ligone\`:

| Путь | Содержимое |
| --- | --- |
| `auth/accounts.json` | сохранённые аккаунты (без токенов) |
| `auth/client_token.json` | лаунчерный `clientToken` (не секрет) |
| `instances/<id>/` | инстансы (`instance.json`, `mods/`, `saves/`, …) |
| `versions/`, `libraries/`, `assets/` | общие кэши игры |
| `runtime/` | рантаймы Java |
| `cache/` | кэш манифестов версий, jar установщика Forge |

## Структура проекта

```
src/                 фронтенд: App.tsx, components/, lib/ (типизированные сервисы invoke, i18n)
src-tauri/src/       бэкенд: lib.rs (точка входа), auth/, downloader.rs, launch.rs, jre.rs,
                     instances.rs, content.rs, mcmeta.rs, fabric.rs, forge.rs, settings.rs, paths.rs
docs/ely-by-oauth.md как устроен вход через Ely.by
AGENTS.md            соглашения репозитория и подсказки для агентов
```

Версия и идентификатор приложения синхронизируются между `package.json`, `src-tauri/Cargo.toml` и `src-tauri/tauri.conf.json` (`com.ligone.launcher`).
