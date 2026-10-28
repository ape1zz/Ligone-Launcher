import { useEffect, useState } from "react";
import {
  instances,
  instanceErrorMessage,
  type Instance,
} from "../lib/instances";
import type { ContentCategory } from "../lib/content";
import { useI18n } from "../lib/i18n";
import ContentBrowserModal from "./ContentBrowserModal";

interface InstanceSettingsModalProps {
  instance: Instance;
  onClose: () => void;
  onChanged: (instance: Instance) => void;
}

const FOLDERS = [
  { key: "mods", labelKey: "instset.open_mods" },
  { key: "resourcepacks", labelKey: "instset.open_resourcepacks" },
  { key: "shaderpacks", labelKey: "instset.open_shaderpacks" },
  { key: "saves", labelKey: "instset.open_saves" },
] as const;

const CONTENT_FOLDERS: readonly string[] = ["mods", "resourcepacks", "shaderpacks"];

export default function InstanceSettingsModal({
  instance,
  onClose,
  onChanged,
}: InstanceSettingsModalProps) {
  const { t } = useI18n();
  const [name, setName] = useState(instance.name);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [contentCategory, setContentCategory] = useState<ContentCategory | null>(
    null,
  );

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape" && !busy) {
        onClose();
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose, busy]);

  function runAction(action: () => Promise<unknown>) {
    if (busy) {
      return;
    }
    setBusy(true);
    setNotice(null);
    action()
      .catch((error: unknown) => {
        setNotice(instanceErrorMessage(error, t));
      })
      .finally(() => {
        setBusy(false);
      });
  }

  function openFolder(sub: string | null) {
    runAction(() => instances.openFolder(instance.id, sub));
  }

  function saveName() {
    const trimmed = name.trim();
    if (trimmed === "" || trimmed === instance.name) {
      onClose();
      return;
    }
    setBusy(true);
    setNotice(null);
    instances
      .rename(instance.id, trimmed)
      .then((updated) => {
        onChanged(updated);
        onClose();
      })
      .catch((error: unknown) => {
        setNotice(instanceErrorMessage(error, t));
        setBusy(false);
      });
  }

  return (
    <div
      className="shell__overlay"
      role="presentation"
      onClick={(event) => {
        if (event.currentTarget === event.target && !busy) {
          onClose();
        }
      }}
    >
      <div
        className="settings"
        role="dialog"
        aria-modal="true"
        aria-label={t("instset.aria")}
      >
        <div className="settings__head">
          <h2 className="settings__title">{t("instset.title")}</h2>
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

        <div className="settings__section">
          <p className="settings__label">{t("instset.version_label")}</p>
          <p className="instset__version">
            {instance.versionId}
            {instance.loader === "fabric" && instance.loaderVersion
              ? ` · Fabric ${instance.loaderVersion}`
              : instance.loader === "vanilla"
                ? ` · ${t("instances.badge_vanilla")}`
                : ""}
          </p>
        </div>

        <div className="settings__section">
          <p className="settings__label">{t("create.name_label")}</p>
          <input
            className="settings__input"
            type="text"
            maxLength={64}
            spellCheck={false}
            value={name}
            disabled={busy}
            onChange={(event) => setName(event.currentTarget.value)}
          />
        </div>

        <div className="settings__section">
          <p className="settings__label">{t("instset.folders_label")}</p>
          <div className="instset__rows">
            <button
              className="instset__row"
              type="button"
              disabled={busy}
              onClick={() => openFolder(null)}
            >
              <span className="instset__row-icon">📁</span>
              <span>{t("instset.open_root")}</span>
            </button>
            {FOLDERS.map((folder) => (
              <div key={folder.key} className="instset__row-wrap">
                <button
                  className="instset__row"
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    CONTENT_FOLDERS.includes(folder.key)
                      ? setContentCategory(folder.key as ContentCategory)
                      : openFolder(folder.key)
                  }
                >
                  <span>{t(folder.labelKey)}</span>
                </button>
                {CONTENT_FOLDERS.includes(folder.key) && (
                  <button
                    className="instset__row-folder"
                    type="button"
                    disabled={busy}
                    title={t("instset.open_folder_title")}
                    onClick={() => openFolder(folder.key)}
                  >
                    📁
                  </button>
                )}
              </div>
            ))}
          </div>
        </div>

        {contentCategory !== null && (
        <ContentBrowserModal
          instanceId={instance.id}
          category={contentCategory}
          onClose={() => setContentCategory(null)}
        />
      )}

      {notice !== null && <p className="settings__notice">{notice}</p>}

        <div className="settings__footer">
          <button
            className="settings__save"
            type="button"
            disabled={busy || name.trim() === ""}
            onClick={saveName}
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
