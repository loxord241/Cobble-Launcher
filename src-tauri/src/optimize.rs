//! Оптимизация инстанса (спека §6.9): стек производительности через Modrinth,
//! RAM-гайды. Версии НЕ хардкодятся — резолвятся по совместимости.

use crate::errors::{LauncherError, Result};
use crate::instances::Instance;
use crate::net::download::DownloadEngine;
use crate::net::http::HttpClient;
use crate::paths::Paths;
use serde::Serialize;
use std::sync::Arc;

/// Стек оптимизации: конфигурируемый JSON в коде (спека §6.9), версии решает
/// резолвер совместимости. OptiFine сознательно отсутствует (конфликт с Sodium).
const OPTIMIZE_STACK: &str = r#"[
  { "id": "AANobbMI", "why": "sodium" },
  { "id": "gvQqBUqZ", "why": "lithium" },
  { "id": "uXXizFIs", "why": "ferrite-core" },
  { "id": "NNAgCjsB", "why": "entityculling" },
  { "id": "5ZwdcRci", "why": "immediatelyfast" },
  { "id": "nmDcB62a", "why": "modernfix" },
  { "id": "LQ3K71Q1", "why": "dynamic-fps" },
  { "id": "P7dR8mSH", "why": "fabric-api" }
]"#;

#[derive(serde::Deserialize)]
struct StackEntry {
    id: String,
    /// Человекочитаемое имя (в логи установки).
    #[serde(default)]
    #[allow(dead_code)]
    why: String,
}

/// Кнопка «Оптимизировать»: ставит стек в FABRIC-инстанс. Для не-fabric —
/// честная ошибка (спека §6.9: OptiFine не устанавливать, ноль инжекций).
pub async fn optimize(
    paths: &Paths,
    client: Arc<HttpClient>,
    engine: Arc<DownloadEngine>,
    inst: &Instance,
) -> Result<Vec<String>> {
    if inst.loader.as_deref() != Some("fabric") {
        return Err(LauncherError::InvalidInput(
            "стек оптимизации ставится в Fabric-инстансы (создайте инстанс с Fabric)".into(),
        ));
    }
    let stack: Vec<StackEntry> = serde_json::from_str(OPTIMIZE_STACK)?;
    let mut installed = Vec::new();
    for entry in &stack {
        match crate::modrinth::install::install_project(
            paths,
            client.clone(),
            engine.clone(),
            inst,
            &entry.id,
            None,
        )
        .await
        {
            Ok(files) => installed.extend(files.into_iter().map(|f| f.file)),
            // Нет совместимой версии для этой пары (mc, loader) — честно
            // пропускаем (спека §6.9: версии НЕ хардкодятся, стек подстраивается).
            Err(LauncherError::VersionNotFound(reason)) => {
                tracing::info!("оптимизация: {} пропущен ({reason})", entry.why);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(installed)
}

/// RAM-гайды (спека §6.9): рекомендация min(6G, systemRAM/4),
/// предупреждение при >8G.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RamGuide {
    pub total_mb: u64,
    pub recommended_mb: u64,
    /// Непусто, если recommended > 8192 — «длинные GC-паузы».
    pub warning: Option<String>,
}

pub fn ram_guide() -> RamGuide {
    let total = sysinfo::System::new_all()
        .total_memory()
        .saturating_mul(1024)
        .saturating_div(1024 * 1024); // байты → МБ
    let quarter = total / 4;
    let recommended = quarter.clamp(1024, 6144);
    let warning = if recommended > 8192 {
        Some(
            "Больше 8 ГБ — длинные GC-паузы. Ставьте столько, сколько нужно конкретному модпаку."
                .into(),
        )
    } else {
        None
    };
    RamGuide {
        total_mb: total,
        recommended_mb: recommended,
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_guide_bounds() {
        let g = ram_guide();
        assert!(g.recommended_mb >= 1024 && g.recommended_mb <= 6144);
    }

    #[test]
    fn stack_json_parses() {
        let stack: Vec<StackEntry> = serde_json::from_str(OPTIMIZE_STACK).unwrap();
        assert_eq!(stack.len(), 8);
        assert!(stack.iter().any(|e| e.why == "sodium"));
    }
}
