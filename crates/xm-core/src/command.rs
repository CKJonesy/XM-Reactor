//! Command builders: each returns the table and payload to wrap in a frame.
//! Byte layouts follow `docs/protocol/protocol.md`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Table {
    One,
    Two,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub table: Table,
    pub payload: Vec<u8>,
    /// Stable, unique per builder; later layers map it to a capability.
    pub name: &'static str,
}

fn t1(name: &'static str, payload: Vec<u8>) -> Command {
    Command {
        table: Table::One,
        payload,
        name,
    }
}

fn t2(name: &'static str, payload: Vec<u8>) -> Command {
    Command {
        table: Table::Two,
        payload,
        name,
    }
}

/// Most families encode on as 0 and off as 1.
fn onoff(on: bool) -> u8 {
    if on {
        0x00
    } else {
        0x01
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NcAmbMode {
    NoiseCancelling,
    Ambient,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NcAmb {
    pub mode: NcAmbMode,
    /// 0..=20
    pub ambient_level: u8,
    pub focus_on_voice: bool,
    /// Type 0x19 only: (adaptiveOn, sensitivity), echoed from the last reply; zeros when `None`.
    pub adaptive: Option<(u8, u8)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EqPreset(pub u8);

impl EqPreset {
    pub const FLAT: Self = Self(0x00);
    pub const BASS_BOOST: Self = Self(0x16);
    pub const GAMING: Self = Self(0x20);
    pub const HEAVY: Self = Self(0x30);
    pub const CLEAR: Self = Self(0x31);
    pub const HARD: Self = Self(0x32);
    pub const SOFT: Self = Self(0x33);
    pub const MANUAL: Self = Self(0xA0);
    pub const CUSTOM1: Self = Self(0xA1);
    pub const CUSTOM2: Self = Self(0xA2);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPowerOff {
    WhenTakenOff,
    Never,
    Minutes5,
    Minutes30,
    Minutes60,
    Minutes180,
}

impl AutoPowerOff {
    pub(crate) fn wire(self) -> u8 {
        match self {
            Self::WhenTakenOff => 0x10,
            Self::Never => 0x11,
            Self::Minutes5 => 0x00,
            Self::Minutes30 => 0x01,
            Self::Minutes60 => 0x02,
            Self::Minutes180 => 0x03,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgmRoom {
    MyRoom = 0,
    LivingRoom = 1,
    Cafe = 2,
}

pub fn get_protocol_info() -> Command {
    t1("get_protocol_info", vec![0x00, 0x00])
}

pub fn get_support_functions() -> Command {
    t1("get_support_functions", vec![0x06, 0x00])
}

pub fn get_nc_amb(kind: u8) -> Command {
    t1("get_nc_amb", vec![0x66, kind])
}

pub fn set_nc_amb(kind: u8, v: &NcAmb) -> Command {
    let effect = u8::from(v.mode != NcAmbMode::Off);
    let asm = u8::from(v.mode == NcAmbMode::Ambient);
    let voice = u8::from(v.focus_on_voice);
    let level = v.ambient_level.min(20);
    let mut p = vec![0x68, kind, 0x01, effect];
    match kind {
        0x22 => p.extend([voice, level]),
        0x19 => {
            let (on, sens) = v.adaptive.unwrap_or((0, 0));
            p.extend([asm, voice, level, on, sens]);
        }
        _ => p.extend([asm, voice, level]),
    }
    t1("set_nc_amb", p)
}

pub fn get_eq_status(kind: u8) -> Command {
    t1("get_eq_status", vec![0x52, kind])
}

pub fn get_eq(kind: u8) -> Command {
    t1("get_eq", vec![0x56, kind])
}

pub fn set_eq_preset(kind: u8, p: EqPreset) -> Command {
    t1("set_eq_preset", vec![0x58, kind, p.0, 0x00])
}

/// With `clear_bass`, writes the 6-band layout (`bands` is the five others, offset +10).
/// Without it, writes the 10-band layout (offset +6). Values are clamped to the range each allows.
pub fn set_eq_bands(kind: u8, p: EqPreset, clear_bass: Option<i8>, bands: &[i8]) -> Command {
    let mut out = vec![0x58, kind, p.0];
    match clear_bass {
        Some(cb) => {
            out.extend([bands.len() as u8 + 1, (cb.clamp(-10, 10) + 10) as u8]);
            out.extend(bands.iter().map(|b| (b.clamp(&-10, &10) + 10) as u8));
        }
        None => {
            out.push(bands.len() as u8);
            out.extend(bands.iter().map(|b| (b.clamp(&-6, &6) + 6) as u8));
        }
    }
    t1("set_eq_bands", out)
}

pub fn get_battery(kind: u8) -> Command {
    t1("get_battery", vec![0x22, kind])
}

pub fn get_speak_to_chat() -> Command {
    t1("get_speak_to_chat", vec![0xF6, 0x0C])
}

pub fn set_speak_to_chat(on: bool) -> Command {
    t1("set_speak_to_chat", vec![0xF8, 0x0C, onoff(on), 0x01])
}

pub fn get_auto_pause() -> Command {
    t1("get_auto_pause", vec![0xF6, 0x01])
}

pub fn set_auto_pause(on: bool) -> Command {
    t1("set_auto_pause", vec![0xF8, 0x01, onoff(on)])
}

pub fn get_dsee() -> Command {
    t1("get_dsee", vec![0xE6, 0x01])
}

/// DSEE is the one E8 setting where 1 means on (protocol.md, `v2.setDsee.on`).
pub fn set_dsee(on: bool) -> Command {
    t1("set_dsee", vec![0xE8, 0x01, u8::from(on)])
}

/// Asks the variant with wearing detection, which the XM6 answers.
pub fn get_auto_power_off() -> Command {
    t1("get_auto_power_off", vec![0x26, 0x05])
}

pub fn set_auto_power_off(v: AutoPowerOff) -> Command {
    t1("set_auto_power_off", vec![0x28, 0x05, v.wire(), 0x00])
}

pub fn power_off() -> Command {
    t1("power_off", vec![0x24, 0x03, 0x01])
}

pub fn get_wearing_status() -> Command {
    t2("get_wearing_status", vec![0xF2, 0x00])
}

/// The current track (title, album, artist); the XM6 answers with A7 01 in the A9 01 layout.
pub fn get_now_playing() -> Command {
    t1("get_now_playing", vec![0xA6, 0x01])
}

pub fn get_firmware() -> Command {
    t1("get_firmware", vec![0x04, 0x02])
}

pub fn get_codec() -> Command {
    t1("get_codec", vec![0x12, 0x02])
}

pub fn get_bgm() -> Command {
    t1("get_bgm", vec![0xE6, 0x09])
}

pub fn set_bgm(on: bool, room: BgmRoom) -> Command {
    t1("set_bgm", vec![0xE8, 0x09, onoff(on), room as u8])
}

pub fn get_cinema() -> Command {
    t1("get_cinema", vec![0xE6, 0x04])
}

pub fn set_cinema(on: bool) -> Command {
    t1("set_cinema", vec![0xE8, 0x04, onoff(on)])
}

pub fn get_device_list() -> Command {
    t2("get_device_list", vec![0x36, 0x02])
}

/// `None` unless `mac` is exactly 17 characters (`AA:BB:CC:DD:EE:FF`).
pub fn switch_source(mac: &str) -> Option<Command> {
    if mac.len() != 17 || !mac.is_ascii() {
        return None;
    }
    let mut p = vec![0x3C, 0x01];
    p.extend(mac.bytes());
    Some(t2("switch_source", p))
}

pub fn get_adaptive_volume() -> Command {
    t1("get_adaptive_volume", vec![0xF6, 0x0A])
}

pub fn set_adaptive_volume(on: bool) -> Command {
    t1("set_adaptive_volume", vec![0xF8, 0x0A, onoff(on)])
}

pub fn get_colour() -> Command {
    t1("get_colour", vec![0x04, 0x03])
}

pub fn get_nc_button() -> Command {
    t1("get_nc_button", vec![0x66, 0x30])
}

pub fn get_voice_guidance_volume() -> Command {
    t2("get_voice_guidance_volume", vec![0x46, 0x20])
}

/// General setting slots are 1..=4 (wire `D1`..`D4`). The trailing `01` asks for English labels.
pub fn get_general_setting_capability(slot: u8) -> Command {
    t1(
        "get_general_setting_capability",
        vec![0xD0, 0xD0u8.wrapping_add(slot), 0x01],
    )
}

pub fn get_general_setting(slot: u8) -> Command {
    t1("get_general_setting", vec![0xD6, 0xD0u8.wrapping_add(slot)])
}

pub fn set_general_setting(slot: u8, on: bool) -> Command {
    t1(
        "set_general_setting",
        vec![0xD8, 0xD0u8.wrapping_add(slot), 0x00, onoff(on)],
    )
}
