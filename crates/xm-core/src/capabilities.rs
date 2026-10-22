//! What a headset can do, derived from its announced support-function ids.

use crate::command::{AutoPowerOff, Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryKind {
    Single,
    LeftRight,
    LeftRightCradle,
}

/// Features found by probing rather than announced; all false until probed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProbedFeatures {
    pub firmware: bool,
    pub codec: bool,
    pub bgm: bool,
    pub cinema: bool,
    pub multipoint: bool,
    pub adaptive_volume: bool,
    pub colour: bool,
    pub nc_button: bool,
    pub voice_guidance: bool,
}

/// The support functions the WH-1000XM6 announces (`docs/protocol/protocol.md`, XM6 section).
pub const XM6_FUNCTIONS: [u8; 45] = [
    0x10, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x19, 0x20, 0x23, 0x25, 0x27, 0x34, 0x41, 0x42, 0x44,
    0x56, 0x57, 0x69, 0x6D, 0x71, 0x90, 0x94, 0xA1, 0xB0, 0xB2, 0xB6, 0xB7, 0xB8, 0xC1, 0xC2, 0xD1,
    0xD2, 0xD3, 0xE2, 0xE5, 0xE6, 0xE7, 0xEB, 0xF1, 0xF4, 0xF9, 0xFC, 0xFD, 0xFF,
];

/// What a probe round found on a model: keyed on the reported name, or failing that on the exact
/// support list (a renamed headset). WH-1000XM6 firmware 3.1.5, probed 2026-10-20
/// (`docs/protocol/probe-results.md`).
pub fn probed_for(model_name: &str, functions: &[u8]) -> ProbedFeatures {
    let mut sorted = functions.to_vec();
    sorted.sort_unstable();
    if model_name.starts_with("WH-1000XM6") || sorted == XM6_FUNCTIONS {
        ProbedFeatures {
            firmware: true,
            codec: true,
            bgm: true,
            cinema: true,
            multipoint: true,
            adaptive_volume: false,
            colour: true,
            nc_button: true,
            voice_guidance: true,
        }
    } else {
        crate::log_info!("no probe results for {model_name:?}; probed features stay off");
        ProbedFeatures::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub nc_amb_kind: Option<u8>,
    pub eq_kind: Option<u8>,
    pub eq_bands: u8,
    pub has_clear_bass: bool,
    pub battery: BatteryKind,
    pub speak_to_chat: bool,
    pub auto_pause: bool,
    pub dsee: bool,
    pub auto_power_off: Vec<AutoPowerOff>,
    pub wearing_status: bool,
    pub power_off: bool,
    pub table2: bool,
    pub probed: ProbedFeatures,
    /// General setting slots (1..=4) announced as support functions `D1`..`D4`.
    pub general_setting_slots: Vec<u8>,
    /// Slot labels learnt from `D1` replies after Ready.
    pub general_settings: Vec<(u8, String)>,
}

/// Only these general settings may be written. The touch sensor refused a write on the XM6, and
/// multipoint stays read only in v1 because turning it off could drop the PC link.
const WRITABLE_GENERAL_SETTINGS: [&str; 1] = ["SIDETONE_SETTING"];

impl Capabilities {
    pub fn learn_general_setting(&mut self, slot: u8, label: &str) {
        self.general_settings.retain(|(s, _)| *s != slot);
        self.general_settings.push((slot, label.to_string()));
    }
}

pub fn from_support(functions: &[u8], table2: bool) -> Capabilities {
    let has = |id: u8| functions.contains(&id);
    let nc_amb_kind = [(0x6B, 0x17), (0x6D, 0x19), (0x67, 0x22)]
        .into_iter()
        .find_map(|(f, k)| has(f).then_some(k));
    // EQ under inquired type 04 (10 bands, no clear bass) when 57 is announced.
    let (eq_kind, eq_bands, has_clear_bass) = if has(0x57) {
        (Some(0x04), 10, false)
    } else if has(0x50) || has(0x52) {
        (Some(0x00), 5, true)
    } else {
        (None, 5, false)
    };
    let battery = if has(0x22) {
        BatteryKind::LeftRightCradle
    } else if has(0x21) {
        BatteryKind::LeftRight
    } else {
        BatteryKind::Single
    };
    let auto_power_off = if has(0x25) {
        vec![AutoPowerOff::WhenTakenOff, AutoPowerOff::Never]
    } else if has(0x24) {
        vec![
            AutoPowerOff::Minutes5,
            AutoPowerOff::Minutes30,
            AutoPowerOff::Minutes60,
            AutoPowerOff::Minutes180,
            AutoPowerOff::Never,
        ]
    } else {
        Vec::new()
    };
    Capabilities {
        nc_amb_kind,
        eq_kind,
        eq_bands,
        has_clear_bass,
        battery,
        speak_to_chat: has(0xFC),
        auto_pause: has(0xF1),
        dsee: has(0xE5) || has(0xE6) || has(0xE7),
        auto_power_off,
        wearing_status: table2 || has(0xF1) || has(0xF6),
        power_off: has(0x23),
        table2,
        probed: ProbedFeatures::default(),
        general_setting_slots: (1..=4).filter(|n| has(0xD0 + n)).collect(),
        general_settings: Vec::new(),
    }
}

/// Whether the headset supports `cmd`. Names not mapped here are allowed.
pub fn allows(caps: &Capabilities, cmd: &Command) -> bool {
    match cmd.name {
        "set_nc_amb" => caps.nc_amb_kind.is_some(),
        "set_eq_preset" | "set_eq_bands" => caps.eq_kind.is_some(),
        "set_speak_to_chat" => caps.speak_to_chat,
        "set_auto_pause" => caps.auto_pause,
        "set_dsee" => caps.dsee,
        "power_off" => caps.power_off,
        "set_auto_power_off" => cmd
            .payload
            .get(2)
            .is_some_and(|&b| caps.auto_power_off.iter().any(|v| v.wire() == b)),
        "get_bgm" | "set_bgm" => caps.probed.bgm,
        "get_cinema" | "set_cinema" => caps.probed.cinema,
        "get_adaptive_volume" | "set_adaptive_volume" => caps.probed.adaptive_volume,
        "get_device_list" | "switch_source" => caps.probed.multipoint,
        "get_firmware" => caps.probed.firmware,
        "get_codec" => caps.probed.codec,
        "get_colour" => caps.probed.colour,
        "get_nc_button" => caps.probed.nc_button,
        "get_voice_guidance_volume" => caps.probed.voice_guidance,
        "get_general_setting_capability" | "get_general_setting" => {
            general_slot(cmd).is_some_and(|n| caps.general_setting_slots.contains(&n))
        }
        "set_general_setting" => general_slot(cmd).is_some_and(|n| {
            caps.general_settings
                .iter()
                .any(|(s, l)| *s == n && WRITABLE_GENERAL_SETTINGS.contains(&l.as_str()))
        }),
        _ => true,
    }
}

fn general_slot(cmd: &Command) -> Option<u8> {
    cmd.payload.get(1)?.checked_sub(0xD0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command;

    const XM6: [u8; 45] = [
        0x10, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x19, 0x20, 0x23, 0x25, 0x27, 0x34, 0x41, 0x42,
        0x44, 0x56, 0x57, 0x69, 0x6D, 0x71, 0x90, 0x94, 0xA1, 0xB0, 0xB2, 0xB6, 0xB7, 0xB8, 0xC1,
        0xC2, 0xD1, 0xD2, 0xD3, 0xE2, 0xE5, 0xE6, 0xE7, 0xEB, 0xF1, 0xF4, 0xF9, 0xFC, 0xFD, 0xFF,
    ];

    #[test]
    fn xm6_capabilities() {
        let c = from_support(&XM6, true);
        assert_eq!(c.nc_amb_kind, Some(0x19));
        assert_eq!(c.eq_kind, Some(0x04));
        assert_eq!(c.eq_bands, 10);
        assert!(!c.has_clear_bass);
        assert_eq!(c.battery, BatteryKind::Single);
        assert_eq!(
            c.auto_power_off,
            vec![AutoPowerOff::WhenTakenOff, AutoPowerOff::Never]
        );
        assert!(c.speak_to_chat && c.auto_pause && c.dsee && c.wearing_status && c.power_off);
        assert!(c.table2);
        assert_eq!(c.probed, ProbedFeatures::default());
    }

    #[test]
    fn vector_capabilities() {
        let c = from_support(&[0x6B, 0x20, 0x23], false);
        assert_eq!(c.nc_amb_kind, Some(0x17));
        assert_eq!(c.battery, BatteryKind::Single);
        assert!(c.power_off);
        assert_eq!(c.eq_kind, None);
        assert!(!c.dsee && !c.wearing_status && !c.table2);
        assert!(c.auto_power_off.is_empty());
    }

    #[test]
    fn other_variants() {
        assert_eq!(from_support(&[0x67], false).nc_amb_kind, Some(0x22));
        assert_eq!(from_support(&[0x21], false).battery, BatteryKind::LeftRight);
        assert_eq!(
            from_support(&[0x22], false).battery,
            BatteryKind::LeftRightCradle
        );
        let e = from_support(&[0x50], false);
        assert_eq!(
            (e.eq_kind, e.eq_bands, e.has_clear_bass),
            (Some(0), 5, true)
        );
        assert_eq!(from_support(&[0x52], false).eq_kind, Some(0));
        assert!(from_support(&[0xF6], false).wearing_status);
        assert_eq!(
            from_support(&[0x24], false).auto_power_off,
            vec![
                AutoPowerOff::Minutes5,
                AutoPowerOff::Minutes30,
                AutoPowerOff::Minutes60,
                AutoPowerOff::Minutes180,
                AutoPowerOff::Never
            ]
        );
    }

    #[test]
    fn allows_gates_commands() {
        let xm6 = from_support(&XM6, true);
        assert!(allows(&xm6, &command::set_dsee(true)));
        assert!(allows(
            &xm6,
            &command::set_auto_power_off(AutoPowerOff::Never)
        ));
        assert!(!allows(
            &xm6,
            &command::set_auto_power_off(AutoPowerOff::Minutes30)
        ));
        let v = from_support(&[0x6B, 0x20, 0x23], false);
        assert!(!allows(&v, &command::set_dsee(true)));
        assert!(!allows(
            &v,
            &command::set_eq_preset(0, crate::command::EqPreset(0x00))
        ));
        assert!(!allows(&v, &command::set_speak_to_chat(true)));
        assert!(allows(&v, &command::power_off()));
        assert!(!allows(&from_support(&[], false), &command::power_off()));
        let raw = Command {
            table: command::Table::One,
            payload: vec![1],
            name: "raw",
        };
        assert!(allows(&v, &raw));
    }

    #[test]
    fn allows_gates_probed_additions() {
        let mut c = from_support(&XM6, true);
        let mac = "00:11:22:33:44:55";
        let cmds = || {
            vec![
                (command::set_bgm(true, command::BgmRoom::MyRoom), 0),
                (command::set_cinema(true), 1),
                (command::set_adaptive_volume(true), 2),
                (command::switch_source(mac).unwrap(), 3),
                (command::get_firmware(), 4),
                (command::get_codec(), 5),
            ]
        };
        for (cmd, _) in cmds() {
            assert!(!allows(&c, &cmd), "{} should be refused", cmd.name);
        }
        c.probed = ProbedFeatures {
            firmware: true,
            codec: true,
            bgm: true,
            cinema: true,
            multipoint: true,
            adaptive_volume: true,
            ..ProbedFeatures::default()
        };
        for (cmd, _) in cmds() {
            assert!(allows(&c, &cmd), "{} should be allowed", cmd.name);
        }
        let raw = Command {
            table: command::Table::One,
            payload: vec![1],
            name: "raw",
        };
        assert!(allows(&from_support(&XM6, true), &raw));
    }

    #[test]
    fn general_setting_writes_only_for_sidetone() {
        let mut c = from_support(&XM6, true);
        assert_eq!(c.general_setting_slots, vec![1, 2, 3]);
        assert!(allows(&c, &command::get_general_setting_capability(3)));
        assert!(allows(&c, &command::get_general_setting(3)));
        assert!(!allows(&c, &command::get_general_setting(4)));
        assert!(!allows(&c, &command::get_general_setting_capability(4)));
        // Nothing is writable until a slot's label is known.
        assert!(!allows(&c, &command::set_general_setting(1, true)));
        c.learn_general_setting(1, "SIDETONE_SETTING");
        c.learn_general_setting(2, "MULTIPOINT_SETTING");
        c.learn_general_setting(3, "TOUCH_PANEL_SETTING");
        c.learn_general_setting(3, "TOUCH_PANEL_SETTING");
        assert_eq!(c.general_settings.len(), 3);
        assert!(allows(&c, &command::set_general_setting(1, false)));
        // Multipoint is read only in v1: turning it off could drop the PC link.
        assert!(!allows(&c, &command::set_general_setting(2, true)));
        assert!(!allows(&c, &command::set_general_setting(3, false)));
        assert!(!allows(&c, &command::set_general_setting(4, false)));
        let none = from_support(&[], false);
        assert!(none.general_setting_slots.is_empty());
        assert!(!allows(&none, &command::get_general_setting(1)));
    }

    #[test]
    fn colour_button_and_voice_guidance_gated_on_probe() {
        let mut c = from_support(&XM6, true);
        let cmds = [
            command::get_colour(),
            command::get_nc_button(),
            command::get_voice_guidance_volume(),
        ];
        assert!(cmds.iter().all(|cmd| !allows(&c, cmd)));
        c.probed = probed_for("WH-1000XM6", &[]);
        assert!(cmds.iter().all(|cmd| allows(&c, cmd)));
    }

    #[test]
    fn probed_for_names_the_xm6() {
        let p = probed_for("WH-1000XM6", &[]);
        assert!(p.firmware && p.codec && p.bgm && p.cinema && p.multipoint);
        assert!(p.colour && p.nc_button && p.voice_guidance);
        assert!(!p.adaptive_volume);
        assert_eq!(probed_for("WH-1000XM6 (demo)", &[]), p);
        assert_eq!(probed_for("WH-1000XM5", &[]), ProbedFeatures::default());
        assert_eq!(probed_for("", &[0x6D]), ProbedFeatures::default());
    }

    #[test]
    fn probed_for_recognises_the_xm6_support_list() {
        let p = probed_for("WH-1000XM6", &[]);
        // A renamed headset is still an XM6 by its announced functions, in any order.
        assert_eq!(probed_for("My headphones", &XM6), p);
        let mut shuffled = XM6;
        shuffled.reverse();
        assert_eq!(probed_for("", &shuffled), p);
        // One function more or less is not the probed model.
        assert_eq!(probed_for("x", &XM6[1..]), ProbedFeatures::default());
        let mut more = XM6.to_vec();
        more.push(0x50);
        assert_eq!(probed_for("x", &more), ProbedFeatures::default());
    }
}
