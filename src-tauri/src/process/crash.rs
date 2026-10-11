//! Краш-анализ (спека §6.10): расширяемые правила → человеческая подсказка.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub rule_id: String,
    pub title: String,
    pub advice: String,
}

/// Правила (расширяемый JSON в коде, спека §6.10).
const RULES: &str = r#"[
  {
    "id": "java_too_old",
    "patterns": ["UnsupportedClassVersionError", "has been compiled by a more recent version"],
    "title": "Java ниже требуемой",
    "advice": "Установите нужную Java: Настройки → Java → Установить (для 1.20.5+ нужна Java 21, для 1.18–1.20.4 — 17)."
  },
  {
    "id": "gpu_driver",
    "patterns": ["GLX error", "Pixel format not accelerated", "Failed to create EGL", "dxdiag"],
    "title": "Проблема с GPU-драйвером",
    "advice": "Обновите драйвер видеокарты (NVIDIA/AMD/Intel) до последней версии и перезапустите игру."
  },
  {
    "id": "out_of_memory",
    "patterns": ["OutOfMemoryError", "java.lang.OutOfMemory", "_NettyBufferAllocator"],
    "title": "Не хватило памяти",
    "advice": "Увеличьте RAM в настройках инстанса (например, 4096 МБ). Больше 8 ГБ ставить не стоит — длинные GC-паузы."
  },
  {
    "id": "duplicate_mod",
    "patterns": ["DuplicateModsFoundException", "Duplicate key", "Mod file has duplicate mod id"],
    "title": "Дубликат мода",
    "advice": "В папке mods лежат два jar с одним модом. Удалите лишнюю версию (смотрите имя файла в ошибке)."
  },
  {
    "id": "missing_fabric_api",
    "patterns": ["ModResolutionException: Could not find required mod", "fabric-api", "Unmet dependency listing"],
    "title": "Не хватает зависимости (обычно Fabric API)",
    "advice": "Установите Fabric API: страница «Моды» → поиск fabric-api → Установить в инстанс."
  },
  {
    "id": "antivirus_natives",
    "patterns": ["Failed to unpack natives", "lwjgl.dll: Access is denied", "Can't load library"],
    "title": "Антивирус удалил нативы",
    "advice": "Добавьте каталог данных лаунчера в исключения Windows Defender (Безопасность Windows → Защита от вирусов → Исключения) и запустите игру заново — файлы скачаются."
  }
]"#;

#[derive(serde::Deserialize)]
struct Rule {
    id: String,
    patterns: Vec<String>,
    title: String,
    advice: String,
}

fn rules() -> Vec<Rule> {
    serde_json::from_str(RULES).expect("встроенные правила краш-анализа валидны")
}

/// Проанализировать хвост лога (последние N строк достаточно — стек в конце).
pub fn analyze(log_text: &str) -> Vec<Diagnosis> {
    let tail: String = log_text
        .lines()
        .rev()
        .take(400)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
    for rule in rules() {
        if rule.patterns.iter().any(|p| tail.contains(p.as_str())) {
            out.push(Diagnosis {
                rule_id: rule.id,
                title: rule.title,
                advice: rule.advice,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_too_old_detected() {
        let log = "Exception: UnsupportedClassVersionError: class has been compiled by a more recent version (61.0)";
        let d = analyze(log);
        assert!(d.iter().any(|x| x.rule_id == "java_too_old"));
    }

    #[test]
    fn out_of_memory_detected() {
        let d = analyze("java.lang.OutOfMemoryError: Java heap space");
        assert!(d.iter().any(|x| x.rule_id == "out_of_memory"));
    }

    #[test]
    fn duplicate_mod_detected() {
        let d = analyze("org.quiltmc.loader.api.QuiltLoaderError: DuplicateModsFoundException");
        assert!(d.iter().any(|x| x.rule_id == "duplicate_mod"));
    }

    #[test]
    fn clean_log_no_diagnosis() {
        assert!(analyze("Sound engine started\nSetting user: Steve").is_empty());
    }

    #[test]
    fn multiple_rules() {
        let log = "OutOfMemoryError happened after GLX error";
        let d = analyze(log);
        assert_eq!(d.len(), 2);
    }
}
