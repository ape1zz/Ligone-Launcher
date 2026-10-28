import { useEffect, useState } from "react";
import { auth, type Account } from "./lib/auth";
import { applyTheme, settings } from "./lib/settings";
import { LanguageProvider, useI18n } from "./lib/i18n";
import LoginScreen from "./components/LoginScreen";
import AccountView from "./components/AccountView";
import "./App.css";

export default function App() {
  return (
    <LanguageProvider>
      <AppInner />
    </LanguageProvider>
  );
}

function AppInner() {
  const { setLanguage } = useI18n();
  const [account, setAccount] = useState<Account | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    settings
      .get()
      .then((loaded) => {
        if (!cancelled) {
          applyTheme(loaded.theme);
          setLanguage(loaded.language);
        }
      })
      .catch(() => {
        if (!cancelled) {
          applyTheme("light");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [setLanguage]);

  useEffect(() => {
    let cancelled = false;
    auth.getCurrentAccount().then((current) => {
      if (!cancelled) {
        setAccount(current);
        setLoading(false);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  if (loading) {
    return <main className="welcome welcome--loading">Загрузка…</main>;
  }
  return account ? (
    <AccountView
      account={account}
      onLoggedOut={() => setAccount(null)}
      onSwitched={setAccount}
    />
  ) : (
    <LoginScreen onLoggedIn={setAccount} />
  );
}
