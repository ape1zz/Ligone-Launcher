import { createContext, useContext, useMemo, useState } from "react";
import type { ReactNode } from "react";

export type Language = "ru" | "en";

export type TranslateFn = (
  key: string,
  params?: Record<string, string | number>,
) => string;

type Dictionary = Record<string, string>;

const ru: Dictionary = {
  "common.loading": "Загрузка…",
  "common.close_title": "Закрыть",

  "welcome.title": "Добро пожаловать",
  "welcome.nick_placeholder": "Ник игрока",
  "welcome.offline": "Офлайн",
  "welcome.or_login_via": "Или войти через",

  "chooser.aria": "Вход через Ely.by",
  "chooser.title": "Выберите способ входа через Ely.by",
  "chooser.browser": "Через браузер",
  "chooser.browser_hint": "Рекомендуемый способ",
  "chooser.password": "Логин и пароль",
  "chooser.password_hint": "Запасной вариант",
  "chooser.login_placeholder": "E-mail или ник",
  "chooser.password_placeholder": "Пароль",
  "chooser.login": "Войти",
  "chooser.back": "Вернуться к выбору способа",
  "chooser.waiting_title": "Открываю браузер — завершите вход там",
  "chooser.waiting_hint":
    "Как только авторизация пройдёт, лаунчер продолжит автоматически. Можно подождать или вернуться к выбору способа.",

  "shell.placeholder": "Здесь будет основная область лаунчера",

  "instances.title": "Сборки",
  "instances.add": "Создать сборку",
  "instances.empty": "Сборок пока нет — создайте первую",
  "instances.play_ready": "Готова к запуску",
  "instances.play": "Играть",
  "instances.running": "Запущена",
  "instances.not_downloaded": "Не скачана",
  "instances.download": "Скачать",
  "instances.cancel_download": "Отменить",
  "instances.remove_title": "Удалить сборку",
  "instances.remove_confirm":
    "Удалить сборку «{name}»? Папка с мирами, модами и настройками будет удалена безвозвратно.",
  "instances.remove_confirm_yes": "Удалить",
  "instances.remove_confirm_no": "Отмена",
  "instances.stage_jar": "игровой jar",
  "instances.stage_libraries": "библиотеки",
  "instances.stage_assets": "ассеты",
  "instances.stage_forge": "устанавливаю Forge",
  "instances.badge_vanilla": "Vanilla",
  "instances.open_home": "Открыть папку лаунчера",
  "instances.card_settings": "Настройки сборки",

  "instset.aria": "Настройки сборки",
  "instset.title": "Настройки сборки",
  "instset.version_label": "Версия",
  "instset.folders_label": "Папки",
  "instset.open_root": "Открыть папку сборки",
  "instset.open_mods": "Моды",
  "instset.open_resourcepacks": "Ресурс-паки",
  "instset.open_shaderpacks": "Шейдеры",
  "instset.open_saves": "Миры",
  "instset.open_folder_title": "Открыть папку",

  "content.title_mods": "Моды",
  "content.title_resourcepacks": "Ресурспаки",
  "content.title_shaderpacks": "Шейдеры",
  "content.search_placeholder": "Поиск в Modrinth и CurseForge…",
  "content.search_button": "Найти",
  "content.empty_results": "Ничего не найдено",
  "content.empty_versions": "Нет версий для этой версии игры",
  "content.back": "Назад к результатам",
  "content.install": "Установить",
  "content.installing": "Установка…",
  "content.installed_notice": "Установлено файлов: {count}",
  "content.installed_label": "Установлено",
  "content.empty_installed": "Пока ничего не установлено",
  "content.remove_title": "Удалить файл",
  "content.source_modrinth": "Modrinth",
  "content.source_curseforge": "CurseForge",

  "create.aria": "Создание сборки",
  "create.title": "Новая сборка",
  "create.name_label": "Название",
  "create.name_placeholder": "Например: Выживание с друзьями",
  "create.version_label": "Версия игры",
  "create.filter_releases": "Релизы",
  "create.filter_snapshots": "Снапшоты",
  "create.search_placeholder": "Поиск версии…",
  "create.versions_loading": "Загружаю список версий…",
  "create.versions_empty": "Версии не найдены",
  "create.loader_label": "Загрузчик",
  "create.loader_vanilla": "Vanilla",
  "create.loader_fabric": "Fabric",
  "create.loader_forge": "Forge",
  "create.fabric_version_label": "Версия Fabric",
  "create.fabric_loading": "Загружаю версии Fabric…",
  "create.fabric_error": "Не удалось загрузить версии Fabric",
  "create.fabric_empty": "Версии Fabric не найдены",
  "create.fabric_beta": "beta",
  "create.forge_version_label": "Версия Forge",
  "create.forge_loading": "Загружаю версии Forge…",
  "create.forge_error": "Не удалось загрузить версии Forge",
  "create.forge_empty": "Версии Forge не найдены",
  "create.forge_recommended": "Рекомендуемая",
  "create.forge_unsupported":
    "Пока что скачивание Forge доступно только для версий 1.13 и выше",
  "create.forge_none": "На эту версию Forge нет",
  "create.create": "Создать и скачать",
  "create.cancel": "Отмена",

  "manage.panel_aria": "Управление аккаунтами",
  "manage.button": "Управление аккаунтами",
  "manage.gear_title": "Настройки",
  "manage.panel_title": "Аккаунты",
  "manage.empty": "Сохранённых аккаунтов нет",
  "manage.current_kind": "сейчас",
  "manage.offline_kind": "офлайн",
  "manage.add": "Добавить",
  "manage.signout": "Выйти",
  "manage.switch_title": "Сменить на {username}",
  "manage.remove_title": "Удалить {username}",
  "manage.remove_disabled": "Сначала смените аккаунт или выйдите",
  "manage.ely_badge": "Аккаунт Ely.by",
  "manage.ely_badge_current": "Аккаунт Ely.by, сейчас активен",

  "error.unexpected": "Непредвиденная ошибка входа",
  "error.invalid_username": "Недопустимый ник «{username}» — {reason}",
  "error.unknown_account": "Неизвестный аккаунт «{username}»",
  "error.storage": "Ошибка хранилища: {message}",
  "error.oauth": "Ошибка авторизации: {message}",
  "error.ely_api": "Ely.by: {message}",
  "error.cancelled": "Авторизация отменена",
  "error.unexpected_settings": "Непредвиденная ошибка настроек",
  "error.unexpected_instance": "Непредвиденная ошибка сборок",
  "error.instance_network": "Ошибка сети: {message}",
  "error.instance_download": "Ошибка загрузки: {message}",
  "error.instance_invalid_name": "Некорректное имя сборки: {message}",
  "error.instance_not_found": "Сборка не найдена",
  "error.instance_busy": "Лаунчер уже занят другой загрузкой",
  "error.instance_cancelled": "Загрузка отменена",
  "error.instance_launch": "Ошибка запуска: {message}",
  "error.content_network": "Ошибка сети: {message}",
  "error.content_api": "Ошибка API: {message}",
  "error.content_not_found": "Не найдено: {id}",

  "java.idle": "Java не требуется",
  "java.checking": "Проверяю Java…",
  "java.ready_system": "Системная Java {version}",
  "java.ready_bundled": "Встроенная Java {version}",
  "java.ready_custom": "Java из настроек",
  "java.installing": "Устанавливаю Java… {percent}%",
  "java.error": "Ошибка Java: {message}",

  "settings.title": "Настройки",
  "settings.tab_appearance": "Внешний вид",
  "settings.tab_game": "Игра",
  "settings.theme": "Тема оформления",
  "settings.theme_light": "Светлая",
  "settings.theme_dark": "Тёмная",
  "settings.language": "Язык",
  "settings.ram": "Выделение памяти (МБ)",
  "settings.ram_hint":
    "Допустимо от {min} до {max} МБ; если минимум больше максимума, значения поменяются местами при сохранении.",
  "settings.java": "Путь к Java",
  "settings.java_placeholder": "Пусто — автоопределение",
  "settings.java_hint": "Например: C:\\Program Files\\Java\\bin\\javaw.exe",
  "settings.save": "Сохранить",
  "settings.cancel": "Отмена",
};

const en: Dictionary = {
  "common.loading": "Loading…",
  "common.close_title": "Close",

  "welcome.title": "Welcome",
  "welcome.nick_placeholder": "Player nickname",
  "welcome.offline": "Offline",
  "welcome.or_login_via": "Or sign in with",

  "chooser.aria": "Sign in with Ely.by",
  "chooser.title": "Choose an Ely.by sign-in method",
  "chooser.browser": "Via browser",
  "chooser.browser_hint": "Recommended",
  "chooser.password": "Login and password",
  "chooser.password_hint": "Fallback option",
  "chooser.login_placeholder": "E-mail or nickname",
  "chooser.password_placeholder": "Password",
  "chooser.login": "Sign in",
  "chooser.back": "Back to method selection",
  "chooser.waiting_title": "Opening the browser — finish sign-in there",
  "chooser.waiting_hint":
    "Once sign-in completes, the launcher will continue automatically. You can wait or return to method selection.",

  "shell.placeholder": "The main launcher area will live here",

  "instances.title": "Instances",
  "instances.add": "New instance",
  "instances.empty": "No instances yet — create your first one",
  "instances.play_ready": "Ready to play",
  "instances.play": "Play",
  "instances.running": "Running",
  "instances.not_downloaded": "Not downloaded",
  "instances.download": "Download",
  "instances.cancel_download": "Cancel",
  "instances.remove_title": "Delete instance",
  "instances.remove_confirm":
    "Delete instance \"{name}\"? Its worlds, mods and settings folder will be removed permanently.",
  "instances.remove_confirm_yes": "Delete",
  "instances.remove_confirm_no": "Cancel",
  "instances.stage_jar": "game jar",
  "instances.stage_libraries": "libraries",
  "instances.stage_assets": "assets",
  "instances.stage_forge": "installing Forge",
  "instances.badge_vanilla": "Vanilla",
  "instances.open_home": "Open launcher folder",
  "instances.card_settings": "Instance settings",

  "instset.aria": "Instance settings",
  "instset.title": "Instance settings",
  "instset.version_label": "Version",
  "instset.folders_label": "Folders",
  "instset.open_root": "Open instance folder",
  "instset.open_mods": "Mods",
  "instset.open_resourcepacks": "Resource packs",
  "instset.open_shaderpacks": "Shader packs",
  "instset.open_saves": "Worlds",
  "instset.open_folder_title": "Open folder",

  "content.title_mods": "Mods",
  "content.title_resourcepacks": "Resource packs",
  "content.title_shaderpacks": "Shaders",
  "content.search_placeholder": "Search Modrinth and CurseForge…",
  "content.search_button": "Search",
  "content.empty_results": "Nothing found",
  "content.empty_versions": "No versions for this game version",
  "content.back": "Back to results",
  "content.install": "Install",
  "content.installing": "Installing…",
  "content.installed_notice": "Installed files: {count}",
  "content.installed_label": "Installed",
  "content.empty_installed": "Nothing installed yet",
  "content.remove_title": "Remove file",
  "content.source_modrinth": "Modrinth",
  "content.source_curseforge": "CurseForge",

  "create.aria": "Create instance",
  "create.title": "New instance",
  "create.name_label": "Name",
  "create.name_placeholder": "E.g. Survival with friends",
  "create.version_label": "Game version",
  "create.filter_releases": "Releases",
  "create.filter_snapshots": "Snapshots",
  "create.search_placeholder": "Search version…",
  "create.versions_loading": "Loading version list…",
  "create.versions_empty": "No versions found",
  "create.loader_label": "Mod loader",
  "create.loader_vanilla": "Vanilla",
  "create.loader_fabric": "Fabric",
  "create.loader_forge": "Forge",
  "create.fabric_version_label": "Fabric version",
  "create.fabric_loading": "Loading Fabric versions…",
  "create.fabric_error": "Failed to load Fabric versions",
  "create.fabric_empty": "No Fabric versions found",
  "create.fabric_beta": "beta",
  "create.forge_version_label": "Forge version",
  "create.forge_loading": "Loading Forge versions…",
  "create.forge_error": "Failed to load Forge versions",
  "create.forge_empty": "No Forge versions found",
  "create.forge_recommended": "Recommended",
  "create.forge_unsupported":
    "Forge downloads are currently only available for Minecraft 1.13 and above",
  "create.forge_none": "Forge is not available for this version",
  "create.create": "Create and download",
  "create.cancel": "Cancel",

  "manage.panel_aria": "Account management",
  "manage.button": "Account management",
  "manage.gear_title": "Settings",
  "manage.panel_title": "Accounts",
  "manage.empty": "No saved accounts",
  "manage.current_kind": "now",
  "manage.offline_kind": "offline",
  "manage.add": "Add",
  "manage.signout": "Sign out",
  "manage.switch_title": "Switch to {username}",
  "manage.remove_title": "Delete {username}",
  "manage.remove_disabled": "Switch account or sign out first",
  "manage.ely_badge": "Ely.by account",
  "manage.ely_badge_current": "Ely.by account, currently active",

  "error.unexpected": "Unexpected sign-in error",
  "error.invalid_username": "Invalid nickname \"{username}\" — {reason}",
  "error.unknown_account": "Unknown account \"{username}\"",
  "error.storage": "Storage error: {message}",
  "error.oauth": "Authorization error: {message}",
  "error.ely_api": "Ely.by: {message}",
  "error.cancelled": "Authorization cancelled",
  "error.unexpected_settings": "Unexpected settings error",
  "error.unexpected_instance": "Unexpected instance error",
  "error.instance_network": "Network error: {message}",
  "error.instance_download": "Download error: {message}",
  "error.instance_invalid_name": "Invalid instance name: {message}",
  "error.instance_not_found": "Instance not found",
  "error.instance_busy": "The launcher is busy with another download",
  "error.instance_cancelled": "Download cancelled",
  "error.instance_launch": "Launch error: {message}",
  "error.content_network": "Network error: {message}",
  "error.content_api": "API error: {message}",
  "error.content_not_found": "Not found: {id}",

  "java.idle": "Java not required",
  "java.checking": "Checking Java…",
  "java.ready_system": "System Java {version}",
  "java.ready_bundled": "Bundled Java {version}",
  "java.ready_custom": "Java from settings",
  "java.installing": "Installing Java… {percent}%",
  "java.error": "Java error: {message}",

  "settings.title": "Settings",
  "settings.tab_appearance": "Appearance",
  "settings.tab_game": "Game",
  "settings.theme": "Theme",
  "settings.theme_light": "Light",
  "settings.theme_dark": "Dark",
  "settings.language": "Language",
  "settings.ram": "Memory allocation (MB)",
  "settings.ram_hint":
    "Allowed from {min} to {max} MB; if the minimum exceeds the maximum, the values will be swapped on save.",
  "settings.java": "Java path",
  "settings.java_placeholder": "Empty — auto-detect",
  "settings.java_hint": "Meaning: C:\\Program Files\\Java\\bin\\javaw.exe",
  "settings.save": "Save",
  "settings.cancel": "Cancel",
};

const dictionaries: Record<Language, Dictionary> = { ru, en };

interface LanguageContextValue {
  language: Language;
  t: TranslateFn;
  setLanguage: (language: Language) => void;
}

const LanguageContext = createContext<LanguageContextValue | null>(null);

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [language, setLanguage] = useState<Language>("ru");
  const value = useMemo<LanguageContextValue>(
    () => ({
      language,
      setLanguage,
      t: (key, params) => {
        let text = dictionaries[language][key] ?? key;
        if (params !== undefined) {
          for (const [name, param] of Object.entries(params)) {
            text = text.split(`{${name}}`).join(String(param));
          }
        }
        return text;
      },
    }),
    [language],
  );
  return (
    <LanguageContext.Provider value={value}>{children}</LanguageContext.Provider>
  );
}

export function useI18n(): LanguageContextValue {
  const value = useContext(LanguageContext);
  if (value === null) {
    throw new Error("useI18n must be used within LanguageProvider");
  }
  return value;
}
