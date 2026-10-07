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
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

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

/// D64: сериализация read-modify-write accounts.json (пары list→retain/push→save):
/// без замка параллельные add/remove читали список одновременно и затирали
/// записи друг друга последним save. Синхронный Mutex: под замком только
/// файловый IO, без await — секции короткие.
static ACCOUNTS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn accounts_lock() -> std::sync::MutexGuard<'static, ()> {
    ACCOUNTS_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn add(paths: &crate::paths::Paths, account: Account) -> Result<Account> {
    let _lock = accounts_lock();
    let mut list = list(paths);
    list.retain(|a| a.id != account.id);
    list.push(account.clone());
    save(paths, &list)?;
    Ok(account)
}

pub fn remove(paths: &crate::paths::Paths, id: &str) -> Result<()> {
    let _lock = accounts_lock();
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
    // Токен из keyring тоже удалить. Не молчим при неудаче — недоступный
    // keyring стоит знать (аудит 2026-10-06), но удаление аккаунта не отменяем.
    if let Some(acc) = removed {
        if let Some(rr) = &acc.refresh_ref {
            if let Err(e) = crate::auth::keyring_delete(rr) {
                tracing::warn!("accounts: токен {rr} не удалён из keyring: {e}");
            }
        }
    }
    // Удалённый был активным — иначе в settings остался бы висячий id и launch
    // молча играл под «Player». Назначаем первого оставшегося (тем же
    // set_active, что и команда account_active_set) или очищаем при пустом списке.
    if was_active {
        match list.first() {
            // set_active_locked: замок уже держим здесь — публичный set_active
            // захватил бы его повторно и deadlock'нулся бы.
            Some(first) => set_active_locked(paths, &first.id)?,
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
    let _lock = accounts_lock();
    set_active_locked(paths, id)
}

/// Тело set_active БЕЗ захвата замка — для вызова из-под уже удерживаемого
/// ACCOUNTS_LOCK (remove). Проверка list→save settings под тем же замком.
fn set_active_locked(paths: &crate::paths::Paths, id: &str) -> Result<()> {
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
    entry.get_password().map_err(|e| match e {
        // Нет записи — штатное «не сохранён» (NotFound); прочее — реальная
        // недоступность хранилища (Internal), чтобы диагностика не врала
        // (аудит 2026-10-06).
        keyring::Error::NoEntry => LauncherError::NotFound(format!(
            "токен {ref_name} не сохранён"
        )),
        other => LauncherError::Internal(format!("keyring недоступен: {other}")),
    })
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

/// Single-flight refresh (аудит 2026-10-06): конкурентные потребители одного
/// аккаунта (запуск игры, смена скина) не должны гонять refresh параллельно —
/// сервер инвалидирует прежний access_token, и второй запрос ловит 401.
/// Мьютекс per refresh_ref из глобального реестра; под замком — чтение
/// refresh-токена из keyring и сетевой refresh_session (D64: чтение ДО gate
/// гоняло протухший токен после параллельной ротации). Запись ротации —
/// вне мьютекса.
pub(crate) fn refresh_gate(refresh_ref: &str) -> Arc<tokio::sync::Mutex<()>> {
    static GATES: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let gates = GATES.get_or_init(|| Mutex::new(HashMap::new()));
    gates
        .lock()
        .unwrap()
        .entry(refresh_ref.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// D64: после refresh имя профиля могло измениться на сайте (Mojang/ely
/// отдают актуальный player_name) — синхронизируем accounts.json, иначе UI и
/// launch играют под старым ником. add() переиспользует запись по id
/// (retain по id + push), так что обновляется только поле name.
/// Best-effort: неудача записи — warn, запуск игры не отменяем.
fn sync_account_name(paths: &crate::paths::Paths, acc: &Account, player_name: &str) {
    if acc.name == player_name {
        return;
    }
    let mut updated = acc.clone();
    updated.name = player_name.to_string();
    if let Err(e) = add(paths, updated) {
        tracing::warn!("accounts: не обновить имя аккаунта {}: {e}", acc.id);
    }
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
            let settings =
                crate::settings::Settings::load(&paths.settings_file())?;
            let cid = settings.azure_client_id.ok_or_else(|| {
                LauncherError::InvalidInput(
                    "нет azureClientId в настройках — укажите его в Настройках (MSA-вход)".into(),
                )
            })?;
            // Single-flight: под мьютексом чтение refresh-токена из keyring и
            // сетевой refresh (D64: чтение ДО gate успевало протухнуть после
            // параллельной ротации); запись ротации — вне блока.
            // Arc биндим отдельно: guard заимствует его, временного значения
            // недостаточно (E0716).
            let (session, refresh) = {
                let gate = refresh_gate(rr);
                let _held = gate.lock().await;
                let refresh = keyring_get(rr)?;
                let session = msa::refresh_session(client, &refresh, &cid).await?;
                (session, refresh)
            };
            // Обновлённый refresh-токен (если пришёл) — обратно в keyring.
            if session.refresh_token != refresh {
                keyring_set(rr, &session.refresh_token)?;
            }
            // D64: ник могли сменить на сайте — протухшее имя в accounts.json
            // обновляем (best-effort, add() переиспользует запись по id).
            sync_account_name(paths, &acc, &session.player_name);
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
            let (session, refresh) = {
                let gate = refresh_gate(rr);
                let _held = gate.lock().await;
                let refresh = keyring_get(rr)?;
                let session = crate::auth::ely::refresh_session(client, &refresh).await?;
                (session, refresh)
            };
            if session.refresh_token != refresh {
                keyring_set(rr, &session.refresh_token)?;
            }
            sync_account_name(paths, &acc, &session.player_name);
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
            let (session, refresh) = {
                let gate = refresh_gate(rr);
                let _held = gate.lock().await;
                let refresh = keyring_get(rr)?;
                let session = custom::refresh_session(client, server, &refresh).await?;
                (session, refresh)
            };
            if session.refresh_token != refresh {
                keyring_set(rr, &session.refresh_token)?;
            }
            sync_account_name(paths, &acc, &session.player_name);
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
    // Refresh-токены MSA начинаются с "M.C" (consumer-токены M.C1..M.C999,
    // аудит 2026-10-06: без «5» — не все ветки); ely — длинные случайные.
    // Ищем явные паттерны и поле access_token.
    for bad in ["\"accessToken\"", "M.C", "\"refreshToken\""] {
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

    /// Single-flight refresh (аудит 2026-10-06): один refresh_ref — один
    /// мьютекс, второй конкурентный потребитель ждёт; разные ref независимы.
    /// Сетевую часть не замокать (URL зашиты) — логический тест реестра.
    #[tokio::test]
    async fn refresh_gate_single_flight_per_ref() {
        use std::time::Duration;
        let same1 = refresh_gate("gate-test-ref");
        let same2 = refresh_gate("gate-test-ref");
        let other = refresh_gate("gate-test-other");
        assert!(Arc::ptr_eq(&same1, &same2), "один ref — один мьютекс");
        assert!(!Arc::ptr_eq(&same1, &other), "разные ref — разные мьютексы");

        // Захватили gate: второй «потребитель» того же ref ждёт освобождения.
        let guard = same1.lock().await;
        let task = tokio::spawn({
            let gate = same2.clone();
            async move {
                let _second = gate.lock().await;
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!task.is_finished(), "конкурентный refresh ждёт мьютекс");
        drop(guard);
        task.await.unwrap();

        // Чужой ref не блокируется занятым мьютексом.
        let _own = other.lock().await;
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
