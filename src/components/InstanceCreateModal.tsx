import { useEffect, useMemo, useState } from "react";
import type { FormEvent } from "react";
import {
  instances,
  instanceErrorMessage,
  type FabricLoaderVersion,
  type ForgeLoaderVersion,
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

/**
 * Why the Forge button is blocked for a given version: `"old"` — no Forge
 * existed before 1.13 (and previews); `"none"` — the year-based versions
 * (26.1+) Forge simply never shipped; `null` — Forge is available.
 */
function forgeBlockReason(versionId: string | null, versions: VersionInfo[] | null): "old" | "none" | null {
  if (versionId === null) {
    return "old";
  }
  const version = versions?.find((entry) => entry.id === versionId);
  if (!version || version.type !== "release") {
    return "old";
  }
  const major = /^\d+(?=\.)/.exec(version.id);
  if (major && Number(major[0]) >= 26) {
    return "none";
  }
  const minor = /^1\.(\d+)/.exec(version.id);
  if (!minor || Number(minor[1]) < 13) {
    return "old";
  }
  return null;
}

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
  const [forgeVersions, setForgeVersions] = useState<
    Record<string, ForgeLoaderVersion[] | "error">
  >({});
  const [forgeChoice, setForgeChoice] = useState<string | null>(null);
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

  const forgeCanLoad = loader === "forge" && versionId !== null;
  const forgeList = forgeCanLoad ? forgeVersions[versionId] : undefined;

  useEffect(() => {
    if (!forgeCanLoad) {
      return;
    }
    if (forgeVersions[versionId] !== undefined) {
      return;
    }
    let cancelled = false;
    instances
      .forgeLoaderVersions(versionId)
      .then((loaded) => {
        if (!cancelled) {
          setForgeVersions((previous) => ({
            ...previous,
            [versionId]: loaded,
          }));
        }
      })
      .catch(() => {
        if (!cancelled) {
          setForgeVersions((previous) => ({
            ...previous,
            [versionId]: "error",
          }));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [forgeCanLoad, versionId, forgeVersions]);

  const selectedForgeVersion = Array.isArray(forgeList)
    ? (forgeList.find((entry) => entry.version === forgeChoice) ?? forgeList[0])
    : undefined;

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (versionId === null || busy) {
      return;
    }
    const loaderVersion =
      loader === "fabric"
        ? (selectedFabricVersion?.version ?? null)
        : loader === "forge"
          ? (selectedForgeVersion?.version ?? null)
          : null;
    if (loader !== "vanilla" && loaderVersion === null) {
      return;
    }
    setBusy(true);
    setNotice(null);
    instances
      .create({
        name: name.trim() === "" ? versionId : name.trim(),
        versionId,
        loader,
        loaderVersion,
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
                    setForgeChoice(null);
                    if (forgeBlockReason(version.id, versions) !== null) {
                      setLoader((current) =>
                        current === "forge" ? "vanilla" : current,
                      );
                    }
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
            <span
              className="instcreate__loaderwrap"
              data-blocked={forgeBlockReason(versionId, versions) !== null}
            >
              <button
                className={`settings__segment-btn${loader === "forge" ? " settings__segment-btn--active" : ""}`}
                type="button"
                disabled={busy || forgeBlockReason(versionId, versions) !== null}
                onClick={() => setLoader("forge")}
              >
                {t("create.loader_forge")}
              </button>
              {forgeBlockReason(versionId, versions) === "old" && (
                <span className="instcreate__loaderhint" role="tooltip">
                  {t("create.forge_unsupported")}
                </span>
              )}
              {forgeBlockReason(versionId, versions) === "none" && (
                <span className="instcreate__loaderhint" role="tooltip">
                  {t("create.forge_none")}
                </span>
              )}
            </span>
          </div>
          {loader === "forge" && (
            <div className="instcreate__fabric">
              <p className="settings__label">{t("create.forge_version_label")}</p>
              {forgeList === undefined ? (
                <p className="instcreate__hint">{t("create.forge_loading")}</p>
              ) : forgeList === "error" ? (
                <p className="settings__notice">{t("create.forge_error")}</p>
              ) : forgeList.length === 0 ? (
                <p className="instcreate__hint">{t("create.forge_empty")}</p>
              ) : (
                <select
                  className="settings__input"
                  value={selectedForgeVersion?.version ?? ""}
                  disabled={busy}
                  onChange={(event) =>
                    setForgeChoice(event.currentTarget.value)
                  }
                >
                  {forgeList.map((entry) => (
                    <option key={entry.version} value={entry.version}>
                      {entry.version}
                      {entry.recommended
                        ? ` (${t("create.forge_recommended")})`
                        : ""}
                    </option>
                  ))}
                </select>
              )}
            </div>
          )}
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
              (loader === "fabric" && selectedFabricVersion === undefined) ||
              (loader === "forge" && selectedForgeVersion === undefined)
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
