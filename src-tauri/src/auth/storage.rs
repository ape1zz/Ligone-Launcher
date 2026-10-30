use std::{fs, path::{Path, PathBuf}};

use keyring::error::Error as KeyringError;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::model::{Account, AuthError, ElyAccount};

#[derive(Debug, Default)]
pub(crate) struct Saved {
    pub(crate) accounts: Vec<Account>,
    pub(crate) last: Option<Account>,
}

/// The secret blob stored in the OS keyring for one Ely.by account is the
/// serialized [`ElyAccount`]; on disk only a tokenless marker is kept.
const KEYRING_SERVICE: &str = "com.ligone.launcher";

/// What is persisted to `%APPDATA%/auth/accounts.json`. Ely tokens live in the
/// OS keyring, so `ely` is kept as a minimal marker here; the optional legacy
/// token fields exist only to migrate old files created before the keyring.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    #[serde(default)]
    accounts: Vec<StoredAccount>,
    #[serde(default)]
    last: Option<StoredAccount>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAccount {
    username: String,
    uuid: String,
    is_offline: bool,
    logged_in_at: u64,
    #[serde(default)]
    ely: Option<StoredEly>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredEly {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token_expires_at: Option<u64>,
}

impl StoredAccount {
    fn from_account(account: &Account) -> Self {
        Self {
            username: account.username.clone(),
            uuid: account.uuid.clone(),
            is_offline: account.is_offline,
            logged_in_at: account.logged_in_at,
            ely: account
                .ely
                .as_ref()
                .map(|_| StoredEly::default()),
        }
    }
}

fn storage_error(message: impl Into<String>) -> AuthError {
    AuthError::Storage {
        message: message.into(),
    }
}

fn keyring_entry(username: &str) -> Result<keyring::Entry, AuthError> {
    let user = format!("ely@{username}");
    keyring::Entry::new(KEYRING_SERVICE, &user)
        .map_err(|error| storage_error(format!("хранилище учётных данных: {error}")))
}

/// Persists Ely.by tokens for `username` in the OS keyring.
pub(crate) fn write_tokens(username: &str, ely: &ElyAccount) -> Result<(), AuthError> {
    let blob = serde_json::to_string(ely)
        .map_err(|error| storage_error(format!("сериализация токенов: {error}")))?;
    keyring_entry(username)?
        .set_password(&blob)
        .map_err(|error| storage_error(format!("запись токенов в хранилище: {error}")))
}

/// Reads Ely.by tokens for `username` from the OS keyring;
/// `Ok(None)` means no entry has ever been stored.
pub(crate) fn read_tokens(username: &str) -> Result<Option<ElyAccount>, AuthError> {
    match keyring_entry(username)?.get_password() {
        Ok(blob) => {
            let ely = serde_json::from_str::<ElyAccount>(&blob).map_err(|error| {
                storage_error(format!("не удалось разобрать токены из хранилища: {error}"))
            })?;
            Ok(Some(ely))
        }
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(storage_error(format!("чтение токенов из хранилища: {error}"))),
    }
}

/// Removes the keyring entry for `username`; a missing entry is not an error.
pub(crate) fn delete_tokens(username: &str) -> Result<(), AuthError> {
    match keyring_entry(username)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(storage_error(format!(
            "удаление токенов из хранилища: {error}"
        ))),
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientToken {
    client_token: String,
}

impl Saved {
    pub(crate) fn find(&self, username: &str, is_offline: bool) -> Option<&Account> {
        self.accounts.iter().find(|account| {
            account.username == username && account.is_offline == is_offline
        })
    }

    pub(crate) fn set_last(&mut self, account: Account) {
        self.last = Some(account);
    }

    pub(crate) fn upsert(&mut self, account: Account) {
        self.accounts.retain(|saved| {
            !(saved.username == account.username
                && saved.is_offline == account.is_offline)
        });
        self.accounts.insert(0, account.clone());
        self.last = Some(account);
    }

    pub(crate) fn take_last(&mut self) -> Option<Account> {
        self.last.take()
    }
}

fn home_dir(app: &AppHandle) -> Result<PathBuf, AuthError> {
    crate::paths::launcher_home(app).ok_or_else(|| AuthError::Storage {
        message: "не удалось определить домашнюю папку лаунчера".to_owned(),
    })
}

fn path(app: &AppHandle) -> Result<PathBuf, AuthError> {
    Ok(home_dir(app)?.join("auth").join("accounts.json"))
}

fn client_token_path(app: &AppHandle) -> Result<PathBuf, AuthError> {
    Ok(home_dir(app)?.join("auth").join("client_token.json"))
}

fn legacy_path(app: &AppHandle) -> Result<PathBuf, AuthError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AuthError::Storage {
            message: error.to_string(),
        })?;
    Ok(dir.join("auth").join("accounts.json"))
}

fn legacy_client_token_path(app: &AppHandle) -> Result<PathBuf, AuthError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AuthError::Storage {
            message: error.to_string(),
        })?;
    Ok(dir.join("auth").join("client_token.json"))
}

fn ensure_parent(path: &std::path::Path) -> Result<(), AuthError> {
    let parent = path.parent().ok_or_else(|| AuthError::Storage {
        message: format!("нет родительского каталога: {}", path.display()),
    })?;
    fs::create_dir_all(parent)?;
    Ok(())
}

pub(crate) fn load_client_token(app: &AppHandle) -> Result<String, AuthError> {
    let path = client_token_path(app)?;
    if let Ok(bytes) = fs::read(&path) {
        return serde_json::from_slice::<ClientToken>(&bytes)
            .map(|stored| stored.client_token)
            .map_err(|error| AuthError::Storage {
                message: format!("не удалось разобрать {}: {error}", path.display()),
            });
    }
    if let Ok(bytes) = fs::read(legacy_client_token_path(app)?) {
        if let Ok(stored) = serde_json::from_slice::<ClientToken>(&bytes) {
            let client_token = stored.client_token.clone();
            ensure_parent(&path)?;
            let bytes = serde_json::to_vec_pretty(&ClientToken { client_token: client_token.clone() })
                .map_err(|error| AuthError::Storage { message: error.to_string() })?;
            fs::write(&path, bytes)?;
            return Ok(client_token);
        }
    }
    let client_token = uuid::Uuid::new_v4().to_string();
    ensure_parent(&path)?;
    let bytes = serde_json::to_vec_pretty(&ClientToken {
        client_token: client_token.clone(),
    })
        .map_err(|error| AuthError::Storage { message: error.to_string() })?;
    fs::write(&path, bytes)?;
    Ok(client_token)
}

fn parse_stored(path: &Path, bytes: &[u8]) -> Result<Stored, AuthError> {
    serde_json::from_slice(bytes).map_err(|error| {
        storage_error(format!("не удалось разобрать {}: {error}", path.display()))
    })
}

/// Turns an on-disk account (tokenless marker, possibly with legacy inline
/// tokens) into a full in-memory account backed by the OS keyring. Returns
/// whether the file needs a one-time rewrite to strip migrated tokens.
fn hydrate_account(
    stored: StoredAccount,
) -> Result<(Account, bool), AuthError> {
    let ely = match &stored.ely {
        None => None,
        Some(marker) => {
            if let Some(ely) = read_tokens(&stored.username)? {
                Some(ely)
            } else if let Some(ely) = tokens_from_marker(marker) {
                write_tokens(&stored.username, &ely)?;
                Some(ely)
            } else {
                Some(synthetic_tokens())
            }
        }
    };
    let migrated = stored
        .ely
        .as_ref()
        .is_some_and(|marker| tokens_from_marker(marker).is_some());
    Ok((
        Account {
            username: stored.username,
            uuid: stored.uuid,
            is_offline: stored.is_offline,
            logged_in_at: stored.logged_in_at,
            ely,
        },
        migrated,
    ))
}

/// Rebuilds full tokens from a legacy on-disk marker; `None` when the marker
/// is already tokenless (the new format).
fn tokens_from_marker(marker: &StoredEly) -> Option<ElyAccount> {
    let has_any = marker.access_token.is_some()
        || marker.refresh_token.is_some()
        || marker.client_token.is_some()
        || marker.access_token_expires_at.is_some();
    has_any.then(|| ElyAccount {
        access_token: marker.access_token.clone().unwrap_or_default(),
        refresh_token: marker.refresh_token.clone(),
        client_token: marker.client_token.clone(),
        access_token_expires_at: marker.access_token_expires_at,
    })
}

/// Placeholder for Ely accounts whose tokens are absent from both the file
/// and the keyring: expired stamp with no refresh token, so any refresh
/// attempt reports "требуется повторный вход".
fn synthetic_tokens() -> ElyAccount {
    ElyAccount {
        access_token: String::new(),
        refresh_token: None,
        client_token: None,
        access_token_expires_at: Some(0),
    }
}

/// Merges the keyring into parsed storage; rewrites the file once when inline
/// legacy tokens had to be migrated into the keyring.
fn hydrate(app: &AppHandle, stored: Stored) -> Result<Saved, AuthError> {
    let mut saved = Saved::default();
    let mut migrated = false;
    for stored_account in stored.accounts {
        let (account, need_rewrite) = hydrate_account(stored_account)?;
        migrated |= need_rewrite;
        saved.accounts.push(account);
    }
    if let Some(stored_last) = stored.last {
        let (account, need_rewrite) = hydrate_account(stored_last)?;
        migrated |= need_rewrite;
        saved.last = Some(account);
    }
    if migrated {
        save(app, &saved)?;
    }
    Ok(saved)
}

pub(crate) fn load(app: &AppHandle) -> Result<Saved, AuthError> {
    let path = path(app)?;
    match fs::read(&path) {
        Ok(bytes) => hydrate(app, parse_stored(&path, &bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Ok(bytes) = fs::read(legacy_path(app)?) {
                let stored = serde_json::from_slice::<Stored>(&bytes).map_err(|error| {
                    storage_error(format!("не удалось разобрать старый accounts.json: {error}"))
                })?;
                let saved = hydrate(app, stored)?;
                save(app, &saved)?;
                return Ok(saved);
            }
            Ok(Saved::default())
        }
        Err(error) => Err(error.into()),
    }
}

/// Persists accounts: Ely tokens go to the OS keyring, the file keeps only
/// tokenless markers. Keyring writes happen before the file is touched.
pub(crate) fn save(app: &AppHandle, saved: &Saved) -> Result<(), AuthError> {
    for account in &saved.accounts {
        if let Some(ely) = &account.ely {
            write_tokens(&account.username, ely)?;
        }
    }
    if let Some(last) = &saved.last {
        if let Some(ely) = &last.ely {
            write_tokens(&last.username, ely)?;
        }
    }
    let path = path(app)?;
    ensure_parent(&path)?;
    let stored = Stored {
        accounts: saved.accounts.iter().map(StoredAccount::from_account).collect(),
        last: saved.last.as_ref().map(StoredAccount::from_account),
    };
    let bytes = serde_json::to_vec_pretty(&stored).map_err(|error| AuthError::Storage {
        message: error.to_string(),
    })?;
    fs::write(&path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Saved, Stored, StoredAccount, StoredEly, tokens_from_marker};
    use super::{delete_tokens, read_tokens, write_tokens};
    use crate::auth::model::{Account, ElyAccount};

    fn account(username: &str) -> Account {
        Account {
            username: username.to_owned(),
            uuid: "test-uuid".to_owned(),
            is_offline: true,
            logged_in_at: 1,
            ely: None,
        }
    }

    fn ely_account(username: &str) -> Account {
        Account {
            username: username.to_owned(),
            uuid: "ely-uuid".to_owned(),
            is_offline: false,
            logged_in_at: 2,
            ely: None,
        }
    }

    fn ely_tokens() -> ElyAccount {
        ElyAccount {
            access_token: "at".to_owned(),
            refresh_token: Some("rt".to_owned()),
            client_token: Some("ct".to_owned()),
            access_token_expires_at: Some(42),
        }
    }

    /// An Entry::new-backed store shared across calls: data survives between
    /// helpers, unlike the crate-provided mock (which is entry-only).
    mod keyring_test_store {
        use std::any::Any;
        use std::collections::HashMap;
        use std::sync::{LazyLock, Mutex};

        use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
        use keyring::error::{Error, Result};

        type Key = (String, String);
        type DataStore = Option<Vec<u8>>;

        static ENTRIES: LazyLock<Mutex<HashMap<Key, DataStore>>> =
            LazyLock::new(|| Mutex::new(HashMap::new()));

        pub(crate) struct SharedCredential {
            key: (String, String),
        }

        impl CredentialApi for SharedCredential {
            fn set_secret(&self, secret: &[u8]) -> Result<()> {
                ENTRIES
                    .lock()
                    .unwrap()
                    .insert(self.key.clone(), Some(secret.to_vec()));
                Ok(())
            }

            fn get_secret(&self) -> Result<Vec<u8>> {
                ENTRIES
                    .lock()
                    .unwrap()
                    .get(&self.key)
                    .cloned()
                    .flatten()
                    .ok_or(Error::NoEntry)
            }

            fn delete_credential(&self) -> Result<()> {
                let mut entries = ENTRIES.lock().unwrap();
                match entries.get(&self.key).cloned().flatten() {
                    Some(_) => {
                        entries.remove(&self.key);
                        Ok(())
                    }
                    None => Err(Error::NoEntry),
                }
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        pub(crate) struct SharedBuilder;

        impl CredentialBuilderApi for SharedBuilder {
            fn build(
                &self,
                _target: Option<&str>,
                service: &str,
                user: &str,
            ) -> Result<Box<Credential>> {
                Ok(Box::new(SharedCredential {
                    key: (service.to_owned(), user.to_owned()),
                }))
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        pub(crate) fn install_shared_store() {
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| {
                keyring::set_default_credential_builder(Box::new(SharedBuilder));
            });
        }
    }

    #[test]
    fn upsert_replaces_and_moves_to_front() {
        let mut saved = Saved::default();
        let first = account("Alice");
        let second = account("Bob");
        saved.upsert(first.clone());
        saved.upsert(second.clone());
        saved.upsert(first.clone());
        let names: Vec<&str> = saved.accounts.iter().map(|a| a.username.as_str()).collect();
        assert_eq!(names, ["Alice", "Bob"]);
        assert_eq!(saved.last.as_ref().unwrap().username, "Alice");
        assert_eq!(saved.accounts.len(), 2);
    }

    #[test]
    fn same_username_offline_and_ely_stay_separate() {
        let mut saved = Saved::default();
        saved.upsert(account("realquave"));
        saved.upsert(ely_account("realquave"));
        assert_eq!(saved.accounts.len(), 2);
        assert!(saved.find("realquave", true).is_some());
        assert!(saved.find("realquave", false).is_some());
        saved.upsert(ely_account("realquave"));
        assert_eq!(saved.accounts.len(), 2);
        assert_eq!(saved.last.as_ref().unwrap().is_offline, false);
    }

    #[test]
    fn take_last_clears_saved_last() {
        let mut saved = Saved::default();
        saved.upsert(account("Alice"));
        let taken = saved.take_last().unwrap();
        assert_eq!(taken.username, "Alice");
        assert!(saved.last.is_none());
    }

    #[test]
    fn stored_json_never_contains_secrets() {
        keyring_test_store::install_shared_store();
        let ely_only = Account {
            username: "Notch".to_owned(),
            uuid: "ely-uuid".to_owned(),
            is_offline: false,
            logged_in_at: 2,
            ely: Some(ely_tokens()),
        };
        let stored = Stored {
            accounts: vec![StoredAccount::from_account(&ely_only), StoredAccount::from_account(&account("Bob"))],
            last: Some(StoredAccount::from_account(&ely_only)),
        };
        let json = serde_json::to_string(&stored).unwrap();
        for forbidden in ["accessToken", "refreshToken", "clientToken", "password"] {
            assert!(!json.contains(forbidden), "file leaked secret name: {forbidden}");
        }
    }

    #[test]
    fn keyring_tokens_roundtrip() {
        keyring_test_store::install_shared_store();
        let tokens = ely_tokens();
        write_tokens("Notch", &tokens).unwrap();
        assert_eq!(read_tokens("Notch").unwrap(), Some(tokens));
        assert!(read_tokens("Ghost").unwrap().is_none());
        delete_tokens("Notch").unwrap();
        assert!(read_tokens("Notch").unwrap().is_none());
        delete_tokens("Notch").unwrap();
    }

    #[test]
    fn legacy_marker_becomes_tokens_but_empty_marker_does_not() {
        let legacy = StoredEly {
            access_token: Some("at".to_owned()),
            refresh_token: Some("rt".to_owned()),
            client_token: None,
            access_token_expires_at: Some(42),
        };
        let tokens = tokens_from_marker(&legacy).unwrap();
        assert_eq!(tokens.access_token, "at");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
        assert!(tokens_from_marker(&StoredEly::default()).is_none());
    }

    #[test]
    fn legacy_json_with_inline_tokens_parses() {
        let json = r#"{"accounts":[{"username":"Notch","uuid":"u","isOffline":false,"loggedInAt":1,"ely":{"accessToken":"at","refreshToken":"rt"}}],"last":null}"#;
        let stored: Stored = serde_json::from_str(json).unwrap();
        let marker = stored.accounts[0].ely.as_ref().unwrap();
        assert!(tokens_from_marker(marker).is_some());
    }
}
