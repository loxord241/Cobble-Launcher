//! Панорамы Minecraft как дефолтный арт инстанса (D63): официальные
//! пейзажи титульного экрана уже скачаны в общие ассеты пользователя
//! (assets/objects по хэшам из asset index). Читаем СВОИ скачанные файлы —
//! в репозитории и бандле нет ни байта Mojang. Грань куба выбирается
//! детерминированно от id инстанса — у разных инстансов разные пейзажи.

use crate::errors::{LauncherError, Result};
use crate::paths::Paths;
use crate::util::fs::long_path;
use base64::Engine as _;

/// FNV-1a 32 бит — общий с ядром/фронтом хэш.
fn fnv1a(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// D71: разумный верхний предел размера файла панорамы: png титульного
/// экрана — сотни килобайт, всё, что претендует на большее, — мусор из
/// битого индекса, в base64-дата-URL его тащить нельзя (32 МБ в IPC).
const MAX_PANORAMA_BYTES: u64 = 32 * 1024 * 1024;

/// D71: хэш объекта из asset index — прежде чем собирать из него путь
/// (`objects/<префикс>/<хэш>`), проверяем формат: непустой, ≤128 символов,
/// только строчные [0-9a-f]. Отсекает traversal («../../evil» не пройдёт —
/// в наборе нет точек и слэшей) и мусор битого индекса.
fn valid_object_hash(hash: &str) -> bool {
    // Нижняя граница 2: из хэша берётся префикс `&hash[..2]` (каталог objects).
    (2..=128).contains(&hash.len())
        && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// data-URL панорамы инстанса: Ok(None) = файлов ещё нет (свежий инстанс,
/// игру не скачивали) — фронт покажет запасной пиксель-пейзаж.
pub fn panorama_data_url(paths: &Paths, instance_id: &str) -> Result<Option<String>> {
    let inst = super::load(paths, instance_id)?;
    let mc_version = inst.mc_version;

    // Версия → asset index id (кэш манифеста версии; может отсутствовать).
    let manifest_path = paths.manifests_cache().join(format!("{mc_version}.json"));
    let manifest: serde_json::Value = match std::fs::read(long_path(&manifest_path)) {
        Ok(data) => serde_json::from_slice(&data)?,
        Err(_) => return Ok(None),
    };
    let Some(index_id) = manifest
        .pointer("/assetIndex/id")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
    else {
        return Ok(None);
    };

    // Asset index → хэш граней панорамы (6 штук, у новых версий есть).
    let index_path = paths.assets_indexes().join(format!("{index_id}.json"));
    let index: serde_json::Value = match std::fs::read(long_path(&index_path)) {
        Ok(data) => serde_json::from_slice(&data)?,
        Err(_) => return Ok(None),
    };
    let face = fnv1a(instance_id) % 6;
    let name = format!("minecraft/textures/gui/title/background/panorama_{face}.png");
    // Ключ в objects СОДЕРЖИТ слэши (имя файла) — JSON Pointer тут не годится,
    // он делит по слэшам на уровни; берём ключ целиком.
    let Some(hash) = index
        .get("objects")
        .and_then(|o| o.get(name.as_str()))
        .and_then(|e| e.get("hash"))
        .and_then(|v| v.as_str())
    else {
        // Легаси-индексы (1.12) панорам не имеют — честный None.
        return Ok(None);
    };

    // D71: битый assets-индекс может отдать короткий, не-ASCII или
    // traversal-хэш — такой объект честно пропускаем (как missing), путь из
    // непроверенного хэша не собираем вовсе.
    if !valid_object_hash(hash) {
        tracing::warn!("панорама: мусорный хэш объекта в индексе {index_id:?} — пропущен");
        return Ok(None);
    }
    let prefix = &hash[..2];
    let object_path = paths.assets_objects().join(prefix).join(hash);
    let long = long_path(&object_path);
    // D71: размер проверяем до чтения — гигантский «объект» из битого индекса
    // не уезжает в память целиком.
    let meta = match std::fs::metadata(&long) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(LauncherError::Io(e)),
    };
    if meta.len() > MAX_PANORAMA_BYTES {
        tracing::warn!(
            "панорама: объект {} больше {MAX_PANORAMA_BYTES} байт — пропущен",
            object_path.display()
        );
        return Ok(None);
    }
    let data = match std::fs::read(&long) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(LauncherError::Io(e)),
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    Ok(Some(format!("data:image/png;base64,{b64}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_is_stable_and_varied() {
        assert_eq!(fnv1a("abc") % 6, fnv1a("abc") % 6);
        // На 12 id грань меняется — не одна и та же для всех.
        let faces: std::collections::HashSet<u32> =
            (0..12).map(|i| fnv1a(&format!("inst-{i}")) % 6).collect();
        assert!(faces.len() >= 3, "грани должны различаться, вышло {faces:?}");
    }

    /// D71: хэш объекта из asset index валидируется до сборки пути:
    /// настоящий sha1 проходит, traversal/верхний регистр/пустой/слишком
    /// длинный/односимвольный (нужен префикс из 2) — нет.
    #[test]
    fn object_hash_charset_and_length() {
        assert!(valid_object_hash(&"a".repeat(40)), "настоящий sha1");
        assert!(valid_object_hash("0f"), "минимальная длина (префикс 2)");
        for bad in [
            "",
            "a",
            "A".repeat(40).as_str(),        // верхний регистр не в [0-9a-f]
            "../../evil",                   // traversal: точки и слэши не hex
            "abc/def",                      // слэш внутри
            "abc.def",                      // точка внутри
            "g0000000000000000000000000000000000000000", // не-hex символ
            &"f".repeat(129),               // длиннее разумного предела
        ] {
            assert!(!valid_object_hash(bad), "должен быть отклонён: {bad:?}");
        }
    }
}
