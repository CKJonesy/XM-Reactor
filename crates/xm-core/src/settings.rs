//! User settings, stored as camelCase JSON. A missing or corrupt file means defaults.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HeadsetColour {
    PlatinumSilver,
    MidnightBlue,
    Black,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeChoice {
    #[default]
    FollowWindows,
    Light,
    Dark,
}

/// A preset's bands in dB: the first the headset reported (a built-in preset's factory curve, for a
/// future "Reset to default") and the latest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetCurve {
    pub default: Vec<i8>,
    pub latest: Vec<i8>,
}

/// Each preset's curve as the headset reported it: headset name, then the preset id in hex ("20").
pub type EqCurves = BTreeMap<String, BTreeMap<String, PresetCurve>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub preferred_address: Option<String>,
    pub pause_on_removal: bool,
    pub resume_on_wear: bool,
    pub headset_colour: Option<HeadsetColour>,
    pub battery_in_tooltip: bool,
    pub low_battery_notifications: bool,
    pub theme: ThemeChoice,
    /// App-only nicknames for custom EQ presets, keyed by the preset id in hex ("A1").
    pub eq_names: BTreeMap<String, String>,
    /// So a preset's curve shows at once, before the headset replies; see `EqCurves`.
    pub eq_curves: EqCurves,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            preferred_address: None,
            pause_on_removal: true,
            resume_on_wear: true,
            headset_colour: None,
            battery_in_tooltip: true,
            low_battery_notifications: true,
            theme: ThemeChoice::default(),
            eq_names: BTreeMap::new(),
            eq_curves: EqCurves::new(),
        }
    }
}

/// `%LOCALAPPDATA%\XM Reactor`, falling back to the temp folder if that is unset.
pub fn app_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("XM Reactor")
}

pub fn default_path() -> PathBuf {
    app_dir().join("settings.json")
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let Some(serde_json::Value::Object(m)) = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        else {
            return Settings::default();
        };
        // Each field is read on its own, so one bad value costs only that field.
        fn field<T: serde::de::DeserializeOwned>(
            m: &serde_json::Map<String, serde_json::Value>,
            key: &str,
            default: T,
        ) -> T {
            m.get(key)
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or(default)
        }
        let d = Settings::default();
        Settings {
            preferred_address: field(&m, "preferredAddress", d.preferred_address),
            pause_on_removal: field(&m, "pauseOnRemoval", d.pause_on_removal),
            resume_on_wear: field(&m, "resumeOnWear", d.resume_on_wear),
            headset_colour: field(&m, "headsetColour", d.headset_colour),
            battery_in_tooltip: field(&m, "batteryInTooltip", d.battery_in_tooltip),
            low_battery_notifications: field(
                &m,
                "lowBatteryNotifications",
                d.low_battery_notifications,
            ),
            theme: field(&m, "theme", d.theme),
            eq_names: field(&m, "eqNames", d.eq_names),
            eq_curves: field(&m, "eqCurves", d.eq_curves),
        }
    }

    /// Writes a temp file beside the target, then renames it over, so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xm-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn settings_round_trip_and_corrupt_file_gives_defaults() {
        let dir = temp("settings");
        let path = dir.join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());

        let s = Settings {
            preferred_address: Some("AA:BB".into()),
            pause_on_removal: false,
            headset_colour: Some(HeadsetColour::MidnightBlue),
            theme: ThemeChoice::Dark,
            ..Settings::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("pauseOnRemoval"));
        assert!(!dir.join("settings.json.tmp").exists());

        std::fs::write(&path, r#"{"theme":"light","somethingNew":1}"#).unwrap();
        let l = Settings::load(&path);
        assert_eq!(l.theme, ThemeChoice::Light);
        assert!(l.pause_on_removal);

        std::fs::write(
            &path,
            r#"{"theme":"sepia","pauseOnRemoval":false,"headsetColour":7}"#,
        )
        .unwrap();
        let l = Settings::load(&path);
        assert_eq!(l.theme, ThemeChoice::FollowWindows);
        assert!(!l.pause_on_removal);
        assert_eq!(l.headset_colour, None);

        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn eq_names_load_and_save() {
        let dir = temp("eq-names");
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        assert!(s.eq_names.is_empty());
        s.eq_names.insert("A1".into(), "Desk".into());
        s.save(&path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("eqNames"));
        assert_eq!(Settings::load(&path).eq_names, s.eq_names);

        // A file from before nicknames, or a bad value, keeps the other fields.
        std::fs::write(&path, r#"{"theme":"dark","eqNames":[1,2]}"#).unwrap();
        let l = Settings::load(&path);
        assert!(l.eq_names.is_empty());
        assert_eq!(l.theme, ThemeChoice::Dark);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn eq_curves_load_and_save() {
        let dir = temp("eq-curves");
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        let gaming = PresetCurve {
            default: vec![-6, -2, 2, 3, 1, -1, 1, 3, 0, -2],
            latest: vec![0; 10],
        };
        s.eq_curves
            .entry("WH-1000XM6".into())
            .or_default()
            .insert("20".into(), gaming.clone());
        s.save(&path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("eqCurves"));
        assert_eq!(Settings::load(&path).eq_curves["WH-1000XM6"]["20"], gaming);

        // A file from before curves, or a bad value, keeps the other fields; unknown keys are fine.
        std::fs::write(
            &path,
            r#"{"theme":"dark","eqCurves":{"WH-1000XM6":{"20":"loud"}},"later":1}"#,
        )
        .unwrap();
        let l = Settings::load(&path);
        assert!(l.eq_curves.is_empty());
        assert_eq!(l.theme, ThemeChoice::Dark);
        std::fs::write(&path, r#"{"eqNames":{"A1":"Desk"}}"#).unwrap();
        let l = Settings::load(&path);
        assert!(l.eq_curves.is_empty());
        assert_eq!(l.eq_names["A1"], "Desk");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
