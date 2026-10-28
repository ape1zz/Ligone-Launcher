use std::{fmt, time::{SystemTime, UNIX_EPOCH}};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub username: String,
    pub uuid: String,
    pub is_offline: bool,
    pub logged_in_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ely: Option<ElyAccount>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElyAccount {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token_expires_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthError {
    InvalidUsername {
        username: String,
        reason: String,
    },
    UnknownAccount {
        username: String,
    },
    Storage {
        message: String,
    },
    OAuth {
        message: String,
    },
    ElyApi {
        message: String,
    },
    Cancelled,
}

impl AuthError {
    pub(crate) fn invalid_username(username: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidUsername {
            username: username.into(),
            reason: reason.into(),
        }
    }

    pub(crate) fn unknown_account(username: impl Into<String>) -> Self {
        Self::UnknownAccount {
            username: username.into(),
        }
    }

    pub(crate) fn oauth(message: impl Into<String>) -> Self {
        Self::OAuth {
            message: message.into(),
        }
    }

    pub(crate) fn ely_api(message: impl Into<String>) -> Self {
        Self::ElyApi {
            message: message.into(),
        }
    }

    pub(crate) fn cancelled() -> Self {
        Self::Cancelled
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUsername { username, reason } => {
                write!(formatter, "недопустимый ник «{username}»: {reason}")
            }
            Self::UnknownAccount { username } => {
                write!(formatter, "неизвестный аккаунт «{username}»")
            }
            Self::Storage { message } => {
                write!(formatter, "ошибка хранилища: {message}")
            }
            Self::OAuth { message } => {
                write!(formatter, "ошибка OAuth: {message}")
            }
            Self::ElyApi { message } => {
                write!(formatter, "ошибка API Ely.by: {message}")
            }
            Self::Cancelled => {
                write!(formatter, "авторизация отменена")
            }
        }
    }
}

pub(crate) fn now_millis() -> Result<u64, AuthError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .map_err(|error| AuthError::Storage {
            message: error.to_string(),
        })
}

impl std::error::Error for AuthError {}

impl From<std::io::Error> for AuthError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage {
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Account, ElyAccount};

    #[test]
    fn account_serializes_to_camel_case() {
        let account = Account {
            username: "Notch".to_owned(),
            uuid: "b50ad385-829d-3141-a216-7e7d7539ba7f".to_owned(),
            is_offline: true,
            logged_in_at: 1,
            ely: None,
        };
        let json = serde_json::to_value(&account).unwrap();
        assert_eq!(json["username"], "Notch");
        assert_eq!(json["uuid"], "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(json["isOffline"], true);
        assert_eq!(json["loggedInAt"], 1);
        assert!(json.get("ely").is_none());
    }

    #[test]
    fn ely_account_serializes_to_camel_case() {
        let account = Account {
            username: "Notch".to_owned(),
            uuid: "some-uuid".to_owned(),
            is_offline: false,
            logged_in_at: 1,
            ely: Some(ElyAccount {
                access_token: "at".to_owned(),
                refresh_token: Some("rt".to_owned()),
                client_token: None,
                access_token_expires_at: Some(42),
            }),
        };
        let json = serde_json::to_value(&account).unwrap();
        assert_eq!(json["ely"]["accessToken"], "at");
        assert_eq!(json["ely"]["refreshToken"], "rt");
        assert_eq!(json["ely"]["accessTokenExpiresAt"], 42);
    }
}
