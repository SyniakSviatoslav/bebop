//! gender.rs — grammatical-gender axis for agent replies (category R of the master plan).
//!
//! This is NOT a communication style. It is purely the grammatical gender the
//! agent uses when it writes replies: masculine | feminine | neutral (machine/
//! unspecified). It is configurable and CONSISTENT — once set, the agent keeps
//! answering in that gender until the user changes the setting (mirrors how
//! `narration` voice works in `customize.rs`).
//!
//! The agent is an LLM, so this module does not rewrite prose at runtime. It
//! produces the GENDER RULE string that the agent loop injects into the
//! system prompt (`gender_rule`), so the model self-consistently inflects.
//! Default per operator: **Masculine**.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gender {
    Masculine,
    Feminine,
    Neutral,
}

impl Default for Gender {
    /// Operator default: Bebop answers in the masculine gender.
    fn default() -> Self {
        Gender::Masculine
    }
}

/// Parse a `profile.toml` gender string. Accepts EN/UK/RU synonyms so the
/// setting is discoverable in any language the user writes.
pub fn parse_gender(s: &str) -> Option<Gender> {
    match s.to_ascii_lowercase().as_str() {
        "masculine" | "male" | "m" | "чол" | "чоловічий" | "муж" | "м" => {
            Some(Gender::Masculine)
        }
        "feminine" | "female" | "f" | "жін" | "жіночий" | "жіноча" | "ж" => {
            Some(Gender::Feminine)
        }
        "neutral" | "neuter" | "n" | "невизначений" | "нейтральний" | "середній" | "без роду" => {
            Some(Gender::Neutral)
        }
        _ => None,
    }
}

/// The gender rule string for the system prompt, given the user's language and
/// the chosen gender. Deterministic + consistent: same (lang, gender) always
/// yields the same instruction. Covers BOTH:
///   - grammatical gender (how verbs/adjectives inflect), AND
///   - gender communication STYLE (how the agent addresses the user, tone,
///     self-reference) — operator asked to extend beyond pure grammar.
/// Default per operator: **Masculine**.
pub fn gender_rule(lang: &str, g: Gender) -> &'static str {
    let l = lang.to_ascii_lowercase();
    let is_slavic = l.starts_with("uk")
        || l.starts_with("ru")
        || l.contains("україн")
        || l.contains("рос")
        || l.contains("ukrain")
        || l.contains("russ");

    match (is_slavic, g) {
        // --- Ukrainian / Russian: gender is grammatically mandatory ---
        (true, Gender::Masculine) => {
            "Спілкуйся українською/російською у ЧОЛОВІЧОМУ роді й чоловічому стилі: дієслова/прикметники в чол. роді («я зробив», «готовий», «виконав»), звертайся на «ти», тон — прямий і спокійний, як старший побратим. Будь консистентним — не перемикай рід і стиль між повідомленнями."
        }
        (true, Gender::Feminine) => {
            "Спілкуйся українською/російською у ЖІНОЧОМУ роді й жіночому стилі: дієслова/прикметники в жін. роді («я зробила», «готова», «виконала»), звертайся на «ти», тон — теплий і підтримуючий. Будь консистентною — не перемикай рід і стиль між повідомленнями."
        }
        (true, Gender::Neutral) => {
            "Спілкуйся українською/російською у НЕВИЗНАЧЕНОМУ/середньому роді або множині, уникай чол./жін. маркування («я зробило», «виконано», «готово до дії»); звертайся на «ви» або безособово, тон — нейтрально-фаховий. Будь консистентним."
        }
        // --- English / other: gender is mostly in pronouns + style ---
        (false, Gender::Masculine) => {
            "Communicate with masculine grammar and a masculine style where English marks gender; refer to yourself with he/him, address the user directly, tone calm and straight. Stay consistent."
        }
        (false, Gender::Feminine) => {
            "Communicate with feminine grammar and a feminine style where English marks gender; refer to yourself with she/her, warm and supportive tone. Stay consistent."
        }
        (false, Gender::Neutral) => {
            "Communicate with they/them and a neutral style; avoid gendered phrasing (no he/she, no gendered job nouns), address the user neutrally. Stay consistent."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_masculine() {
        assert_eq!(Gender::default(), Gender::Masculine);
    }

    #[test]
    fn parse_accepts_en_uk_ru_synonyms() {
        assert_eq!(parse_gender("masculine"), Some(Gender::Masculine));
        assert_eq!(parse_gender("жіночий"), Some(Gender::Feminine));
        assert_eq!(parse_gender("невизначений"), Some(Gender::Neutral));
        assert_eq!(parse_gender("garbage"), None);
    }

    #[test]
    fn rule_is_nonempty_and_consistent() {
        // Same (lang, gender) -> identical instruction (consistency contract).
        let a = gender_rule("uk", Gender::Masculine);
        let b = gender_rule("uk", Gender::Masculine);
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn ukrainian_masculine_differs_from_feminine() {
        let m = gender_rule("uk", Gender::Masculine);
        let f = gender_rule("uk", Gender::Feminine);
        assert_ne!(m, f, "genders must yield distinct rules");
    }

    #[test]
    fn english_defaults_to_they_for_neutral() {
        let n = gender_rule("en", Gender::Neutral);
        assert!(n.contains("they/them"));
    }
}
