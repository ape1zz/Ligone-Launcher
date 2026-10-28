import { useState } from "react";
import type { FormEvent } from "react";
import {
  auth,
  authErrorMessage,
  parseAuthError,
  type Account,
} from "../lib/auth";
import { useI18n } from "../lib/i18n";

interface LoginScreenProps {
  onLoggedIn: (account: Account) => void;
}

export default function LoginScreen({ onLoggedIn }: LoginScreenProps) {
  const { t } = useI18n();
  const [username, setUsername] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [chooser, setChooser] = useState(false);
  const [method, setMethod] = useState<"browse" | "password" | "waiting">(
    "browse",
  );
  const [elyLogin, setElyLogin] = useState("");
  const [elyPassword, setElyPassword] = useState("");
  const [dialogNotice, setDialogNotice] = useState<string | null>(null);

  function closeChooser() {
    if (busy) return;
    setChooser(false);
    setDialogNotice(null);
    setElyPassword("");
  }

  async function runLogin(action: () => Promise<Account>) {
    setBusy(true);
    setNotice(null);
    setDialogNotice(null);
    try {
      onLoggedIn(await action());
    } catch (error: unknown) {
      const message = authErrorMessage(error, t);
      setNotice(message);
      setDialogNotice(message);
    } finally {
      setBusy(false);
    }
  }

  function handleOfflineLogin(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void runLogin(() => auth.loginOffline(username));
  }

  function handleElyOAuth() {
    setNotice(null);
    setDialogNotice(null);
    setMethod("waiting");
    void (async () => {
      setBusy(true);
      try {
        onLoggedIn(await auth.loginEly());
      } catch (error: unknown) {
        const authError = parseAuthError(error);
        if (authError?.type !== "cancelled") {
          const message = authErrorMessage(error, t);
          setNotice(message);
          setDialogNotice(message);
          setMethod("browse");
        }
      } finally {
        setBusy(false);
      }
    })();
  }

  async function handleCancelOAuth(): Promise<void> {
    setMethod("browse");
    await auth.cancelEly().catch(() => undefined);
  }

  function handleElyPasswordLogin(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void runLogin(() => auth.loginElyPassword(elyLogin, elyPassword));
  }

  return (
    <main className="welcome">
      <h1 className="welcome__title">{t("welcome.title")}</h1>
      <form className="welcome__form" onSubmit={handleOfflineLogin}>
        <input
          className="welcome__input"
          type="text"
          placeholder={t("welcome.nick_placeholder")}
          maxLength={16}
          autoComplete="off"
          spellCheck={false}
          autoFocus
          value={username}
          onChange={(event) => setUsername(event.currentTarget.value)}
        />
        <button
          className="welcome__button welcome__button--offline"
          type="submit"
          disabled={busy}
        >
          <span className="welcome__skull">☠️</span>
          <span>{t("welcome.offline")}</span>
        </button>
      </form>
      <p className="welcome__separator">{t("welcome.or_login_via")}</p>
      <button
        className="welcome__button welcome__button--ely"
        type="button"
        disabled={busy}
        onClick={() => {
          setNotice(null);
          setDialogNotice(null);
          setMethod("browse");
          setChooser(true);
        }}
      >
        <span className="welcome__ely-logo">E</span>
        <span>Ely.by</span>
      </button>
      {notice !== null && <p className="welcome__notice">{notice}</p>}
      {chooser && (
        <div
          className="welcome__overlay"
          role="presentation"
          onClick={(event) => {
            if (event.currentTarget === event.target) {
              closeChooser();
            }
          }}
        >
          <div
            className="ely-choose"
            role="dialog"
            aria-modal="true"
            aria-label={t("chooser.aria")}
          >
            <h2 className="ely-choose__title">{t("chooser.title")}</h2>
            {method === "browse" ? (
              <>
                <button
                  className="ely-choose__method ely-choose__method--primary"
                  type="button"
                  disabled={busy}
                  onClick={handleElyOAuth}
                >
                  <span className="ely-choose__method-name">
                    {t("chooser.browser")}
                  </span>
                  <span className="ely-choose__method-hint">
                    {t("chooser.browser_hint")}
                  </span>
                </button>
                <button
                  className="ely-choose__method"
                  type="button"
                  disabled={busy}
                  onClick={() => setMethod("password")}
                >
                  <span className="ely-choose__method-name">
                    {t("chooser.password")}
                  </span>
                  <span className="ely-choose__method-hint">
                    {t("chooser.password_hint")}
                  </span>
                </button>
              </>
            ) : method === "waiting" ? (
              <div className="ely-choose__waiting">
                <p className="ely-choose__waiting-title">
                  {t("chooser.waiting_title")}
                </p>
                <p className="ely-choose__method-hint">
                  {t("chooser.waiting_hint")}
                </p>
                <button
                  className="ely-choose__cancel"
                  type="button"
                  onClick={handleCancelOAuth}
                >
                  {t("chooser.back")}
                </button>
              </div>
            ) : (
              <form className="ely-choose__form" onSubmit={handleElyPasswordLogin}>
                <input
                  className="ely-choose__input"
                  type="text"
                  placeholder={t("chooser.login_placeholder")}
                  autoComplete="username"
                  spellCheck={false}
                  autoFocus
                  value={elyLogin}
                  onChange={(event) => setElyLogin(event.currentTarget.value)}
                />
                <input
                  className="ely-choose__input"
                  type="password"
                  placeholder={t("chooser.password_placeholder")}
                  autoComplete="current-password"
                  value={elyPassword}
                  onChange={(event) => setElyPassword(event.currentTarget.value)}
                />
                <button
                  className="ely-choose__submit"
                  type="submit"
                  disabled={busy || elyLogin.trim() === "" || elyPassword === ""}
                >
                  {t("chooser.login")}
                </button>
                <button
                  className="ely-choose__back"
                  type="button"
                  disabled={busy}
                  onClick={() => setMethod("browse")}
                >
                  {t("chooser.back")}
                </button>
              </form>
            )}
            {dialogNotice !== null && (
              <p className="ely-choose__notice">{dialogNotice}</p>
            )}
            {method !== "waiting" && (
              <button
                className="ely-choose__close"
                type="button"
                disabled={busy}
                onClick={closeChooser}
              >
                {t("common.close_title")}
              </button>
            )}
          </div>
        </div>
      )}
    </main>
  );
}
