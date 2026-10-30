import { useEffect, useState } from "react";
import {
  instances,
  instanceErrorMessage,
  onDownloadProgress,
  onRunState,
  parseInstanceError,
  type DownloadProgress,
  type Instance,
} from "../lib/instances";
import { settings, settingsErrorMessage } from "../lib/settings";
import { javaStatusText, useJavaStatus } from "../lib/java";
import { useI18n } from "../lib/i18n";
import InstanceCreateModal from "./InstanceCreateModal";
import InstanceSettingsModal from "./InstanceSettingsModal";

export default function InstancesView() {
  const { t } = useI18n();
  const javaStatus = useJavaStatus();
  const [list, setList] = useState<Instance[] | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [downloading, setDownloading] = useState<
    Record<string, DownloadProgress>
  >({});
  const [createOpen, setCreateOpen] = useState(false);
  const [pendingRemove, setPendingRemove] = useState<Instance | null>(null);
  const [settingsTarget, setSettingsTarget] = useState<Instance | null>(null);
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState<Record<string, boolean>>({});

  useEffect(() => {
    let cancelled = false;
    instances
      .list()
      .then((loaded) => {
        if (!cancelled) {
          setList(loaded);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setList([]);
          setNotice(instanceErrorMessage(error, t));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let active = true;
    onDownloadProgress((progress) => {
      if (!active) {
        return;
      }
      setDownloading((previous) => ({
        ...previous,
        [progress.instanceId]: progress,
      }));
    })
      .then((stop) => {
        if (active) {
          unlisten = stop;
        } else {
          stop();
        }
      })
      .catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let active = true;
    instances
      .runningIds()
      .then((ids) => {
        if (active) {
          setRunning(Object.fromEntries(ids.map((id) => [id, true])));
        }
      })
      .catch(() => undefined);
    onRunState((state) => {
      if (!active) {
        return;
      }
      if (state.error) {
        setNotice(state.error);
      }
      setRunning((previous) => {
        const next = { ...previous };
        if (state.running) {
          next[state.instanceId] = true;
        } else {
          delete next[state.instanceId];
        }
        return next;
      });
    })
      .then((stop) => {
        if (active) {
          unlisten = stop;
        } else {
          stop();
        }
      })
      .catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  function refresh() {
    instances
      .list()
      .then(setList)
      .catch(() => undefined);
  }

  async function startDownload(id: string): Promise<void> {
    setBusy(true);
    setNotice(null);
    try {
      await instances.download(id);
    } catch (error: unknown) {
      if (parseInstanceError(error)?.type !== "cancelled") {
        setNotice(instanceErrorMessage(error, t));
      }
    } finally {
      setBusy(false);
      setDownloading((previous) => {
        const next = { ...previous };
        delete next[id];
        return next;
      });
      refresh();
    }
  }

  async function cancelDownload(id: string): Promise<void> {
    await instances.cancelDownload(id).catch(() => undefined);
  }

  async function startLaunch(id: string): Promise<void> {
    setNotice(null);
    try {
      await instances.launch(id);
    } catch (error: unknown) {
      setNotice(instanceErrorMessage(error, t));
    }
  }

  async function confirmRemove(): Promise<void> {
    if (pendingRemove === null) {
      return;
    }
    const target = pendingRemove;
    setBusy(true);
    setNotice(null);
    try {
      await instances.remove(target.id);
      setPendingRemove(null);
      refresh();
    } catch (error: unknown) {
      setNotice(instanceErrorMessage(error, t));
    } finally {
      setBusy(false);
    }
  }

  function handleCreated(instance: Instance) {
    setCreateOpen(false);
    refresh();
    void startDownload(instance.id);
  }

  function openLauncherHome() {
    settings
      .openHome()
      .catch((error: unknown) => {
        setNotice(settingsErrorMessage(error, t));
      });
  }

  return (
    <section className="instances">
      <div className="instances__head">
        <h2 className="instances__title">{t("instances.title")}</h2>
        <div className="instances__head-actions">
          <button
            className="instances__icon-btn"
            type="button"
            title={t("instances.open_home")}
            onClick={openLauncherHome}
          >
            📁
          </button>
          <button
            className="instances__add"
            type="button"
            onClick={() => setCreateOpen(true)}
          >
            {t("instances.add")}
          </button>
        </div>
      </div>
      {notice !== null && <p className="instances__notice">{notice}</p>}
      {(javaStatus?.type === "installing" || javaStatus?.type === "error") && (
        <p
          className={`instances__java${javaStatus.type === "error" ? " instances__java--error" : ""}`}
        >
          {javaStatusText(javaStatus, t)}
        </p>
      )}
      {list === null ? (
        <p className="instances__hint">{t("common.loading")}</p>
      ) : list.length === 0 ? (
        <p className="instances__hint">{t("instances.empty")}</p>
      ) : (
        <div className="instances__grid">
          {list.map((instance) => {
            const progress = downloading[instance.id];
            const isRunning = running[instance.id] === true;
            const percent =
              progress === undefined
                ? 0
                : progress.bytesTotal > 0
                  ? Math.round((progress.bytesDone / progress.bytesTotal) * 100)
                  : progress.total > 0
                    ? Math.round((progress.done / progress.total) * 100)
                    : 0;
            return (
              <article
                key={instance.id}
                className={`instcard${instance.downloaded ? " instcard--ready" : ""}`}
              >
                <div className="instcard__top">
                  <span className="instcard__name">{instance.name}</span>
                  <div className="instcard__top-actions">
                    <button
                      className="instcard__settings"
                      type="button"
                      disabled={busy}
                      title={t("instances.card_settings")}
                      onClick={() => setSettingsTarget(instance)}
                    >
                      ⚙
                    </button>
                    <button
                      className="instcard__remove"
                      type="button"
                      disabled={busy}
                      title={t("instances.remove_title")}
                      onClick={() => setPendingRemove(instance)}
                    >
                      ×
                    </button>
                  </div>
                </div>
                <p className="instcard__version">
                  {instance.versionId}
                  {instance.loader === "fabric" && instance.loaderVersion
                    ? ` · Fabric ${instance.loaderVersion}`
                    : instance.loader === "forge" && instance.loaderVersion
                      ? ` · Forge ${instance.loaderVersion}`
                      : instance.loader === "vanilla"
                        ? ` · ${t("instances.badge_vanilla")}`
                        : ""}
                </p>
                {progress !== undefined ? (
                  <div className="instcard__progress-wrap">
                    <div className="instcard__progress">
                      <div
                        className="instcard__progress-fill"
                        style={{ width: `${percent}%` }}
                      />
                    </div>
                    <p className="instcard__progress-label">
                      {percent}% ·{" "}
                      {progress.stage === "jar"
                        ? t("instances.stage_jar")
                        : progress.stage === "libraries"
                          ? t("instances.stage_libraries")
                          : progress.stage === "forge"
                            ? t("instances.stage_forge")
                            : t("instances.stage_assets")}
                    </p>
                  </div>
                ) : (
                  <p className="instcard__status">
                    {isRunning
                      ? t("instances.running")
                      : instance.downloaded
                        ? t("instances.play_ready")
                        : t("instances.not_downloaded")}
                  </p>
                )}
                <div className="instcard__actions">
                  {progress !== undefined ? (
                    <button
                      className="instcard__btn instcard__btn--cancel"
                      type="button"
                      onClick={() => void cancelDownload(instance.id)}
                    >
                      {t("instances.cancel_download")}
                    </button>
                  ) : !instance.downloaded ? (
                    <button
                      className="instcard__btn"
                      type="button"
                      disabled={busy}
                      onClick={() => void startDownload(instance.id)}
                    >
                      {t("instances.download")}
                    </button>
                  ) : (
                    <button
                      className="instcard__btn instcard__btn--play"
                      type="button"
                      disabled={isRunning}
                      onClick={() => void startLaunch(instance.id)}
                    >
                      {isRunning ? t("instances.running") : t("instances.play")}
                    </button>
                  )}
                </div>
              </article>
            );
          })}
        </div>
      )}
      {createOpen && (
        <InstanceCreateModal
          onClose={() => setCreateOpen(false)}
          onCreated={handleCreated}
        />
      )}
      {settingsTarget !== null && (
        <InstanceSettingsModal
          instance={settingsTarget}
          onClose={() => setSettingsTarget(null)}
          onChanged={(updated) => {
            setSettingsTarget(null);
            setList((previous) =>
              previous === null
                ? previous
                : previous.map((item) =>
                    item.id === updated.id ? updated : item,
                  ),
            );
          }}
        />
      )}
      {pendingRemove !== null && (
        <div
          className="shell__overlay"
          role="presentation"
          onClick={(event) => {
            if (event.currentTarget === event.target && !busy) {
              setPendingRemove(null);
            }
          }}
        >
          <div
            className="instremove"
            role="dialog"
            aria-modal="true"
            aria-label={t("instances.remove_title")}
          >
            <p className="instremove__text">
              {t("instances.remove_confirm", { name: pendingRemove.name })}
            </p>
            <div className="instremove__actions">
              <button
                className="instremove__yes"
                type="button"
                disabled={busy}
                onClick={() => void confirmRemove()}
              >
                {t("instances.remove_confirm_yes")}
              </button>
              <button
                className="instremove__no"
                type="button"
                disabled={busy}
                onClick={() => setPendingRemove(null)}
              >
                {t("instances.remove_confirm_no")}
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
