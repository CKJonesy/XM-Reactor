//! Headphones model: the last known device state plus the rules for changing it.
//! Pure: events and intents in, effects out; the caller sends commands and drives `on_tick`.

use crate::capabilities::{allows, Capabilities};
use crate::clock::Clock;
use crate::command::{self, AutoPowerOff, BgmRoom, Command, EqPreset, NcAmb, NcAmbMode};
use crate::event::{Codec, DeviceEvent, HeadsetColourCode, SourceDevice, Wearing};
use crate::media::MediaControl;
use crate::runner::SessionEvent;
use crate::session::{Outcome, SessionState};
use crate::settings::{EqCurves, HeadsetColour, PresetCurve};
use crate::transport::DeviceInfo;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(250);
const ADAPTIVE_WAIT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatteryReading {
    Single {
        level: u8,
        charging: bool,
    },
    LeftRight {
        left: u8,
        left_charging: bool,
        right: u8,
        right_charging: bool,
        case: Option<(u8, bool)>,
    },
}

/// A general setting slot (1..=4), its label, and its value once read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneralSetting {
    pub slot: u8,
    pub label: String,
    pub on: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeadphonesState {
    pub session: SessionState,
    pub device: Option<DeviceInfo>,
    pub caps: Option<Capabilities>,
    pub nc: Option<NcAmb>,
    pub eq_available: bool,
    pub eq: Option<(EqPreset, Option<i8>, Vec<i8>)>,
    pub battery: Option<BatteryReading>,
    pub speak_to_chat: Option<bool>,
    pub talking: bool,
    pub auto_pause: Option<bool>,
    pub dsee: Option<bool>,
    pub auto_power_off: Option<AutoPowerOff>,
    pub wearing: Wearing,
    pub in_call: bool,
    pub firmware: Option<String>,
    pub codec: Option<Codec>,
    pub now_playing: Option<(Option<String>, Option<String>)>,
    pub bgm: Option<(bool, BgmRoom)>,
    pub cinema: Option<bool>,
    pub sources: Vec<SourceDevice>,
    pub adaptive_volume: Option<bool>,
    /// The reported colour, when it is one the app has a picture for.
    pub headset_colour: Option<HeadsetColour>,
    pub nc_button: Option<u8>,
    pub voice_guidance_volume: Option<i8>,
    pub general_settings: Vec<GeneralSetting>,
    pub sound_effect_unavailable: bool,
    pub auto_ambient: Option<bool>,
    /// The headset has switched NC/ambient off because it is off the head; `nc` keeps the user's mode.
    pub effect_suspended: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    SetMode(NcAmbMode),
    SetAmbientLevel(u8),
    SetFocusOnVoice(bool),
    SetEqPreset(EqPreset),
    SetEqBands(Option<i8>, Vec<i8>),
    SetSpeakToChat(bool),
    SetAutoPause(bool),
    SetDsee(bool),
    SetAutoPowerOff(AutoPowerOff),
    SetBgm(bool, BgmRoom),
    SetCinema(bool),
    SwitchSource(String),
    SetAdaptiveVolume(bool),
    SetGeneralSetting(u8, bool),
    PowerOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    Pause,
    Play,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Send(Command),
    Media(MediaAction),
    StateChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BehaviourPrefs {
    pub pause_on_removal: bool,
    pub resume_on_wear: bool,
}

struct PendingEq {
    preset: EqPreset,
    clear_bass: Option<i8>,
    bands: Vec<i8>,
    at: Instant,
}

pub struct Model {
    media: Box<dyn MediaControl>,
    prefs: BehaviourPrefs,
    clock: Arc<dyn Clock>,
    st: HeadphonesState,
    paused_by_us: bool,
    pending_ambient: Option<(u8, Instant)>,
    pending_eq: Option<PendingEq>,
    /// Each preset's last reported bands, so switching shows the right curve before the reply.
    eq_curves: EqCurves,
    /// An NC write waiting for the adaptive bytes it must echo, and when it started waiting.
    held_nc: Option<(NcAmb, Instant)>,
    adaptive_seen: Option<(u8, u8)>,
    in_flight: HashMap<u64, &'static str>,
    /// An NC Off that arrived before wearing was known, with the mode it replaced: if the headset
    /// turns out to be off the head, the Off was a suspension and that mode comes back.
    unsure_off: Option<Option<NcAmb>>,
}

impl Model {
    pub fn new(media: Box<dyn MediaControl>, prefs: BehaviourPrefs, clock: Arc<dyn Clock>) -> Self {
        Self {
            media,
            prefs,
            clock,
            st: HeadphonesState {
                session: SessionState::Disconnected,
                device: None,
                caps: None,
                nc: None,
                eq_available: true,
                eq: None,
                battery: None,
                speak_to_chat: None,
                talking: false,
                auto_pause: None,
                dsee: None,
                auto_power_off: None,
                wearing: Wearing::Unknown,
                in_call: false,
                firmware: None,
                codec: None,
                now_playing: None,
                bgm: None,
                cinema: None,
                sources: Vec::new(),
                adaptive_volume: None,
                headset_colour: None,
                nc_button: None,
                voice_guidance_volume: None,
                general_settings: Vec::new(),
                sound_effect_unavailable: false,
                auto_ambient: None,
                effect_suspended: false,
            },
            paused_by_us: false,
            pending_ambient: None,
            pending_eq: None,
            eq_curves: EqCurves::new(),
            held_nc: None,
            adaptive_seen: None,
            in_flight: HashMap::new(),
            unsure_off: None,
        }
    }

    pub fn state(&self) -> &HeadphonesState {
        &self.st
    }

    /// The remembered curves, loaded from the settings at start-up.
    pub fn set_eq_curves(&mut self, curves: EqCurves) {
        self.eq_curves = curves;
    }

    /// The remembered curves, for the app to save when they change.
    pub fn eq_curves(&self) -> &EqCurves {
        &self.eq_curves
    }

    /// The connected headset's latest remembered bands for preset `p`.
    fn eq_curve(&self, p: EqPreset) -> Option<&Vec<i8>> {
        let name = &self.st.device.as_ref()?.name;
        Some(
            &self
                .eq_curves
                .get(name)?
                .get(&format!("{:02X}", p.0))?
                .latest,
        )
    }

    pub fn set_prefs(&mut self, prefs: BehaviourPrefs) {
        self.prefs = prefs;
    }

    /// Records the id the session gave a command this model emitted, so a failure can re-read it.
    pub fn on_sent(&mut self, id: u64, cmd_name: &'static str) {
        self.in_flight.insert(id, cmd_name);
    }

    pub fn on_event(&mut self, e: SessionEvent) -> Vec<Effect> {
        let before = self.st.clone();
        let mut fx = Vec::new();
        match e {
            SessionEvent::State(s) => self.on_state(s),
            SessionEvent::Connected(info) => self.on_connected(info),
            SessionEvent::Outcome(o) => fx.extend(self.on_outcome(o).into_iter().map(Effect::Send)),
            SessionEvent::Device(d) => self.on_device(d, &mut fx),
        }
        self.finish(before, fx)
    }

    pub fn apply(&mut self, i: Intent) -> Vec<Effect> {
        if !self.is_ready() {
            return Vec::new();
        }
        let before = self.st.clone();
        let send = self.intent(i);
        self.finish(before, send.into_iter().map(Effect::Send).collect())
    }

    /// Flushes debounced writes that have been quiet long enough and gives up waiting on adaptive bytes.
    pub fn on_tick(&mut self) -> Vec<Effect> {
        if !self.is_ready() {
            return Vec::new();
        }
        let before = self.st.clone();
        let now = self.clock.now();
        let mut out = Vec::new();
        if self
            .pending_ambient
            .is_some_and(|(_, t)| now.duration_since(t) >= DEBOUNCE)
        {
            if let (Some((level, _)), Some(mut nc)) = (self.pending_ambient.take(), self.st.nc) {
                nc.ambient_level = level;
                self.st.nc = Some(nc);
                out.extend(self.nc_write(nc));
            }
        }
        if self
            .pending_eq
            .as_ref()
            .is_some_and(|p| now.duration_since(p.at) >= DEBOUNCE)
        {
            if let (Some(p), Some(kind)) = (self.pending_eq.take(), self.eq_kind()) {
                out.extend(self.permitted(command::set_eq_bands(
                    kind,
                    p.preset,
                    p.clear_bass,
                    &p.bands,
                )));
            }
        }
        if self
            .held_nc
            .as_ref()
            .is_some_and(|(_, t)| now.duration_since(*t) >= ADAPTIVE_WAIT)
        {
            if let (Some((nc, _)), Some(kind)) = (self.held_nc.take(), self.nc_kind()) {
                out.extend(self.permitted(command::set_nc_amb(
                    kind,
                    &NcAmb {
                        adaptive: None,
                        ..nc
                    },
                )));
            }
        }
        self.finish(before, out.into_iter().map(Effect::Send).collect())
    }

    fn finish(&self, before: HeadphonesState, mut fx: Vec<Effect>) -> Vec<Effect> {
        if self.st != before {
            fx.push(Effect::StateChanged);
        }
        fx
    }

    fn is_ready(&self) -> bool {
        matches!(self.st.session, SessionState::Ready(_))
    }

    fn nc_kind(&self) -> Option<u8> {
        self.st.caps.as_ref()?.nc_amb_kind
    }

    fn eq_kind(&self) -> Option<u8> {
        self.st.caps.as_ref()?.eq_kind
    }

    fn permitted(&self, c: Command) -> Option<Command> {
        self.st
            .caps
            .as_ref()
            .is_some_and(|caps| allows(caps, &c))
            .then_some(c)
    }

    fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Builds the NC write. Type 0x19 echoes the adaptive bytes from the last reply, so with none
    /// seen it asks for them first and holds the write.
    fn nc_write(&mut self, mut nc: NcAmb) -> Option<Command> {
        let kind = self.nc_kind()?;
        if kind == 0x19 {
            match self.adaptive_seen {
                Some(a) => nc.adaptive = Some(a),
                None => {
                    let first = self.held_nc.is_none();
                    let since = self.held_nc.map_or_else(|| self.now(), |(_, t)| t);
                    self.held_nc = Some((nc, since));
                    return first.then(|| command::get_nc_amb(kind));
                }
            }
        }
        self.permitted(command::set_nc_amb(kind, &nc))
    }

    fn nc_edit(&mut self, write: bool, f: impl FnOnce(&mut NcAmb)) -> Option<Command> {
        self.nc_kind()?;
        let mut nc = self.st.nc.unwrap_or(NcAmb {
            mode: NcAmbMode::NoiseCancelling,
            ambient_level: 0,
            focus_on_voice: false,
            adaptive: None,
        });
        f(&mut nc);
        self.st.nc = Some(nc);
        write.then(|| self.nc_write(nc)).flatten()
    }

    fn intent(&mut self, i: Intent) -> Option<Command> {
        match i {
            Intent::SetMode(m) => self.nc_edit(true, |nc| nc.mode = m),
            Intent::SetFocusOnVoice(v) => self.nc_edit(true, |nc| nc.focus_on_voice = v),
            Intent::SetAmbientLevel(l) => {
                let sent = self.nc_edit(false, |nc| nc.ambient_level = l.min(20));
                // The flush on on_tick writes; nothing goes out now.
                if self.nc_kind().is_some() {
                    self.pending_ambient = Some((l.min(20), self.now()));
                }
                sent
            }
            Intent::SetEqPreset(p) => {
                let kind = self.eq_kind().filter(|_| self.st.eq_available)?;
                let c = self.permitted(command::set_eq_preset(kind, p))?;
                // The preset replaces any band drag still waiting to be written.
                self.pending_eq = None;
                let (cb, bands) = self
                    .st
                    .eq
                    .clone()
                    .map_or((None, Vec::new()), |(_, c, b)| (c, b));
                // The preset's remembered curve shows at once; the headset's reply has the last word.
                let bands = self
                    .eq_curve(p)
                    .filter(|c| bands.is_empty() || c.len() == bands.len())
                    .cloned()
                    .unwrap_or(bands);
                self.st.eq = Some((p, cb, bands));
                Some(c)
            }
            Intent::SetEqBands(cb, bands) => {
                let kind = self.eq_kind().filter(|_| self.st.eq_available)?;
                let caps = self.st.caps.as_ref()?;
                let fits = if caps.has_clear_bass {
                    cb.is_some() && bands.len() == 5
                } else {
                    cb.is_none() && bands.len() == usize::from(caps.eq_bands)
                };
                self.permitted(command::set_eq_bands(kind, EqPreset::MANUAL, cb, &bands))
                    .filter(|_| fits)?;
                let keep = self.st.eq.as_ref().map(|e| e.0);
                let preset = keep
                    .filter(|p| *p == EqPreset::CUSTOM1 || *p == EqPreset::CUSTOM2)
                    .unwrap_or(EqPreset::MANUAL);
                self.st.eq = Some((preset, cb, bands.clone()));
                self.pending_eq = Some(PendingEq {
                    preset,
                    clear_bass: cb,
                    bands,
                    at: self.now(),
                });
                None
            }
            Intent::SetSpeakToChat(v) => {
                let c = self.permitted(command::set_speak_to_chat(v))?;
                self.st.speak_to_chat = Some(v);
                Some(c)
            }
            Intent::SetAutoPause(v) => {
                let c = self.permitted(command::set_auto_pause(v))?;
                self.st.auto_pause = Some(v);
                Some(c)
            }
            Intent::SetDsee(v) => {
                let c = self.permitted(command::set_dsee(v))?;
                self.st.dsee = Some(v);
                Some(c)
            }
            Intent::SetAutoPowerOff(v) => {
                let c = self.permitted(command::set_auto_power_off(v))?;
                self.st.auto_power_off = Some(v);
                Some(c)
            }
            Intent::SetBgm(on, room) => {
                let c = self.permitted(command::set_bgm(on, room))?;
                self.st.bgm = Some((on, room));
                Some(c)
            }
            Intent::SetCinema(v) => {
                let c = self.permitted(command::set_cinema(v))?;
                self.st.cinema = Some(v);
                Some(c)
            }
            Intent::SwitchSource(mac) => self.permitted(command::switch_source(&mac)?),
            Intent::SetAdaptiveVolume(v) => {
                let c = self.permitted(command::set_adaptive_volume(v))?;
                self.st.adaptive_volume = Some(v);
                Some(c)
            }
            Intent::SetGeneralSetting(slot, on) => {
                let c = self.permitted(command::set_general_setting(slot, on))?;
                if let Some(g) = self.st.general_settings.iter_mut().find(|g| g.slot == slot) {
                    g.on = Some(on);
                }
                Some(c)
            }
            Intent::PowerOff => self.permitted(command::power_off()),
        }
    }

    /// A different headset must not inherit the last one's readings.
    fn on_connected(&mut self, info: DeviceInfo) {
        if self
            .st
            .device
            .as_ref()
            .is_some_and(|d| d.address != info.address)
        {
            let st = &mut self.st;
            st.firmware = None;
            st.codec = None;
            st.sources.clear();
            st.bgm = None;
            st.cinema = None;
            st.adaptive_volume = None;
            st.battery = None;
            st.now_playing = None;
            st.eq = None;
            st.nc = None;
            st.headset_colour = None;
            st.nc_button = None;
            st.voice_guidance_volume = None;
            st.general_settings.clear();
            st.sound_effect_unavailable = false;
            st.auto_ambient = None;
            self.pending_eq = None;
            self.pending_ambient = None;
            self.held_nc = None;
            self.adaptive_seen = None;
        }
        self.st.device = Some(info);
    }

    fn on_state(&mut self, s: SessionState) {
        match &s {
            SessionState::Ready(caps) => {
                self.st.caps = Some(caps.clone());
                self.st.eq_available = true;
                self.st.in_call = false;
                self.adaptive_seen = None;
            }
            SessionState::Connecting => {}
            _ => {
                self.pending_ambient = None;
                self.paused_by_us = false;
                self.pending_eq = None;
                self.held_nc = None;
                self.adaptive_seen = None;
                self.in_flight.clear();
                self.unsure_off = None;
                // A stale Worn would read as a removal when the headset comes back.
                self.st.wearing = Wearing::Unknown;
                self.st.effect_suspended = false;
            }
        }
        self.st.session = s;
    }

    /// A failed write re-reads the feature it touched; anything else reads nothing.
    fn on_outcome(&mut self, o: Outcome) -> Vec<Command> {
        match o {
            Outcome::Done(id) => {
                self.in_flight.remove(&id);
                Vec::new()
            }
            Outcome::Failed(id, _) if !self.is_ready() => {
                self.in_flight.remove(&id);
                Vec::new()
            }
            Outcome::Failed(id, _) => {
                let name = self.in_flight.remove(&id);
                let Some((name, caps)) = name.zip(self.st.caps.as_ref()) else {
                    return Vec::new();
                };
                // The write does not say which slot it was, so every learnt slot is read again.
                if name == "set_general_setting" {
                    return caps
                        .general_settings
                        .iter()
                        .filter_map(|(slot, _)| self.permitted(command::get_general_setting(*slot)))
                        .collect();
                }
                let read = match name {
                    "set_nc_amb" => caps.nc_amb_kind.map(command::get_nc_amb),
                    "set_eq_preset" | "set_eq_bands" => caps.eq_kind.map(command::get_eq),
                    "set_speak_to_chat" => Some(command::get_speak_to_chat()),
                    "set_auto_pause" => Some(command::get_auto_pause()),
                    "set_dsee" => Some(command::get_dsee()),
                    "set_auto_power_off" => Some(command::get_auto_power_off()),
                    "set_bgm" => Some(command::get_bgm()),
                    "set_cinema" => Some(command::get_cinema()),
                    "set_adaptive_volume" => Some(command::get_adaptive_volume()),
                    "switch_source" => Some(command::get_device_list()),
                    _ => None,
                };
                read.and_then(|r| self.permitted(r)).into_iter().collect()
            }
        }
    }

    fn on_device(&mut self, d: DeviceEvent, fx: &mut Vec<Effect>) {
        match d {
            DeviceEvent::NcAmb {
                mode,
                ambient_level,
                focus_on_voice,
                adaptive,
            } => {
                if adaptive.is_some() {
                    self.adaptive_seen = adaptive;
                    self.st.auto_ambient = adaptive.map(|(on, _)| on == 1);
                }
                // Off the head the headset reports effect off; that is not the user choosing Off.
                let off = mode == NcAmbMode::Off;
                let users_mode_on = self.st.nc.is_none_or(|n| n.mode != NcAmbMode::Off);
                self.st.effect_suspended =
                    off && self.st.wearing == Wearing::Removed && users_mode_on;
                self.unsure_off =
                    (off && self.st.wearing == Wearing::Unknown).then_some(self.st.nc);
                if self.st.effect_suspended && self.held_nc.is_none() {
                    return;
                }
                if let (Some((held, _)), Some(a)) = (self.held_nc.take(), adaptive) {
                    // The user's pending choice stays on screen; the reply only supplies the bytes.
                    fx.extend(
                        self.nc_write(NcAmb {
                            adaptive: Some(a),
                            ..held
                        })
                        .map(Effect::Send),
                    );
                } else {
                    // A slider drag still waiting to be sent outranks the device's older level.
                    let ambient_level = self.pending_ambient.map_or(ambient_level, |(l, _)| l);
                    self.st.nc = Some(NcAmb {
                        mode,
                        ambient_level,
                        focus_on_voice,
                        adaptive,
                    });
                }
            }
            DeviceEvent::EqStatus { available } => self.st.eq_available = available,
            DeviceEvent::Eq {
                preset,
                clear_bass,
                bands,
            } => {
                // A band write still waiting to be sent outranks the headset's older values (an
                // echo mid-drag would snap the point back); a different preset means the headset
                // moved on, so the write is dropped.
                self.st.eq = match &self.pending_eq {
                    Some(p) if p.preset == preset => Some((preset, p.clear_bass, p.bands.clone())),
                    _ => {
                        self.pending_eq = None;
                        if let Some(d) = self.st.device.as_ref().filter(|_| !bands.is_empty()) {
                            // The first report stays as the default; later ones update latest.
                            self.eq_curves
                                .entry(d.name.clone())
                                .or_default()
                                .entry(format!("{:02X}", preset.0))
                                .or_insert_with(|| PresetCurve {
                                    default: bands.clone(),
                                    latest: bands.clone(),
                                })
                                .latest = bands.clone();
                        }
                        Some((preset, clear_bass, bands))
                    }
                };
            }
            DeviceEvent::Battery { level, charging } => {
                self.st.battery = Some(BatteryReading::Single { level, charging });
            }
            DeviceEvent::LeftRightBattery {
                left,
                left_charging,
                right,
                right_charging,
            } => {
                let case = match self.st.battery {
                    Some(BatteryReading::LeftRight { case, .. }) => case,
                    _ => None,
                };
                self.st.battery = Some(BatteryReading::LeftRight {
                    left,
                    left_charging,
                    right,
                    right_charging,
                    case,
                });
            }
            DeviceEvent::CradleBattery { level, charging } => {
                if let Some(BatteryReading::LeftRight { case, .. }) = &mut self.st.battery {
                    *case = Some((level, charging));
                }
            }
            DeviceEvent::SpeakToChat { enabled } => self.st.speak_to_chat = Some(enabled),
            DeviceEvent::SpeakToChatTalking { active } => self.st.talking = active,
            DeviceEvent::AutoPause { enabled } => self.st.auto_pause = Some(enabled),
            DeviceEvent::Dsee { enabled } => self.st.dsee = Some(enabled),
            DeviceEvent::AutoPowerOff { setting, .. } => self.st.auto_power_off = Some(setting),
            DeviceEvent::Wearing(w) => self.on_wearing(w, fx),
            DeviceEvent::CallActive(active) => {
                self.st.in_call = active;
                if active {
                    self.st.eq_available = false;
                }
            }
            DeviceEvent::Firmware(v) => self.st.firmware = Some(v),
            DeviceEvent::Codec(c) => self.st.codec = Some(c),
            DeviceEvent::NowPlaying { title, artist } => {
                self.st.now_playing = Some((title, artist))
            }
            DeviceEvent::Bgm { enabled, room } => self.st.bgm = Some((enabled, room)),
            DeviceEvent::Cinema { enabled } => self.st.cinema = Some(enabled),
            DeviceEvent::DeviceList(v) => self.st.sources = v,
            DeviceEvent::AdaptiveVolume { enabled } => self.st.adaptive_volume = Some(enabled),
            DeviceEvent::Colour { colour, .. } => {
                self.st.headset_colour = match colour {
                    HeadsetColourCode::Silver => Some(HeadsetColour::PlatinumSilver),
                    HeadsetColourCode::Blue => Some(HeadsetColour::MidnightBlue),
                    HeadsetColourCode::Black => Some(HeadsetColour::Black),
                    _ => None,
                }
            }
            DeviceEvent::NcButton(v) => self.st.nc_button = Some(v),
            DeviceEvent::VoiceGuidanceVolume(v) => self.st.voice_guidance_volume = Some(v),
            DeviceEvent::GeneralSettingInfo { slot, label } => {
                if let Some(caps) = &mut self.st.caps {
                    caps.learn_general_setting(slot, &label);
                }
                match self.st.general_settings.iter_mut().find(|g| g.slot == slot) {
                    Some(g) => g.label = label,
                    None => self.st.general_settings.push(GeneralSetting {
                        slot,
                        label,
                        on: None,
                    }),
                }
            }
            DeviceEvent::GeneralSetting { slot, on } => {
                match self.st.general_settings.iter_mut().find(|g| g.slot == slot) {
                    Some(g) => g.on = Some(on),
                    None => self.st.general_settings.push(GeneralSetting {
                        slot,
                        label: String::new(),
                        on: Some(on),
                    }),
                }
            }
            DeviceEvent::SoundEffectUnavailable(v) => self.st.sound_effect_unavailable = v,
            DeviceEvent::ProtocolInfo { .. } | DeviceEvent::SupportFunctions(_) => {}
        }
    }

    fn on_wearing(&mut self, w: Wearing, fx: &mut Vec<Effect>) {
        let prev = std::mem::replace(&mut self.st.wearing, w);
        let unsure = self.unsure_off.take();
        if let (Wearing::Removed, Some(before)) = (w, unsure) {
            self.st.effect_suspended = before.is_none_or(|n| n.mode != NcAmbMode::Off);
            if self.st.effect_suspended {
                self.st.nc = before;
            }
        }
        let ready = self.is_ready();
        // The headset's own auto-pause setting is the one truth: when it is off, so is ours.
        let headset_allows = self.st.auto_pause != Some(false);
        match (prev, w) {
            (Wearing::Worn, Wearing::Removed)
                if ready && headset_allows && self.prefs.pause_on_removal =>
            {
                self.paused_by_us = self.media.pause();
                if self.paused_by_us {
                    fx.push(Effect::Media(MediaAction::Pause));
                }
            }
            (Wearing::Removed, Wearing::Worn) => {
                self.st.effect_suspended = false;
                let ours = std::mem::take(&mut self.paused_by_us);
                if ours && headset_allows && self.prefs.resume_on_wear && self.media.play() {
                    fx.push(Effect::Media(MediaAction::Play));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::from_support;
    use crate::clock::FakeClock;
    use crate::command::{self, NcAmbMode};
    use crate::event::DeviceEvent;
    use crate::session::{Outcome, SendError};
    use std::sync::Mutex;
    use std::time::Duration;

    const XM6: [u8; 45] = [
        0x10, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x19, 0x20, 0x23, 0x25, 0x27, 0x34, 0x41, 0x42,
        0x44, 0x56, 0x57, 0x69, 0x6D, 0x71, 0x90, 0x94, 0xA1, 0xB0, 0xB2, 0xB6, 0xB7, 0xB8, 0xC1,
        0xC2, 0xD1, 0xD2, 0xD3, 0xE2, 0xE5, 0xE6, 0xE7, 0xEB, 0xF1, 0xF4, 0xF9, 0xFC, 0xFD, 0xFF,
    ];

    #[derive(Default)]
    struct FakeMedia {
        playing: Mutex<bool>,
        calls: Mutex<Vec<&'static str>>,
    }

    struct Shared(Arc<FakeMedia>);

    impl MediaControl for Shared {
        fn pause(&self) -> bool {
            self.0.calls.lock().unwrap().push("pause");
            std::mem::take(&mut *self.0.playing.lock().unwrap())
        }
        fn play(&self) -> bool {
            self.0.calls.lock().unwrap().push("play");
            *self.0.playing.lock().unwrap() = true;
            true
        }
    }

    struct Rig {
        m: Model,
        clock: Arc<FakeClock>,
        media: Arc<FakeMedia>,
    }

    fn caps() -> Capabilities {
        from_support(&XM6, true)
    }

    fn rig_with(caps: Capabilities, prefs: BehaviourPrefs) -> Rig {
        let clock = Arc::new(FakeClock::new());
        let media = Arc::new(FakeMedia::default());
        let mut m = Model::new(Box::new(Shared(media.clone())), prefs, clock.clone());
        m.on_event(SessionEvent::State(SessionState::Ready(caps)));
        Rig { m, clock, media }
    }

    fn prefs() -> BehaviourPrefs {
        BehaviourPrefs {
            pause_on_removal: true,
            resume_on_wear: true,
        }
    }

    fn nc_reply(m: &mut Model) {
        m.on_event(SessionEvent::Device(DeviceEvent::NcAmb {
            mode: NcAmbMode::Off,
            ambient_level: 3,
            focus_on_voice: false,
            adaptive: Some((1, 2)),
        }));
    }

    /// A rig whose adaptive bytes are already known, so NC writes go straight out.
    fn rig() -> Rig {
        let mut r = rig_with(caps(), prefs());
        nc_reply(&mut r.m);
        r
    }

    fn sends(fx: &[Effect]) -> Vec<Command> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::Send(c) => Some(c.clone()),
                _ => None,
            })
            .collect()
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn wear(m: &mut Model, w: Wearing) -> Vec<Effect> {
        m.on_event(SessionEvent::Device(DeviceEvent::Wearing(w)))
    }

    #[test]
    fn optimistic_mode_change_sends_once() {
        let mut r = rig();
        let fx = r.m.apply(Intent::SetMode(NcAmbMode::Ambient));
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Ambient);
        let s = sends(&fx);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "set_nc_amb");
        assert_eq!(s[0].payload, vec![0x68, 0x19, 0x01, 1, 1, 0, 3, 1, 2]);
        assert!(fx.contains(&Effect::StateChanged));
    }

    #[test]
    fn ambient_level_debounced_250ms() {
        let mut r = rig();
        for v in [5, 9, 14] {
            assert!(sends(&r.m.apply(Intent::SetAmbientLevel(v))).is_empty());
            r.clock.advance(ms(70));
        }
        assert!(sends(&r.m.on_tick()).is_empty());
        r.clock.advance(ms(200));
        let s = sends(&r.m.on_tick());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].payload[6], 14);
        assert!(sends(&r.m.on_tick()).is_empty());
    }

    #[test]
    fn failed_write_rereads() {
        let mut r = rig();
        r.m.apply(Intent::SetMode(NcAmbMode::Ambient));
        r.m.on_sent(7, "set_nc_amb");
        let fx = r.m.on_event(SessionEvent::Outcome(Outcome::Failed(
            7,
            SendError::Timeout,
        )));
        assert_eq!(sends(&fx), vec![command::get_nc_amb(0x19)]);
        // Unknown ids and successes read nothing.
        let fx = r.m.on_event(SessionEvent::Outcome(Outcome::Failed(
            7,
            SendError::Timeout,
        )));
        assert!(sends(&fx).is_empty());
        r.m.on_sent(8, "set_dsee");
        let fx = r.m.on_event(SessionEvent::Outcome(Outcome::Done(8)));
        assert!(sends(&fx).is_empty());
    }

    #[test]
    fn pending_write_dropped_on_disconnect() {
        let mut r = rig();
        r.m.apply(Intent::SetAmbientLevel(9));
        r.m.on_sent(1, "set_nc_amb");
        r.m.on_event(SessionEvent::State(SessionState::Disconnected));
        assert_eq!(r.m.state().session, SessionState::Disconnected);
        assert_eq!(r.m.state().nc.unwrap().ambient_level, 9);
        r.clock.advance(ms(500));
        assert!(sends(&r.m.on_tick()).is_empty());
        let fx = r.m.on_event(SessionEvent::Outcome(Outcome::Failed(
            1,
            SendError::Disconnected,
        )));
        assert!(sends(&fx).is_empty());
    }

    #[test]
    fn removed_pauses_when_enabled() {
        let mut r = rig();
        *r.media.playing.lock().unwrap() = true;
        wear(&mut r.m, Wearing::Worn);
        let fx = wear(&mut r.m, Wearing::Removed);
        assert!(fx.contains(&Effect::Media(MediaAction::Pause)));
        let fx = wear(&mut r.m, Wearing::Worn);
        assert!(fx.contains(&Effect::Media(MediaAction::Play)));
        assert_eq!(*r.media.calls.lock().unwrap(), vec!["pause", "play"]);
    }

    #[test]
    fn removal_does_nothing_when_disabled() {
        let mut r = rig();
        r.m.set_prefs(BehaviourPrefs {
            pause_on_removal: false,
            resume_on_wear: true,
        });
        *r.media.playing.lock().unwrap() = true;
        wear(&mut r.m, Wearing::Worn);
        assert!(wear(&mut r.m, Wearing::Removed)
            .iter()
            .all(|e| !matches!(e, Effect::Media(_))));
        assert!(r.media.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn headset_auto_pause_off_overrides_the_app_prefs() {
        let mut r = rig();
        r.m.st.auto_pause = Some(false);
        *r.media.playing.lock().unwrap() = true;
        wear(&mut r.m, Wearing::Worn);
        assert!(wear(&mut r.m, Wearing::Removed)
            .iter()
            .all(|e| !matches!(e, Effect::Media(_))));
        assert!(r.media.calls.lock().unwrap().is_empty());
        // Unknown (not yet reported) still follows the prefs.
        r.m.st.auto_pause = None;
        wear(&mut r.m, Wearing::Worn);
        assert!(wear(&mut r.m, Wearing::Removed).contains(&Effect::Media(MediaAction::Pause)));
        // Turned off while they are off the head: no resume either.
        r.m.st.auto_pause = Some(false);
        assert!(wear(&mut r.m, Wearing::Worn)
            .iter()
            .all(|e| !matches!(e, Effect::Media(_))));
        assert_eq!(*r.media.calls.lock().unwrap(), vec!["pause"]);
    }

    #[test]
    fn no_resume_unless_we_paused() {
        let mut r = rig();
        wear(&mut r.m, Wearing::Worn);
        // Nothing playing: pause reports false, so we must not resume.
        let fx = wear(&mut r.m, Wearing::Removed);
        assert!(fx.iter().all(|e| !matches!(e, Effect::Media(_))));
        let fx = wear(&mut r.m, Wearing::Worn);
        assert!(fx.iter().all(|e| !matches!(e, Effect::Media(_))));
        assert_eq!(*r.media.calls.lock().unwrap(), vec!["pause"]);
    }

    #[test]
    fn ambient_drag_survives_device_reply() {
        let mut r = rig();
        r.m.apply(Intent::SetAmbientLevel(14));
        r.clock.advance(ms(100));
        nc_reply(&mut r.m);
        assert_eq!(r.m.state().nc.unwrap().ambient_level, 14);
        r.clock.advance(ms(160));
        let s = sends(&r.m.on_tick());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].payload[6], 14);
        assert_eq!(r.m.state().nc.unwrap().ambient_level, 14);
    }

    #[test]
    fn no_resume_after_reconnect_without_pause() {
        let mut r = rig();
        *r.media.playing.lock().unwrap() = true;
        wear(&mut r.m, Wearing::Worn);
        wear(&mut r.m, Wearing::Removed);
        r.m.on_event(SessionEvent::State(SessionState::Disconnected));
        r.m.on_event(SessionEvent::State(SessionState::Ready(caps())));
        wear(&mut r.m, Wearing::Worn);
        wear(&mut r.m, Wearing::Removed);
        let fx = wear(&mut r.m, Wearing::Worn);
        assert!(fx.iter().all(|e| !matches!(e, Effect::Media(_))));
        assert_eq!(*r.media.calls.lock().unwrap(), vec!["pause", "pause"]);
    }

    #[test]
    fn no_sends_while_disconnected() {
        let mut r = rig();
        r.m.apply(Intent::SetAmbientLevel(9));
        r.m.on_event(SessionEvent::State(SessionState::Disconnected));
        let before = r.m.state().clone();
        assert!(r.m.apply(Intent::SetMode(NcAmbMode::Ambient)).is_empty());
        assert_eq!(*r.m.state(), before);
        r.clock.advance(ms(500));
        assert!(r.m.on_tick().is_empty());
    }

    #[test]
    fn eq_disabled_during_call() {
        let mut r = rig();
        r.m.on_event(SessionEvent::Device(DeviceEvent::CallActive(true)));
        assert!(!r.m.state().eq_available);
        assert!(sends(&r.m.apply(Intent::SetEqPreset(EqPreset::BASS_BOOST))).is_empty());
        r.m.on_event(SessionEvent::Device(DeviceEvent::EqStatus {
            available: true,
        }));
        assert!(r.m.state().eq_available);
        assert_eq!(
            sends(&r.m.apply(Intent::SetEqPreset(EqPreset::BASS_BOOST))).len(),
            1
        );
    }

    #[test]
    fn headset_button_mode_change_updates_state() {
        let mut r = rig();
        let fx = r.m.on_event(SessionEvent::Device(DeviceEvent::NcAmb {
            mode: NcAmbMode::NoiseCancelling,
            ambient_level: 0,
            focus_on_voice: true,
            adaptive: Some((1, 2)),
        }));
        let nc = r.m.state().nc.unwrap();
        assert_eq!(nc.mode, NcAmbMode::NoiseCancelling);
        assert!(nc.focus_on_voice);
        assert!(fx.contains(&Effect::StateChanged));
    }

    #[test]
    fn nc_write_waits_for_adaptive_echo() {
        let mut r = rig_with(caps(), prefs());
        let fx = r.m.apply(Intent::SetMode(NcAmbMode::Ambient));
        assert_eq!(sends(&fx), vec![command::get_nc_amb(0x19)]);
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Ambient);
        let fx = r.m.on_event(SessionEvent::Device(DeviceEvent::NcAmb {
            mode: NcAmbMode::Off,
            ambient_level: 0,
            focus_on_voice: false,
            adaptive: Some((1, 2)),
        }));
        let s = sends(&fx);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].payload, vec![0x68, 0x19, 0x01, 1, 1, 0, 0, 1, 2]);
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Ambient);
    }

    #[test]
    fn nc_write_sent_without_adaptive_after_3s() {
        let mut r = rig_with(caps(), prefs());
        r.m.apply(Intent::SetMode(NcAmbMode::Ambient));
        r.clock.advance(ms(2900));
        assert!(sends(&r.m.on_tick()).is_empty());
        r.clock.advance(ms(200));
        let s = sends(&r.m.on_tick());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].payload, vec![0x68, 0x19, 0x01, 1, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn eq_bands_refused_on_wrong_count() {
        let mut r = rig();
        assert!(sends(&r.m.apply(Intent::SetEqBands(None, vec![0; 5]))).is_empty());
        assert!(sends(&r.m.apply(Intent::SetEqBands(Some(0), vec![0; 10]))).is_empty());
        assert_eq!(r.m.state().eq, None);
        r.m.apply(Intent::SetEqBands(None, vec![1; 10]));
        r.m.apply(Intent::SetEqBands(None, vec![2; 10]));
        r.clock.advance(ms(300));
        let s = sends(&r.m.on_tick());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "set_eq_bands");
        assert_eq!(s[0].payload[4..], [8u8; 10]);
    }

    #[test]
    fn preset_cancels_pending_band_write() {
        let mut r = rig();
        r.m.apply(Intent::SetEqBands(None, vec![1; 10]));
        let s = sends(&r.m.apply(Intent::SetEqPreset(EqPreset::BASS_BOOST)));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "set_eq_preset");
        r.clock.advance(ms(300));
        assert!(sends(&r.m.on_tick()).is_empty());
    }

    #[test]
    fn pending_band_write_outranks_the_headset_echo() {
        let mut r = rig();
        let echo = |m: &mut Model, preset, bands: Vec<i8>| {
            m.on_event(SessionEvent::Device(DeviceEvent::Eq {
                preset,
                clear_bass: None,
                bands,
            }))
        };
        r.m.apply(Intent::SetEqBands(None, vec![3; 10]));
        // An older value for the same preset arrives mid-drag: the drag stays on screen and goes out.
        echo(&mut r.m, EqPreset::MANUAL, vec![0; 10]);
        assert_eq!(r.m.state().eq, Some((EqPreset::MANUAL, None, vec![3; 10])));
        r.clock.advance(ms(300));
        let s = sends(&r.m.on_tick());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].payload[4..], [9u8; 10]);
        // Once written, the headset's values count again.
        echo(&mut r.m, EqPreset::MANUAL, vec![1; 10]);
        assert_eq!(r.m.state().eq, Some((EqPreset::MANUAL, None, vec![1; 10])));
        // The headset switching preset mid-drag drops the write.
        r.m.apply(Intent::SetEqBands(None, vec![2; 10]));
        echo(&mut r.m, EqPreset::HEAVY, vec![0; 10]);
        assert_eq!(r.m.state().eq, Some((EqPreset::HEAVY, None, vec![0; 10])));
        r.clock.advance(ms(300));
        assert!(sends(&r.m.on_tick()).is_empty());
    }

    #[test]
    fn preset_switch_shows_the_remembered_curve_until_the_reply() {
        let mut r = rig();
        r.m.on_event(SessionEvent::Connected(DeviceInfo {
            name: "WH-1000XM6".into(),
            address: "AA".into(),
        }));
        let report = |m: &mut Model, preset, bands: Vec<i8>| {
            m.on_event(SessionEvent::Device(DeviceEvent::Eq {
                preset,
                clear_bass: None,
                bands,
            }))
        };
        let gaming = vec![-6, -2, 2, 3, 1, -1, 1, 3, 0, -2];
        // Reports are remembered per headset and preset.
        report(&mut r.m, EqPreset::FLAT, vec![0; 10]);
        report(&mut r.m, EqPreset::GAMING, gaming.clone());
        assert_eq!(r.m.eq_curves()["WH-1000XM6"]["20"].latest, gaming);
        assert_eq!(r.m.eq_curves()["WH-1000XM6"]["00"].latest, vec![0; 10]);
        // Back to Flat, then Gaming again: its curve shows before any reply.
        report(&mut r.m, EqPreset::FLAT, vec![0; 10]);
        r.m.apply(Intent::SetEqPreset(EqPreset::GAMING));
        assert_eq!(
            r.m.state().eq,
            Some((EqPreset::GAMING, None, gaming.clone()))
        );
        // The reply overwrites the shown curve and the remembered one.
        let newer = vec![1; 10];
        report(&mut r.m, EqPreset::GAMING, newer.clone());
        assert_eq!(
            r.m.state().eq,
            Some((EqPreset::GAMING, None, newer.clone()))
        );
        // The first report is kept as the preset's default.
        let kept = &r.m.eq_curves()["WH-1000XM6"]["20"];
        assert_eq!((&kept.default, &kept.latest), (&gaming, &newer));
        // A preset never reported keeps the current bands; loaded curves work the same way.
        r.m.apply(Intent::SetEqPreset(EqPreset::HEAVY));
        assert_eq!(r.m.state().eq, Some((EqPreset::HEAVY, None, newer)));
        let mut loaded = EqCurves::new();
        loaded.entry("WH-1000XM6".into()).or_default().insert(
            "31".into(),
            PresetCurve {
                default: vec![0; 10],
                latest: vec![2; 10],
            },
        );
        r.m.set_eq_curves(loaded);
        r.m.apply(Intent::SetEqPreset(EqPreset::CLEAR));
        assert_eq!(r.m.state().eq, Some((EqPreset::CLEAR, None, vec![2; 10])));
    }

    #[test]
    fn new_headset_resets_device_fields() {
        let mut r = rig();
        let info = |a: &str| DeviceInfo {
            name: "XM6".into(),
            address: a.into(),
        };
        r.m.on_event(SessionEvent::Connected(info("AA")));
        r.m.on_event(SessionEvent::Device(DeviceEvent::Firmware("1.0".into())));
        r.m.on_event(SessionEvent::Device(DeviceEvent::Codec(
            crate::event::Codec::Ldac,
        )));
        r.m.on_event(SessionEvent::Device(DeviceEvent::Battery {
            level: 50,
            charging: false,
        }));
        r.m.on_event(SessionEvent::Connected(info("AA")));
        assert_eq!(r.m.state().firmware.as_deref(), Some("1.0"));
        assert!(r.m.state().nc.is_some());
        r.m.on_event(SessionEvent::Connected(info("BB")));
        let st = r.m.state();
        assert_eq!(st.firmware, None);
        assert_eq!(st.codec, None);
        assert_eq!(st.battery, None);
        assert_eq!(st.nc, None);
        assert_eq!(st.device.as_ref().unwrap().address, "BB");
    }

    #[test]
    fn eq_bands_with_clear_bass_layout() {
        let mut c = caps();
        c.has_clear_bass = true;
        c.eq_bands = 6;
        let mut r = rig_with(c, prefs());
        assert!(sends(&r.m.apply(Intent::SetEqBands(None, vec![0; 5]))).is_empty());
        r.m.apply(Intent::SetEqBands(Some(2), vec![0; 5]));
        r.clock.advance(ms(300));
        assert_eq!(sends(&r.m.on_tick()).len(), 1);
    }

    #[test]
    fn unsupported_intents_change_nothing() {
        let mut c = caps();
        c.dsee = false;
        c.nc_amb_kind = None;
        let mut r = rig_with(c, prefs());
        assert!(sends(&r.m.apply(Intent::SetDsee(true))).is_empty());
        assert_eq!(r.m.state().dsee, None);
        assert!(sends(&r.m.apply(Intent::SetMode(NcAmbMode::Off))).is_empty());
        assert_eq!(r.m.state().nc, None);
    }

    #[test]
    fn simple_intent_updates_and_sends() {
        let mut r = rig();
        let s = sends(&r.m.apply(Intent::SetSpeakToChat(true)));
        assert_eq!(s, vec![command::set_speak_to_chat(true)]);
        assert_eq!(r.m.state().speak_to_chat, Some(true));
    }

    fn dev(m: &mut Model, d: DeviceEvent) -> Vec<Effect> {
        m.on_event(SessionEvent::Device(d))
    }

    /// Feeds a live table 1 payload through the parser, as the session would.
    fn live(m: &mut Model, p: &[u8]) -> Vec<Effect> {
        dev(
            m,
            crate::event::parse(command::Table::One, p).expect("parses"),
        )
    }

    fn probed_rig() -> Rig {
        let mut c = caps();
        c.probed = crate::capabilities::probed_for("WH-1000XM6", &[]);
        let mut r = rig_with(c, prefs());
        nc_reply(&mut r.m);
        r
    }

    #[test]
    fn hardware_batch_events_fill_state() {
        let mut r = rig();
        live(&mut r.m, &[0x05, 0x03, 0x30, 0x03]);
        live(&mut r.m, &[0x67, 0x30, 0x02]);
        dev(&mut r.m, DeviceEvent::VoiceGuidanceVolume(0));
        dev(
            &mut r.m,
            DeviceEvent::GeneralSettingInfo {
                slot: 1,
                label: "SIDETONE_SETTING".into(),
            },
        );
        live(&mut r.m, &[0xD7, 0xD1, 0x00, 0x00]);
        live(&mut r.m, &[0xE5, 0x01, 0x01]);
        live(
            &mut r.m,
            &[0x67, 0x19, 0x01, 0x01, 0x00, 0x01, 0x14, 0x00, 0x00],
        );
        let st = r.m.state();
        assert_eq!(st.headset_colour, Some(HeadsetColour::PlatinumSilver));
        assert_eq!(st.nc_button, Some(2));
        assert_eq!(st.voice_guidance_volume, Some(0));
        assert_eq!(
            st.general_settings,
            vec![GeneralSetting {
                slot: 1,
                label: "SIDETONE_SETTING".into(),
                on: Some(true)
            }]
        );
        assert!(st.sound_effect_unavailable);
        assert_eq!(st.auto_ambient, Some(false));
        // EQ stays usable while sound effects are flagged unavailable.
        assert_eq!(
            sends(&r.m.apply(Intent::SetEqPreset(EqPreset::FLAT))).len(),
            1
        );
        live(&mut r.m, &[0xE5, 0x01, 0x00]);
        assert!(!r.m.state().sound_effect_unavailable);
        live(&mut r.m, &[0x05, 0x03, 0x30, 0x05]);
        assert_eq!(
            r.m.state().headset_colour,
            Some(HeadsetColour::MidnightBlue)
        );
        live(&mut r.m, &[0x05, 0x03, 0x30, 0x0A]);
        assert_eq!(r.m.state().headset_colour, None);
    }

    #[test]
    fn general_setting_intent_only_for_writable_slots() {
        let mut r = rig();
        let info = |slot, label: &str| DeviceEvent::GeneralSettingInfo {
            slot,
            label: label.into(),
        };
        // Before any label arrives nothing is writable.
        assert!(sends(&r.m.apply(Intent::SetGeneralSetting(1, false))).is_empty());
        dev(&mut r.m, info(1, "SIDETONE_SETTING"));
        dev(&mut r.m, info(2, "MULTIPOINT_SETTING"));
        dev(&mut r.m, info(3, "TOUCH_PANEL_SETTING"));
        live(&mut r.m, &[0xD7, 0xD2, 0x00, 0x00]);
        live(&mut r.m, &[0xD7, 0xD3, 0x00, 0x00]);
        let s = sends(&r.m.apply(Intent::SetGeneralSetting(1, false)));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].payload, [0xD8, 0xD1, 0x00, 0x01]);
        assert_eq!(r.m.state().general_settings[0].on, Some(false));
        // Multipoint and touch are read only in v1.
        assert!(sends(&r.m.apply(Intent::SetGeneralSetting(2, false))).is_empty());
        assert!(sends(&r.m.apply(Intent::SetGeneralSetting(3, false))).is_empty());
        assert_eq!(r.m.state().general_settings[1].on, Some(true));
        assert_eq!(r.m.state().general_settings[2].on, Some(true));
    }

    #[test]
    fn failed_general_setting_write_rereads_values() {
        let mut r = rig();
        let info = |slot, label: &str| DeviceEvent::GeneralSettingInfo {
            slot,
            label: label.into(),
        };
        dev(&mut r.m, info(1, "SIDETONE_SETTING"));
        dev(&mut r.m, info(3, "TOUCH_PANEL_SETTING"));
        r.m.apply(Intent::SetGeneralSetting(1, false));
        r.m.on_sent(4, "set_general_setting");
        let fx = r.m.on_event(SessionEvent::Outcome(Outcome::Failed(
            4,
            SendError::Timeout,
        )));
        assert_eq!(
            sends(&fx),
            vec![
                command::get_general_setting(1),
                command::get_general_setting(3)
            ]
        );
    }

    #[test]
    fn bgm_cinema_and_source_intents_send_when_probed() {
        let mut r = probed_rig();
        let s = sends(&r.m.apply(Intent::SetBgm(true, BgmRoom::Cafe)));
        assert_eq!(s, vec![command::set_bgm(true, BgmRoom::Cafe)]);
        assert_eq!(r.m.state().bgm, Some((true, BgmRoom::Cafe)));
        assert_eq!(
            sends(&r.m.apply(Intent::SetCinema(true))),
            vec![command::set_cinema(true)]
        );
        let mac = "02:1D:55:0D:7D:AF";
        assert_eq!(
            sends(&r.m.apply(Intent::SwitchSource(mac.into()))),
            vec![command::switch_source(mac).unwrap()]
        );
        assert!(sends(&r.m.apply(Intent::SetAdaptiveVolume(true))).is_empty());
    }

    #[test]
    fn effect_off_while_removed_keeps_the_users_mode() {
        let mut r = rig();
        // Live capture 2026-10-20: button to ambient, take off, put on.
        live(&mut r.m, &[0xF5, 0x10, 0x01, 0x00]);
        live(
            &mut r.m,
            &[0x69, 0x19, 0x01, 0x01, 0x01, 0x01, 0x14, 0x00, 0x00],
        );
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Ambient);
        live(&mut r.m, &[0xF5, 0x10, 0x01, 0x01]);
        let fx = live(
            &mut r.m,
            &[0x69, 0x19, 0x00, 0x00, 0x01, 0x01, 0x14, 0x00, 0x00],
        );
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Ambient);
        assert!(r.m.state().effect_suspended);
        assert!(fx.contains(&Effect::StateChanged));
        live(&mut r.m, &[0xF5, 0x10, 0x01, 0x00]);
        assert!(!r.m.state().effect_suspended);
        live(
            &mut r.m,
            &[0x69, 0x19, 0x00, 0x01, 0x01, 0x01, 0x14, 0x00, 0x00],
        );
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Ambient);
        assert!(!r.m.state().effect_suspended);
        // Worn, an effect-off notification is the user choosing Off.
        live(
            &mut r.m,
            &[0x69, 0x19, 0x01, 0x00, 0x01, 0x01, 0x14, 0x00, 0x00],
        );
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Off);
        assert!(!r.m.state().effect_suspended);
    }

    #[test]
    fn suspension_needs_a_mode_other_than_off() {
        let mut r = rig();
        live(&mut r.m, &[0xF5, 0x10, 0x01, 0x00]);
        live(
            &mut r.m,
            &[0x69, 0x19, 0x01, 0x00, 0x01, 0x01, 0x14, 0x00, 0x00],
        );
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Off);
        live(&mut r.m, &[0xF5, 0x10, 0x01, 0x01]);
        live(
            &mut r.m,
            &[0x69, 0x19, 0x00, 0x00, 0x01, 0x01, 0x14, 0x00, 0x00],
        );
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Off);
        assert!(!r.m.state().effect_suspended);
    }

    /// Reconnects (same headset) while it is off the head; `nc_first` replays the old read order.
    fn reconnect_removed(nc_first: bool) -> Rig {
        let mut r = rig();
        live(&mut r.m, &[0xF5, 0x10, 0x01, 0x00]);
        live(
            &mut r.m,
            &[0x69, 0x19, 0x01, 0x01, 0x00, 0x01, 0x14, 0x00, 0x00],
        );
        r.m.on_event(SessionEvent::State(SessionState::Disconnected));
        r.m.on_event(SessionEvent::State(SessionState::Ready(caps())));
        let removed = [0xF5, 0x10, 0x01, 0x01];
        let nc_off = [0x67, 0x19, 0x00, 0x00, 0x00, 0x01, 0x14, 0x00, 0x00];
        if nc_first {
            live(&mut r.m, &nc_off);
            live(&mut r.m, &removed);
        } else {
            live(&mut r.m, &removed);
            live(&mut r.m, &nc_off);
        }
        r
    }

    #[test]
    fn reconnect_while_removed_keeps_the_users_mode() {
        for nc_first in [false, true] {
            let r = reconnect_removed(nc_first);
            let st = r.m.state();
            assert_eq!(
                st.nc.unwrap().mode,
                NcAmbMode::NoiseCancelling,
                "nc_first {nc_first}"
            );
            assert!(st.effect_suspended, "nc_first {nc_first}");
        }
    }

    #[test]
    fn off_reported_before_wearing_then_worn_is_the_users_off() {
        let mut r = rig();
        live(&mut r.m, &[0xF5, 0x10, 0x01, 0x00]);
        live(
            &mut r.m,
            &[0x69, 0x19, 0x01, 0x01, 0x00, 0x01, 0x14, 0x00, 0x00],
        );
        r.m.on_event(SessionEvent::State(SessionState::Disconnected));
        r.m.on_event(SessionEvent::State(SessionState::Ready(caps())));
        live(
            &mut r.m,
            &[0x67, 0x19, 0x00, 0x00, 0x00, 0x01, 0x14, 0x00, 0x00],
        );
        live(&mut r.m, &[0xF5, 0x10, 0x01, 0x00]);
        live(&mut r.m, &[0xF5, 0x10, 0x01, 0x01]);
        assert_eq!(r.m.state().nc.unwrap().mode, NcAmbMode::Off);
        assert!(!r.m.state().effect_suspended);
    }

    #[test]
    fn disconnect_clears_held_write_and_wearing() {
        let mut r = rig_with(caps(), prefs());
        r.m.apply(Intent::SetMode(NcAmbMode::Ambient));
        wear(&mut r.m, Wearing::Worn);
        r.m.on_event(SessionEvent::State(SessionState::NotConnected));
        assert_eq!(r.m.state().wearing, Wearing::Unknown);
        r.clock.advance(Duration::from_secs(5));
        assert!(sends(&r.m.on_tick()).is_empty());
    }
}
