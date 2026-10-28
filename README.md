# Ligone Launcher

Лаунчер Minecraft с поддержкой офлайн-входа и Ely.by. Десктоп-приложение на Tauri 2: фронтенд — React 19 + TypeScript (Vite), бэкенд — Rust.

> Your language is not Russian? Check the [English README](README_en.md).

## Скачать

Готовые сборки (EXE и установщики) — на вкладке **Releases** этого репозитория. Скачайте архив, распакуйте и запустите `ligone-launcher.exe`. Для входа через Ely.by останется только настройка учётных данных (см. [ниже](#настройка-elyby-обязательно-для-oauthпарольного-входа)).

Разделы ниже — для тех, кто хочет собрать лаунчер вручную из исходного кода.

## Возможности

- **Аккаунты**: офлайн-вход (UUID как `nameUUIDFromBytes` в Java) и Ely.by — OAuth (рекомендуемый способ) и запасной вход по паролю. Аккаунты сохраняются между запусками; токены Ely.by хранятся в OS keyring (Windows Credential Manager), в файлах на диске секретов нет.
- **Инстансы**: создание, переименование, удаление; скачивание выбранной версии Minecraft и Fabric (jar, библиотеки, ассеты) с проверкой по sha1 — повторные скачивания пропускают валидные файлы. У каждого инстанса свои `mods/`, `resourcepacks/`, `shaderpacks/`, `saves/`.
- **Браузер контента**: поиск и установка модов/ресурспаков с Modrinth и CurseForge (всегда отфильтровано по версии игры; для модов на Fabric — по загрузчику). Зависимости модов ставятся автоматически. CurseForge используется при наличии `CURSEFORGE_API_KEY`, иначе — только Modrinth.
- **Java**: автоматический подбор нужной версии — пользовательский путь → рантайм Mojang (скачивается без UAC/MSI) → системная Java.
- **Запуск игры**: JVM-аргументы, распаковка нативных библиотек, authlib-injector (скины по нику работают и офлайн). Лог игры пишется в `<инстанс>/launcher.log`, при аварийном выходе ошибка показывается в UI.
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
| `src-tauri/target/release/ligone-launcher.exe` | готовый портативный EXE |
| `src-tauri/target/release/bundle/nsis/ligone-launcher_0.1.0_x64-setup.exe` | установщик (NSIS) |
| `src-tauri/target/release/bundle/msi/ligone-launcher_0.1.0_x64_en-US.msi` | установщик (MSI) |

Нужен только EXE без установщиков:

```bash
npm run tauri build -- --no-bundle
```

Важные отличия release-сборки от dev:

- **Redirect URI для OAuth**: в dev браузер попадает на `http://localhost:1420/ely/callback` через прокси Vite; в release прокси нет, поэтому в приложении Ely.by должен быть зарегистрирован `http://localhost:8921/ely/callback`, и перед запуском EXE задайте переменную окружения `ELYBY_REDIRECT_URI=http://localhost:8921/ely/callback` (или пропишите этот URI в приложении Ely.by и переменной).
- **Учётные данные Ely.by** по-прежнему читаются из `%APPDATA%\.ligone\.env` или переменных окружения — положите `.env` рядом с данными лаунчера, рядом с EXE они не нужны.
- Консоль в release-сборке скрыта (`windows_subsystem = "windows"`); лог игры — в `<инстанс>/launcher.log`.

## Проверки

```bash
npm run build                                       # фронтенд: tsc (typecheck) + vite build
cargo test  --manifest-path src-tauri/Cargo.toml    # юнит-тесты Rust
cargo check --manifest-path src-tauri/Cargo.toml    # Rust без фронтенда
```

## Настройка Ely.by (обязательно для OAuth/парольного входа)

1. Зарегистрируйте приложение на <https://account.ely.by/dev/applications/new> (тип **Website**), Redirect URI — `http://localhost:1420/ely/callback` (dev; для release — `http://localhost:8921/ely/callback` или своя переменная `ELYBY_REDIRECT_URI`).
2. Положите `clientId`/`clientSecret` в файл `.env` в домашней папке лаунчера (`%APPDATA%\.ligone\.env`) либо задайте переменные окружения:

   ```
   ELYBY_CLIENT_ID=...
   ELYBY_CLIENT_SECRET=...
   ```

   Переменные из окружения имеют приоритет над `.env`; секреты нигде не хардкодятся.

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
| `cache/` | кэш манифестов версий |

## Структура проекта

```
src/                 фронтенд: App.tsx, components/, lib/ (типизированные сервисы invoke, i18n)
src-tauri/src/       бэкенд: lib.rs (точка входа), auth/, downloader.rs, launch.rs, jre.rs,
                     instances.rs, content.rs, mcmeta.rs, fabric.rs, settings.rs, paths.rs
docs/ely-by-oauth.md как устроен вход через Ely.by
AGENTS.md            соглашения репозитория и подсказки для агентов
```

Версия и идентификатор приложения синхронизируются между `package.json`, `src-tauri/Cargo.toml` и `src-tauri/tauri.conf.json` (`com.ligone.launcher`).
