use md5::{Digest, Md5};
use uuid::Uuid;

use super::model::{now_millis, Account, AuthError};

const MIN_USERNAME_LEN: usize = 3;
const MAX_USERNAME_LEN: usize = 16;

pub(crate) fn validate_username(username: &str) -> Result<(), AuthError> {
    let length = username.chars().count();
    if length < MIN_USERNAME_LEN {
        return Err(AuthError::invalid_username(
            username,
            "ник короче трёх символов",
        ));
    }
    if length > MAX_USERNAME_LEN {
        return Err(AuthError::invalid_username(
            username,
            "ник длиннее шестнадцати символов",
        ));
    }
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(AuthError::invalid_username(
            username,
            "разрешены только латинские буквы, цифры и подчёркивание",
        ));
    }
    Ok(())
}

pub(crate) fn offline_uuid(username: &str) -> String {
    let mut digest = [0u8; 16];
    digest.copy_from_slice(&Md5::digest(
        format!("OfflinePlayer:{username}").into_bytes(),
    ));
    digest[6] = (digest[6] & 0x0F) | 0x30;
    digest[8] = (digest[8] & 0x3F) | 0x80;
    Uuid::from_bytes(digest).to_string()
}

pub(crate) fn login_offline(username: &str) -> Result<Account, AuthError> {
    let username = username.trim();
    validate_username(username)?;
    Ok(Account {
        username: username.to_owned(),
        uuid: offline_uuid(username),
        is_offline: true,
        logged_in_at: now_millis()?,
        ely: None,
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use uuid::{Uuid, Variant};

    use super::{login_offline, offline_uuid, validate_username};
    use crate::auth::AuthError;

    #[test]
    fn accepts_valid_usernames() {
        for name in ["abc", "Notch", "Player_123", "0123456789abcdef"] {
            assert!(validate_username(name).is_ok(), "{name} должен пройти");
        }
    }

    #[test]
    fn rejects_short_long_and_bad_chars() {
        for name in ["", "ab", "0123456789abcdefg", "Player 1", "Ник", "a-b"] {
            assert!(matches!(
                validate_username(name),
                Err(AuthError::InvalidUsername { .. })
            ));
        }
    }

    #[test]
    fn offline_uuid_is_deterministic_v3() {
        let uuid = Uuid::from_str(&offline_uuid("Notch")).unwrap();
        assert_eq!(uuid.get_version_num(), 3);
        assert_eq!(uuid.get_variant(), Variant::RFC4122);
        assert_eq!(offline_uuid("Notch"), offline_uuid("Notch"));
        assert_ne!(offline_uuid("Notch"), offline_uuid("notch"));
    }

    #[test]
    fn offline_uuid_matches_java_name_uuid_form_bytes() {
        assert_eq!(
            offline_uuid("Notch"),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
    }

    #[test]
    fn login_offline_trims_and_creates_account() {
        let account = login_offline("  Notch  ").unwrap();
        assert_eq!(account.username, "Notch");
        assert_eq!(account.uuid, "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert!(account.is_offline);
        assert!(account.logged_in_at > 0);
    }

    #[test]
    fn login_offline_rejects_bad_name() {
        assert!(matches!(
            login_offline("x"),
            Err(AuthError::InvalidUsername { .. })
        ));
    }
}
