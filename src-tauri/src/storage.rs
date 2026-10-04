//! Подсчёт дискового пространства и уборка кэша/корзины (F18, D37).
//!
//! Области каталога данных (спека §4.2): `instances/`, `cache/`, корзина
//! `instances/.trash`, `logs/`. Уборка трогает только `cache/` и корзину —
//! `store/` (жёсткие ссылки!) и `instances/` не удаляются никогда.

use crate::errors::Result;
use crate::paths::Paths;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Размеры областей каталога данных. Кэмел-кейс — зеркало типа UI (`StorageStats`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub instances_bytes: u64,
    pub cache_bytes: u64,
    pub trash_bytes: u64,
    pub logs_bytes: u64,
}

/// Каталог корзины: `instances/.trash` — туда `recycle_dir` (instances/mod.rs)
/// переносит удалённые инстансы вне Windows. На Windows корзина системная,
/// каталог обычно отсутствует → 0. «Мусорные» `.trash` внутри самих инстансов
/// (откат модов, modrinth/updates.rs) входят в `instances`, здесь их нет.
fn trash_dir(paths: &Paths) -> PathBuf {
    paths.instances_dir().join(".trash")
}

/// Рекурсивный размер каталога в байтах. Отсутствующий/нечитаемый каталог — 0:
/// уборка и статистика не должны падать из-за пустой области. Симлинки и
/// junction не разворачиваем (иначе зацикливание и двойной счёт) — считаем
/// только саму ссылку.
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(read) = std::fs::read_dir(crate::util::fs::long_path(dir)) else {
        return 0;
    };
    for entry in read.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            total += dir_size(&entry.path());
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}

/// Статистика по областям каталога данных (F18). Каталога нет → 0.
pub fn stats(paths: &Paths) -> Result<Stats> {
    Ok(Stats {
        instances_bytes: dir_size(&paths.instances_dir()),
        cache_bytes: dir_size(&paths.cache_dir()),
        trash_bytes: dir_size(&trash_dir(paths)),
        logs_bytes: dir_size(&paths.logs_dir()),
    })
}

/// Удалить СОДЕРЖИМОЕ каталога, сам каталог оставить (его ждут `ensure_dirs`
/// и код, читающий фиксированные пути без create_dir_all).
fn empty_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(crate::util::fs::long_path(dir))? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(crate::util::fs::long_path(&path))?;
        } else {
            std::fs::remove_file(crate::util::fs::long_path(&path))?;
        }
    }
    Ok(())
}

/// Уборка (F18): очистить `cache/` (включая cache/skins, manifests) и корзину.
/// `store/` и `instances/` не трогаем: стор — общие жёсткие ссылки, удаление
/// сломало бы все инстансы разом. Возвращает освобождённые байты — сумму,
/// посчитанную ДО удаления (после remove_dir_all мерить уже нечего).
pub fn clean(paths: &Paths) -> Result<u64> {
    let freed = dir_size(&paths.cache_dir()) + dir_size(&trash_dir(paths));
    empty_dir(&paths.cache_dir())?;
    empty_dir(&trash_dir(paths))?;
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Фейковый Paths на tempdir + файл `size` байт по относительному хвосту.
    fn make_file(root: &Path, tail: &[&str], size: usize) -> PathBuf {
        let mut path = root.to_path_buf();
        for part in tail {
            path.push(part);
        }
        std::fs::create_dir_all(crate::util::fs::long_path(
            path.parent().expect("хвост без корня"),
        ))
        .expect("каталоги созданы");
        std::fs::write(crate::util::fs::long_path(&path), vec![0u8; size]).expect("файл записан");
        path
    }

    #[test]
    fn stats_counts_instances_cache_trash_logs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::new(dir.path().to_path_buf());
        // cache: manifests/x.json + skins/skin.png (вложенность учитывается).
        make_file(dir.path(), &["cache", "manifests", "x.json"], 30);
        make_file(dir.path(), &["cache", "skins", "skin.png"], 12);
        // instances: обычный контент инстанса; корзина instances/.trash вложена
        // в instances dir, поэтому instances_bytes включает её (на диске она
        // реально занята); категория trash показывает её размер отдельно.
        make_file(dir.path(), &["instances", "inst1", "minecraft", "mods", "a.jar"], 100);
        // Корзина: как recycle_dir вне Windows кладёт удалённые инстансы.
        make_file(dir.path(), &["instances", ".trash", "old-1", "instance.json"], 50);
        // Логи ядра: launcher.log в logs/.
        make_file(dir.path(), &["logs", "launcher.log"], 7);

        let s = stats(&paths).expect("статистика считается");
        assert_eq!(s.cache_bytes, 42, "cache = manifests + skins");
        assert_eq!(s.instances_bytes, 150, "instances включает вложенную корзину");
        assert_eq!(s.trash_bytes, 50, "instances/.trash считается корзиной");
        assert_eq!(s.logs_bytes, 7);
    }

    #[test]
    fn stats_missing_dirs_are_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::new(dir.path().to_path_buf());
        let s = stats(&paths).expect("статистика по пустому корню");
        assert_eq!(s.instances_bytes, 0);
        assert_eq!(s.cache_bytes, 0);
        assert_eq!(s.trash_bytes, 0);
        assert_eq!(s.logs_bytes, 0);
    }

    #[test]
    fn clean_empties_cache_and_trash_keeps_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::new(dir.path().to_path_buf());
        make_file(dir.path(), &["cache", "manifests", "x.json"], 30);
        make_file(dir.path(), &["cache", "skins", "skin.png"], 12);
        make_file(dir.path(), &["instances", ".trash", "old-1", "mods.zip"], 50);
        // store и инстанс уборка не должна трогать.
        let store_file = make_file(dir.path(), &["store", "clients", "a.jar"], 40);
        let instance_file = make_file(dir.path(), &["instances", "inst1", "instance.json"], 10);

        let freed = clean(&paths).expect("уборка проходит");
        assert_eq!(freed, 42 + 50, "возвращены байты кэша и корзины");

        // Кэш и корзина пусты (сами каталоги остаются на месте).
        assert!(paths.cache_dir().exists(), "каталог cache остаётся");
        assert!(
            std::fs::read_dir(crate::util::fs::long_path(&paths.cache_dir()))
                .expect("чтение cache")
                .next()
                .is_none(),
            "cache пуст"
        );
        assert!(
            std::fs::read_dir(crate::util::fs::long_path(&trash_dir(&paths)))
                .expect("чтение корзины")
                .next()
                .is_none(),
            "корзина пуста"
        );
        // store/ и instances/ не тронуты.
        assert!(store_file.exists(), "store (жёсткие ссылки) не трогаем");
        assert!(instance_file.exists(), "инстансы не трогаем");
        // Повторная уборка освобождает 0.
        assert_eq!(clean(&paths).expect("повторная уборка"), 0);
    }

    #[test]
    fn clean_missing_dirs_is_ok_and_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::new(dir.path().to_path_buf());
        assert_eq!(clean(&paths).expect("нет каталогов — не ошибка"), 0);
    }
}
