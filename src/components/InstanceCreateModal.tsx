import { useEffect, useMemo, useState } from "react";
import type { FormEvent } from "react";
import {
  instances,
  instanceErrorMessage,
  type FabricLoaderVersion,
  type Instance,
  type Loader,
  type VersionInfo,
} from "../lib/instances";
import { useI18n } from "../lib/i18n";

interface InstanceCreateModalProps {
  onClose: () => void;
  onCreated: (instance: Instance) => void;
}

type VersionFilter = "release" | "snapshot";

export default function InstanceCreateModal({
  onClose,
  onCreated,
}: InstanceCreateModalProps) {
  const { t } = useI18n();
  const [name, setName] = useState("");
  const [filter, setFilter] = useState<VersionFilter>("release");
  const [search, setSearch] = useState("");
  const [versions, setVersions] = useState<VersionInfo[] | null>(null);
  const [versionsError, setVersionsError] = useState<string | null>(null);
  const [versionId, setVersionId] = useState<string | null>(null);
  const [loader, setLoader] = useState<Loader>("vanilla");
  const [fabricVersions, setFabricVersions] = useState<
    Record<string, FabricLoaderVersion[] | "error">
  >({});
  const [fabricChoice, setFabricChoice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    instances
      .versions()
      .then((loaded) => {
        if (!cancelled) {
          setVersions(loaded);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setVersionsError(instanceErrorMessage(error, t));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

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

  const filtered = useMemo(() => {
    if (versions === null) {
      return [];
    }
    const query = search.trim().toLowerCase();
    return versions.filter(
      (version) =>
        version.type === filter &&
        (query === "" || version.id.toLowerCase().includes(query)),
    );
  }, [versions, filter, search]);

  useEffect(() => {
    if (filtered.length > 0 && (versionId === null || !filtered.some((version) => version.id === versionId))) {
      setVersionId(filtered[0].id);
    }
  }, [filtered, versionId]);

  const fabricList =
    loader === "fabric" && versionId !== null
      ? fabricVersions[versionId]
      : undefined;

  useEffect(() => {
    if (loader !== "fabric" || versionId === null) {
      return;
    }
    if (fabricVersions[versionId] !== undefined) {
      return;
    }
    let cancelled = false;
    instances
      .fabricLoaderVersions(versionId)
      .then((loaded) => {
        if (!cancelled) {
          setFabricVersions((previous) => ({
            ...previous,
            [versionId]: loaded,
          }));
        }
      })
      .catch(() => {
        if (!cancelled) {
          setFabricVersions((previous) => ({
            ...previous,
            [versionId]: "error",
          }));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [loader, versionId, fabricVersions]);

  const selectedFabricVersion = Array.isArray(fabricList)
    ? (fabricList.find((entry) => entry.version === fabricChoice) ??
      fabricList.find((entry) => entry.stable) ??
      fabricList[0])
    : undefined;

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (versionId === null || busy) {
      return;
    }
    const fabricVersion =
      loader === "fabric" ? (selectedFabricVersion?.version ?? null) : null;
    if (loader === "fabric" && fabricVersion === null) {
      return;
    }
    setBusy(true);
    setNotice(null);
    instances
      .create({
        name: name.trim() === "" ? versionId : name.trim(),
        versionId,
        loader,
        loaderVersion: fabricVersion,
      })
      .then(onCreated)
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
      <form
        className="instcreate"
        role="dialog"
        aria-modal="true"
        aria-label={t("create.aria")}
        onSubmit={handleSubmit}
      >
        <div className="settings__head">
          <h2 className="settings__title">{t("create.title")}</h2>
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
          <p className="settings__label">{t("create.name_label")}</p>
          <input
            className="settings__input"
            type="text"
            placeholder={t("create.name_placeholder")}
            maxLength={64}
            spellCheck={false}
            autoFocus
            value={name}
            disabled={busy}
            onChange={(event) => setName(event.currentTarget.value)}
          />
        </div>

        <div className="settings__section">
          <p className="settings__label">{t("create.version_label")}</p>
          <div className="instcreate__filters">
            <div className="settings__segment">
              <button
                className={`settings__segment-btn${filter === "release" ? " settings__segment-btn--active" : ""}`}
                type="button"
                disabled={busy}
                onClick={() => setFilter("release")}
              >
                {t("create.filter_releases")}
              </button>
              <button
                className={`settings__segment-btn${filter === "snapshot" ? " settings__segment-btn--active" : ""}`}
                type="button"
                disabled={busy}
                onClick={() => setFilter("snapshot")}
              >
                {t("create.filter_snapshots")}
              </button>
            </div>
            <input
              className="settings__input instcreate__search"
              type="text"
              placeholder={t("create.search_placeholder")}
              spellCheck={false}
              value={search}
              disabled={busy}
              onChange={(event) => setSearch(event.currentTarget.value)}
            />
          </div>
          {versionsError !== null ? (
            <p className="settings__notice">{versionsError}</p>
          ) : versions === null ? (
            <p className="instcreate__hint">{t("create.versions_loading")}</p>
          ) : filtered.length === 0 ? (
            <p className="instcreate__hint">{t("create.versions_empty")}</p>
          ) : (
            <div className="instcreate__versions">
              {filtered.map((version) => (
                <button
                  key={version.id}
                  className={`instcreate__version${versionId === version.id ? " instcreate__version--active" : ""}`}
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    setVersionId(version.id);
                    setFabricChoice(null);
                  }}
                >
                  <span className="instcreate__version-id">{version.id}</span>
                  <span className="instcreate__version-date">
                    {version.releaseTime.slice(0, 10)}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>

        <div className="settings__section">
          <p className="settings__label">{t("create.loader_label")}</p>
          <div className="settings__segment">
            <button
              className={`settings__segment-btn${loader === "vanilla" ? " settings__segment-btn--active" : ""}`}
              type="button"
              disabled={busy}
              onClick={() => setLoader("vanilla")}
            >
              {t("create.loader_vanilla")}
            </button>
            <button
              className={`settings__segment-btn${loader === "fabric" ? " settings__segment-btn--active" : ""}`}
              type="button"
              disabled={busy}
              onClick={() => setLoader("fabric")}
            >
              {t("create.loader_fabric")}
            </button>
          </div>
          {loader === "fabric" && (
            <div className="instcreate__fabric">
              <p className="settings__label">{t("create.fabric_version_label")}</p>
              {fabricList === undefined ? (
                <p className="instcreate__hint">{t("create.fabric_loading")}</p>
              ) : fabricList === "error" ? (
                <p className="settings__notice">{t("create.fabric_error")}</p>
              ) : fabricList.length === 0 ? (
                <p className="instcreate__hint">{t("create.fabric_empty")}</p>
              ) : (
                <select
                  className="settings__input"
                  value={selectedFabricVersion?.version ?? ""}
                  disabled={busy}
                  onChange={(event) =>
                    setFabricChoice(event.currentTarget.value)
                  }
                >
                  {fabricList.map((entry) => (
                    <option key={entry.version} value={entry.version}>
                      {entry.version}
                      {entry.stable ? "" : ` (${t("create.fabric_beta")})`}
                    </option>
                  ))}
                </select>
              )}
            </div>
          )}
        </div>

        {notice !== null && <p className="settings__notice">{notice}</p>}

        <div className="settings__footer">
          <button
            className="settings__save"
            type="submit"
            disabled={
              busy ||
              versionId === null ||
              (loader === "fabric" && selectedFabricVersion === undefined)
            }
          >
            {t("create.create")}
          </button>
          <button
            className="settings__cancel"
            type="button"
            disabled={busy}
            onClick={onClose}
          >
            {t("create.cancel")}
          </button>
        </div>
      </form>
    </div>
  );
}
