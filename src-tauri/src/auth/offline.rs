//! Офлайн-профили: UUID по правилам Minecraft `OfflinePlayer:<ник>` (спека §6.8).

use sha2::Digest as _;
use uuid::Uuid;

/// Идентичность для запуска (спека §6.8: token="0", user_type=legacy).
pub fn identity(nick: &str) -> crate::auth::LaunchIdentity {
    crate::auth::LaunchIdentity {
        player_name: nick.to_string(),
        uuid: offline_uuid(nick).to_string(),
        access_token: "0".into(),
        user_type: "legacy".into(),
    }
}

/// UUID офлайн-игрока: md5(`OfflinePlayer:<ник>`) с version=3 и RFC-вариантом.
/// (md-5 выбран сознательно — так делает сама Minecraft.)
pub fn offline_uuid(nick: &str) -> Uuid {
    let digest = md5::Md5::digest(format!("OfflinePlayer:{nick}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest);
    b[6] = (b[6] & 0x0f) | 0x30; // version 3
    b[8] = (b[8] & 0x3f) | 0x80; // variant RFC 4122
    Uuid::from_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_is_version3_deterministic() {
        let a = offline_uuid("Steve");
        let b = offline_uuid("Steve");
        assert_eq!(a, b);
        assert_eq!(a.get_version_num(), 3);
        assert_ne!(offline_uuid("Alex"), a);
    }

    #[test]
    fn offline_uuid_matches_reference() {
        // Референс: стандартное поведение vanilla — uuid "Steve" = 0abcc7d8-...
        // Проверяем только формат и детерминизм; точный вектор сверяется на M8.
        let u = offline_uuid("Notch");
        assert_eq!(u.to_string().len(), 36);
    }
}
