//! Мониторинг ресурсов запущенной игры (F28): CPU/RAM по PID через sysinfo.
//! Обновляем ТОЛЬКО запрошенный PID — остальные процессы не открываем
//! (инвариант D16, стиль `pid_alive` в instances/mod.rs).

/// Семпл ресурсов игры для UI: доля CPU (%) и резидентная память (байты).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameResourceSample {
    pub cpu_percent: f32,
    pub ram_bytes: u64,
}

/// Снять семпл ресурсов процесса. None — процесса с таким PID нет (игра
/// уже завершилась). CPU в sysinfo считается по дельте между двумя refresh,
/// поэтому первый снимок — база, затем пауза MINIMUM_CPU_UPDATE_INTERVAL
/// (~200 мс, рекомендованный докой минимум) и второй снимок. Пауза в функции
/// приемлема: UI опрашивает раз в 2 секунды, и это блокирующий вызов фоновой
/// задачи, не UI-потока.
pub fn sample(pid: u32) -> Option<GameResourceSample> {
    let target = sysinfo::Pid::from_u32(pid);
    // Только память и CPU: не открываем exe/cmd/tasks/disk_usage процессов.
    let kinds = sysinfo::ProcessRefreshKind::nothing().with_memory().with_cpu();
    let mut sys = sysinfo::System::new();
    // Первый refresh — база для дельты CPU.
    sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[target]), true, kinds);
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    // Второй refresh — cpu_usage получает осмысленное значение.
    sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[target]), true, kinds);
    let process = sys.process(target)?;
    Some(GameResourceSample {
        cpu_percent: process.cpu_usage(),
        ram_bytes: process.memory(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F28: поля сериализуются в camelCase — contract с client.ts
    /// (GameResourceSample {cpuPercent, ramBytes}).
    #[test]
    fn serializes_camel_case() {
        let s = GameResourceSample {
            cpu_percent: 12.5,
            ram_bytes: 1024,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"cpuPercent\":12.5"), "camelCase cpu: {json}");
        assert!(json.contains("\"ramBytes\":1024"), "camelCase ram: {json}");
        assert!(!json.contains("cpu_percent"), "без snake_case: {json}");
    }

    /// F28: несуществующий PID (u32::MAX) — None, без паники
    /// (тот же инвариант, что в pid_alive-тесте instances).
    #[test]
    fn sample_dead_pid_is_none() {
        assert!(sample(u32::MAX).is_none(), "мёртвый PID → None");
    }

    /// F28: семпл собственного процесса не паникует. Строго Some не ассертим
    /// (теоретически процесс может завершиться между lock-чтением и семплом).
    #[test]
    fn sample_own_pid_does_not_panic() {
        let _ = sample(std::process::id());
    }
}
