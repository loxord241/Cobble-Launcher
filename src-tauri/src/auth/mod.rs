//! Аккаунты (спека §6.8): реестр accounts.json, offline/MSA/ely.by.
//! Refresh-токены — в Windows Credential Manager (keyring), в файле — ссылки.

pub mod custom;
pub mod ely;
pub mod msa;
pub mod offline;
pub mod skins;

use crate::errors::{LauncherError, Result};
use crate::util::fs::{atomic_write, long_path};
use serde::{Deserialize, Serialize};

/// Тип профиля. Offline — «нелицензионный профиль» (пометка в UI, спека §6.8).
/// Authlib — свой authlib-injector сервер (F13); ely.by — частный случай с
/// зашитым URL (auth/ely.rs), поэтому отдельный вид.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Offline,
    Msa,
    Ely,
    Authlib,
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
    /// Корень authlib-сервера (нормализованный, auth::custom::normalize_server_url)
    /// для kind = Authlib; у ely/MSA/offline — None (и в JSON не пишется:
    /// старый accounts.json без поля читается как раньше).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authlib_server: Option<String>,
}

impl Account {
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            AccountKind::Offline => "офлайн (нелицензионный)",
            AccountKind::Msa => "Microsoft",
            AccountKind::Ely => "ely.by",
            AccountKind::Authlib => "authlib-сервер",
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
    // Активный id читаем только при реальном удалении: иначе remove стал бы
    // падать на битых settings и без причины.
    let active_id = if removed.is_some() {
        crate::settings::Settings::load(&paths.settings_file())?.accounts_active_id
    } else {
        None
    };
    let was_active = active_id.as_deref() == Some(id);
    list.retain(|a| a.id != id);
    save(paths, &list)?;
    // Токен из keyring тоже удалить.
    if let Some(acc) = removed {
        if let Some(rr) = &acc.refresh_ref {
            let _ = crate::auth::keyring_delete(rr);
        }
    }
    // Удалённый был активным — иначе в settings остался бы висячий id и launch
    // молча играл под «Player». Назначаем первого оставшегося (тем же
    // set_active, что и команда account_active_set) или очищаем при пустом списке.
    if was_active {
        match list.first() {
            Some(first) => set_active(paths, &first.id)?,
            None => {
                let mut settings = crate::settings::Settings::load(&paths.settings_file())?;
                settings.accounts_active_id = None;
                settings.save(&paths.settings_file())?;
            }
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
            let settings =
                crate::settings::Settings::load(&paths.settings_file())?;
            let cid = settings.azure_client_id.ok_or_else(|| {
                LauncherError::InvalidInput(
                    "нет azureClientId в настройках — укажите его в Настройках (MSA-вход)".into(),
                )
            })?;
            let session = msa::refresh_session(client, &refresh, &cid).await?;
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
        AccountKind::Authlib => {
            // Свой authlib-сервер (F13): refresh по тому же протоколу, что у
            // ely, но URL берётся из аккаунта.
            let Some(server) = &acc.authlib_server else {
                return Err(LauncherError::InvalidInput(
                    "у authlib-аккаунта нет URL сервера".into(),
                ));
            };
            let Some(rr) = &acc.refresh_ref else {
                return Err(LauncherError::InvalidInput(
                    "у authlib-аккаунта нет refresh-токена".into(),
                ));
            };
            let refresh = keyring_get(rr)?;
            let session = custom::refresh_session(client, server, &refresh).await?;
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
            authlib_server: None,
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

    fn offline_acc(id: &str, name: &str) -> Account {
        Account {
            id: id.into(),
            kind: AccountKind::Offline,
            name: name.into(),
            uuid: format!("uuid-{id}"),
            // Без keyring — тесты герметичны.
            refresh_ref: None,
            authlib_server: None,
        }
    }

    /// Удалили активный аккаунт: активным становится первый оставшийся,
    /// иначе в settings остаётся висячий id и launch играет под «Player».
    #[test]
    fn remove_active_account_reassigns_to_first_remaining() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        add(&paths, offline_acc("a1", "Steve")).unwrap();
        add(&paths, offline_acc("a2", "Alex")).unwrap();
        set_active(&paths, "a1").unwrap();
        remove(&paths, "a1").unwrap();
        let active = active(&paths).expect("активный должен быть переназначен");
        assert_eq!(active.id, "a2");
        let settings = crate::settings::Settings::load(&paths.settings_file()).unwrap();
        assert_eq!(settings.accounts_active_id.as_deref(), Some("a2"));
    }

    /// Удалили последний аккаунт: активного нет, id в settings очищен.
    #[test]
    fn remove_last_account_clears_active() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        add(&paths, offline_acc("a1", "Steve")).unwrap();
        set_active(&paths, "a1").unwrap();
        remove(&paths, "a1").unwrap();
        assert!(active(&paths).is_none());
        let settings = crate::settings::Settings::load(&paths.settings_file()).unwrap();
        assert!(settings.accounts_active_id.is_none());
    }

    /// Удаление неактивного аккаунта активный не меняет.
    #[test]
    fn remove_non_active_account_keeps_active() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        add(&paths, offline_acc("a1", "Steve")).unwrap();
        add(&paths, offline_acc("a2", "Alex")).unwrap();
        set_active(&paths, "a2").unwrap();
        remove(&paths, "a1").unwrap();
        assert_eq!(active(&paths).unwrap().id, "a2");
        let settings = crate::settings::Settings::load(&paths.settings_file()).unwrap();
        assert_eq!(settings.accounts_active_id.as_deref(), Some("a2"));
    }

    /// F13/serde-совместимость: старый accounts.json без поля authlibServer
    /// читается как раньше (поле — default None), существующие kinds не менялись.
    #[test]
    fn old_accounts_json_without_authlib_server_parses() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        std::fs::write(
            crate::util::fs::long_path(&accounts_path(&paths)),
            br#"[{"id":"x","kind":"ely","name":"S","uuid":"u","refreshRef":"ely-refresh-1"}]"#,
        )
        .unwrap();
        let list = list(&paths);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, AccountKind::Ely);
        assert_eq!(list[0].authlib_server, None);
        assert_eq!(list[0].kind_label(), "ely.by");
    }

    /// Authlib-аккаунт: kind "authlib" и authlibServer переживают roundtrip;
    /// для аккаунтов без сервера поле в JSON не пишется (skip_serializing_if).
    #[test]
    fn authlib_account_roundtrip_keeps_server() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        add(
            &paths,
            Account {
                id: "c1".into(),
                kind: AccountKind::Authlib,
                name: "Steve".into(),
                uuid: "c1uuid".into(),
                refresh_ref: Some("authlib-refresh-1".into()),
                authlib_server: Some("https://auth.example.org".into()),
            },
        )
        .unwrap();
        // Соседний offline-аккаунт — чтобы проверить отсутствие поля у других kinds.
        add(&paths, offline_acc("o1", "Alex")).unwrap();
        let raw = std::fs::read(crate::util::fs::long_path(&accounts_path(&paths))).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_slice(&raw).unwrap();
        let c1 = parsed
            .iter()
            .find(|v| v["id"] == "c1")
            .expect("authlib-аккаунт в JSON");
        assert_eq!(c1["kind"], "authlib");
        assert_eq!(c1["authlibServer"], "https://auth.example.org");
        let o1 = parsed
            .iter()
            .find(|v| v["id"] == "o1")
            .expect("offline-аккаунт в JSON");
        assert!(
            o1.get("authlibServer").is_none(),
            "None не сериализуется: {o1}"
        );

        // И обратно в Account: сервер на месте, label честный.
        let list = list(&paths);
        let authlib = list.iter().find(|a| a.id == "c1").unwrap();
        assert_eq!(authlib.kind, AccountKind::Authlib);
        assert_eq!(
            authlib.authlib_server.as_deref(),
            Some("https://auth.example.org")
        );
        assert_eq!(authlib.kind_label(), "authlib-сервер");
    }
}
