//! Event parser: turns a reply or notification payload into a `DeviceEvent`.
//! Layouts follow `docs/protocol/protocol.md`; anything not fully understood is `None`.

use crate::command::{AutoPowerOff, BgmRoom, EqPreset, NcAmbMode, Table};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wearing {
    Worn,
    Removed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Sbc,
    Aac,
    Ldac,
    AptX,
    AptXHd,
    Other(u8),
}

/// The colour byte of the `05 03` device-info reply; values without a name are kept raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadsetColourCode {
    Black,
    White,
    Silver,
    Red,
    Blue,
    Pink,
    Yellow,
    Green,
    Grey,
    Gold,
    Other(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDevice {
    pub mac: String,
    pub name: String,
    pub connected: bool,
    pub playing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    ProtocolInfo {
        version: u32,
        table1: bool,
        table2: bool,
    },
    SupportFunctions(Vec<u8>),
    NcAmb {
        mode: NcAmbMode,
        ambient_level: u8,
        focus_on_voice: bool,
        /// `Some((adaptiveOn, sensitivity))` for the 9-byte 0x19 layout.
        adaptive: Option<(u8, u8)>,
    },
    EqStatus {
        available: bool,
    },
    Eq {
        preset: EqPreset,
        clear_bass: Option<i8>,
        bands: Vec<i8>,
    },
    Battery {
        level: u8,
        charging: bool,
    },
    LeftRightBattery {
        left: u8,
        left_charging: bool,
        right: u8,
        right_charging: bool,
    },
    CradleBattery {
        level: u8,
        charging: bool,
    },
    SpeakToChat {
        enabled: bool,
    },
    SpeakToChatTalking {
        active: bool,
    },
    AutoPause {
        enabled: bool,
    },
    Dsee {
        enabled: bool,
    },
    AutoPowerOff {
        setting: AutoPowerOff,
        wearing_detection: bool,
    },
    Wearing(Wearing),
    CallActive(bool),
    Firmware(String),
    Codec(Codec),
    NowPlaying {
        title: Option<String>,
        artist: Option<String>,
    },
    Bgm {
        enabled: bool,
        room: BgmRoom,
    },
    Cinema {
        enabled: bool,
    },
    DeviceList(Vec<SourceDevice>),
    AdaptiveVolume {
        enabled: bool,
    },
    Colour {
        series: u8,
        colour: HeadsetColourCode,
    },
    /// What the headset's NC/ambient button cycles through (02 = NC and ambient on the XM6).
    NcButton(u8),
    /// Signed, -2..=2.
    VoiceGuidanceVolume(i8),
    /// A general setting slot (1..=4) and the label naming what it controls.
    GeneralSettingInfo {
        slot: u8,
        label: String,
    },
    GeneralSetting {
        slot: u8,
        on: bool,
    },
    /// `E5 01 01` arrives as BGM or Cinema turns on; read as sound effects being unavailable (unverified).
    SoundEffectUnavailable(bool),
}

/// Most families encode on as 0 and off as 1.
fn on(b: u8) -> Option<bool> {
    match b {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

/// Charging 1 only; 0 not charging, 2 unknown, 3 charged all read as false.
fn charging(b: u8) -> bool {
    b == 1
}

pub fn parse(table: Table, p: &[u8]) -> Option<DeviceEvent> {
    match table {
        Table::One => parse_one(p),
        Table::Two => parse_two(p),
    }
}

fn parse_two(p: &[u8]) -> Option<DeviceEvent> {
    match (*p.first()?, *p.get(1)?) {
        (0xF3 | 0xF5, 0x00) => Some(DeviceEvent::Wearing(match *p.get(2)? {
            0 => Wearing::Worn,
            2..=4 => Wearing::Removed,
            _ => Wearing::Unknown,
        })),
        (0xF5, 0x0B) => Some(DeviceEvent::CallActive(match *p.get(2)? {
            0 => false,
            1 => true,
            _ => return None,
        })),
        (0x37 | 0x39, 0x02) => parse_device_list(p.get(2..)?),
        (0x47, 0x20) => Some(DeviceEvent::VoiceGuidanceVolume(*p.get(2)? as i8)),
        _ => None,
    }
}

/// `count`, then per device: mac(17 ascii), state, 3 unknown bytes, name length, name; then one
/// byte naming the playing slot. State 0 is disconnected, 1 or more connected, and the device
/// whose state equals the trailing byte is the one playing (read from one capture).
fn parse_device_list(mut r: &[u8]) -> Option<DeviceEvent> {
    let count = *r.first()?;
    r = &r[1..];
    let mut out = Vec::new();
    let mut states = Vec::new();
    for _ in 0..count {
        let mac = std::str::from_utf8(r.get(..17)?).ok()?.to_string();
        let state = *r.get(17)?;
        let len = *r.get(21)? as usize;
        let name = std::str::from_utf8(r.get(22..22 + len)?).ok()?.to_string();
        r = &r[22 + len..];
        states.push(state);
        out.push(SourceDevice {
            mac,
            name,
            connected: state >= 1,
            playing: false,
        });
    }
    let playing = *r.first()?;
    for (d, state) in out.iter_mut().zip(states) {
        d.playing = state != 0 && state == playing;
    }
    Some(DeviceEvent::DeviceList(out))
}

fn parse_one(p: &[u8]) -> Option<DeviceEvent> {
    let op = *p.first()?;
    let sub = *p.get(1)?;
    match (op, sub) {
        (0x01, 0x00) => {
            let v = p.get(2..6)?;
            Some(DeviceEvent::ProtocolInfo {
                version: u32::from_be_bytes([v[0], v[1], v[2], v[3]]),
                table1: *p.get(6)? == 0,
                table2: *p.get(7)? == 0,
            })
        }
        (0x07, 0x00) => {
            let n = *p.get(2)? as usize;
            let body = p.get(3..3 + 2 * n)?;
            let mut f: Vec<u8> = body.chunks(2).map(|c| c[0]).collect();
            f.sort_unstable();
            Some(DeviceEvent::SupportFunctions(f))
        }
        (0x67 | 0x69, 0x17 | 0x19 | 0x22) => parse_nc_amb(p),
        (0x53 | 0x55, 0x00) => Some(DeviceEvent::EqStatus {
            available: *p.get(2)? == 0,
        }),
        (0x53 | 0x55, 0x04) => {
            p.get(3)?;
            Some(DeviceEvent::EqStatus {
                available: *p.get(2)? == 0,
            })
        }
        (0x57 | 0x59, 0x00 | 0x04) => parse_eq(p),
        (0x23 | 0x25, 0x00) => Some(DeviceEvent::Battery {
            level: *p.get(2)?,
            charging: charging(*p.get(3)?),
        }),
        (0x23 | 0x25, 0x01) => Some(DeviceEvent::LeftRightBattery {
            left: *p.get(2)?,
            left_charging: charging(*p.get(3)?),
            right: *p.get(4)?,
            right_charging: charging(*p.get(5)?),
        }),
        (0x23 | 0x25, 0x02) => Some(DeviceEvent::CradleBattery {
            level: *p.get(2)?,
            charging: charging(*p.get(3)?),
        }),
        (0xF7 | 0xF9, 0x0C) => {
            p.get(3)?;
            Some(DeviceEvent::SpeakToChat {
                enabled: on(*p.get(2)?)?,
            })
        }
        (0xF5, 0x0C) => {
            p.get(2)?;
            Some(DeviceEvent::SpeakToChatTalking {
                active: *p.get(3)? == 1,
            })
        }
        (0xF7 | 0xF9, 0x01) => Some(DeviceEvent::AutoPause {
            enabled: on(*p.get(2)?)?,
        }),
        (0xE7 | 0xE9, 0x01) => Some(DeviceEvent::Dsee {
            enabled: *p.get(2)? == 1,
        }),
        (0x27 | 0x29, 0x04 | 0x05) => {
            p.get(3)?;
            let setting = match *p.get(2)? {
                0x10 => AutoPowerOff::WhenTakenOff,
                0x11 => AutoPowerOff::Never,
                0x00 => AutoPowerOff::Minutes5,
                0x01 => AutoPowerOff::Minutes30,
                0x02 => AutoPowerOff::Minutes60,
                0x03 => AutoPowerOff::Minutes180,
                _ => return None,
            };
            Some(DeviceEvent::AutoPowerOff {
                setting,
                wearing_detection: sub == 0x05,
            })
        }
        (0xF5, 0x10) => {
            if *p.get(2)? != 0x01 {
                return None;
            }
            Some(DeviceEvent::Wearing(match *p.get(3)? {
                0 => Wearing::Worn,
                1 => Wearing::Removed,
                _ => Wearing::Unknown,
            }))
        }
        (0x05, 0x02) => {
            // The reply is `len text`; a bare text form is also accepted.
            let body = p.get(2..)?;
            let body = match body.first() {
                Some(&n) if usize::from(n) == body.len() - 1 => &body[1..],
                _ => body,
            };
            let s = std::str::from_utf8(body).ok()?;
            (!s.is_empty()).then(|| DeviceEvent::Firmware(s.to_string()))
        }
        (0x13, 0x02) => Some(DeviceEvent::Codec(match *p.get(2)? {
            0x01 => Codec::Sbc,
            0x02 => Codec::Aac,
            0x10 => Codec::Ldac,
            0x20 => Codec::AptX,
            0x21 => Codec::AptXHd,
            n => Codec::Other(n),
        })),
        // A7 answers get_now_playing; A9 is the notification on a track change. Same layout.
        (0xA7 | 0xA9, 0x01) => parse_now_playing(p.get(2..)?),
        (0xE7 | 0xE9, 0x09) => Some(DeviceEvent::Bgm {
            enabled: on(*p.get(2)?)?,
            room: match *p.get(3)? {
                0 => BgmRoom::MyRoom,
                1 => BgmRoom::LivingRoom,
                2 => BgmRoom::Cafe,
                _ => return None,
            },
        }),
        (0xE7 | 0xE9, 0x04) => Some(DeviceEvent::Cinema {
            enabled: on(*p.get(2)?)?,
        }),
        (0xF7 | 0xF9, 0x0A) => Some(DeviceEvent::AdaptiveVolume {
            enabled: on(*p.get(2)?)?,
        }),
        (0x05, 0x03) => Some(DeviceEvent::Colour {
            series: *p.get(2)?,
            colour: match *p.get(3)? {
                0x01 => HeadsetColourCode::Black,
                0x02 => HeadsetColourCode::White,
                0x03 => HeadsetColourCode::Silver,
                0x04 => HeadsetColourCode::Red,
                0x05 => HeadsetColourCode::Blue,
                0x06 => HeadsetColourCode::Pink,
                0x07 => HeadsetColourCode::Yellow,
                0x08 => HeadsetColourCode::Green,
                0x09 => HeadsetColourCode::Grey,
                0x0A => HeadsetColourCode::Gold,
                n => HeadsetColourCode::Other(n),
            },
        }),
        (0x67 | 0x69, 0x30) => Some(DeviceEvent::NcButton(*p.get(2)?)),
        (0xD1, 0xD1..=0xD4) => parse_general_setting_info(sub - 0xD0, p.get(4..)?),
        (0xD7 | 0xD9, 0xD1..=0xD4) => {
            p.get(2)?;
            Some(DeviceEvent::GeneralSetting {
                slot: sub - 0xD0,
                on: on(*p.get(3)?)?,
            })
        }
        (0xE5, 0x01) => Some(DeviceEvent::SoundEffectUnavailable(match *p.get(2)? {
            0 => false,
            1 => true,
            _ => return None,
        })),
        _ => None,
    }
}

/// `<len>label <len>summary`, after the type and format bytes; only the label is kept.
fn parse_general_setting_info(slot: u8, r: &[u8]) -> Option<DeviceEvent> {
    let len = usize::from(*r.first()?);
    let label = std::str::from_utf8(r.get(1..1 + len)?).ok()?.to_string();
    let rest = r.get(1 + len..)?;
    rest.get(1..1 + usize::from(*rest.first()?))?;
    Some(DeviceEvent::GeneralSettingInfo { slot, label })
}

fn parse_nc_amb(p: &[u8]) -> Option<DeviceEvent> {
    let ty = p[1];
    // 0x22 has no ambient/NC selector byte: on means ambient.
    let (effect, asm, voice, level) = if ty == 0x22 {
        (*p.get(3)?, 1, *p.get(4)?, *p.get(5)?)
    } else {
        (*p.get(3)?, *p.get(4)?, *p.get(5)?, *p.get(6)?)
    };
    let adaptive = if ty == 0x19 {
        match p.len() {
            7 => None,
            n if n >= 9 => Some((p[7], p[8])),
            _ => return None,
        }
    } else {
        None
    };
    if effect > 1 || asm > 1 || voice > 1 {
        return None;
    }
    let mode = match (effect, asm) {
        (0, _) => NcAmbMode::Off,
        (_, 1) => NcAmbMode::Ambient,
        _ => NcAmbMode::NoiseCancelling,
    };
    Some(DeviceEvent::NcAmb {
        mode,
        ambient_level: level.min(20),
        focus_on_voice: voice == 1,
        adaptive,
    })
}

fn parse_eq(p: &[u8]) -> Option<DeviceEvent> {
    let preset = EqPreset(*p.get(2)?);
    let band = |b: u8, off: i16| i8::try_from(i16::from(b) - off).ok();
    let (clear_bass, bands) = match *p.get(3)? {
        0 => (None, Vec::new()),
        6 => {
            let raw = p.get(4..10)?;
            let bands = raw[1..]
                .iter()
                .map(|&b| band(b, 10))
                .collect::<Option<_>>()?;
            (Some(band(raw[0], 10)?), bands)
        }
        10 => {
            let bands = p
                .get(4..14)?
                .iter()
                .map(|&b| band(b, 6))
                .collect::<Option<_>>()?;
            (None, bands)
        }
        _ => return None,
    };
    Some(DeviceEvent::Eq {
        preset,
        clear_bass,
        bands,
    })
}

/// Four fields of `tag len text`; tag 2 carries text, tag 1 is empty. Observed on an XM6:
/// title is the first field and the artist the third.
fn parse_now_playing(mut r: &[u8]) -> Option<DeviceEvent> {
    let mut fields: Vec<Option<String>> = Vec::with_capacity(4);
    for _ in 0..4 {
        let tag = *r.first()?;
        let len = *r.get(1)? as usize;
        let text = r.get(2..2 + len)?;
        fields.push(match tag {
            1 if len == 0 => None,
            2 => Some(std::str::from_utf8(text).ok()?.to_string()),
            _ => return None,
        });
        r = &r[2 + len..];
    }
    let artist = fields.swap_remove(2);
    let title = fields.swap_remove(0);
    Some(DeviceEvent::NowPlaying { title, artist })
}

#[cfg(test)]
mod tests {
    use super::*;
    use DeviceEvent as E;

    fn one(p: &[u8]) -> Option<DeviceEvent> {
        parse(Table::One, p)
    }

    #[test]
    fn xm6_wearing_and_talking() {
        assert_eq!(
            one(&[0xF5, 0x10, 0x01, 0x01]),
            Some(E::Wearing(Wearing::Removed))
        );
        assert_eq!(
            one(&[0xF5, 0x10, 0x01, 0x00]),
            Some(E::Wearing(Wearing::Worn))
        );
        assert_eq!(
            one(&[0xF5, 0x0C, 0x00, 0x01]),
            Some(E::SpeakToChatTalking { active: true })
        );
        assert_eq!(
            one(&[0xF5, 0x0C, 0x00, 0x00]),
            Some(E::SpeakToChatTalking { active: false })
        );
        assert_eq!(
            parse(Table::Two, &[0xF5, 0x0B, 0x01]),
            Some(E::CallActive(true))
        );
        assert_eq!(
            parse(Table::Two, &[0xF5, 0x0B, 0x00]),
            Some(E::CallActive(false))
        );
    }

    #[test]
    fn xm6_eq_status() {
        assert_eq!(
            one(&[0x55, 0x04, 0x00, 0x00]),
            Some(E::EqStatus { available: true })
        );
        assert_eq!(
            one(&[0x55, 0x04, 0x01, 0x01, 0x00]),
            Some(E::EqStatus { available: false })
        );
    }

    #[test]
    fn xm6_nc_amb_and_eq() {
        assert_eq!(
            one(&[0x69, 0x19, 0x01, 0x01, 0x00, 0x01, 0x14, 0x00, 0x00]),
            Some(E::NcAmb {
                mode: NcAmbMode::NoiseCancelling,
                ambient_level: 20,
                focus_on_voice: true,
                adaptive: Some((0, 0)),
            })
        );
        assert_eq!(
            one(&[
                0x57, 0x04, 0x20, 0x0A, 0x00, 0x04, 0x08, 0x09, 0x07, 0x05, 0x07, 0x09, 0x06, 0x04
            ]),
            Some(E::Eq {
                preset: EqPreset::GAMING,
                clear_bass: None,
                bands: vec![-6, -2, 2, 3, 1, -1, 1, 3, 0, -2],
            })
        );
    }

    #[test]
    fn xm6_codec_firmware_bgm() {
        assert_eq!(one(&[0x13, 0x02, 0x10]), Some(E::Codec(Codec::Ldac)));
        let mut fw = vec![0x05, 0x02, 0x05];
        fw.extend(b"3.0.1");
        assert_eq!(one(&fw), Some(E::Firmware("3.0.1".into())));
        let mut bare = vec![0x05, 0x02];
        bare.extend(b"3.0.1");
        assert_eq!(one(&bare), Some(E::Firmware("3.0.1".into())));
        assert_eq!(
            one(&[0xE9, 0x09, 0x00, 0x02]),
            Some(E::Bgm {
                enabled: true,
                room: BgmRoom::Cafe
            })
        );
    }

    fn device_list() -> Vec<u8> {
        let mut dl = vec![0x39, 0x02, 0x01];
        dl.extend(b"02:1D:55:0D:7D:AF");
        dl.extend([0x02, 0x2A, 0x41, 0x0C, 0x04]);
        dl.extend(b"Desk");
        dl.push(0x02);
        dl
    }

    #[test]
    fn now_playing_reply_decodes_like_the_notification() {
        // The XM6's answer to get_now_playing (A6 01), captured 2026-10-22: title, album, artist.
        let mut reply = vec![0xA7, 0x01, 2, 0x11];
        reply.extend(b"Undercover Martyn");
        reply.extend([2, 0x0F]);
        reply.extend(b"Tourist History");
        reply.extend([2, 0x14]);
        reply.extend(b"Two Door Cinema Club");
        reply.extend([1, 0]);
        assert_eq!(
            one(&reply),
            Some(E::NowPlaying {
                title: Some("Undercover Martyn".into()),
                artist: Some("Two Door Cinema Club".into())
            })
        );
    }

    #[test]
    fn now_playing_and_device_list_from_capture_layouts() {
        assert_eq!(
            one(&[0xA9, 0x01, 1, 0, 1, 0, 1, 0, 1, 0]),
            Some(E::NowPlaying {
                title: None,
                artist: None
            })
        );
        let mut np = vec![0xA9, 0x01, 2, 2];
        np.extend(b"Hi");
        np.extend([1, 0, 2, 3]);
        np.extend(b"Abc");
        np.extend([1, 0]);
        assert_eq!(
            one(&np),
            Some(E::NowPlaying {
                title: Some("Hi".into()),
                artist: Some("Abc".into())
            })
        );
        assert_eq!(
            parse(Table::Two, &device_list()),
            Some(E::DeviceList(vec![SourceDevice {
                mac: "02:1D:55:0D:7D:AF".into(),
                name: "Desk".into(),
                connected: true,
                playing: true,
            }]))
        );
    }

    #[test]
    fn device_list_playing_slot_from_capture() {
        let line =
            include_str!("../../../docs/protocol/captures/2026-10-19-wh-1000xm6-connect.jsonl")
                .lines()
                .find(|l| l.contains(r#""bytes":"39 02 06"#))
                .unwrap();
        let hex = line
            .split(r#""bytes":""#)
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let bytes: Vec<u8> = hex
            .split(' ')
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect();
        let Some(E::DeviceList(d)) = parse(Table::Two, &bytes) else {
            panic!("no device list")
        };
        assert_eq!(d.len(), 6);
        let playing: Vec<_> = d.iter().map(|x| x.playing).collect();
        assert_eq!(playing, [true, false, false, false, false, false]);
        assert!(d[1].connected && !d[2].connected);
    }

    #[test]
    fn xm6_hardware_batch_replies() {
        assert_eq!(
            one(&[0x05, 0x03, 0x30, 0x03]),
            Some(E::Colour {
                series: 0x30,
                colour: HeadsetColourCode::Silver
            })
        );
        assert_eq!(
            one(&[0x05, 0x03, 0x30, 0x0B]),
            Some(E::Colour {
                series: 0x30,
                colour: HeadsetColourCode::Other(0x0B)
            })
        );
        assert_eq!(one(&[0x67, 0x30, 0x02]), Some(E::NcButton(2)));
        assert_eq!(one(&[0x69, 0x30, 0x01]), Some(E::NcButton(1)));
        assert_eq!(
            parse(Table::Two, &[0x47, 0x20, 0x00]),
            Some(E::VoiceGuidanceVolume(0))
        );
        assert_eq!(
            parse(Table::Two, &[0x47, 0x20, 0xFE]),
            Some(E::VoiceGuidanceVolume(-2))
        );
        let mut info = vec![0xD1, 0xD1, 0x00, 0x01, 0x10];
        info.extend(b"SIDETONE_SETTING");
        info.push(0x18);
        info.extend(b"SIDETONE_SETTING_SUMMARY");
        assert_eq!(
            one(&info),
            Some(E::GeneralSettingInfo {
                slot: 1,
                label: "SIDETONE_SETTING".into()
            })
        );
        assert_eq!(
            one(&[0xD7, 0xD1, 0x00, 0x00]),
            Some(E::GeneralSetting { slot: 1, on: true })
        );
        assert_eq!(
            one(&[0xD9, 0xD1, 0x00, 0x01]),
            Some(E::GeneralSetting { slot: 1, on: false })
        );
        assert_eq!(
            one(&[0xD9, 0xD3, 0x00, 0x00]),
            Some(E::GeneralSetting { slot: 3, on: true })
        );
        assert_eq!(
            one(&[0xE5, 0x01, 0x01]),
            Some(E::SoundEffectUnavailable(true))
        );
        assert_eq!(
            one(&[0xE5, 0x01, 0x00]),
            Some(E::SoundEffectUnavailable(false))
        );
        assert_eq!(
            one(&[0xE7, 0x09, 0x01, 0x02]),
            Some(E::Bgm {
                enabled: false,
                room: BgmRoom::Cafe
            })
        );
        assert_eq!(one(&[0xE9, 0x04, 0x00]), Some(E::Cinema { enabled: true }));
    }

    #[test]
    fn nc_notifications_with_value_changing_byte() {
        let nc = |p: &[u8]| match one(p) {
            Some(E::NcAmb { mode, .. }) => mode,
            e => panic!("{e:?}"),
        };
        // Live: button press to ambient, take off (effect off), put on (effect back).
        assert_eq!(
            nc(&[0x69, 0x19, 0x01, 0x01, 0x01, 0x01, 0x14, 0x00, 0x00]),
            NcAmbMode::Ambient
        );
        assert_eq!(
            nc(&[0x69, 0x19, 0x00, 0x00, 0x01, 0x01, 0x14, 0x00, 0x00]),
            NcAmbMode::Off
        );
        assert_eq!(
            nc(&[0x69, 0x19, 0x00, 0x01, 0x01, 0x01, 0x14, 0x00, 0x00]),
            NcAmbMode::Ambient
        );
    }

    #[test]
    fn unknown_payloads_are_none() {
        assert_eq!(one(&[0xE5, 0x04, 0x00]), None);
        assert_eq!(one(&[0xC9, 0x01, 0x00]), None);
        assert_eq!(one(&[]), None);
        assert_eq!(one(&[0x68]), None);
        assert_eq!(parse(Table::Two, &[]), None);
    }

    #[test]
    fn truncated_prefixes_are_none() {
        let t1 = |p: &[u8], skip: &[usize]| (Table::One, p.to_vec(), skip.to_vec());
        // The skip list names prefixes that are themselves a valid shorter layout.
        let cases = vec![
            t1(&[0x01, 0x00, 0x03, 0x00, 0x30, 0x32, 0x00, 0x00], &[]),
            t1(&[0x07, 0x00, 0x03, 0x6B, 0x01, 0x20, 0x01, 0x23, 0x01], &[]),
            t1(&[0x69, 0x17, 0x01, 0x01, 0x01, 0x01, 0x0F], &[]),
            t1(
                &[0x69, 0x19, 0x01, 0x01, 0x00, 0x01, 0x14, 0x00, 0x00],
                &[7],
            ),
            t1(&[0x69, 0x22, 0x01, 0x01, 0x01, 0x14], &[]),
            t1(&[0x53, 0x00, 0x00], &[]),
            t1(&[0x55, 0x04, 0x00, 0x00], &[]),
            t1(&[0x57, 0x04, 0x20, 0x0A, 0, 4, 8, 9, 7, 5, 7, 9, 6, 4], &[]),
            t1(
                &[0x57, 0x00, 0xA1, 0x06, 0x14, 0x00, 0x05, 0x0A, 0x0F, 0x14],
                &[],
            ),
            t1(&[0x23, 0x00, 0x54, 0x00], &[]),
            t1(&[0x23, 0x01, 0x50, 0x01, 0x4B, 0x00], &[]),
            t1(&[0x23, 0x02, 0x3C, 0x00], &[]),
            t1(&[0xF7, 0x0C, 0x00, 0x00], &[]),
            t1(&[0xF5, 0x0C, 0x00, 0x01], &[]),
            t1(&[0xF7, 0x01, 0x01], &[]),
            t1(&[0xE7, 0x01, 0x01], &[]),
            t1(&[0x27, 0x05, 0x10, 0x00], &[]),
            t1(&[0xF5, 0x10, 0x01, 0x01], &[]),
            t1(&[0x13, 0x02, 0x10], &[]),
            t1(&[0x05, 0x02, 0x33], &[]),
            t1(&[0xA9, 0x01, 1, 0, 1, 0, 1, 0, 1, 0], &[]),
            t1(&[0xE9, 0x09, 0x00, 0x02], &[]),
            t1(&[0xE9, 0x04, 0x00], &[]),
            t1(&[0xF9, 0x0A, 0x00], &[]),
            t1(&[0x05, 0x03, 0x30, 0x03], &[]),
            t1(&[0x67, 0x30, 0x02], &[]),
            t1(&[0xD1, 0xD2, 0x00, 0x01, 0x02, b'M', b'P', 0x01, b'S'], &[]),
            t1(&[0xD7, 0xD1, 0x00, 0x00], &[]),
            t1(&[0xE5, 0x01, 0x01], &[]),
            (Table::Two, vec![0x47, 0x20, 0x00], vec![]),
            (Table::Two, vec![0xF3, 0x00, 0x04], vec![]),
            (Table::Two, vec![0xF5, 0x0B, 0x01], vec![]),
            (Table::Two, device_list(), vec![]),
        ];
        for (table, payload, skip) in cases {
            assert!(
                parse(table, &payload).is_some(),
                "full payload {payload:02X?}"
            );
            for n in (0..payload.len()).filter(|n| !skip.contains(n)) {
                assert_eq!(
                    parse(table, &payload[..n]),
                    None,
                    "prefix {n} of {payload:02X?}"
                );
            }
        }
    }
}
