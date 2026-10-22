use serde_json::Value;
use xm_core::command::{AutoPowerOff, EqPreset, NcAmbMode, Table};
use xm_core::event::{parse, DeviceEvent, Wearing};

fn hex(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .map(|h| u8::from_str_radix(h, 16).unwrap())
        .collect()
}

fn preset(name: &str) -> EqPreset {
    match name {
        "off" => EqPreset::FLAT,
        "gamingEq" => EqPreset::GAMING,
        "heavy" => EqPreset::HEAVY,
        "clear" => EqPreset::CLEAR,
        "hard" => EqPreset::HARD,
        "soft" => EqPreset::SOFT,
        "manual" => EqPreset::MANUAL,
        "custom1" => EqPreset::CUSTOM1,
        "custom2" => EqPreset::CUSTOM2,
        other => {
            let raw = other
                .strip_prefix("unknown(0x")
                .unwrap()
                .trim_end_matches(')');
            EqPreset(u8::from_str_radix(raw, 16).unwrap())
        }
    }
}

fn expected(e: &Value) -> DeviceEvent {
    let b = |k: &str| e[k].as_bool().unwrap();
    let u = |k: &str| e[k].as_u64().unwrap();
    let charging = |k: &str| e[k].as_str().unwrap() == "charging";
    match e["event"].as_str().unwrap() {
        "protocolInfo" => DeviceEvent::ProtocolInfo {
            version: u("version") as u32,
            table1: b("supportsTable1"),
            table2: b("supportsTable2"),
        },
        "supportFunctions" => DeviceEvent::SupportFunctions(
            e["functions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect(),
        ),
        "ncAmb" => DeviceEvent::NcAmb {
            mode: match e["mode"].as_str().unwrap() {
                "noiseCancelling" => NcAmbMode::NoiseCancelling,
                "ambient" => NcAmbMode::Ambient,
                "off" => NcAmbMode::Off,
                m => panic!("mode {m}"),
            },
            ambient_level: u("ambientLevel") as u8,
            focus_on_voice: b("focusOnVoice"),
            adaptive: e["adaptive"].as_object().map(|a| {
                (
                    a["enabled"].as_bool().unwrap() as u8,
                    a["sensitivity"].as_u64().unwrap() as u8,
                )
            }),
        },
        "eqStatus" => DeviceEvent::EqStatus {
            available: b("available"),
        },
        "eq" => DeviceEvent::Eq {
            preset: preset(e["preset"].as_str().unwrap()),
            clear_bass: None,
            bands: e["bands"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i8)
                .collect(),
        },
        "battery" => DeviceEvent::Battery {
            level: u("level") as u8,
            charging: charging("charging"),
        },
        "leftRightBattery" => DeviceEvent::LeftRightBattery {
            left: u("leftLevel") as u8,
            left_charging: charging("leftCharging"),
            right: u("rightLevel") as u8,
            right_charging: charging("rightCharging"),
        },
        "cradleBattery" => DeviceEvent::CradleBattery {
            level: u("level") as u8,
            charging: charging("charging"),
        },
        "speakToChat" => DeviceEvent::SpeakToChat {
            enabled: b("enabled"),
        },
        "autoPause" => DeviceEvent::AutoPause {
            enabled: b("enabled"),
        },
        "dsee" => DeviceEvent::Dsee {
            enabled: b("enabled"),
        },
        "autoPowerOff" => DeviceEvent::AutoPowerOff {
            setting: match e["setting"].as_str().unwrap() {
                "whenTakenOff" => AutoPowerOff::WhenTakenOff,
                "never" => AutoPowerOff::Never,
                s => panic!("setting {s}"),
            },
            wearing_detection: b("wearingDetection"),
        },
        "wearing" => DeviceEvent::Wearing(match e["state"].as_str().unwrap() {
            "worn" => Wearing::Worn,
            "removed" => Wearing::Removed,
            _ => Wearing::Unknown,
        }),
        other => panic!("event {other}"),
    }
}

#[test]
fn v2_replies_match_vectors() {
    let root: Value =
        serde_json::from_str(include_str!("../../../docs/protocol/vectors.json")).unwrap();
    let mut n = 0;
    for r in root["replies"].as_array().unwrap() {
        if r["protocol"] == "v1" {
            continue;
        }
        let id = r["id"].as_str().unwrap();
        let table = if r["table"].as_u64().unwrap_or(1) == 2 {
            Table::Two
        } else {
            Table::One
        };
        let got = parse(table, &hex(r["bytes"].as_str().unwrap()));
        let mut want = expected(&r["expect"]);
        // The vectors give clearBass 0 for layouts without one; only 6-band replies carry it.
        if let (DeviceEvent::Eq { clear_bass, .. }, Some(DeviceEvent::Eq { clear_bass: g, .. })) =
            (&mut want, &got)
        {
            *clear_bass = *g;
            let cb = r["expect"]["clearBass"].as_i64().unwrap();
            assert_eq!(i64::from(g.unwrap_or(0)), cb, "{id} clearBass");
        }
        assert_eq!(got, Some(want), "{id}");
        n += 1;
    }
    assert!(n > 40, "only {n} v2 vectors checked");
}
