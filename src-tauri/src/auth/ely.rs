use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use super::model::{now_millis, AuthError, ElyAccount};

const ACCOUNT_BASE: &str = "https://account.ely.by";
const TOKEN_PATH: &str = "/api/oauth2/v1/token";
const ACCOUNT_INFO_PATH: &str = "/api/account/v1/info";
const AUTHORIZE_PATH: &str = "/oauth2/v1";
const AUTHSERVER_BASE: &str = "https://authserver.ely.by";
const AUTHENTICATE_PATH: &str = "/auth/authenticate";
const AUTHSERVER_REFRESH_PATH: &str = "/auth/refresh";
const SCOPES: &str = "account_info offline_access minecraft_server_session";
const CALLBACK_BIND_PORT: u16 = 8921;
const DEFAULT_REDIRECT_URI: &str = "http://localhost:1420/ely/callback";
const CALLBACK_TIMEOUT_SECS: u64 = 120;
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_TOKEN_TTL_SECS: u64 = 86_400;

const TWO_FACTOR_MARK: &str = "two factor";
const TWO_FACTOR_MESSAGE: &str = "аккаунт защищён двухфакторной аутентификацией; войдите через кнопку Ely.by (рекомендуемый способ)";
const INVALID_CREDENTIALS_MARK: &str = "invalid credentials";
const INVALID_CREDENTIALS_MESSAGE: &str = "неверный логин или пароль";

#[derive(Debug)]
pub(crate) struct Session {
    pub(crate) username: String,
    pub(crate) uuid: String,
    pub(crate) ely: ElyAccount,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AccountInfo {
    uuid: String,
    username: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    error: String,
    error_description: String,
}

fn cleaned_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// OAuth client credentials come exclusively from the environment
/// (`.env` in the launcher home or parent process variables).
pub(crate) fn credentials() -> Result<(String, String), AuthError> {
    require_credentials(
        cleaned_env("ELYBY_CLIENT_ID").as_deref(),
        cleaned_env("ELYBY_CLIENT_SECRET").as_deref(),
    )
}

/// Pure helper so tests can exercise the "not configured" path
/// without touching process-wide environment variables.
pub(crate) fn require_credentials(
    client_id: Option<&str>,
    client_secret: Option<&str>,
) -> Result<(String, String), AuthError> {
    let client_id = client_id.unwrap_or_default().trim().to_owned();
    let client_secret = client_secret.unwrap_or_default().trim().to_owned();
    if client_id.is_empty() || client_secret.is_empty() {
        return Err(AuthError::oauth(
            "Ely.by OAuth не настроен: задайте ELYBY_CLIENT_ID и ELYBY_CLIENT_SECRET в .env (домашняя папка лаунчера) или в переменных окружения",
        ));
    }
    Ok((client_id, client_secret))
}

pub(crate) fn redirect_uri() -> String {
    std::env::var("ELYBY_REDIRECT_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REDIRECT_URI.to_owned())
}

pub(crate) fn callback_bind_port() -> u16 {
    redirect_uri()
        .split("//")
        .nth(1)
        .and_then(|host| host.split('/').next())
        .and_then(|host| host.rsplit(':').next())
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or(CALLBACK_BIND_PORT)
}

pub(crate) fn url_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub(crate) fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = (bytes[index + 1] as char).to_digit(16);
                let low = (bytes[index + 2] as char).to_digit(16);
                if let (Some(high), Some(low)) = (high, low) {
                    decoded.push((high * 16 + low) as u8);
                    index += 3;
                } else {
                    decoded.push(b'%');
                    index += 1;
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

pub(crate) fn parse_query(raw: &str) -> HashMap<String, String> {
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let mut halves = pair.splitn(2, '=');
            let key = percent_decode(halves.next().unwrap_or_default());
            let value = percent_decode(halves.next().unwrap_or_default());
            (key, value)
        })
        .collect()
}

pub(crate) fn authorize_url(state: &str) -> Result<String, AuthError> {
    let (client_id, _) = credentials()?;
    let redirect_uri = redirect_uri();
    let params = [
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("state", state),
        ("description", "Ligone Launcher"),
    ];
    let query = params
        .iter()
        .map(|(key, value)| format!("{}={}", url_encode(key), url_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    Ok(format!("{ACCOUNT_BASE}{AUTHORIZE_PATH}?{query}"))
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("\u{26}amp;"),
            '<' => escaped.push_str("\u{26}lt;"),
            '>' => escaped.push_str("\u{26}gt;"),
            '"' => escaped.push_str("\u{26}quot;"),
            '\'' => escaped.push_str("\u{26}#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn html_page(message: &str) -> String {
    let escaped = escape_html(message);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Ligone Launcher</title></head>\
        <body style=\"font-family: Arial, sans-serif; text-align: center; padding-top: 48px;\">\
        <h2>{escaped}</h2></body></html>"
    )
}

fn read_request_head(stream: &mut TcpStream) -> Option<String> {
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut request_line = String::new();
    let mut reader = BufReader::new(stream);
    reader.read_line(&mut request_line).ok()?;
    Some(request_line.trim().to_owned())
}

fn handle_stream(
    stream: &mut TcpStream,
    expected_state: &str,
) -> Option<Result<String, AuthError>> {
    let request_line = read_request_head(stream)?;
    let target = request_line.split_whitespace().nth(1)?.to_owned();
    if target.starts_with("/favicon") || target.starts_with("/robots.txt") {
        return None;
    }
    let query = parse_query(target.split_once('?').map(|(_, query)| query).unwrap_or(""));
    let (page, result) = if let Some(error) = query.get("error") {
        let description = query.get("error_message").map(String::as_str).unwrap_or_default();
        let message = if description.is_empty() {
            format!("Ely.by отклонил авторизацию: {}", error)
        } else {
            format!(
                "Ely.by отклонил авторизацию: {}: {}",
                error, description
            )
        };
        let result = Some(Err(AuthError::oauth(message.clone())));
        (html_page(&message), result)
    } else if let (Some(code), Some(state)) = (query.get("code"), query.get("state")) {
        if state == expected_state {
            (
                html_page("Авторизация успешна. Эту вкладку можно закрыть и вернуться в лаунчер."),
                Some(Ok(code.clone())),
            )
        } else {
            let result = Some(Err(AuthError::oauth(
                "state не совпадает с отправленным значением",
            )));
            (html_page("Некорректный state, вернитесь в лаунчер"), result)
        }
    } else {
        (html_page("Неизвестный запрос, вернитесь в лаунчер"), None)
    };
    let body = page.as_bytes();
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
    result
}

fn wait_for_code(expected_state: &str, cancelled: &AtomicBool) -> Result<String, AuthError> {
    let port = callback_bind_port();
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| {
        AuthError::oauth(format!(
            "не удалось открыть локальный сервер колбэка на порту {port}: {error}"
        ))
    })?;
    listener
        .set_nonblocking(true)
        .map_err(|error| AuthError::oauth(error.to_string()))?;
    let deadline = Instant::now() + Duration::from_secs(CALLBACK_TIMEOUT_SECS);
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(AuthError::cancelled());
        }
        if Instant::now() >= deadline {
            return Err(AuthError::oauth(format!(
                "время на авторизацию в браузере истекло ({CALLBACK_TIMEOUT_SECS} секунд)"
            )));
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Some(result) = handle_stream(&mut stream, expected_state) {
                    return result;
                }
            }
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) => return Err(AuthError::oauth(format!("приём соединения: {error}"))),
        }
    }
}

pub(crate) fn obtain_session(
    app: &AppHandle,
    cancelled: &AtomicBool,
) -> Result<Session, AuthError> {
    let state = Uuid::new_v4().simple().to_string();
    let url = authorize_url(&state)?;
    app.opener()
        .open_url(&url, None::<String>)
        .map_err(|error| AuthError::oauth(error.to_string()))?;
    let code = wait_for_code(&state, cancelled)?;
    let tokens = exchange_code(&code)?;
    session_from_tokens(tokens, None)
}

pub(crate) fn refresh_session(refresh_token: &str) -> Result<Session, AuthError> {
    let tokens = refresh_tokens(refresh_token)?;
    session_from_tokens(tokens, Some(refresh_token))
}

fn session_from_tokens(
    tokens: TokenResponse,
    fallback_refresh_token: Option<&str>,
) -> Result<Session, AuthError> {
    let refresh_token = match tokens.refresh_token {
        Some(token) => token,
        None => match fallback_refresh_token {
            Some(token) => token.to_owned(),
            None => {
                return Err(AuthError::oauth(
                    "сервер не вернул refresh_token для сохранения сессии",
                ))
            }
        },
    };
    let expires_in = tokens.expires_in.unwrap_or(DEFAULT_TOKEN_TTL_SECS);
    let access_token_expires_at = now_millis()? + expires_in * 1000;
    let info = account_info(&tokens.access_token)?;
    Ok(Session {
        username: info.username,
        uuid: info.uuid,
        ely: ElyAccount {
            access_token: tokens.access_token,
            refresh_token: Some(refresh_token),
            client_token: None,
            access_token_expires_at: Some(access_token_expires_at),
        },
    })
}

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .build()
}

fn error_message(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(_, response) => response
            .into_string()
            .unwrap_or_else(|_| "HTTP-ответ без читаемого тела".to_owned()),
        other => other.to_string(),
    }
}

fn parse_body<T: DeserializeOwned>(url: &str, body: &str) -> Result<T, AuthError> {
    match serde_json::from_str::<T>(body) {
        Ok(parsed) => Ok(parsed),
        Err(_) => match serde_json::from_str::<ApiErrorBody>(body) {
            Ok(api_error) => Err(AuthError::ely_api(format!(
                "{}: {}",
                api_error.error, api_error.error_description
            ))),
            Err(_) => Err(AuthError::ely_api(format!(
                "неопознанный ответ от {url}: {body}"
            ))),
        },
    }
}

fn post_form<T: DeserializeOwned>(url: &str, params: &[(&str, &str)]) -> Result<T, AuthError> {
    let response = http()
        .post(url)
        .send_form(params)
        .map_err(|error| AuthError::ely_api(error_message(error)))?;
    let body = response
        .into_string()
        .map_err(|error| AuthError::ely_api(error.to_string()))?;
    parse_body(url, &body)
}

fn exchange_code(code: &str) -> Result<TokenResponse, AuthError> {
    let (client_id, client_secret) = credentials()?;
    let redirect_uri = redirect_uri();
    post_form(
        &format!("{ACCOUNT_BASE}{TOKEN_PATH}"),
        &[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code),
        ],
    )
}

fn refresh_tokens(refresh_token: &str) -> Result<TokenResponse, AuthError> {
    let (client_id, client_secret) = credentials()?;
    post_form(
        &format!("{ACCOUNT_BASE}{TOKEN_PATH}"),
        &[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("scope", SCOPES),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ],
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyProfile {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyAuthResponse {
    access_token: String,
    client_token: String,
    selected_profile: Option<LegacyProfile>,
}

#[derive(Debug, Deserialize)]
struct LegacyErrorBody {
    error: String,
    #[serde(default, rename = "errorMessage")]
    error_message: String,
}

fn legacy_authenticate_payload(username: &str, password: &str, client_token: &str) -> serde_json::Value {
    serde_json::json!({
        "username": username,
        "password": password,
        "clientToken": client_token,
        "requestUser": false,
    })
}

fn legacy_refresh_payload(access_token: &str, client_token: &str) -> serde_json::Value {
    serde_json::json!({
        "accessToken": access_token,
        "clientToken": client_token,
        "requestUser": false,
    })
}

fn send_legacy_json(url: &str, payload: serde_json::Value) -> Result<LegacyAuthResponse, AuthError> {
    let response = http()
        .post(url)
        .send_json(payload)
        .map_err(|error| legacy_http_error(error))?;
    let body = response
        .into_string()
        .map_err(|error| AuthError::ely_api(error.to_string()))?;
    match serde_json::from_str::<LegacyAuthResponse>(&body) {
        Ok(parsed) => Ok(parsed),
        Err(_) => Err(legacy_api_error(&body)),
    }
}

fn legacy_http_error(error: ureq::Error) -> AuthError {
    match error {
        ureq::Error::Status(_, response) => match response.into_string() {
            Ok(body) => legacy_api_error(&body),
            Err(_) => AuthError::ely_api("HTTP-ответ без читаемого тела"),
        },
        other => AuthError::ely_api(other.to_string()),
    }
}

fn legacy_api_error(body: &str) -> AuthError {
    match serde_json::from_str::<LegacyErrorBody>(body) {
        Ok(api_error) => {
            let message = api_error.error_message.to_lowercase();
            if message.contains(TWO_FACTOR_MARK) {
                AuthError::ely_api(TWO_FACTOR_MESSAGE)
            } else if message.contains(INVALID_CREDENTIALS_MARK) {
                AuthError::ely_api(INVALID_CREDENTIALS_MESSAGE)
            } else if api_error.error_message.is_empty() {
                AuthError::ely_api(api_error.error)
            } else {
                AuthError::ely_api(format!("{}: {}", api_error.error, api_error.error_message))
            }
        }
        Err(_) => AuthError::ely_api(format!("неопознанный ответ: {body}")),
    }
}

fn legacy_session(response: LegacyAuthResponse) -> Result<Session, AuthError> {
    let profile = response
        .selected_profile
        .ok_or_else(|| AuthError::ely_api("Ely.by не вернул профиль пользователя"))?;
    let uuid = Uuid::parse_str(profile.id.as_str())
        .map_err(|error| AuthError::ely_api(format!("некорректный UUID профиля: {error}")))?
        .to_string();
    Ok(Session {
        username: profile.name,
        uuid,
        ely: ElyAccount {
            access_token: response.access_token,
            refresh_token: None,
            client_token: Some(response.client_token),
            access_token_expires_at: None,
        },
    })
}

pub(crate) fn authenticate_with_password(
    username: &str,
    password: &str,
    client_token: &str,
) -> Result<Session, AuthError> {
    let username = username.trim();
    if username.is_empty() {
        return Err(AuthError::ely_api("введите логин"));
    }
    if password.is_empty() {
        return Err(AuthError::ely_api("введите пароль"));
    }
    let payload = legacy_authenticate_payload(username, password, client_token);
    send_legacy_json(&format!("{AUTHSERVER_BASE}{AUTHENTICATE_PATH}"), payload)
        .and_then(legacy_session)
}

pub(crate) fn refresh_legacy_session(
    access_token: &str,
    client_token: &str,
) -> Result<Session, AuthError> {
    let payload = legacy_refresh_payload(access_token, client_token);
    send_legacy_json(&format!("{AUTHSERVER_BASE}{AUTHSERVER_REFRESH_PATH}"), payload)
        .and_then(legacy_session)
}

fn account_info(access_token: &str) -> Result<AccountInfo, AuthError> {
    let url = format!("{ACCOUNT_BASE}{ACCOUNT_INFO_PATH}");
    let response = http()
        .get(&url)
        .set("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|error| AuthError::ely_api(error_message(error)))?;
    let body = response
        .into_string()
        .map_err(|error| AuthError::ely_api(error.to_string()))?;
    parse_body(&url, &body)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    use super::{
        authorize_url, handle_stream, parse_query, percent_decode, redirect_uri, require_credentials,
        url_encode,
    };
    use crate::auth::model::AuthError;

    /// Other unit tests never read these variables, so a one-time global set
    /// is safe for the parallel test runner.
    fn ensure_test_credentials() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            unsafe { std::env::set_var("ELYBY_CLIENT_ID", "test-client-id") };
            unsafe { std::env::set_var("ELYBY_CLIENT_SECRET", "test-client-secret") };
        });
    }

    fn spawn_request(listener: TcpListener, url: String) -> std::sync::mpsc::Receiver<String> {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut socket, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(_) => return,
            };
            let _ = socket.write_all(format!("GET {url} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes());
            let mut response = String::new();
            let mut reader = BufReader::new(socket);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(count) if count > 0 => {
                        if line == "\r\n" {
                            break;
                        }
                        response.push_str(&line);
                    }
                    _ => break,
                }
            }
            let _ = sender.send(response);
        });
        receiver
    }

    fn connected_client(port: u16) -> TcpStream {
        let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        client
    }

    #[test]
    fn url_encode_leaves_unreserved_and_escapes_rest() {
        assert_eq!(url_encode("abc-DEF_1.2~"), "abc-DEF_1.2~");
        assert_eq!(
            url_encode("http://localhost:8921/ely/callback"),
            "http%3A%2F%2Flocalhost%3A8921%2Fely%2Fcallback"
        );
        assert_eq!(url_encode("Ligone Launcher"), "Ligone%20Launcher");
    }

    #[test]
    fn percent_decode_decodes_plus_and_hex() {
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("%3A%2F"), ":/");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn parse_query_extracts_code_and_state() {
        let query = parse_query("code=abc123&state=s1&error_message=a+b%3D1");
        assert_eq!(query.get("code").map(String::as_str), Some("abc123"));
        assert_eq!(query.get("state").map(String::as_str), Some("s1"));
        assert_eq!(
            query.get("error_message").map(String::as_str),
            Some("a b=1")
        );
    }

    #[test]
    fn parse_query_tolerates_missing_value() {
        let query = parse_query("code&state=");
        assert_eq!(query.get("code").map(String::as_str), Some(""));
        assert_eq!(query.get("state").map(String::as_str), Some(""));
    }

    #[test]
    fn escape_html_encodes_markup() {
        assert_eq!(
            super::escape_html("<b>&\"x'"),
            "\u{26}lt;b\u{26}gt;\u{26}amp;\u{26}quot;x\u{26}#39;"
        );
    }

    #[test]
    fn authorize_url_contains_required_params() {
        ensure_test_credentials();
        let url = authorize_url("state123").unwrap();
        assert!(url.starts_with("https://account.ely.by/oauth2/v1?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("state=state123"));
        assert!(url.contains(&format!("redirect_uri={}", url_encode(&redirect_uri()))));
        assert!(url.contains("scope=account_info%20offline_access%20minecraft_server_session"));
    }

    #[test]
    fn require_credentials_accepts_clean_pair() {
        assert_eq!(
            require_credentials(Some("id "), Some(" secret")).unwrap(),
            ("id".to_owned(), "secret".to_owned())
        );
    }

    #[test]
    fn require_credentials_rejects_missing_or_blank() {
        assert!(matches!(
            require_credentials(None, Some("secret")),
            Err(AuthError::OAuth { .. })
        ));
        assert!(matches!(
            require_credentials(Some("id"), Some("  ")),
            Err(AuthError::OAuth { .. })
        ));
        assert!(matches!(
            require_credentials(Some(""), None),
            Err(AuthError::OAuth { .. })
        ));
    }

    #[test]
    fn handle_stream_answers_success_and_returns_code() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let receiver = spawn_request(
            listener,
            "/ely/callback?code=xyz&state=match&pinned=1".to_owned(),
        );
        let mut client = connected_client(port);
        match handle_stream(&mut client, "match") {
            Some(Ok(code)) => {
                assert_eq!(code, "xyz");
                let response = receiver
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                assert!(response.contains("200 OK"), "{response}");
                assert!(response.contains("charset=utf-8"), "{response}");
            }
            other => panic!("expected authorization code, got: {other:?}"),
        }
    }

    #[test]
    fn handle_stream_reports_error_param() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        spawn_request(
            listener,
            "/ely/callback?error=access_denied&error_message=user+denied".to_owned(),
        );
        let mut client = connected_client(port);
        match handle_stream(&mut client, "other") {
            Some(Err(error)) => {
                assert!(error.to_string().contains("access_denied"), "{error}");
            }
            other => panic!("expected oauth error, got: {other:?}"),
        }
    }

    #[test]
    fn handle_stream_rejects_wrong_state() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        spawn_request(listener, "/ely/callback?code=xyz&state=stranger".to_owned());
        let mut client = connected_client(port);
        match handle_stream(&mut client, "expected") {
            Some(Err(error)) => {
                assert!(error.to_string().contains("state"), "{error}");
            }
            other => panic!("expected oauth state error, got: {other:?}"),
        }
    }
}

#[cfg(test)]
mod legacy_tests {
    use super::{
        legacy_authenticate_payload, legacy_api_error, legacy_refresh_payload, legacy_session,
    };
    use crate::auth::model::Account;

    #[test]
    fn authenticate_payload_matches_authserver_contract() {
        let payload = legacy_authenticate_payload("notch@example.com", "secret", "client-tok");
        assert_eq!(payload["username"], "notch@example.com");
        assert_eq!(payload["password"], "secret");
        assert_eq!(payload["clientToken"], "client-tok");
        assert_eq!(payload["requestUser"], false);
    }

    #[test]
    fn refresh_payload_matches_authserver_contract() {
        let payload = legacy_refresh_payload("at", "client-tok");
        assert_eq!(payload["accessToken"], "at");
        assert_eq!(payload["clientToken"], "client-tok");
        assert_eq!(payload["requestUser"], false);
    }

    #[test]
    fn legacy_session_normalizes_profile_uuid() {
        let response: super::LegacyAuthResponse = serde_json::from_value(serde_json::json!({
            "accessToken": "at-long-token",
            "clientToken": "client-tok",
            "selectedProfile": {
                "id": "069a79f444e94726a5befca90e38aaf5",
                "name": "Notch"
            }
        }))
        .unwrap();
        let session = legacy_session(response).unwrap();
        let account = Account {
            username: session.username.clone(),
            uuid: session.uuid.clone(),
            is_offline: false,
            logged_in_at: 0,
            ely: Some(session.ely.clone()),
        };
        assert_eq!(session.username, "Notch");
        assert_eq!(session.uuid, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert_eq!(session.ely.refresh_token, None);
        assert_eq!(session.ely.client_token.as_deref(), Some("client-tok"));
        assert_eq!(session.ely.access_token_expires_at, None);
        assert_eq!(account.ely.as_ref().unwrap().access_token, "at-long-token");
    }

    #[test]
    fn legacy_session_fails_without_profile() {
        let response: super::LegacyAuthResponse = serde_json::from_value(serde_json::json!({
            "accessToken": "at",
            "clientToken": "client-tok"
        }))
        .unwrap();
        let error = legacy_session(response).unwrap_err();
        assert!(error.to_string().contains("профиль"), "{error}");
    }

    #[test]
    fn legacy_error_marks_two_factor() {
        let error = legacy_api_error(
            r#"{"error":"ForbiddenOperationException","errorMessage":"Account protected with two factor auth."}"#,
        );
        let text = error.to_string();
        assert!(text.contains("двухфакторной"), "{text}");
    }

    #[test]
    fn legacy_error_marks_invalid_credentials() {
        let error = legacy_api_error(
            r#"{"error":"ForbiddenOperationException","errorMessage":"Invalid credentials. Invalid email or password."}"#,
        );
        assert_eq!(error.to_string(), "ошибка API Ely.by: неверный логин или пароль");
    }

    #[test]
    fn legacy_error_passes_through_unknown_messages() {
        let error = legacy_api_error(r#"{"error":"SomeError","errorMessage":"Something else"}"#);
        assert!(
            error.to_string().contains("Something else"),
            "{error}"
        );
    }
}
