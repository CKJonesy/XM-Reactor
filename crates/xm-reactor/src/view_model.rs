//! Display-ready view of the headphones: a pure function of the model state and the app settings.

use crate::strings;
use xm_core::capabilities::Capabilities;
use xm_core::command::{AutoPowerOff, BgmRoom, EqPreset, NcAmbMode};
use xm_core::event::{Codec, Wearing};
use xm_core::model::{BatteryReading, GeneralSetting, HeadphonesState};
use xm_core::session::SessionState;
use xm_core::settings::{HeadsetColour, Settings, ThemeChoice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Angle {
    Front,
    Side,
    ThreeQuarter,
    Rear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderChoice {
    pub colour: HeadsetColour,
    pub angle: Angle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    NoiseCancelling,
    Ambient,
    Off,
    AutoPause,
    SpeakToChat,
    Dsee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileVm {
    pub kind: TileKind,
    pub label: String,
    pub subtitle: Option<String>,
    pub visible: bool,
    pub enabled: bool,
    pub on: bool,
    /// What the tile does.
    pub tooltip: &'static str,
}

/// The ambient level and Focus on voice, present whenever noise control is known; the panel mounts
/// them only in Ambient mode (`enabled`), the only mode they apply to (Ruling 32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbientVm {
    pub level: u8,
    pub focus_on_voice: bool,
    pub enabled: bool,
}

/// What the person can do about a missing connection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recovery {
    pub open_bluetooth: bool,
    pub try_again: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EqVm {
    pub presets: Vec<(EqPreset, String)>,
    pub selected: EqPreset,
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListeningMode {
    Standard,
    Bgm,
    Cinema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceVm {
    pub mac: String,
    pub name: String,
    pub connected: bool,
    pub playing: bool,
    pub status: String,
    /// Playback can move to a connected device that is not already playing.
    pub can_switch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoRow {
    pub label: String,
    pub description: Option<&'static str>,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppSettingsVm {
    pub pause_on_removal: bool,
    pub resume_on_wear: bool,
    pub battery_in_tooltip: bool,
    pub low_battery_notifications: bool,
    pub theme: ThemeChoice,
    pub headset_colour: Option<HeadsetColour>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsVm {
    pub clear_bass: Option<i8>,
    /// Band gains; empty until the headphones report them.
    pub bands: Vec<i8>,
    pub band_labels: Vec<&'static str>,
    /// Bands and Clear Bass run from -band_max to +band_max.
    pub band_max: i8,
    /// The main view shows the equaliser curve under the chips for any preset once its bands are
    /// known, and the flyout takes its tall height (Ruling 29).
    pub show_bands: bool,
    /// Manual or a Custom preset: the bands can be edited and the preset renamed.
    pub custom_preset: bool,
    /// The curve can be edited: a custom preset, outside a call or a listening mode (the equaliser
    /// chips show why). A built-in preset's curve is shown read-only.
    pub bands_enabled: bool,
    /// None when the headphones offer neither BGM nor cinema.
    pub listening_mode: Option<ListeningMode>,
    /// The modes this headset was probed for; empty when the section is hidden.
    pub listening_modes: Vec<(ListeningMode, String)>,
    pub room: Option<BgmRoom>,
    pub auto_power_off: Vec<(AutoPowerOff, String)>,
    pub auto_power_off_selected: Option<AutoPowerOff>,
    /// Sidetone general-setting slot and its value.
    pub sidetone: Option<(u8, bool)>,
    pub info: Vec<InfoRow>,
    pub sources: Vec<SourceVm>,
    pub firmware: Option<String>,
    pub codec: Option<String>,
    pub app: AppSettingsVm,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelVm {
    pub device_name: String,
    pub status: String,
    pub battery_text: Option<String>,
    pub battery_fraction: Option<f64>,
    pub charging: bool,
    pub now_playing: Option<String>,
    /// None draws the outline instead of a render.
    pub render: Option<RenderChoice>,
    pub connected: bool,
    pub recovery: Recovery,
    pub tiles: [TileVm; 6],
    pub ambient: Option<AmbientVm>,
    pub eq: Option<EqVm>,
    pub effect_suspended: bool,
    pub low_battery_toast: Option<u8>,
    pub settings: SettingsVm,
}

pub const ROOMS: [(BgmRoom, &str); 3] = [
    (BgmRoom::MyRoom, strings::ROOM_MY),
    (BgmRoom::LivingRoom, strings::ROOM_LIVING),
    (BgmRoom::Cafe, strings::ROOM_CAFE),
];

/// The nine presets the WH-1000XM6 accepts (Ruling 28; Bass boost is not one of them): the six
/// built-ins, then the three editable ones, so on three columns the editable presets are the last
/// row.
pub const EQ_PRESETS: [(EqPreset, &str); 9] = [
    (EqPreset::FLAT, strings::EQ_FLAT),
    (EqPreset::GAMING, strings::EQ_GAMING),
    (EqPreset::HEAVY, strings::EQ_HEAVY),
    (EqPreset::CLEAR, strings::EQ_CLEAR),
    (EqPreset::HARD, strings::EQ_HARD),
    (EqPreset::SOFT, strings::EQ_SOFT),
    (EqPreset::MANUAL, strings::EQ_MANUAL),
    (EqPreset::CUSTOM1, strings::EQ_CUSTOM1),
    (EqPreset::CUSTOM2, strings::EQ_CUSTOM2),
];

fn auto_power_off_label(a: AutoPowerOff) -> &'static str {
    match a {
        AutoPowerOff::WhenTakenOff => strings::APO_WHEN_TAKEN_OFF,
        AutoPowerOff::Never => strings::APO_NEVER,
        AutoPowerOff::Minutes5 => strings::APO_5_MIN,
        AutoPowerOff::Minutes30 => strings::APO_30_MIN,
        AutoPowerOff::Minutes60 => strings::APO_60_MIN,
        AutoPowerOff::Minutes180 => strings::APO_180_MIN,
    }
}

const BANDS_10: [&str; 10] = [
    "31", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k",
];
const BANDS_5: [&str; 5] = ["400", "1k", "2.5k", "6.3k", "16k"];

fn codec_name(c: Codec) -> String {
    match c {
        Codec::Sbc => strings::CODEC_SBC.into(),
        Codec::Aac => strings::CODEC_AAC.into(),
        Codec::Ldac => strings::CODEC_LDAC.into(),
        Codec::AptX => strings::CODEC_APTX.into(),
        Codec::AptXHd => strings::CODEC_APTX_HD.into(),
        Codec::Other(n) => strings::codec_other(n),
    }
}

/// The level shown for the pair is the lower earbud, since that one runs out first.
pub fn battery_level(b: &BatteryReading) -> (u8, bool) {
    match b {
        BatteryReading::Single { level, charging } => (*level, *charging),
        BatteryReading::LeftRight {
            left,
            left_charging,
            right,
            right_charging,
            ..
        } => ((*left).min(*right), *left_charging || *right_charging),
    }
}

/// "SIDETONE_SETTING" becomes "Sidetone setting".
fn humanise(label: &str) -> String {
    let mut s = label.replace('_', " ").to_lowercase();
    if let Some(first) = s.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    s
}

/// `turned_off`: the app turned the headphones off and they have not come back since.
fn status(st: &HeadphonesState, turned_off: bool) -> &'static str {
    match &st.session {
        SessionState::Ready(_) | SessionState::Connecting => {}
        _ if turned_off => return strings::STATUS_TURNED_OFF,
        _ => {}
    }
    match &st.session {
        SessionState::Disconnected => strings::STATUS_DISCONNECTED,
        SessionState::NotConnected => strings::STATUS_NOT_FOUND,
        SessionState::BluetoothOff => strings::STATUS_BLUETOOTH_OFF,
        SessionState::InUse => strings::STATUS_IN_USE,
        SessionState::Connecting => strings::STATUS_CONNECTING,
        SessionState::Ready(_) if st.effect_suspended => strings::STATUS_REMOVED,
        SessionState::Ready(_) => match st.wearing {
            Wearing::Worn => strings::STATUS_WORN,
            Wearing::Removed => strings::STATUS_REMOVED,
            Wearing::Unknown => strings::STATUS_CONNECTED,
        },
    }
}

fn tiles(st: &HeadphonesState, caps: Option<&Capabilities>) -> [TileVm; 6] {
    let mode = st.nc.map(|n| n.mode);
    // A state kept from before a disconnect is stale, so nothing reads as on without a link.
    let tile = |kind, label: &str, tooltip, visible: bool, on: bool| {
        let on = on && caps.is_some();
        TileVm {
            kind,
            label: label.into(),
            subtitle: (st.effect_suspended
                && on
                && matches!(
                    kind,
                    TileKind::NoiseCancelling | TileKind::Ambient | TileKind::Off
                ))
            .then(|| strings::TILE_SUSPENDED.into()),
            visible,
            enabled: caps.is_some() && visible,
            on,
            tooltip,
        }
    };
    // Without capabilities (not connected) every tile shows, greyed out.
    let nc = caps.is_none_or(|c| c.nc_amb_kind.is_some());
    [
        tile(
            TileKind::NoiseCancelling,
            strings::TILE_NOISE_CANCELLING,
            strings::TIP_NOISE_CANCELLING,
            nc,
            mode == Some(NcAmbMode::NoiseCancelling),
        ),
        tile(
            TileKind::Ambient,
            strings::TILE_AMBIENT,
            strings::TIP_AMBIENT,
            nc,
            mode == Some(NcAmbMode::Ambient),
        ),
        tile(
            TileKind::Off,
            strings::TILE_OFF,
            strings::TIP_OFF,
            nc,
            mode == Some(NcAmbMode::Off),
        ),
        tile(
            TileKind::AutoPause,
            strings::TILE_AUTO_PAUSE,
            strings::TIP_AUTO_PAUSE,
            caps.is_none_or(|c| c.auto_pause),
            st.auto_pause == Some(true),
        ),
        tile(
            TileKind::SpeakToChat,
            strings::TILE_SPEAK_TO_CHAT,
            strings::TIP_SPEAK_TO_CHAT,
            caps.is_none_or(|c| c.speak_to_chat),
            st.speak_to_chat == Some(true),
        ),
        tile(
            TileKind::Dsee,
            strings::TILE_DSEE,
            strings::TIP_DSEE,
            caps.is_none_or(|c| c.dsee),
            st.dsee == Some(true),
        ),
    ]
}

/// Why the headphones will not take an equaliser change now, if they will not: the reason names
/// the listening mode and where to turn it off.
fn eq_reason(st: &HeadphonesState) -> Option<&'static str> {
    if st.in_call {
        Some(strings::EQ_REASON_CALL)
    } else if st.bgm.is_some_and(|(on, _)| on) {
        Some(strings::EQ_REASON_BGM)
    } else if st.cinema == Some(true) {
        Some(strings::EQ_REASON_CINEMA)
    } else if st.sound_effect_unavailable {
        Some(strings::EQ_REASON_LISTENING_MODE)
    } else if !st.eq_available {
        // The headphones refused without saying why; a call that began before the app connected
        // looks like this, since the call notice only comes as it starts.
        Some(strings::EQ_REASON_UNAVAILABLE)
    } else {
        None
    }
}

/// The name the headphones give preset `p`, before any nickname.
pub fn preset_name(p: EqPreset) -> Option<&'static str> {
    EQ_PRESETS.iter().find(|(q, _)| *q == p).map(|(_, n)| *n)
}

/// The presets whose bands the user sets, and the only ones that can be renamed in the app.
pub fn is_custom(p: EqPreset) -> bool {
    p == EqPreset::MANUAL || p == EqPreset::CUSTOM1 || p == EqPreset::CUSTOM2
}

/// The key a custom preset's nickname is saved under in the settings.
pub fn eq_name_key(p: EqPreset) -> String {
    format!("{:02X}", p.0)
}

fn eq(st: &HeadphonesState, caps: &Capabilities, settings: &Settings) -> Option<EqVm> {
    caps.eq_kind?;
    let reason = eq_reason(st);
    Some(EqVm {
        presets: EQ_PRESETS
            .iter()
            .map(|(p, name)| {
                let nickname = is_custom(*p)
                    .then(|| settings.eq_names.get(&eq_name_key(*p)))
                    .flatten();
                (*p, nickname.map_or_else(|| (*name).into(), Clone::clone))
            })
            .collect(),
        selected: st.eq.as_ref().map_or(EqPreset::FLAT, |e| e.0),
        enabled: st.eq_available && reason.is_none(),
        reason: reason.map(Into::into),
    })
}

fn sidetone(g: &GeneralSetting) -> Option<(u8, bool)> {
    (g.label == "SIDETONE_SETTING").then_some((g.slot, g.on.unwrap_or(false)))
}

fn general_label(label: &str) -> (String, Option<&'static str>) {
    match label {
        "TOUCH_PANEL_SETTING" => (
            strings::TOUCH_CONTROLS.into(),
            Some(strings::TOUCH_CONTROLS_DESC),
        ),
        "MULTIPOINT_SETTING" => (strings::MULTIPOINT.into(), Some(strings::MULTIPOINT_DESC)),
        other => (humanise(other), None),
    }
}

fn on_off(on: Option<bool>) -> String {
    match on {
        Some(true) => strings::VALUE_ON,
        Some(false) => strings::VALUE_OFF,
        None => strings::VALUE_UNKNOWN,
    }
    .into()
}

/// Touch controls, the NC/AMB button, voice guidance, then multipoint and any other read-only slot.
fn info_rows(st: &HeadphonesState) -> Vec<InfoRow> {
    let row = |label: &str, description: &'static str, value: String| InfoRow {
        label: label.into(),
        description: Some(description),
        value,
    };
    let general = |g: &GeneralSetting| {
        let (label, description) = general_label(&g.label);
        InfoRow {
            label,
            description,
            value: on_off(g.on),
        }
    };
    let (touch, rest): (Vec<_>, Vec<_>) = st
        .general_settings
        .iter()
        .filter(|g| sidetone(g).is_none())
        .partition(|g| g.label == "TOUCH_PANEL_SETTING");
    let mut info: Vec<InfoRow> = touch.into_iter().map(general).collect();
    // Only "NC and ambient" (02) has been seen on a real headset.
    info.extend(st.nc_button.map(|b| {
        let v = if b == 2 {
            strings::NC_BUTTON_NC_AMB
        } else {
            strings::VALUE_UNKNOWN
        };
        row(strings::NC_BUTTON, strings::NC_BUTTON_DESC, v.into())
    }));
    info.extend(st.voice_guidance_volume.map(|v| {
        row(
            strings::VOICE_GUIDANCE,
            strings::VOICE_GUIDANCE_DESC,
            strings::voice_guidance(v),
        )
    }));
    info.extend(rest.into_iter().map(general));
    if let Some(d) = &st.device {
        info.push(row(
            strings::INFO_ADDRESS,
            strings::ADDRESS_DESC,
            d.address.clone(),
        ));
    }
    info
}

fn settings_vm(
    st: &HeadphonesState,
    settings: &Settings,
    ready: Option<&Capabilities>,
) -> SettingsVm {
    let (preset, clear_bass, bands) = st
        .eq
        .as_ref()
        .map_or((None, None, Vec::new()), |(p, cb, b)| {
            (Some(*p), *cb, b.clone())
        });
    let probed = ready.map(|c| c.probed).unwrap_or_default();
    let listening_modes: Vec<(ListeningMode, String)> = [
        (true, ListeningMode::Standard, strings::LM_STANDARD),
        (probed.bgm, ListeningMode::Bgm, strings::LM_BGM),
        (probed.cinema, ListeningMode::Cinema, strings::LM_CINEMA),
    ]
    .into_iter()
    .filter(|(ok, ..)| *ok && (probed.bgm || probed.cinema))
    .map(|(_, m, label)| (m, label.into()))
    .collect();
    let listening_mode = (!listening_modes.is_empty()).then(|| {
        if st.bgm.is_some_and(|(on, _)| on) {
            ListeningMode::Bgm
        } else if st.cinema == Some(true) {
            ListeningMode::Cinema
        } else {
            ListeningMode::Standard
        }
    });
    let custom = preset.is_some_and(is_custom);
    let band_labels = match bands.len() {
        5 => BANDS_5.to_vec(),
        10 => BANDS_10.to_vec(),
        _ => Vec::new(),
    };
    // Built-in presets show their curve too, read-only; only custom presets can be edited.
    let show_bands = !band_labels.is_empty() && ready.is_some_and(|c| c.eq_kind.is_some());
    let clear_bass_model = ready.is_some_and(|c| c.has_clear_bass);
    SettingsVm {
        clear_bass,
        bands,
        band_labels,
        band_max: if clear_bass_model { 10 } else { 6 },
        show_bands,
        custom_preset: custom,
        bands_enabled: show_bands && custom && st.eq_available && eq_reason(st).is_none(),
        listening_mode,
        listening_modes,
        room: st.bgm.map(|(_, r)| r),
        auto_power_off: ready.map_or_else(Vec::new, |c| {
            c.auto_power_off
                .iter()
                .map(|a| (*a, auto_power_off_label(*a).into()))
                .collect()
        }),
        auto_power_off_selected: st.auto_power_off,
        sidetone: st.general_settings.iter().find_map(sidetone),
        info: info_rows(st),
        sources: st
            .sources
            .iter()
            .map(|s| SourceVm {
                mac: s.mac.clone(),
                name: s.name.clone(),
                connected: s.connected,
                playing: s.playing,
                status: if s.playing {
                    strings::SOURCE_PLAYING
                } else if s.connected {
                    strings::SOURCE_CONNECTED
                } else {
                    strings::SOURCE_NOT_CONNECTED
                }
                .into(),
                can_switch: s.connected && !s.playing,
            })
            .collect(),
        firmware: st.firmware.clone(),
        codec: st.codec.map(codec_name),
        app: AppSettingsVm {
            pause_on_removal: settings.pause_on_removal,
            resume_on_wear: settings.resume_on_wear,
            battery_in_tooltip: settings.battery_in_tooltip,
            low_battery_notifications: settings.low_battery_notifications,
            theme: settings.theme,
            headset_colour: settings.headset_colour,
        },
    }
}

/// What can be done about the session state. After the app turns the headphones off, Try again
/// still helps once the power button is pressed, rather than waiting out the backoff.
fn recovery(session: &SessionState) -> Recovery {
    match session {
        SessionState::BluetoothOff => Recovery {
            open_bluetooth: true,
            try_again: false,
        },
        SessionState::NotConnected => Recovery {
            open_bluetooth: true,
            try_again: true,
        },
        _ => Recovery::default(),
    }
}

/// `turned_off`: the app turned the headphones off and they have not come back since.
pub fn build(st: &HeadphonesState, settings: &Settings, demo: bool, turned_off: bool) -> PanelVm {
    let caps = match &st.session {
        SessionState::Ready(c) => Some(c),
        _ => None,
    };
    let name = st
        .device
        .as_ref()
        .map_or(strings::DEFAULT_DEVICE_NAME, |d| d.name.as_str());
    // A long name trims, so the demo marker lives in the status line where it stays visible.
    let device_name = name
        .strip_suffix(strings::DEMO_SUFFIX)
        .filter(|_| demo)
        .unwrap_or(name)
        .to_string();
    let status = status(st, turned_off);
    let battery = st.battery.as_ref().map(battery_level);
    let settings_vm = settings_vm(st, settings, caps);
    PanelVm {
        device_name,
        status: if demo {
            strings::status_demo(status)
        } else {
            status.into()
        },
        battery_text: battery.map(|(l, _)| strings::battery(l)),
        battery_fraction: battery.map(|(l, _)| f64::from(l) / 100.0),
        charging: battery.is_some_and(|(_, c)| c),
        // A track kept from before a disconnect is stale, so it shows only while connected.
        now_playing: st.now_playing.as_ref().filter(|_| caps.is_some()).and_then(
            |(title, artist)| match (title.as_deref(), artist.as_deref()) {
                (Some(t), Some(a)) => Some(strings::now_playing(t, a)),
                (Some(x), None) | (None, Some(x)) => Some(x.to_string()),
                (None, None) => None,
            },
        ),
        render: caps.map(|_| RenderChoice {
            colour: st
                .headset_colour
                .or(settings.headset_colour)
                .unwrap_or(HeadsetColour::Black),
            angle: Angle::ThreeQuarter,
        }),
        connected: caps.is_some(),
        recovery: recovery(&st.session),
        tiles: tiles(st, caps),
        ambient: st
            .nc
            .filter(|_| caps.is_some_and(|c| c.nc_amb_kind.is_some()))
            .map(|n| AmbientVm {
                level: n.ambient_level,
                focus_on_voice: n.focus_on_voice,
                enabled: n.mode == NcAmbMode::Ambient,
            }),
        eq: caps.and_then(|c| eq(st, c, settings)),
        effect_suspended: st.effect_suspended,
        low_battery_toast: None,
        settings: settings_vm,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use xm_core::capabilities::{from_support, XM6_FUNCTIONS};
    use xm_core::command::NcAmb;
    use xm_core::model::{BatteryReading, BehaviourPrefs, Model};
    use xm_core::transport::DeviceInfo;

    fn xm6_caps() -> Capabilities {
        let mut c = from_support(&XM6_FUNCTIONS, true);
        c.auto_pause = true;
        c.speak_to_chat = true;
        c.dsee = true;
        c.eq_kind = Some(0x04);
        c
    }

    pub(crate) fn base(session: SessionState) -> HeadphonesState {
        let m = Model::new(
            Box::new(xm_core::media::NullMediaControl),
            BehaviourPrefs {
                pause_on_removal: true,
                resume_on_wear: true,
            },
            std::sync::Arc::new(xm_core::clock::SystemClock),
        );
        HeadphonesState {
            session,
            ..m.state().clone()
        }
    }

    pub(crate) fn ready() -> HeadphonesState {
        HeadphonesState {
            device: Some(DeviceInfo {
                name: "WH-1000XM6".into(),
                address: "AA:BB".into(),
            }),
            nc: Some(NcAmb {
                mode: NcAmbMode::NoiseCancelling,
                ambient_level: 12,
                focus_on_voice: true,
                adaptive: None,
            }),
            battery: Some(BatteryReading::Single {
                level: 57,
                charging: false,
            }),
            wearing: Wearing::Worn,
            ..base(SessionState::Ready(xm6_caps()))
        }
    }

    fn vm(st: &HeadphonesState) -> PanelVm {
        build(st, &Settings::default(), false, false)
    }

    fn tile(v: &PanelVm, k: TileKind) -> &TileVm {
        v.tiles.iter().find(|t| t.kind == k).unwrap()
    }

    #[test]
    fn ready_xm6_vm() {
        let v = vm(&ready());
        assert_eq!(v.device_name, "WH-1000XM6");
        assert_eq!(v.status, "Connected · On your head");
        assert!(v.connected);
        assert!(v.tiles.iter().all(|t| t.visible && t.enabled));
        assert!(tile(&v, TileKind::NoiseCancelling).on);
        assert!(!tile(&v, TileKind::Off).on);
        assert_eq!(v.battery_text.as_deref(), Some("Battery 57 %"));
        assert_eq!(v.battery_fraction, Some(0.57));
    }

    #[test]
    fn ambient_vm_present_in_every_mode_enabled_only_in_ambient() {
        let row = |mode| {
            let mut st = ready();
            st.nc.as_mut().unwrap().mode = mode;
            vm(&st).ambient
        };
        let off = Some(AmbientVm {
            level: 12,
            focus_on_voice: true,
            enabled: false,
        });
        assert_eq!(row(NcAmbMode::NoiseCancelling), off);
        assert_eq!(row(NcAmbMode::Off), off);
        let live = row(NcAmbMode::Ambient).unwrap();
        assert!(live.enabled);
        // A headset without noise control never shows the row.
        let mut caps = xm6_caps();
        caps.nc_amb_kind = None;
        assert_eq!(vm(&with_caps(caps)).ambient, None);
    }

    /// Switching mode never changes which blocks the view model offers, so the window height
    /// stays put (Ruling 22); the panel alone hides the ambient row outside Ambient mode.
    #[test]
    fn view_model_blocks_do_not_change_with_mode() {
        let blocks = |v: &PanelVm| {
            (
                v.ambient.is_some(),
                v.eq.is_some(),
                v.settings.show_bands,
                v.tiles.iter().filter(|t| t.visible).count(),
            )
        };
        let modes = [
            NcAmbMode::NoiseCancelling,
            NcAmbMode::Ambient,
            NcAmbMode::Off,
        ];
        let shapes: Vec<_> = modes
            .iter()
            .map(|m| {
                let mut st = ready();
                st.nc.as_mut().unwrap().mode = *m;
                blocks(&vm(&st))
            })
            .collect();
        assert!(shapes.iter().all(|s| *s == shapes[0]), "{shapes:?}");
        assert_eq!(shapes[0], (true, true, false, 6));
        let gone = blocks(&vm(&base(SessionState::NotConnected)));
        assert_eq!(gone, (false, false, false, 6));
    }

    #[test]
    fn now_playing_shows_only_while_connected() {
        let playing = Some((Some("Lean On".to_string()), Some("Major Lazer".to_string())));
        let mut st = ready();
        st.now_playing = playing.clone();
        assert_eq!(
            vm(&st).now_playing.as_deref(),
            Some("Lean On · Major Lazer")
        );
        for session in [
            SessionState::BluetoothOff,
            SessionState::NotConnected,
            SessionState::Disconnected,
        ] {
            let mut st = base(session);
            st.now_playing = playing.clone();
            assert_eq!(vm(&st).now_playing, None, "{:?}", st.session);
        }
    }

    #[test]
    fn disconnected_vm_disables_controls() {
        for (s, text, bluetooth, retry) in [
            (
                SessionState::Disconnected,
                "Connect the headphones to this PC",
                false,
                false,
            ),
            (
                SessionState::NotConnected,
                "Headphones not found. Are they on?",
                true,
                true,
            ),
            (SessionState::BluetoothOff, "Bluetooth is off", true, false),
            (
                SessionState::InUse,
                "The headphones are in use by another app",
                false,
                false,
            ),
            (SessionState::Connecting, "Connecting…", false, false),
        ] {
            // State kept from before the disconnect must not read as on.
            let st = HeadphonesState {
                session: s,
                auto_pause: Some(true),
                dsee: Some(true),
                ..ready()
            };
            let v = vm(&st);
            assert_eq!(v.status, text);
            assert!(!v.connected);
            assert!(v.tiles.iter().all(|t| !t.enabled && !t.on));
            assert!(v.eq.is_none() && v.ambient.is_none() && v.render.is_none());
            assert_eq!(
                (v.recovery.open_bluetooth, v.recovery.try_again),
                (bluetooth, retry)
            );
        }
    }

    #[test]
    fn turned_off_by_the_app_says_so_until_they_come_back() {
        let off = |s: SessionState| build(&base(s), &Settings::default(), false, true);
        let msg = "Turned off. Press the power button to turn them on again.";
        for s in [
            SessionState::Disconnected,
            SessionState::NotConnected,
            SessionState::BluetoothOff,
        ] {
            assert_eq!(off(s).status, msg);
        }
        // Not found underneath, so Try again and the Bluetooth link still help.
        let v = off(SessionState::NotConnected);
        assert!(v.recovery.try_again && v.recovery.open_bluetooth);
        // While the link is still up, or once it is back, the normal status shows.
        assert_eq!(off(SessionState::Connecting).status, "Connecting…");
        let v = build(&ready(), &Settings::default(), false, true);
        assert_eq!(v.status, "Connected · On your head");
    }

    #[test]
    fn unsupported_feature_hidden() {
        let mut caps = xm6_caps();
        caps.dsee = false;
        let st = HeadphonesState {
            session: SessionState::Ready(caps),
            ..ready()
        };
        let v = vm(&st);
        assert!(!tile(&v, TileKind::Dsee).visible);
        assert!(tile(&v, TileKind::SpeakToChat).visible);
    }

    #[test]
    fn eq_reason_during_call() {
        let st = HeadphonesState {
            in_call: true,
            ..ready()
        };
        let eq = vm(&st).eq.unwrap();
        assert!(!eq.enabled);
        assert_eq!(
            eq.reason.as_deref(),
            Some("The headphones pause the equaliser during calls")
        );
    }

    #[test]
    fn eq_reason_names_the_listening_mode_and_the_fix() {
        let reason = |st: HeadphonesState| {
            let eq = vm(&st).eq.unwrap();
            assert!(!eq.enabled);
            eq.reason.unwrap()
        };
        assert_eq!(
            reason(HeadphonesState {
                bgm: Some((true, BgmRoom::Cafe)),
                sound_effect_unavailable: true,
                ..ready()
            }),
            "Background music is on. Switch to Standard to use the equaliser."
        );
        assert_eq!(
            reason(HeadphonesState {
                cinema: Some(true),
                ..ready()
            }),
            "Cinema is on. Switch to Standard to use the equaliser."
        );
        assert_eq!(
            reason(HeadphonesState {
                sound_effect_unavailable: true,
                ..ready()
            }),
            "A listening mode is on. Switch to Standard to use the equaliser."
        );
        assert_eq!(
            reason(HeadphonesState {
                eq_available: false,
                ..ready()
            }),
            "The headphones have paused the equaliser, as they do during calls."
        );
        let ok = vm(&ready()).eq.unwrap();
        assert!(ok.enabled && ok.reason.is_none());
    }

    #[test]
    fn effect_suspended_keeps_mode() {
        let st = HeadphonesState {
            effect_suspended: true,
            wearing: Wearing::Removed,
            ..ready()
        };
        let v = vm(&st);
        assert_eq!(v.status, "Connected · Off your head");
        let nc = tile(&v, TileKind::NoiseCancelling);
        assert!(nc.on);
        assert_eq!(nc.subtitle.as_deref(), Some("Paused while off your head"));
        assert!(!tile(&v, TileKind::Off).on);
        assert_eq!(tile(&v, TileKind::Off).subtitle, None);
        let mut st = st;
        st.dsee = Some(true);
        let v = vm(&st);
        assert!(tile(&v, TileKind::Dsee).on);
        assert_eq!(tile(&v, TileKind::Dsee).subtitle, None);
    }

    #[test]
    fn render_follows_colour() {
        let colour =
            |st: &HeadphonesState, s: &Settings| build(st, s, false, false).render.unwrap().colour;
        let none = Settings::default();
        let blue = Settings {
            headset_colour: Some(HeadsetColour::MidnightBlue),
            ..Settings::default()
        };
        let silver = HeadphonesState {
            headset_colour: Some(HeadsetColour::PlatinumSilver),
            ..ready()
        };
        assert_eq!(colour(&silver, &blue), HeadsetColour::PlatinumSilver);
        assert_eq!(colour(&ready(), &blue), HeadsetColour::MidnightBlue);
        assert_eq!(colour(&ready(), &none), HeadsetColour::Black);
    }

    fn with_caps(caps: Capabilities) -> HeadphonesState {
        HeadphonesState {
            session: SessionState::Ready(caps),
            ..ready()
        }
    }

    #[test]
    fn apo_choices_from_caps() {
        let apo = |st: &HeadphonesState| vm(st).settings.auto_power_off;
        assert_eq!(
            apo(&ready()),
            vec![
                (AutoPowerOff::WhenTakenOff, "When taken off".to_string()),
                (AutoPowerOff::Never, "Never".to_string()),
            ]
        );
        let timers = apo(&with_caps(from_support(&[0x24], false)));
        let labels: Vec<&str> = timers.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            labels,
            [
                "After 5 minutes",
                "After 30 minutes",
                "After 1 hour",
                "After 3 hours",
                "Never"
            ]
        );
        assert!(apo(&base(SessionState::Disconnected)).is_empty());
    }

    #[test]
    fn listening_mode_hidden_unless_probed() {
        let s = vm(&ready()).settings;
        assert_eq!(s.listening_mode, None);
        assert!(s.listening_modes.is_empty());

        let mut caps = xm6_caps();
        caps.probed.bgm = true;
        let s = vm(&with_caps(caps.clone())).settings;
        assert_eq!(s.listening_mode, Some(ListeningMode::Standard));
        let modes: Vec<ListeningMode> = s.listening_modes.iter().map(|(m, _)| *m).collect();
        assert_eq!(modes, [ListeningMode::Standard, ListeningMode::Bgm]);

        caps.probed.cinema = true;
        let st = HeadphonesState {
            bgm: Some((true, BgmRoom::LivingRoom)),
            ..with_caps(caps)
        };
        let s = vm(&st).settings;
        assert_eq!(s.listening_mode, Some(ListeningMode::Bgm));
        assert_eq!(s.room, Some(BgmRoom::LivingRoom));
        let labels: Vec<&str> = s.listening_modes.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(labels, ["Standard", "Background music", "Cinema"]);
    }

    #[test]
    fn curve_for_every_preset_editable_only_for_custom() {
        let with_preset = |p: EqPreset| HeadphonesState {
            eq: Some((p, None, vec![0; 10])),
            ..ready()
        };
        // A built-in preset shows its curve read-only; only custom presets can be edited.
        for p in [EqPreset::GAMING, EqPreset::HEAVY, EqPreset::FLAT] {
            let s = vm(&with_preset(p)).settings;
            assert!(s.show_bands && !s.bands_enabled && !s.custom_preset);
        }
        for p in [EqPreset::MANUAL, EqPreset::CUSTOM1, EqPreset::CUSTOM2] {
            let s = vm(&with_preset(p)).settings;
            assert!(s.show_bands && s.bands_enabled && s.custom_preset);
            assert_eq!(
                s.band_labels,
                ["31", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"]
            );
            assert_eq!(s.band_max, 6);
        }
        // A listening mode or a call keeps the editor, the bands locked and the reason shown.
        let st = HeadphonesState {
            bgm: Some((true, BgmRoom::Cafe)),
            ..with_preset(EqPreset::MANUAL)
        };
        let v = vm(&st);
        assert!(v.settings.show_bands && !v.settings.bands_enabled);
        assert!(v.eq.unwrap().reason.is_some());
        let st = HeadphonesState {
            in_call: true,
            ..with_preset(EqPreset::MANUAL)
        };
        assert!(!vm(&st).settings.bands_enabled);
        // Labels follow the band count, so a count with no labels shows no editor.
        let st = HeadphonesState {
            eq: Some((EqPreset::MANUAL, None, vec![0; 7])),
            ..ready()
        };
        assert!(!vm(&st).settings.show_bands);
        // Not connected: nothing to edit.
        let st = HeadphonesState {
            eq: Some((EqPreset::MANUAL, None, vec![0; 10])),
            ..base(SessionState::Disconnected)
        };
        assert!(!vm(&st).settings.show_bands);
        // Five bands plus Clear Bass on older models, at ±10.
        let st = HeadphonesState {
            eq: Some((EqPreset::MANUAL, Some(0), vec![0; 5])),
            ..with_caps(from_support(&[0x50], false))
        };
        let s = vm(&st).settings;
        assert_eq!(s.band_labels, ["400", "1k", "2.5k", "6.3k", "16k"]);
        assert_eq!(s.band_max, 10);
        assert_eq!(s.clear_bass, Some(0));
    }

    #[test]
    fn sources_rows() {
        let src = |name: &str, connected, playing| xm_core::event::SourceDevice {
            mac: format!("{name}-mac"),
            name: name.into(),
            connected,
            playing,
        };
        let st = HeadphonesState {
            sources: vec![
                src("Desk", true, true),
                src("Phone", true, false),
                src("Laptop", false, false),
            ],
            ..ready()
        };
        let rows: Vec<(String, String, bool)> = vm(&st)
            .settings
            .sources
            .into_iter()
            .map(|s| (s.name, s.status, s.can_switch))
            .collect();
        assert_eq!(
            rows,
            [
                ("Desk".into(), "Playing".into(), false),
                ("Phone".into(), "Connected".into(), true),
                ("Laptop".into(), "Not connected".into(), false),
            ]
        );
    }

    #[test]
    fn read_only_rows() {
        let g = |slot, label: &str, on| GeneralSetting {
            slot,
            label: label.into(),
            on: Some(on),
        };
        let st = HeadphonesState {
            general_settings: vec![
                g(1, "SIDETONE_SETTING", true),
                g(2, "MULTIPOINT_SETTING", true),
                g(3, "TOUCH_PANEL_SETTING", false),
            ],
            nc_button: Some(2),
            voice_guidance_volume: Some(-1),
            ..ready()
        };
        let s = vm(&st).settings;
        assert_eq!(s.sidetone, Some((1, true)));
        let rows: Vec<(&str, &str)> = s
            .info
            .iter()
            .map(|r| (r.label.as_str(), r.value.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("Touch controls", "Off"),
                ("Noise control button", "Noise cancelling ↔ Ambient"),
                ("Voice guidance volume", "-1"),
                ("Multipoint", "On"),
                ("Bluetooth address", "AA:BB"),
            ]
        );
    }

    #[test]
    fn custom_preset_nickname_replaces_label() {
        let mut s = Settings::default();
        s.eq_names.insert("A1".into(), "Desk".into());
        // Fixed presets keep the headset's name even if a nickname is somehow saved.
        s.eq_names.insert("20".into(), "Nope".into());
        let eq = build(&ready(), &s, false, false).eq.unwrap();
        let label = |p: EqPreset| {
            eq.presets
                .iter()
                .find(|(q, _)| *q == p)
                .map(|(_, l)| l.as_str())
        };
        assert_eq!(label(EqPreset::CUSTOM1), Some("Desk"));
        assert_eq!(label(EqPreset::CUSTOM2), Some("Custom 2"));
        assert_eq!(label(EqPreset::GAMING), Some("Gaming"));
        assert_eq!(eq_name_key(EqPreset::CUSTOM1), "A1");
        assert!(is_custom(EqPreset::MANUAL) && !is_custom(EqPreset::FLAT));
        assert_eq!(preset_name(EqPreset::CUSTOM1), Some("Custom 1"));
        assert_eq!(preset_name(EqPreset(0x77)), None);
    }

    #[test]
    fn demo_marker_is_in_the_status_line() {
        let s = Settings::default();
        let mut st = ready();
        st.device.as_mut().unwrap().name = "WH-1000XM6 (demo)".into();
        let v = build(&st, &s, true, false);
        assert_eq!(v.device_name, "WH-1000XM6");
        assert_eq!(v.status, "Connected · On your head · demo");
        let v = build(&base(SessionState::Disconnected), &s, true, false);
        assert_eq!(v.status, "Connect the headphones to this PC · demo");
        // Outside the demo a real name ending in "(demo)" is left alone.
        assert_eq!(
            build(&st, &s, false, false).device_name,
            "WH-1000XM6 (demo)"
        );
    }
}
