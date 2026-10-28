mod ely;
mod model;
mod offline;
mod storage;

use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};

use tauri::{AppHandle, Manager, State};

use ely::Session;
use model::now_millis;
pub use model::Account;
pub use model::AuthError;

#[derive(Default)]
pub struct AuthState {
    current: Mutex<Option<Account>>,
    ely_cancel: Arc<AtomicBool>,
}

#[tauri::command]
pub fn auth_login_offline(
    app: AppHandle,
    state: State<'_, AuthState>,
    username: String,
) -> Result<Account, AuthError> {
    let account = offline::login_offline(&username)?;
    let mut saved = storage::load(&app)?;
    saved.upsert(account.clone());
    storage::save(&app, &saved)?;
    *state.current.lock().unwrap() = Some(account.clone());
    Ok(account)
}

#[tauri::command]
pub fn auth_login_saved(
    app: AppHandle,
    state: State<'_, AuthState>,
    username: String,
    is_offline: bool,
) -> Result<Account, AuthError> {
    let username = username.trim();
    let mut saved = storage::load(&app)?;
    let account = saved
        .find(username, is_offline)
        .ok_or_else(|| AuthError::unknown_account(username))?
        .clone();
    saved.set_last(account.clone());
    storage::save(&app, &saved)?;
    *state.current.lock().unwrap() = Some(account.clone());
    Ok(account)
}

#[tauri::command]
pub fn auth_current(state: State<'_, AuthState>) -> Option<Account> {
    state.current.lock().unwrap().clone()
}

#[tauri::command]
pub fn auth_logout(app: AppHandle, state: State<'_, AuthState>) -> Result<(), AuthError> {
    *state.current.lock().unwrap() = None;
    let mut saved = storage::load(&app)?;
    saved.last = None;
    storage::save(&app, &saved)?;
    Ok(())
}

#[tauri::command]
pub fn auth_saved_accounts(app: AppHandle) -> Result<Vec<Account>, AuthError> {
    let saved = storage::load(&app)?;
    Ok(saved.accounts)
}

#[tauri::command]
pub fn auth_remove_saved(
    app: AppHandle,
    username: String,
    is_offline: bool,
) -> Result<Vec<Account>, AuthError> {
    let username = username.trim();
    let mut saved = storage::load(&app)?;
    let removed = saved
        .accounts
        .iter()
        .find(|account| account.username == username && account.is_offline == is_offline)
        .cloned();
    if let Some(removed) = removed.as_ref().filter(|account| account.ely.is_some()) {
        storage::delete_tokens(&removed.username)?;
    }
    saved
        .accounts
        .retain(|account| !(account.username == username && account.is_offline == is_offline));
    if saved.last.as_ref().is_some_and(|last| {
        last.username == username && last.is_offline == is_offline
    }) {
        saved.last = None;
    }
    storage::save(&app, &saved)?;
    Ok(saved.accounts)
}

#[tauri::command]
pub async fn auth_login_ely(app: AppHandle) -> Result<Account, AuthError> {
    run_blocking(move || {
        let state = app.state::<AuthState>();
        state
            .ely_cancel
            .store(false, Ordering::Relaxed);
        let session = ely::obtain_session(&app, &state.ely_cancel)?;
        finish_ely_login(&app, &state, session)
    })
    .await
}

#[tauri::command]
pub fn auth_cancel_ely(state: State<'_, AuthState>) -> Result<(), AuthError> {
    state.ely_cancel.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn auth_ely_refresh(app: AppHandle) -> Result<Account, AuthError> {
    run_blocking(move || {
        let state = app.state::<AuthState>();
        let account = {
            let current = state.current.lock().unwrap();
            current
                .clone()
                .ok_or_else(|| AuthError::oauth("нет активной сессии"))?
        };
        let ely = account
            .ely
            .as_ref()
            .ok_or_else(|| AuthError::oauth("текущий аккаунт не привязан к Ely.by"))?
            .clone();
        let session = match (ely.refresh_token.clone(), ely.client_token.clone()) {
            (Some(refresh_token), _) => ely::refresh_session(&refresh_token)?,
            (None, Some(client_token)) => {
                ely::refresh_legacy_session(&ely.access_token, &client_token)?
            }
            (None, None) => {
                return Err(AuthError::oauth(
                    "токен не подлежит обновлению; войдите заново",
                ))
            }
        };
        finish_ely_login(&app, &state, session)
    })
    .await
}

#[tauri::command]
pub async fn auth_login_ely_password(
    app: AppHandle,
    username: String,
    password: String,
) -> Result<Account, AuthError> {
    run_blocking(move || {
        let state = app.state::<AuthState>();
        let client_token = storage::load_client_token(&app)?;
        let session = ely::authenticate_with_password(&username, &password, &client_token)?;
        finish_ely_login(&app, &state, session)
    })
    .await
}

async fn run_blocking<T, F>(job: F) -> Result<T, AuthError>
where
    F: FnOnce() -> Result<T, AuthError> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|error| AuthError::oauth(error.to_string()))?
}

fn finish_ely_login(
    app: &AppHandle,
    state: &AuthState,
    session: Session,
) -> Result<Account, AuthError> {
    let account = Account {
        username: session.username,
        uuid: session.uuid,
        is_offline: false,
        logged_in_at: now_millis()?,
        ely: Some(session.ely),
    };
    let mut saved = storage::load(app)?;
    saved.upsert(account.clone());
    storage::save(app, &saved)?;
    *state.current.lock().unwrap() = Some(account.clone());
    Ok(account)
}

/// Best-effort load of `<launcher home>/.env`; variables already present in
/// the environment always win over the file.
pub(crate) fn load_env(app: &AppHandle) {
    let Some(home) = crate::paths::launcher_home(app) else {
        return;
    };
    let _ = dotenvy::from_path(home.join(".env"));
}

pub(crate) fn restore_last_session(app: &AppHandle) -> Result<(), AuthError> {
    let state = app.state::<AuthState>();
    let mut current = state.current.lock().unwrap();
    if current.is_none() {
        *current = storage::load(app)?.take_last();
    }
    Ok(())
}

/// Currently selected account, if any.
pub(crate) fn current_account(app: &AppHandle) -> Result<Account, AuthError> {
    app.state::<AuthState>()
        .current
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| AuthError::oauth("нет активного аккаунта"))
}

/// Refreshes an Ely.by access token when it is expired or about to expire;
/// persists the new tokens and updates the current session. Offline accounts
/// pass through untouched.
pub(crate) fn refresh_ely_if_expired(
    app: &AppHandle,
    account: &Account,
) -> Result<Account, AuthError> {
    let Some(ely) = &account.ely else {
        return Ok(account.clone());
    };
    let refresh_token = ely
        .refresh_token
        .as_deref()
        .ok_or_else(|| AuthError::oauth("сессия требует повторного входа"))?;
    let now = now_millis()?;
    let expired = ely
        .access_token_expires_at
        .is_some_and(|expires_at| expires_at <= now + 60_000);
    if !expired {
        return Ok(account.clone());
    }
    let session = ely::refresh_session(refresh_token)?;
    let state = app.state::<AuthState>();
    finish_ely_login(app, &state, session)
}
