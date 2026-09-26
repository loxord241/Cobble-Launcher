//! Движок rules версий Mojang (спека §6.1: «стартуем allow=false,
//! побеждает ПОСЛЕДНЕЕ подходящее правило»; arch `x86` — это 32-бит).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsName {
    Windows,
    Linux,
    Osx,
}

impl OsName {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            OsName::Windows
        } else if cfg!(target_os = "macos") {
            OsName::Osx
        } else {
            OsName::Linux
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            OsName::Windows => "windows",
            OsName::Linux => "linux",
            OsName::Osx => "osx",
        }
    }
}

/// Контекст ОС для матчинга правил. arch_bits — битность JVM (32/64).
#[derive(Debug, Clone, Copy)]
pub struct OsContext {
    pub name: OsName,
    pub arch_bits: u32,
    pub is_arm: bool,
}

impl OsContext {
    pub fn linux() -> Self {
        Self {
            name: OsName::Linux,
            arch_bits: 64,
            is_arm: false,
        }
    }

    pub fn current(arch_bits: u32, is_arm: bool) -> Self {
        Self {
            name: OsName::current(),
            arch_bits,
            is_arm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    // Старые версии Mojang использовали `disallow` вместо `deny` (спека —
    // реальность важнее: фикстура 1.12.2 содержит именно `disallow`).
    #[serde(alias = "disallow")]
    Deny,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<BTreeMap<String, bool>>,
}

impl Rule {
    pub fn matches(&self, os: &OsContext, features: &BTreeMap<String, bool>) -> bool {
        if let Some(o) = &self.os {
            if let Some(name) = &o.name {
                if !name.eq_ignore_ascii_case(os.name.as_str()) {
                    return false;
                }
            }
            if let Some(arch) = &o.arch {
                // Спека §6.1: `x86` — про 32-бит, на x64 НЕ матчится.
                let arch_ok = match arch.as_str() {
                    "x86" => os.arch_bits == 32,
                    "arm" => os.is_arm,
                    _ => false,
                };
                if !arch_ok {
                    return false;
                }
            }
        }
        if let Some(req) = &self.features {
            for (k, v) in req {
                if features.get(k) != Some(v) {
                    return false;
                }
            }
        }
        true
    }
}

/// Пустой список правил → false (элемент не разрешён). Непустой → стартуем
/// allow=false и применяем ПОСЛЕДНЕЕ подходящее правило (спека §6.1).
pub fn evaluate(rules: &[Rule], os: &OsContext, features: &BTreeMap<String, bool>) -> bool {
    let mut allowed = false;
    for r in rules {
        if r.matches(os, features) {
            allowed = matches!(r.action, RuleAction::Allow);
        }
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn os_win64() -> OsContext {
        OsContext {
            name: OsName::Windows,
            arch_bits: 64,
            is_arm: false,
        }
    }

    fn os_win32() -> OsContext {
        OsContext {
            name: OsName::Windows,
            arch_bits: 32,
            is_arm: false,
        }
    }

    fn parse(v: serde_json::Value) -> Vec<Rule> {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn empty_rules_disallow() {
        assert!(!evaluate(&[], &os_win64(), &BTreeMap::new()));
    }

    #[test]
    fn allow_windows_rule_matches() {
        let rules = parse(json!([
            {"action": "allow", "os": {"name": "windows"}}
        ]));
        assert!(evaluate(&rules, &os_win64(), &BTreeMap::new()));
        assert!(!evaluate(&rules, &OsContext::linux(), &BTreeMap::new()));
    }

    #[test]
    fn last_matching_rule_wins() {
        let rules = parse(json!([
            {"action": "allow", "os": {"name": "windows"}},
            {"action": "deny", "os": {"name": "windows"}}
        ]));
        assert!(!evaluate(&rules, &os_win64(), &BTreeMap::new()));

        let rules2 = parse(json!([
            {"action": "deny", "os": {"name": "windows"}},
            {"action": "allow", "os": {"name": "windows"}}
        ]));
        assert!(evaluate(&rules2, &os_win64(), &BTreeMap::new()));
    }

    #[test]
    fn non_matching_rule_does_not_override() {
        let rules = parse(json!([
            {"action": "allow", "os": {"name": "windows"}},
            {"action": "deny", "os": {"name": "linux"}}
        ]));
        assert!(evaluate(&rules, &os_win64(), &BTreeMap::new()));
    }

    #[test]
    fn arch_x86_does_not_match_64bit() {
        let rules = parse(json!([
            {"action": "allow", "os": {"arch": "x86"}}
        ]));
        assert!(!evaluate(&rules, &os_win64(), &BTreeMap::new()));
        assert!(evaluate(&rules, &os_win32(), &BTreeMap::new()));
    }

    #[test]
    fn features_must_all_match() {
        let rules = parse(json!([
            {"action": "allow", "features": {"is_demo_user": true, "has_custom_resolution": true}}
        ]));
        let mut f = BTreeMap::new();
        f.insert("is_demo_user".to_string(), true);
        f.insert("has_custom_resolution".to_string(), true);
        assert!(evaluate(&rules, &os_win64(), &f));
        f.insert("is_demo_user".to_string(), false);
        assert!(!evaluate(&rules, &os_win64(), &f));
    }

    #[test]
    fn feature_rule_without_feature_fails() {
        let rules = parse(json!([
            {"action": "allow", "features": {"is_demo_user": true}}
        ]));
        assert!(!evaluate(&rules, &os_win64(), &BTreeMap::new()));
    }

    #[test]
    fn os_name_is_case_insensitive() {
        let rules = parse(json!([
            {"action": "allow", "os": {"name": "Windows"}}
        ]));
        assert!(evaluate(&rules, &os_win64(), &BTreeMap::new()));
    }
}
