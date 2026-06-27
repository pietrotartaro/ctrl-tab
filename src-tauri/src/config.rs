//! Shortcut configuration: combo model, validation, label formatting, and
//! (de)serialization. The pure parts (validate, format_combo_label, serde
//! round-trip) are unit-tested; load/save to disk is acceptance-gated.

use std::path::Path;

use serde::{Deserialize, Serialize};

// CGEventFlags modifier bits.
pub const MOD_CONTROL: u64 = 0x0004_0000;
pub const MOD_SHIFT: u64 = 0x0002_0000;
pub const MOD_OPTION: u64 = 0x0008_0000;
pub const MOD_COMMAND: u64 = 0x0010_0000;
pub const MODS_ALL: u64 = MOD_CONTROL | MOD_SHIFT | MOD_OPTION | MOD_COMMAND;

/// A configurable shortcut: the required modifier flags + the key code, plus a
/// human-readable label (e.g. "⌃Tab").
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Combo {
    pub modifiers: u64,
    pub key_code: i64,
    pub label: String,
}

impl Combo {
    pub fn new(modifiers: u64, key_code: i64) -> Self {
        Self {
            modifiers,
            key_code,
            label: format_combo_label(modifiers, key_code),
        }
    }
}

/// The two configurable actions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub switch_app: Combo,
    pub switch_windows: Combo,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            switch_app: Combo::new(MOD_CONTROL, 48),    // ⌃Tab
            switch_windows: Combo::new(MOD_CONTROL, 10), // ⌃§ (ISO section)
        }
    }
}

/// Load config from `path`, falling back to default on any error.
pub fn load_from(path: &Path) -> Config {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist config to `path` (creating parent dirs).
pub fn save_to(path: &Path, cfg: &Config) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(cfg).expect("serialize config");
    std::fs::write(path, json)
}

/// True if `kc` is itself a modifier key (so it can't be the combo's main key).
pub fn is_modifier_keycode(kc: i64) -> bool {
    matches!(kc, 54 | 55 | 56 | 57 | 58 | 59 | 60 | 61 | 62 | 63)
}

/// Human name for a macOS virtual key code (common keys; falls back to `key{n}`).
pub fn key_name(kc: i64) -> String {
    let s = match kc {
        0 => "A", 1 => "S", 2 => "D", 3 => "F", 4 => "H", 5 => "G", 6 => "Z",
        7 => "X", 8 => "C", 9 => "V", 11 => "B", 12 => "Q", 13 => "W", 14 => "E",
        15 => "R", 16 => "Y", 17 => "T", 18 => "1", 19 => "2", 20 => "3", 21 => "4",
        22 => "6", 23 => "5", 24 => "=", 25 => "9", 26 => "7", 27 => "-", 28 => "8",
        29 => "0", 30 => "]", 31 => "O", 32 => "U", 33 => "[", 34 => "I", 35 => "P",
        36 => "Return", 37 => "L", 38 => "J", 39 => "'", 40 => "K", 41 => ";",
        42 => "\\", 43 => ",", 44 => "/", 45 => "N", 46 => "M", 47 => ".",
        48 => "Tab", 49 => "Space", 50 => "`", 51 => "Delete", 53 => "Esc",
        10 => "§", 123 => "←", 124 => "→", 125 => "↓", 126 => "↑",
        _ => return format!("key{kc}"),
    };
    s.to_string()
}

/// Format a combo as e.g. "⌃⇧Tab". Modifier order: ⌃⌥⇧⌘.
pub fn format_combo_label(modifiers: u64, key_code: i64) -> String {
    let mut s = String::new();
    if modifiers & MOD_CONTROL != 0 {
        s.push('⌃');
    }
    if modifiers & MOD_OPTION != 0 {
        s.push('⌥');
    }
    if modifiers & MOD_SHIFT != 0 {
        s.push('⇧');
    }
    if modifiers & MOD_COMMAND != 0 {
        s.push('⌘');
    }
    s.push_str(&key_name(key_code));
    s
}

/// Validate one combo: needs ≥1 modifier and a non-modifier key.
pub fn validate_combo(c: &Combo) -> Result<(), String> {
    if c.modifiers & MODS_ALL == 0 {
        return Err("Serve almeno un modificatore (⌃⌥⇧⌘).".into());
    }
    if is_modifier_keycode(c.key_code) {
        return Err("Serve un tasto non modificatore.".into());
    }
    Ok(())
}

/// Validate the pair: each valid, and the two must not be identical.
pub fn validate_pair(app: &Combo, win: &Combo) -> Result<(), String> {
    validate_combo(app)?;
    validate_combo(win)?;
    if app.modifiers == win.modifiers && app.key_code == win.key_code {
        return Err("Le due combo non possono essere identiche.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_single_modifier() {
        assert_eq!(format_combo_label(MOD_CONTROL, 48), "⌃Tab");
        assert_eq!(format_combo_label(MOD_CONTROL, 10), "⌃§");
    }

    #[test]
    fn label_multiple_modifiers_in_canonical_order() {
        // ⌃⌥⇧⌘ order regardless of bit order.
        assert_eq!(format_combo_label(MOD_CONTROL | MOD_SHIFT, 48), "⌃⇧Tab");
        assert_eq!(
            format_combo_label(MOD_COMMAND | MOD_OPTION, 12),
            "⌥⌘Q"
        );
        assert_eq!(
            format_combo_label(MOD_CONTROL | MOD_OPTION | MOD_SHIFT | MOD_COMMAND, 49),
            "⌃⌥⇧⌘Space"
        );
    }

    #[test]
    fn validate_rejects_key_only() {
        let c = Combo::new(0, 48);
        assert!(validate_combo(&c).is_err());
    }

    #[test]
    fn validate_rejects_modifier_only_key() {
        // key_code 59 == left control (a modifier key)
        let c = Combo::new(MOD_CONTROL, 59);
        assert!(validate_combo(&c).is_err());
    }

    #[test]
    fn validate_accepts_modifier_plus_key() {
        let c = Combo::new(MOD_CONTROL, 48);
        assert!(validate_combo(&c).is_ok());
    }

    #[test]
    fn validate_pair_rejects_identical() {
        let a = Combo::new(MOD_CONTROL, 48);
        let b = Combo::new(MOD_CONTROL, 48);
        assert!(validate_pair(&a, &b).is_err());
    }

    #[test]
    fn validate_pair_accepts_distinct() {
        let a = Combo::new(MOD_CONTROL, 48);
        let b = Combo::new(MOD_CONTROL, 10);
        assert!(validate_pair(&a, &b).is_ok());
    }

    #[test]
    fn config_json_roundtrips() {
        let cfg = Config::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn default_config_is_ctrl_tab_and_ctrl_section() {
        let cfg = Config::default();
        assert_eq!(cfg.switch_app.label, "⌃Tab");
        assert_eq!(cfg.switch_windows.label, "⌃§");
        assert_eq!(cfg.switch_app.key_code, 48);
        assert_eq!(cfg.switch_windows.key_code, 10);
    }
}
