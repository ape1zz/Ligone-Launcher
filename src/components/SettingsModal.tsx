import { useEffect, useState } from "react";
import {
  applyTheme,
  DEFAULT_SETTINGS,
  RAM_MAX_LIMIT_MB,
  RAM_MIN_LIMIT_MB,
  settings,
  settingsErrorMessage,
  type Theme,
} from "../lib/settings";
import { useI18n } from "../lib/i18n";
import { javaStatusText, useJavaStatus } from "../lib/java";

interface SettingsModalProps {
  onClose: () => void;
}

type SettingsTab = "appearance" | "game";

function parseRam(raw: string, fallback: number): number {
  const parsed = Number.parseInt(raw, 10);
  return Number.isNaN(parsed) ? fallback : parsed;
}

export default function SettingsModal({ onClose }: SettingsModalProps) {
  const { t, language, setLanguage } = useI18n();
  const javaStatus = useJavaStatus();
  const [tab, setTab] = useState<SettingsTab>("appearance");
  const [javaPath, setJavaPath] = useState("");
  const [ramMin, setRamMin] = useState(String(DEFAULT_SETTINGS.ramMinMb));
  const [ramMax, setRamMax] = useState(String(DEFAULT_SETTINGS.ramMaxMb));
  const [theme, setTheme] = useState<Theme>("light");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    settings
      .get()
      .then((loaded) => {
        if (cancelled) {
          return;
        }
        setJavaPath(loaded.javaPath);
        setRamMin(String(loaded.ramMinMb));
        setRamMax(String(loaded.ramMaxMb));
        setTheme(loaded.theme);
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setNotice(settingsErrorMessage(error, t));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose]);

  async function save(): Promise<void> {
    setBusy(true);
    setNotice(null);
    try {
      const saved = await settings.set({
        javaPath,
        ramMinMb: parseRam(ramMin, DEFAULT_SETTINGS.ramMinMb),
        ramMaxMb: parseRam(ramMax, DEFAULT_SETTINGS.ramMaxMb),
        theme,
        language,
      });
      applyTheme(saved.theme);
      setLanguage(saved.language);
      onClose();
    } catch (error: unknown) {
      setNotice(settingsErrorMessage(error, t));
      setBusy(false);
    }
  }

  return (
    <div
      className="shell__overlay"
      role="presentation"
      onClick={(event) => {
        if (event.currentTarget === event.target) {
          onClose();
        }
      }}
    >
      <div
        className="settings"
        role="dialog"
        aria-modal="true"
        aria-label={t("settings.title")}
      >
        <div className="settings__head">
          <h2 className="settings__title">{t("settings.title")}</h2>
          <button
            className="shell__add-close"
            type="button"
            disabled={busy}
            onClick={onClose}
            title={t("common.close_title")}
          >
            ×
          </button>
        </div>

        <div className="settings__tabs">
          <button
            className={`settings__tab-btn${tab === "appearance" ? " settings__tab-btn--active" : ""}`}
            type="button"
            disabled={busy}
            onClick={() => setTab("appearance")}
          >
            {t("settings.tab_appearance")}
          </button>
          <button
            className={`settings__tab-btn${tab === "game" ? " settings__tab-btn--active" : ""}`}
            type="button"
            disabled={busy}
            onClick={() => setTab("game")}
          >
            {t("settings.tab_game")}
          </button>
        </div>

        {tab === "appearance" ? (
          <>
            <div className="settings__section">
              <p className="settings__label">{t("settings.theme")}</p>
              <div className="settings__segment">
                <button
                  className={`settings__segment-btn${theme === "light" ? " settings__segment-btn--active" : ""}`}
                  type="button"
                  disabled={busy}
                  onClick={() => setTheme("light")}
                >
                  {t("settings.theme_light")}
                </button>
                <button
                  className={`settings__segment-btn${theme === "dark" ? " settings__segment-btn--active" : ""}`}
                  type="button"
                  disabled={busy}
                  onClick={() => setTheme("dark")}
                >
                  {t("settings.theme_dark")}
                </button>
              </div>
            </div>
            <div className="settings__section">
              <p className="settings__label">{t("settings.language")}</p>
              <div className="settings__segment">
                <button
                  className={`settings__segment-btn${language === "ru" ? " settings__segment-btn--active" : ""}`}
                  type="button"
                  disabled={busy}
                  onClick={() => setLanguage("ru")}
                >
                  Русский
                </button>
                <button
                  className={`settings__segment-btn${language === "en" ? " settings__segment-btn--active" : ""}`}
                  type="button"
                  disabled={busy}
                  onClick={() => setLanguage("en")}
                >
                  English
                </button>
              </div>
            </div>
          </>
        ) : (
          <>
            <div className="settings__section">
              <p className="settings__label">{t("settings.java")}</p>
              <input
                className="settings__input"
                type="text"
                placeholder={t("settings.java_placeholder")}
                spellCheck={false}
                value={javaPath}
                disabled={busy}
                onChange={(event) => setJavaPath(event.currentTarget.value)}
              />
              <p className="settings__hint">{t("settings.java_hint")}</p>
              <p className="java-status">{javaStatusText(javaStatus, t)}</p>
            </div>
            <div className="settings__section">
              <p className="settings__label">{t("settings.ram")}</p>
              <div className="settings__ram">
                <input
                  className="settings__input"
                  type="number"
                  min={RAM_MIN_LIMIT_MB}
                  max={RAM_MAX_LIMIT_MB}
                  title="min"
                  value={ramMin}
                  disabled={busy}
                  onChange={(event) => setRamMin(event.currentTarget.value)}
                />
                <span className="settings__ram-dash">—</span>
                <input
                  className="settings__input"
                  type="number"
                  min={RAM_MIN_LIMIT_MB}
                  max={RAM_MAX_LIMIT_MB}
                  title="max"
                  value={ramMax}
                  disabled={busy}
                  onChange={(event) => setRamMax(event.currentTarget.value)}
                />
              </div>
              <p className="settings__hint">
                {t("settings.ram_hint", {
                  min: RAM_MIN_LIMIT_MB,
                  max: RAM_MAX_LIMIT_MB,
                })}
              </p>
            </div>
          </>
        )}

        {notice !== null && <p className="settings__notice">{notice}</p>}

        <div className="settings__footer">
          <button
            className="settings__save"
            type="button"
            disabled={busy}
            onClick={() => void save()}
          >
            {t("settings.save")}
          </button>
          <button
            className="settings__cancel"
            type="button"
            disabled={busy}
            onClick={onClose}
          >
            {t("settings.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
