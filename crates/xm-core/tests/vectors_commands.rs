use xm_core::command::*;

#[derive(serde::Deserialize)]
struct Vectors {
    commands: Vec<Entry>,
}

#[derive(serde::Deserialize)]
struct Entry {
    id: String,
    protocol: String,
    table: Option<u8>,
    bytes: String,
}

fn hex(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect()
}

fn amb(mode: NcAmbMode, level: u8, voice: bool) -> NcAmb {
    NcAmb {
        mode,
        ambient_level: level,
        focus_on_voice: voice,
        adaptive: None,
    }
}

/// Timer-style auto power off vectors: the XM6 does not use them, so no builder exists.
const NOT_BUILDABLE: &[&str] = &["v2.getAutoPowerOff.timer", "v2.setAutoPowerOff.timer.15min"];

fn build(id: &str) -> Option<Command> {
    use NcAmbMode::*;
    Some(match id {
        "v2.getProtocolInfo" => get_protocol_info(),
        "v2.getSupportFunctions" => get_support_functions(),
        "v2.getNcAmb.0x17" => get_nc_amb(0x17),
        "v2.getNcAmb.0x19" => get_nc_amb(0x19),
        "v2.getNcAmb.0x22" => get_nc_amb(0x22),
        "v2.getEqStatus" => get_eq_status(0x00),
        "v2.getEqStatus.0x04" => get_eq_status(0x04),
        "v2.getEq" => get_eq(0x00),
        "v2.getEq.0x04" => get_eq(0x04),
        "v2.getBattery.single" => get_battery(0x00),
        "v2.getBattery.leftRight" => get_battery(0x01),
        "v2.getBattery.cradle" => get_battery(0x02),
        "v2.getBattery.singleThreshold" => get_battery(0x08),
        "v2.getBattery.leftRightThreshold" => get_battery(0x09),
        "v2.getBattery.cradleThreshold" => get_battery(0x0A),
        "v2.powerOff" => power_off(),
        "v2.setNcAmb.0x17.nc17" => set_nc_amb(0x17, &amb(NoiseCancelling, 17, false)),
        "v2.setNcAmb.0x17.ambient20voice" => set_nc_amb(0x17, &amb(Ambient, 20, true)),
        "v2.setNcAmb.0x17.off10" => set_nc_amb(0x17, &amb(Off, 10, false)),
        "v2.setNcAmb.0x19.ambient20voice" => set_nc_amb(0x19, &amb(Ambient, 20, true)),
        "v2.setNcAmb.0x19.nc.adaptiveOnSens2" => set_nc_amb(
            0x19,
            &NcAmb {
                adaptive: Some((1, 2)),
                ..amb(NoiseCancelling, 10, false)
            },
        ),
        "v2.setNcAmb.0x22.ambient20voice" => set_nc_amb(0x22, &amb(Ambient, 20, true)),
        "v2.getSpeakToChat" => get_speak_to_chat(),
        "v2.setSpeakToChat.on" => set_speak_to_chat(true),
        "v2.setSpeakToChat.off" => set_speak_to_chat(false),
        "v2.getAutoPause" => get_auto_pause(),
        "v2.setAutoPause.on" => set_auto_pause(true),
        "v2.getDsee" => get_dsee(),
        "v2.setDsee.on" => set_dsee(true),
        "v2.getAutoPowerOff.wearing" => get_auto_power_off(),
        "v2.setAutoPowerOff.wearing.never" => set_auto_power_off(AutoPowerOff::Never),
        "v2.setAutoPowerOff.wearing.takenOff" => set_auto_power_off(AutoPowerOff::WhenTakenOff),
        "v2.getWearingStatus" => get_wearing_status(),
        "v2.setEqPreset.bassBoost" => set_eq_preset(0x00, EqPreset::BASS_BOOST),
        "v2.setEqPreset.custom1.0x04" => set_eq_preset(0x04, EqPreset::CUSTOM1),
        "v2.setEqPreset.off.0x04" => set_eq_preset(0x04, EqPreset::FLAT),
        "v2.setEqPreset.gaming.0x04" => set_eq_preset(0x04, EqPreset::GAMING),
        "v2.setEqPreset.heavy.0x04" => set_eq_preset(0x04, EqPreset::HEAVY),
        "v2.setEqBands.manual" => {
            set_eq_bands(0x00, EqPreset::MANUAL, Some(3), &[-10, -5, 0, 5, 10])
        }
        "v2.setEqBands10.manual" => set_eq_bands(
            0x00,
            EqPreset::MANUAL,
            None,
            &[-6, -3, 0, 1, 2, 3, 4, 5, 6, 6],
        ),
        "v2.setEqBands10.custom1.0x04" => set_eq_bands(
            0x04,
            EqPreset::CUSTOM1,
            None,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 2],
        ),
        _ => return None,
    })
}

#[test]
fn every_v2_vector_is_built_exactly() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/protocol/vectors.json"
    );
    let v: Vectors = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut checked = 0;
    for e in v.commands.iter().filter(|e| e.protocol == "v2") {
        if NOT_BUILDABLE.contains(&e.id.as_str()) {
            continue;
        }
        let c = build(&e.id).unwrap_or_else(|| panic!("unmapped v2 vector {}", e.id));
        let table = if e.table == Some(2) {
            Table::Two
        } else {
            Table::One
        };
        assert_eq!(c.payload, hex(&e.bytes), "{}", e.id);
        assert_eq!(c.table, table, "{}", e.id);
        checked += 1;
    }
    assert!(checked > 30);
}

#[test]
fn hardware_batch_commands() {
    assert_eq!(get_colour().payload, [0x04, 0x03]);
    assert_eq!(get_nc_button().payload, [0x66, 0x30]);
    let v = get_voice_guidance_volume();
    assert_eq!((v.table, v.payload), (Table::Two, vec![0x46, 0x20]));
    assert_eq!(
        get_general_setting_capability(1).payload,
        [0xD0, 0xD1, 0x01]
    );
    assert_eq!(get_general_setting(3).payload, [0xD6, 0xD3]);
    // Observed on the XM6: D8 D1 00 01 turned sidetone off, D8 D1 00 00 back on.
    assert_eq!(
        set_general_setting(1, false).payload,
        [0xD8, 0xD1, 0x00, 0x01]
    );
    assert_eq!(
        set_general_setting(1, true).payload,
        [0xD8, 0xD1, 0x00, 0x00]
    );
    assert_eq!(set_general_setting(2, true).table, Table::One);
}

#[test]
fn additions_beyond_the_vectors() {
    assert_eq!(get_firmware().payload, [0x04, 0x02]);
    assert_eq!(get_codec().payload, [0x12, 0x02]);
    assert_eq!(get_bgm().payload, [0xE6, 0x09]);
    assert_eq!(
        set_bgm(true, BgmRoom::LivingRoom).payload,
        [0xE8, 0x09, 0x00, 0x01]
    );
    assert_eq!(
        set_bgm(false, BgmRoom::Cafe).payload,
        [0xE8, 0x09, 0x01, 0x02]
    );
    assert_eq!(get_cinema().payload, [0xE6, 0x04]);
    assert_eq!(set_cinema(false).payload, [0xE8, 0x04, 0x01]);
    assert_eq!(get_adaptive_volume().payload, [0xF6, 0x0A]);
    assert_eq!(set_adaptive_volume(true).payload, [0xF8, 0x0A, 0x00]);
    assert_eq!(set_adaptive_volume(false).payload, [0xF8, 0x0A, 0x01]);

    let d = get_device_list();
    assert_eq!((d.table, d.payload), (Table::Two, vec![0x36, 0x02]));

    let s = switch_source("AA:BB:CC:DD:EE:FF").unwrap();
    assert_eq!(s.table, Table::Two);
    assert_eq!(
        s.payload,
        [&[0x3C, 0x01][..], b"AA:BB:CC:DD:EE:FF"].concat()
    );
    assert!(switch_source("AA:BB").is_none());

    let eq = set_eq_bands(0x04, EqPreset::MANUAL, None, &[0; 10]);
    assert_eq!(
        eq.payload,
        [0x58, 0x04, 0xA0, 0x0A, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6]
    );
}

#[test]
fn names_are_unique() {
    let all = [
        get_protocol_info(),
        get_support_functions(),
        get_nc_amb(0x19),
        get_eq_status(4),
        get_eq(4),
        get_battery(0),
        get_speak_to_chat(),
        get_auto_pause(),
        get_dsee(),
        get_auto_power_off(),
        power_off(),
        get_wearing_status(),
        get_firmware(),
        get_codec(),
        get_bgm(),
        get_cinema(),
        get_device_list(),
        get_adaptive_volume(),
        set_speak_to_chat(true),
        set_auto_pause(true),
        set_dsee(true),
        set_cinema(true),
        set_adaptive_volume(true),
        set_bgm(true, BgmRoom::MyRoom),
        set_auto_power_off(AutoPowerOff::Never),
        set_eq_preset(4, EqPreset::FLAT),
        set_eq_bands(4, EqPreset::MANUAL, None, &[0; 10]),
        set_nc_amb(
            0x19,
            &NcAmb {
                mode: NcAmbMode::Off,
                ambient_level: 0,
                focus_on_voice: false,
                adaptive: None,
            },
        ),
        switch_source("AA:BB:CC:DD:EE:FF").unwrap(),
        get_colour(),
        get_nc_button(),
        get_voice_guidance_volume(),
        get_general_setting_capability(1),
        get_general_setting(1),
        set_general_setting(1, true),
    ];
    let mut names: Vec<_> = all.iter().map(|c| c.name).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), all.len());
}
