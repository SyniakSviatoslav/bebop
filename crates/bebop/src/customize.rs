//! Bebop customization — the three axes that make the ship YOURS.
//!
//! Per Dowiz brand + Hermes `SOUL.md`/skins precedent, Bebop exposes:
//!   1. looks     — palette override (must keep WCAG-safe pairings)
//!   2. narration — voice axis (bebop | plain | sarcastic | corporate-killer)
//!   3. patrons   — sponsor line (home:), lets a fork re-skin without forking core
//!
//! All three are stored in `~/.bebop/profile.toml` (native, no JS). `bebop init`
//! sets them; `bebop outfit` shows the resulting contract. This is the
//! "make it yours" hook that Claude/OpenCode/Hermes lack as first-class.

use crate::gender::{parse_gender, Gender};
use crate::outfit::{Narration, Outfit, OUTFIT};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Profile {
    pub looks: Option<LooksOverride>,
    pub narration: Option<String>,
    /// Grammatical-gender + gender-communication style axis (category R).
    /// `None` -> operator default Masculine (see `gender::Gender::default`).
    pub gender: Option<String>,
    pub patrons: Option<PatronsOverride>,
    /// Operating mode (category B): plan | build | auto. `None` -> Auto.
    pub mode: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LooksOverride {
    /// Accent override (hex, no `#`). If set, becomes the single saturated accent.
    pub accent: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PatronsOverride {
    /// Sponsor/home line shown in the banner.
    pub home: Option<String>,
}

impl Profile {
    /// Load the profile from `~/.bebop/profile.toml`, or default (empty) if absent.
    pub fn load() -> Self {
        let p = profile_path();
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Profile::default(),
        }
    }

    /// Persist to `~/.bebop/profile.toml`, creating the dir if needed.
    pub fn save(&self) -> std::io::Result<()> {
        let p = profile_path();
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&p, toml::to_string_pretty(self).unwrap())
    }

    /// Resolve the effective outfit: OUTFIT overlaid with this profile's axes.
    pub fn resolve_outfit(&self) -> Outfit {
        let mut o = OUTFIT;
        if let Some(l) = &self.looks {
            if let Some(accent) = &l.accent {
                if let Some(hex) = parse_hex(accent) {
                    // override the single saturated accent (ship) — the launch
                    // + status use it. Keeps the brand's "one color per view" law.
                    o.palette.ship = hex;
                }
            }
        }
        if let Some(n) = &self.narration {
            o.narration = parse_narration(n).unwrap_or(o.narration);
        }
        if let Some(p) = &self.patrons {
            if let Some(home) = &p.home {
                o.home = Box::leak(home.clone().into_boxed_str());
            }
        }
        o
    }

    /// Resolve the effective gender axis, defaulting to Masculine (operator).
    pub fn resolve_gender(&self) -> Gender {
        match &self.gender {
            Some(g) => parse_gender(g).unwrap_or_default(),
            None => Gender::default(),
        }
    }

    /// Resolve the effective mode, defaulting to Auto (operator autopilot).
    pub fn resolve_mode(&self) -> Mode {
        match &self.mode {
            Some(m) => parse_mode(m).unwrap_or_default(),
            None => Mode::default(),
        }
    }
}

/// Agent operating mode (category B of the master plan).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Mode {
    /// Plan only — propose, never execute destructive/red-line actions.
    Plan,
    /// Build — execute, but ask before red-line / destructive ops.
    Build,
    /// Auto — full autopilot; operator pre-authorized (default for this fork).
    #[default]
    Auto,
}

pub fn parse_mode(s: &str) -> Option<Mode> {
    match s.to_ascii_lowercase().as_str() {
        "plan" => Some(Mode::Plan),
        "build" => Some(Mode::Build),
        "auto" => Some(Mode::Auto),
        _ => None,
    }
}

impl Mode {
    /// Whether this mode may execute without a per-step confirmation.
    pub fn autonomous(&self) -> bool {
        matches!(self, Mode::Auto)
    }
    /// Whether destructive / red-line ops require an explicit human gate.
    pub fn needs_red_line_gate(&self) -> bool {
        !matches!(self, Mode::Auto)
    }
}

pub fn profile_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".bebop").join("profile.toml")
}

/// Parse a `#RRGGBB` or `RRGGBB` hex string into a u32.
pub fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

pub fn parse_narration(s: &str) -> Option<Narration> {
    match s.to_ascii_lowercase().as_str() {
        "bebop" => Some(Narration::Bebop),
        "plain" => Some(Narration::Plain),
        "sarcastic" => Some(Narration::Sarcastic),
        "corporate-killer" => Some(Narration::CorporateKiller),
        _ => None,
    }
}

/// The voice line for a given narration axis + situation.
pub fn voice_line(n: Narration, situation: &str) -> String {
    match (n, situation) {
        (Narration::Bebop, "boot") => "Bebop online. The ship is yours.".into(),
        (Narration::Bebop, "dispatch") => {
            "Multipilot engaged — the crew's arguing, I'm deciding.".into()
        }
        (Narration::Plain, _) => "Bebop ready.".into(),
        (Narration::Sarcastic, "boot") => "Booted. Try not to crash the ship this time.".into(),
        (Narration::Sarcastic, "dispatch") => "Ugh, fine, I'll herd the pilots. Again.".into(),
        (Narration::CorporateKiller, "boot") => {
            "Agent initialized. Delivering autonomous value.".into()
        }
        (Narration::CorporateKiller, "dispatch") => {
            "Fanning out to N specialists for synergistic convergence.".into()
        }
        _ => "Bebop online.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_ok() {
        assert_eq!(parse_hex("#E0543E"), Some(0xE0543E));
        assert_eq!(parse_hex("46B0A4"), Some(0x46B0A4));
    }

    #[test]
    fn parse_hex_rejects_garbage() {
        // RED: non-hex must NOT silently become a color.
        assert_eq!(parse_hex("xyz"), None);
        assert_eq!(parse_hex("12345"), None); // 5 digits
    }

    #[test]
    fn profile_resolves_accent_override() {
        // GREEN: an accent override changes the ship (launch) color, nothing else
        // goes off-brand.
        let mut p = Profile::default();
        p.looks = Some(LooksOverride {
            accent: Some("FF0000".into()),
        });
        let o = p.resolve_outfit();
        assert_eq!(o.palette.ship, 0xFF0000);
        // tele/void/bone stay canonical
        assert_eq!(o.palette.tele, OUTFIT.palette.tele);
        assert_eq!(o.palette.void, OUTFIT.palette.void);
    }

    #[test]
    fn narration_axis_maps() {
        assert_eq!(parse_narration("sarcastic"), Some(Narration::Sarcastic));
        assert_eq!(parse_narration("nonsense"), None);
        let p = Profile {
            narration: Some("plain".into()),
            ..Default::default()
        };
        assert_eq!(p.resolve_outfit().narration, Narration::Plain);
    }

    #[test]
    fn voice_line_varies_by_axis() {
        // RED+GREEN: the same situation yields distinct voices per axis.
        let a = voice_line(Narration::Bebop, "boot");
        let b = voice_line(Narration::Sarcastic, "boot");
        assert_ne!(a, b, "voice axes collapsed to one");
    }

    #[test]
    fn toml_roundtrip() {
        // GREEN: a profile survives save→load (so customization persists).
        let dir = std::env::temp_dir().join("bebop-profile-test");
        std::fs::create_dir_all(&dir).ok();
        let p = dir.join("profile.toml");
        std::env::set_var("HOME", &dir);
        let mut prof = Profile::default();
        prof.narration = Some("sarcastic".into());
        prof.save().unwrap();
        let loaded = Profile::load();
        assert_eq!(loaded.narration, Some("sarcastic".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gender_defaults_masculine() {
        // GREEN (operator default): unset gender -> Masculine.
        let p = Profile::default();
        assert_eq!(p.resolve_gender(), crate::gender::Gender::Masculine);
        // explicit override wins + round-trips
        let mut p2 = Profile::default();
        p2.gender = Some("жіночий".into());
        assert_eq!(p2.resolve_gender(), crate::gender::Gender::Feminine);
    }

    #[test]
    fn mode_defaults_auto_and_parses() {
        // GREEN (operator autopilot default): unset mode -> Auto; plan/build parse.
        let p = Profile::default();
        assert_eq!(p.resolve_mode(), Mode::Auto);
        assert!(p.resolve_mode().autonomous());
        assert!(!p.resolve_mode().needs_red_line_gate());

        let mut plan = Profile::default();
        plan.mode = Some("plan".into());
        assert_eq!(plan.resolve_mode(), Mode::Plan);
        assert!(!plan.resolve_mode().autonomous());
        assert!(plan.resolve_mode().needs_red_line_gate());

        let mut build = Profile::default();
        build.mode = Some("build".into());
        assert_eq!(build.resolve_mode(), Mode::Build);
        assert!(!build.resolve_mode().autonomous());

        assert_eq!(parse_mode("garbage"), None);
    }
}
