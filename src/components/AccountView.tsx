import { useEffect, useRef, useState } from "react";
import { auth, authErrorMessage, type Account } from "../lib/auth";
import { useI18n } from "../lib/i18n";
import InstancesView from "./InstancesView";
import LoginScreen from "./LoginScreen";
import SettingsModal from "./SettingsModal";
import logo from "../assets/launcher-logo.png";

interface AccountViewProps {
  account: Account;
  onLoggedOut: () => void;
  onSwitched: (account: Account) => void;
}

export default function AccountView({
  account,
  onLoggedOut,
  onSwitched,
}: AccountViewProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [saved, setSaved] = useState<Account[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    function handlePointerDown(event: PointerEvent) {
      if (rootRef.current !== null && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
      }
    }
    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open]);

  useEffect(() => {
    if (!addOpen) {
      return;
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setAddOpen(false);
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [addOpen]);

  function handleAddedAccount(received: Account) {
    setAddOpen(false);
    onSwitched(received);
  }

  function togglePanel() {
    if (busy) {
      return;
    }
    const next = !open;
    setOpen(next);
    if (next) {
      setNotice(null);
      setSaved(null);
      void auth
        .listSavedAccounts()
        .then(setSaved)
        .catch((error: unknown) => {
          setSaved([]);
          setNotice(authErrorMessage(error, t));
        });
    }
  }

  async function switchTo(item: Account): Promise<void> {
    setBusy(true);
    setNotice(null);
    try {
      onSwitched(await auth.loginSavedAccount(item.username, item.isOffline));
    } catch (error: unknown) {
      setNotice(authErrorMessage(error, t));
    } finally {
      setBusy(false);
    }
  }

  async function remove(item: Account): Promise<void> {
    setBusy(true);
    setNotice(null);
    try {
      setSaved(await auth.removeSavedAccount(item.username, item.isOffline));
    } catch (error: unknown) {
      setNotice(authErrorMessage(error, t));
    } finally {
      setBusy(false);
    }
  }

  async function signOut(): Promise<void> {
    setBusy(true);
    try {
      await auth.logout();
      onLoggedOut();
    } catch (error: unknown) {
      setBusy(false);
      setOpen(true);
      setNotice(authErrorMessage(error, t));
    }
  }

  return (
    <main className="shell">
      <header className="shell__header">
        <div className="shell__brand">
          <img className="shell__logo" src={logo} alt="Ligone Launcher" />
          <span className="shell__title">Ligone Launcher</span>
        </div>
        <div className="shell__account" ref={rootRef}>
          {!account.isOffline && (
            <span
              className="shell__ely-badge"
              title={t("manage.ely_badge")}
            >
              E
            </span>
          )}
          <span className="shell__username">{account.username}</span>
          <button
            className="shell__manage"
            type="button"
            disabled={busy}
            onClick={togglePanel}
          >
            {t("manage.button")}
          </button>
          <button
            className="shell__manage shell__settings"
            type="button"
            title={t("manage.gear_title")}
            onClick={() => setSettingsOpen(true)}
          >
            ⚙
          </button>
          {open && (
            <div
              className="manage__panel"
              role="menu"
              aria-label={t("manage.panel_aria")}
            >
              <p className="manage__title">{t("manage.panel_title")}</p>
              {saved === null ? (
                <p className="manage__hint">{t("common.loading")}</p>
              ) : saved.length === 0 ? (
                <p className="manage__hint">{t("manage.empty")}</p>
              ) : (
                <ul className="manage__list">
                  {saved.map((item) => {
                    const current =
                      item.username === account.username &&
                      item.isOffline === account.isOffline;
                    return (
                      <li
                        key={`${item.username}:${item.isOffline}`}
                        className={`manage__row${current ? " manage__row--current" : ""}`}
                      >
                        <button
                          className="manage__row-main"
                          type="button"
                          disabled={busy || current}
                          onClick={() => void switchTo(item)}
                          title={t("manage.switch_title", {
                            username: item.username,
                          })}
                        >
                          <span className="manage__row-name">{item.username}</span>
                          <span className="manage__row-kind">
                            {item.isOffline
                              ? current
                                ? t("manage.current_kind")
                                : t("manage.offline_kind")
                              : (
                                <span
                                  className="shell__ely-badge"
                                  title={
                                    current
                                      ? t("manage.ely_badge_current")
                                      : t("manage.ely_badge")
                                  }
                                >
                                  E
                                </span>
                              )}
                          </span>
                        </button>
                        <button
                          className="manage__remove"
                          type="button"
                          disabled={busy || current}
                          onClick={() => void remove(item)}
                          title={
                            current
                              ? t("manage.remove_disabled")
                              : t("manage.remove_title", { username: item.username })
                          }
                        >
                          ×
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
              {notice !== null && <p className="manage__notice">{notice}</p>}
              <div className="manage__footer">
                <button
                  className="manage__footer-btn"
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    setOpen(false);
                    setAddOpen(true);
                  }}
                >
                  {t("manage.add")}
                </button>
                <button
                  className="manage__footer-btn manage__footer-btn--danger"
                  type="button"
                  disabled={busy}
                  onClick={() => void signOut()}
                >
                  {t("manage.signout")}
                </button>
              </div>
            </div>
          )}
        </div>
      </header>
      <section className="shell__main">
        <InstancesView />
      </section>
      {settingsOpen && <SettingsModal onClose={() => setSettingsOpen(false)} />}
      {addOpen && (
        <div
          className="shell__overlay"
          role="presentation"
          onClick={(event) => {
            if (event.currentTarget === event.target) {
              setAddOpen(false);
            }
          }}
        >
          <div className="shell__add-card">
            <button
              className="shell__add-close"
              type="button"
              onClick={() => setAddOpen(false)}
              title={t("common.close_title")}
            >
              ×
            </button>
            <LoginScreen onLoggedIn={handleAddedAccount} />
          </div>
        </div>
      )}
    </main>
  );
}
