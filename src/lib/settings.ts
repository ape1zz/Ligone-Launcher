import { invoke } from "@tauri-apps/api/core";
import type { Language } from "./i18n";

export type Theme = "light" | "dark";

export interface Settings {
  javaPath: string;
  ramMinMb: number;
  ramMaxMb: number;
  theme: Theme;
  language: Language;
}

export type SettingsError = { type: "storage"; message: string };

export function parseSettingsError(error: unknown): SettingsError | null {
  if (typeof error !== "object" || error === null) {
    return null;
  }
  if ((error as SettingsError).type === "storage") {
    return error as SettingsError;
  }
  return null;
}

export function settingsErrorMessage(
  error: unknown,
  t: (key: string, params?: Record<string, string | number>) => string,
): string {
  const parsed = parseSettingsError(error);
  return parsed === null
    ? t("error.unexpected_settings")
    : t("error.storage", { message: parsed.message });
}

export const DEFAULT_SETTINGS: Settings = {
  javaPath: "",
  ramMinMb: 1024,
  ramMaxMb: 4096,
  theme: "light",
  language: "ru",
};

export const RAM_MIN_LIMIT_MB = 256;
export const RAM_MAX_LIMIT_MB = 65_536;

export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}

export interface SettingsService {
  get(): Promise<Settings>;
  set(settings: Settings): Promise<Settings>;
  openHome(): Promise<void>;
}

export const settings: SettingsService = {
  get: () => invoke("settings_get"),
  set: (next) => invoke("settings_set", { settings: next }),
  openHome: () => invoke("settings_open_home"),
};
