//! Процесс игры: супервизия (instances/run), краш-анализ, мониторинг ресурсов.

pub mod crash;
pub mod discord;
pub mod monitor;

/// Семпл ресурсов запущенного инстанса (F28): PID берём из `.lock`.
/// None — инстанс не запущен, процесс уже исчез или id некорректен (A1:
/// сигнатура Option не позволяет вернуть ошибку, путь `../x` отсекаем до
/// обращения к файловой системе).
pub fn sample_for_instance(
    paths: &crate::paths::Paths,
    instance_id: &str,
) -> Option<monitor::GameResourceSample> {
    if crate::instances::valid_id(instance_id).is_err() {
        return None;
    }
    let dir = crate::instances::instance_dir(paths, instance_id);
    let pid = crate::instances::running_pid(&dir)?;
    monitor::sample(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> (tempfile::TempDir, crate::paths::Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        (dir, paths)
    }

    /// F28: нет `.lock` → None; живой PID в `.lock` → Some с ненулевой RAM;
    /// мёртвый PID в `.lock` → None; некорректный id → None без обращения
    /// к файловой системе (A1).
    #[test]
    fn sample_for_instance_lock_semantics() {
        let (_d, paths) = test_paths();
        let inst = crate::instances::Instance::new("x", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let dir = crate::instances::instance_dir(&paths, &inst.id);

        // Не запущен.
        assert!(sample_for_instance(&paths, &inst.id).is_none(), "без .lock → None");

        // «Живой» процесс — сам тест: семпл обязателен, RAM своего процесса > 0.
        std::fs::write(dir.join(".lock"), std::process::id().to_string()).unwrap();
        let s = sample_for_instance(&paths, &inst.id).expect("свой живой PID даёт семпл");
        assert!(s.ram_bytes > 0, "RAM своего процесса больше нуля");
        assert!(s.cpu_percent >= 0.0, "CPU не бывает отрицательным");

        // Протухший lock: PID мёртв.
        std::fs::write(dir.join(".lock"), u32::MAX.to_string()).unwrap();
        assert!(sample_for_instance(&paths, &inst.id).is_none(), "мёртвый PID → None");

        // Path traversal по id отсекается до чтения файлов.
        assert!(sample_for_instance(&paths, "../evil").is_none(), "плохой id → None");
    }
}
