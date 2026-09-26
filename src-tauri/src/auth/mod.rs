//! Аккаунты (спека §6.8): реестр accounts.json, offline/MSA/ely.by.
//! Refresh-токены — в Windows Credential Manager (keyring), в файле — ссылки.

pub mod ely;
pub mod msa;
pub mod offline;

use crate::errors::{LauncherError, Result};
use crate::util::fs::{atomic_write, long_path};
use serde::{Deserialize, Serialize};

/// Тип профиля. Offline — «нелицензионный профиль» (пометка в UI, спека §6.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Offline,
    Msa,
    Ely,
}

/// Профиль аккаунта. Токены НЕ хранятся здесь — только ссылка на keyring.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub kind: AccountKind,
    pub name: String,
    pub uuid: String,
    /// Имя записи в keyring (service = "mc-launcher-v2").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_ref: Option<String>,
}

impl Account {
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            AccountKind::Offline => "офлайн (нелицензионный)",
            AccountKind::Msa => "Microsoft",
            AccountKind::Ely => "ely.by",
        }
    }
}

pub fn accounts_path(paths: &crate::paths::Paths) -> std::path::PathBuf {
    paths.root().join("accounts.json")
}

pub fn list(paths: &crate::paths::Paths) -> Vec<Account> {
    let p = accounts_path(paths);
    let long = long_path(&p);
    if !long.exists() {
        return Vec::new();
    }
    match std::fs::read(&long)
        .map_err(crate::errors::LauncherError::from)
        .and_then(|d| serde_json::from_slice::<Vec<Account>>(&d).map_err(crate::errors::LauncherError::from))
    {
        Ok(list) => list,
        Err(e) => {
            tracing::warn!("битый accounts.json: {e}");
            Vec::new()
        }
    }
}

pub fn save(paths: &crate::paths::Paths, accounts: &[Account]) -> Result<()> {
    let data = serde_json::to_vec_pretty(accounts)?;
    atomic_write(&accounts_path(paths), &data)
}

pub fn add(paths: &crate::paths::Paths, account: Account) -> Result<Account> {
    let mut list = list(paths);
    list.retain(|a| a.id != account.id);
    list.push(account.clone());
    save(paths, &list)?;
    Ok(account)
}

pub fn remove(paths: &crate::paths::Paths, id: &str) -> Result<()> {
    let mut list = list(paths);
    let removed = list.iter().find(|a| a.id == id).cloned();
    list.retain(|a| a.id != id);
    save(paths, &list)?;
    // Токен из keyring тоже удалить.
    if let Some(acc) = removed {
        if let Some(rr) = &acc.refresh_ref {
            let _ = crate::auth::keyring_delete(rr);
        }
    }
    Ok(())
}

/// Активный аккаунт (settings.accountsActiveId).
pub fn active(paths: &crate::paths::Paths) -> Option<Account> {
    let settings = crate::settings::Settings::load(&paths.settings_file()).ok()?;
    let id = settings.accounts_active_id?;
    list(paths).into_iter().find(|a| a.id == id)
}

pub fn set_active(paths: &crate::paths::Paths, id: &str) -> Result<()> {
    if !list(paths).iter().any(|a| a.id == id) {
        return Err(LauncherError::NotFound(format!("аккаунт {id}")));
    }
    let mut settings = crate::settings::Settings::load(&paths.settings_file())?;
    settings.accounts_active_id = Some(id.to_string());
    settings.save(&paths.settings_file())
}

// ---------- keyring (спека §6.8, §11: токены никогда не plaintext) ----------

const KEYRING_SERVICE: &str = "mc-launcher-v2";

pub fn keyring_set(ref_name: &str, secret: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, ref_name)
        .map_err(|e| LauncherError::Internal(format!("keyring: {e}")))?;
    entry
        .set_password(secret)
        .map_err(|e| LauncherError::Internal(format!("keyring set: {e}")))
}

pub fn keyring_get(ref_name: &str) -> Result<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, ref_name)
        .map_err(|e| LauncherError::Internal(format!("keyring: {e}")))?;
    entry
        .get_password()
        .map_err(|e| LauncherError::NotFound(format!("keyring {ref_name}: {e}")))
}

pub fn keyring_delete(ref_name: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, ref_name)
        .map_err(|e| LauncherError::Internal(format!("keyring: {e}")))?;
    entry
        .delete_credential()
        .map_err(|e| LauncherError::Internal(format!("keyring delete: {e}")))
}

/// Итог для запуска игры: логин, uuid, access_token, user_type.
#[derive(Debug, Clone)]
pub struct LaunchIdentity {
    pub player_name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
}

/// Идентичность для запуска: MSA/ely — с валидным токеном (refresh при
/// необходимости), offline — token "0".
pub async fn launch_identity(
    paths: &crate::paths::Paths,
    client: &crate::net::http::HttpClient,
) -> Result<LaunchIdentity> {
    let Some(acc) = active(paths) else {
        // Нет активного аккаунта — офлайн-профиль по умолчанию.
        return Ok(offline::identity("Player"));
    };
    match acc.kind {
        AccountKind::Offline => Ok(offline::identity(&acc.name)),
        AccountKind::Msa => {
            let Some(rr) = &acc.refresh_ref else {
                return Err(LauncherError::InvalidInput(
                    "у MSA-аккаунта нет refresh-токена".into(),
                ));
            };
            let refresh = keyring_get(rr)?;
            let session = msa::refresh_session(client, &refresh).await?;
            // Обновлённый refresh-токен (если пришёл) — обратно в keyring.
            if session.refresh_token != refresh {
                keyring_set(rr, &session.refresh_token)?;
            }
            Ok(LaunchIdentity {
                player_name: session.player_name,
                uuid: session.uuid,
                access_token: session.access_token,
                user_type: "msa".into(),
            })
        }
        AccountKind::Ely => {
            let Some(rr) = &acc.refresh_ref else {
                return Err(LauncherError::InvalidInput(
                    "у ely-аккаунта нет refresh-токена".into(),
                ));
            };
            let refresh = keyring_get(rr)?;
            let session = crate::auth::ely::refresh_session(client, &refresh).await?;
            if session.refresh_token != refresh {
                keyring_set(rr, &session.refresh_token)?;
            }
            Ok(LaunchIdentity {
                player_name: session.player_name,
                uuid: session.uuid,
                access_token: session.access_token,
                user_type: "msa".into(),
            })
        }
    }
}

/// Проверить файл аккаунтов: никаких токенов plaintext (спека §14).
pub fn assert_no_plaintext_tokens(paths: &crate::paths::Paths) -> Result<()> {
    let data = std::fs::read(long_path(&accounts_path(paths)))?;
    let text = String::from_utf8_lossy(&data);
    // Refresh-токены MSA начинаются с "M.C"; ely — длинные случайные. Ищем
    // явные паттерны и поле access_token.
    for bad in ["\"accessToken\"", "M.C5", "\"refreshToken\""] {
        if text.contains(bad) {
            return Err(LauncherError::Internal(format!(
                "В accounts.json обнаружен plaintext-токен ({bad}) — нарушение спеки §11"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows Credential Manager: roundtrip (ручная приёмка M8, спека §9).
    #[tokio::test]
    #[ignore]
    async fn keyring_roundtrip() {
        let name = "test-refresh-roundtrip";
        keyring_set(name, "M.CSECRET_REFRESH_TOKEN_VALUE").unwrap();
        let got = keyring_get(name).unwrap();
        assert_eq!(got, "M.CSECRET_REFRESH_TOKEN_VALUE");
        keyring_delete(name).unwrap();
        assert!(keyring_get(name).is_err());
    }

    #[test]
    fn accounts_roundtrip_without_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let acc = Account {
            id: "a1".into(),
            kind: AccountKind::Msa,
            name: "Steve".into(),
            uuid: "0000".into(),
            refresh_ref: Some("msa-refresh-x".into()),
        };
        add(&paths, acc).unwrap();
        let list = list(&paths);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind_label(), "Microsoft");
        // Токенов plaintext нет — только ссылка на keyring.
        assert!(assert_no_plaintext_tokens(&paths).is_ok());
    }

    #[test]
    fn plaintext_token_detected() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let p = accounts_path(&paths);
        std::fs::write(
            crate::util::fs::long_path(&p),
            br#"[{"id":"x","kind":"msa","name":"S","uuid":"u","refreshToken":"M.C5_BLAH"}]"#,
        )
        .unwrap();
        assert!(assert_no_plaintext_tokens(&paths).is_err());
    }
}
