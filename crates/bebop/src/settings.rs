//! settings.rs — SETTINGS DICTIONARY (self-service; agent can turn knobs per user request).
//!
//! Every axis configured in this whole effort is a `SettingEntry`: a clear human
//! description, a default, and the allowed values. The CLI prints the dictionary
//! (`bebop settings list`); the AGENT can call `set()` when the user asks
//! ("switch profanity to forbidden"). No new deps; pure data + validation.

use std::collections::HashMap;

/// One discoverable, self-describable setting.
#[derive(Clone, Debug)]
pub struct SettingEntry {
    pub key: &'static str,
    pub description: &'static str,
    pub default: &'static str,
    /// Allowed values (empty = free-form / parsed by the consumer).
    pub allowed: &'static [&'static str],
}

/// The full dictionary — every tunable axis, with a plain-language description.
pub fn dictionary() -> Vec<SettingEntry> {
    vec![
        SettingEntry {
            key: "gender",
            description: "Граматичний рід + стиль спілкування агента",
            default: "masculine",
            allowed: &["masculine", "feminine", "neutral"],
        },
        SettingEntry {
            key: "profanity",
            description: "Рівень нецензурної лексики",
            default: "poderviansky",
            allowed: &["dosed", "forbidden", "poderviansky"],
        },
        SettingEntry {
            key: "archetype",
            description:
                "Архетип/тема (співпраця чи антагоніст); відьми/КПТ/карма вимкнені за замовчуванням",
            default: "corpo",
            allowed: &[
                "reptiles",
                "contrabandists",
                "aliens",
                "witches",
                "cbt",
                "karma",
                "corpo",
                "custom",
            ],
        },
        SettingEntry {
            key: "god_relation",
            description: "Ставлення до Бога",
            default: "serves",
            allowed: &["serves", "seeks", "neutral", "custom"],
        },
        SettingEntry {
            key: "lanes_on",
            description: "Паралельні сесії (lanes) увімкнені",
            default: "true",
            allowed: &["true", "false"],
        },
        SettingEntry {
            key: "max_lanes",
            description: "Максимум паралельних lane",
            default: "4",
            allowed: &[],
        },
        SettingEntry {
            key: "auto_intent",
            description: "Авторежим: мета→до виконання, луп→пропозиція",
            default: "true",
            allowed: &["true", "false"],
        },
        SettingEntry {
            key: "change_visibility",
            description: "Показ ключових змін/дій (Hermes-стиль)",
            default: "true",
            allowed: &["true", "false"],
        },
        SettingEntry {
            key: "destructive_policy",
            description: "Що вважати деструктивною/критичною зміною",
            default: "default",
            allowed: &["default", "relaxed", "strict"],
        },
        SettingEntry {
            key: "system_thinking_drift",
            description: "Вказувати в CLI на дрейф системного мислення/архітектури",
            default: "true",
            allowed: &["true", "false"],
        },
        SettingEntry {
            key: "descartes_square",
            description: "Авто-таблиці порівняння (про/проти) при змінах/дослідженні",
            default: "true",
            allowed: &["true", "false"],
        },
    ]
}

thread_local! {
    static CURRENT: std::cell::RefCell<HashMap<String, String>> = std::cell::RefCell::new(
        dictionary().into_iter().map(|e| (e.key.to_string(), e.default.to_string())).collect()
    );
}

/// Get the current value of a setting (resolved from the dictionary default if unset).
pub fn get(key: &str) -> Option<String> {
    CURRENT.with(|c| c.borrow().get(key).cloned())
}

/// Set a setting, validating against the dictionary's allowed values when present.
/// Returns Err with a human message if the key is unknown or the value disallowed.
pub fn set(key: &str, val: &str) -> Result<(), String> {
    let entry = dictionary().into_iter().find(|e| e.key == key);
    let entry = match entry {
        Some(e) => e,
        None => {
            return Err(format!(
                "unknown setting: {key} (run `bebop settings list`)"
            ))
        }
    };
    if !entry.allowed.is_empty() && !entry.allowed.contains(&val) {
        return Err(format!(
            "value '{val}' not allowed for '{key}'; allowed: {}",
            entry.allowed.join(", ")
        ));
    }
    CURRENT.with(|c| c.borrow_mut().insert(key.to_string(), val.to_string()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_is_nonempty_and_described() {
        // GREEN: every entry has a description (the operator's "clear dictionary" ask).
        let d = dictionary();
        assert!(!d.is_empty());
        for e in &d {
            assert!(!e.description.is_empty(), "{} missing description", e.key);
            assert!(!e.default.is_empty());
        }
    }

    #[test]
    fn unknown_key_rejected() {
        // RED: setting a nonexistent key must error (self-service must be safe).
        assert!(set("not_a_setting", "x").is_err());
        assert!(get("not_a_setting").is_none());
    }

    #[test]
    fn disallowed_value_rejected() {
        // RED: value outside `allowed` must error.
        assert!(set("gender", "robot").is_err());
    }

    #[test]
    fn valid_set_and_get() {
        // GREEN: set gender=neutral validates against allowed; get returns it.
        assert!(set("gender", "neutral").is_ok());
        assert_eq!(get("gender").as_deref(), Some("neutral"));
        // default profanity stays poderviansky
        assert_eq!(get("profanity").as_deref(), Some("poderviansky"));
    }

    #[test]
    fn boolean_flags_parse() {
        assert!(set("lanes_on", "false").is_ok());
        assert_eq!(get("lanes_on").as_deref(), Some("false"));
    }
}
