import { useEffect, useState } from "react";
import {
  content,
  contentErrorMessage,
  formatBytes,
  type ContentCategory,
  type ContentItem,
  type ContentVersion,
  type InstalledContent,
} from "../lib/content";
import { useI18n } from "../lib/i18n";

interface ContentBrowserModalProps {
  instanceId: string;
  category: ContentCategory;
  onClose: () => void;
}

export default function ContentBrowserModal({
  instanceId,
  category,
  onClose,
}: ContentBrowserModalProps) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<ContentItem[] | null>(null);
  const [versions, setVersions] = useState<ContentVersion[] | null>(null);
  const [selected, setSelected] = useState<ContentItem | null>(null);
  const [installed, setInstalled] = useState<InstalledContent[]>([]);
  const [loading, setLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    content
      .installed(instanceId, category)
      .then((files) => {
        if (!cancelled) {
          setInstalled(files);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setNotice(contentErrorMessage(error, t));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [instanceId, category, t]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    content
      .search(instanceId, category, "")
      .then((found) => {
        if (!cancelled) {
          setItems(found);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setNotice(contentErrorMessage(error, t));
          setItems([]);
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [instanceId, category, t]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape" && !installing) {
        onClose();
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose, installing]);

  function runSearch(nextQuery?: string) {
    const value = (nextQuery ?? query).trim();
    if (loading) {
      return;
    }
    setLoading(true);
    setNotice(null);
    setSelected(null);
    setVersions(null);
    content
      .search(instanceId, category, value)
      .then((found) => {
        setItems(found);
      })
      .catch((error: unknown) => {
        setNotice(contentErrorMessage(error, t));
        setItems([]);
      })
      .finally(() => {
        setLoading(false);
      });
  }

  function openVersions(item: ContentItem) {
    if (loading) {
      return;
    }
    setLoading(true);
    setNotice(null);
    setSelected(item);
    setVersions(null);
    content
      .versions(instanceId, category, item.source, item.projectId)
      .then((found) => {
        setVersions(found);
      })
      .catch((error: unknown) => {
        setNotice(contentErrorMessage(error, t));
        setSelected(null);
      })
      .finally(() => {
        setLoading(false);
      });
  }

  function backToResults() {
    setSelected(null);
    setVersions(null);
  }

  function install(version: ContentVersion) {
    if (installing || selected === null) {
      return;
    }
    setInstalling(true);
    setNotice(null);
    content
      .install(instanceId, category, selected.source, selected.projectId, version.versionId)
      .then((files) => {
        setNotice(t("content.installed_notice", { count: files.length }));
        return content.installed(instanceId, category).then(setInstalled);
      })
      .catch((error: unknown) => {
        setNotice(contentErrorMessage(error, t));
      })
      .finally(() => {
        setInstalling(false);
      });
  }

  function removeFile(fileName: string) {
    if (installing) {
      return;
    }
    setInstalling(true);
    setNotice(null);
    content
      .remove(instanceId, category, fileName)
      .then(() => content.installed(instanceId, category).then(setInstalled))
      .catch((error: unknown) => {
        setNotice(contentErrorMessage(error, t));
      })
      .finally(() => {
        setInstalling(false);
      });
  }

  return (
    <div
      className="shell__overlay"
      role="presentation"
      onClick={(event) => {
        if (event.currentTarget === event.target && !installing) {
          onClose();
        }
      }}
    >
      <div
        className="content"
        role="dialog"
        aria-modal="true"
        aria-label={t(`content.title_${category}`)}
      >
        <div className="settings__head">
          <h2 className="settings__title">{t(`content.title_${category}`)}</h2>
          <button
            className="shell__add-close"
            type="button"
            disabled={installing}
            onClick={onClose}
            title={t("common.close_title")}
          >
            ×
          </button>
        </div>

        <form
          className="content__search"
          onSubmit={(event) => {
            event.preventDefault();
            runSearch();
          }}
        >
          <input
            className="settings__input"
            type="text"
            spellCheck={false}
            value={query}
            placeholder={t("content.search_placeholder")}
            disabled={loading || installing}
            onChange={(event) => setQuery(event.currentTarget.value)}
          />
          <button
            className="settings__save"
            type="submit"
            disabled={loading || installing || query.trim() === ""}
          >
            {t("content.search_button")}
          </button>
        </form>

        {notice !== null && <p className="settings__notice">{notice}</p>}

        {selected !== null ? (
          <div className="content__list">
            <button
              className="content__back"
              type="button"
              disabled={installing}
              onClick={backToResults}
            >
              ← {t("content.back")}
            </button>
            {loading ? (
              <p className="instances__hint">{t("common.loading")}</p>
            ) : versions !== null && versions.length === 0 ? (
              <p className="instances__hint">{t("content.empty_versions")}</p>
            ) : (
              versions?.map((version) => (
                <div key={version.versionId} className="content__row">
                  <div className="content__row-main">
                    <span className="content__row-name">{version.name}</span>
                    <span className="content__row-sub">
                      {version.fileName}
                      {version.fileSize > 0 ? ` · ${formatBytes(version.fileSize)}` : ""}
                    </span>
                  </div>
                  <button
                    className="settings__save"
                    type="button"
                    disabled={installing}
                    onClick={() => install(version)}
                  >
                    {installing ? t("content.installing") : t("content.install")}
                  </button>
                </div>
              ))
            )}
          </div>
        ) : (
          <div className="content__list">
            {loading ? (
              <p className="instances__hint">{t("common.loading")}</p>
            ) : items !== null && items.length === 0 ? (
              <p className="instances__hint">{t("content.empty_results")}</p>
            ) : (
              items?.map((item) => (
                <button
                  key={`${item.source}:${item.projectId}`}
                  className="content__row content__row--clickable"
                  type="button"
                  disabled={loading || installing}
                  onClick={() => openVersions(item)}
                >
                  {item.iconUrl !== null && (
                    <img className="content__icon" src={item.iconUrl} alt="" />
                  )}
                  <span className="content__row-main">
                    <span className="content__row-name">
                      {item.name}
                      <span
                        className={`content__badge content__badge--${item.source}`}
                      >
                        {t(`content.source_${item.source}`)}
                      </span>
                    </span>
                    <span className="content__row-sub">
                      {item.author !== "" ? `${item.author} · ` : ""}
                      {item.description}
                    </span>
                  </span>
                  <span className="content__row-meta">
                    {item.downloads.toLocaleString()}
                  </span>
                </button>
              ))
            )}
          </div>
        )}

        <div className="settings__section">
          <p className="settings__label">{t("content.installed_label")}</p>
          {installed.length === 0 ? (
            <p className="instances__hint">{t("content.empty_installed")}</p>
          ) : (
            <div className="content__list content__list--installed">
              {installed.map((file) => (
                <div key={file.fileName} className="content__row">
                  <span className="content__row-main">
                    <span className="content__row-name">{file.fileName}</span>
                    <span className="content__row-sub">
                      {formatBytes(file.sizeBytes)}
                    </span>
                  </span>
                  <button
                    className="instcard__remove"
                    type="button"
                    disabled={installing}
                    title={t("content.remove_title")}
                    onClick={() => removeFile(file.fileName)}
                  >
                    ×
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
