//! Session state machine: handshake, ACK and sequence rules, one command in flight,
//! retry, periodic refresh and reconnect backoff. Pure: bytes and time in, bytes and events out.

use crate::capabilities::{allows, from_support, probed_for, BatteryKind, Capabilities};
use crate::command::{self, Command, Table};
use crate::event::{parse, DeviceEvent};
use crate::frame::{encode, Frame, FrameError, MessageType, Reassembler};
use crate::log_info;
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

const ACK_TIMEOUT: Duration = Duration::from_secs(1);
const REFRESH_EVERY: Duration = Duration::from_secs(15);
const BACKOFF_CAP_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    Timeout,
    Disconnected,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done(u64),
    Failed(u64, SendError),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Disconnected,
    BluetoothOff,
    NotConnected,
    InUse,
    Connecting,
    Ready(Capabilities),
}

#[derive(Debug, Default, PartialEq)]
pub struct Step {
    pub write: Vec<Vec<u8>>,
    pub events: Vec<DeviceEvent>,
    pub outcomes: Vec<Outcome>,
    pub state: Option<SessionState>,
}

#[derive(Clone, Copy, PartialEq)]
enum Handshake {
    Idle,
    AwaitInfo,
    AwaitSupport,
}

struct InFlight {
    /// None for the session's own handshake and state reads, which never surface as outcomes.
    id: Option<u64>,
    seq: u8,
    bytes: Vec<u8>,
    sent_at: Instant,
    resent: bool,
}

pub struct SessionCore {
    state: SessionState,
    handshake: Handshake,
    table2: bool,
    model_name: String,
    reassembler: Reassembler,
    queue: VecDeque<(Option<u64>, Command)>,
    in_flight: Option<InFlight>,
    tx_seq: u8,
    /// Latest time seen; stamps sends made while pumping.
    now: Instant,
    last_refresh: Instant,
    backoff_step: u32,
    /// Payload shapes and frame errors already logged, so each is reported once.
    seen_unknown: HashSet<(Table, u8, Option<u8>)>,
    seen_frame_errors: HashSet<std::mem::Discriminant<FrameError>>,
}

impl Default for SessionCore {
    fn default() -> Self {
        Self::new()
    }
}

fn state_reads(caps: &Capabilities) -> Vec<Command> {
    let mut v = Vec::new();
    // Wearing first: an NC reply with effect off means something else while the headset is off the head.
    if caps.table2 {
        v.push(command::get_wearing_status());
    }
    if let Some(k) = caps.nc_amb_kind {
        v.push(command::get_nc_amb(k));
    }
    if let Some(k) = caps.eq_kind {
        v.push(command::get_eq_status(k));
        v.push(command::get_eq(k));
    }
    v.push(command::get_battery(match caps.battery {
        BatteryKind::Single => 0,
        BatteryKind::LeftRight => 1,
        BatteryKind::LeftRightCradle => 2,
    }));
    if caps.speak_to_chat {
        v.push(command::get_speak_to_chat());
    }
    if caps.auto_pause {
        v.push(command::get_auto_pause());
    }
    if caps.dsee {
        v.push(command::get_dsee());
    }
    if !caps.auto_power_off.is_empty() {
        v.push(command::get_auto_power_off());
    }
    // Otherwise the card learns the track only at the next change (owner, 2026-10-22).
    v.push(command::get_now_playing());
    v.extend(probed(
        caps,
        [
            command::get_codec(),
            command::get_bgm(),
            command::get_cinema(),
            command::get_device_list(),
        ],
    ));
    v
}

/// Reads sent once after the first state reads. General-setting values are read when a label is
/// learnt, and the model re-reads them after a failed write.
fn ready_reads(caps: &Capabilities) -> Vec<Command> {
    let mut v = probed(
        caps,
        [
            command::get_firmware(),
            command::get_colour(),
            command::get_nc_button(),
            command::get_voice_guidance_volume(),
        ],
    );
    v.extend(
        caps.general_setting_slots
            .iter()
            .map(|&slot| command::get_general_setting_capability(slot)),
    );
    v
}

/// The probed gets this headset supports, dropping table 2 ones when it has no table 2.
fn probed<const N: usize>(caps: &Capabilities, cmds: [Command; N]) -> Vec<Command> {
    cmds.into_iter()
        .filter(|c| allows(caps, c) && (c.table == Table::One || caps.table2))
        .collect()
}

fn frame_bytes(kind: MessageType, seq: u8, payload: &[u8]) -> Vec<u8> {
    encode(&Frame {
        kind,
        seq,
        payload: payload.to_vec(),
    })
}

impl SessionCore {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            state: SessionState::Disconnected,
            handshake: Handshake::Idle,
            table2: false,
            model_name: String::new(),
            reassembler: Reassembler::default(),
            queue: VecDeque::new(),
            in_flight: None,
            tx_seq: 0,
            now,
            last_refresh: now,
            backoff_step: 0,
            seen_unknown: HashSet::new(),
            seen_frame_errors: HashSet::new(),
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    /// The connected headset's name; with its support list it picks the probed features at Ready.
    pub fn set_model_name(&mut self, name: &str) {
        self.model_name = name.to_string();
    }

    /// Starts a fresh session; returns the bytes to write (the `00 00` handshake).
    pub fn on_connected(&mut self, now: Instant) -> Vec<Vec<u8>> {
        self.reassembler = Reassembler::default();
        self.queue.clear();
        self.in_flight = None;
        self.tx_seq = 0;
        self.now = now;
        self.state = SessionState::Connecting;
        self.handshake = Handshake::AwaitInfo;
        self.queue.push_back((None, command::get_protocol_info()));
        let mut write = Vec::new();
        self.pump(&mut write);
        write
    }

    /// Queues a user command. Writes go in `Step.write`; refusals come back as outcomes.
    pub fn enqueue(&mut self, cmd: Command, id: u64, now: Instant) -> Step {
        self.now = now;
        let mut step = Step::default();
        let refusal = match &self.state {
            SessionState::Ready(caps) if allows(caps, &cmd) => None,
            SessionState::Ready(_) => Some(SendError::Unsupported),
            _ => Some(SendError::Disconnected),
        };
        match refusal {
            Some(e) => step.outcomes.push(Outcome::Failed(id, e)),
            None => {
                // User writes go ahead of queued internal reads, behind earlier user writes.
                let at = self
                    .queue
                    .iter()
                    .position(|(id, _)| id.is_none())
                    .unwrap_or(self.queue.len());
                self.queue.insert(at, (Some(id), cmd));
                self.pump(&mut step.write);
            }
        }
        step
    }

    pub fn on_bytes(&mut self, bytes: &[u8], now: Instant) -> Step {
        self.now = now;
        let mut step = Step::default();
        for result in self.reassembler.push(bytes) {
            let frame = match result {
                Ok(f) => f,
                Err(e) => {
                    // Frames that fail to decode are dropped; each kind is logged once.
                    if self.seen_frame_errors.insert(std::mem::discriminant(&e)) {
                        log_info!("frame decode error: {e}");
                    }
                    continue;
                }
            };
            match frame.kind {
                // Only the ACK for what is in flight counts; a duplicate or stray one is dropped.
                MessageType::Ack => {
                    let expected = self.in_flight.as_ref().map(|f| 1u8.wrapping_sub(f.seq));
                    if expected == Some(frame.seq) {
                        self.tx_seq = frame.seq;
                        let f = self.in_flight.take().unwrap();
                        step.outcomes.extend(f.id.map(Outcome::Done));
                    }
                }
                MessageType::DataTable1 | MessageType::DataTable2 => {
                    step.write.push(frame_bytes(
                        MessageType::Ack,
                        1u8.wrapping_sub(frame.seq),
                        &[],
                    ));
                    let table = if frame.kind == MessageType::DataTable1 {
                        Table::One
                    } else {
                        Table::Two
                    };
                    if let Some(ev) = parse(table, &frame.payload) {
                        self.on_event(&ev, &mut step);
                        step.events.push(ev);
                    } else {
                        self.log_unknown(table, &frame.payload);
                    }
                }
            }
        }
        self.pump(&mut step.write);
        step
    }

    fn log_unknown(&mut self, table: Table, p: &[u8]) {
        if self
            .seen_unknown
            .insert((table, p.first().copied().unwrap_or(0), p.get(1).copied()))
        {
            let hex: Vec<String> = p.iter().map(|b| format!("{b:02X}")).collect();
            log_info!("unparsed payload, table {table:?}: {}", hex.join(" "));
        }
    }

    pub fn on_tick(&mut self, now: Instant) -> Step {
        self.now = now;
        let mut step = Step::default();
        if let Some(f) = &mut self.in_flight {
            if now.duration_since(f.sent_at) >= ACK_TIMEOUT {
                if f.resent {
                    let f = self.in_flight.take().unwrap();
                    step.outcomes
                        .extend(f.id.map(|id| Outcome::Failed(id, SendError::Timeout)));
                } else {
                    f.resent = true;
                    f.sent_at = now;
                    step.write.push(f.bytes.clone());
                }
            }
        }
        // A silent headset must not let refresh batches pile up behind each other.
        let reads_pending = self.in_flight.as_ref().is_some_and(|f| f.id.is_none())
            || self.queue.iter().any(|(id, _)| id.is_none());
        if matches!(self.state, SessionState::Ready(_))
            && !reads_pending
            && now.duration_since(self.last_refresh) >= REFRESH_EVERY
        {
            self.last_refresh = now;
            self.queue_reads();
        }
        self.pump(&mut step.write);
        step
    }

    pub fn on_disconnected(&mut self) -> Vec<Outcome> {
        let in_flight = self.in_flight.take().and_then(|f| f.id);
        let queued = self.queue.drain(..).filter_map(|(id, _)| id);
        let out = in_flight
            .into_iter()
            .chain(queued)
            .map(|id| Outcome::Failed(id, SendError::Disconnected))
            .collect();
        self.handshake = Handshake::Idle;
        self.state = SessionState::Disconnected;
        out
    }

    /// 1, 2, 4, 8, 16, then 30 seconds; Ready resets it.
    pub fn next_backoff(&mut self) -> Duration {
        let secs = 1u64
            .checked_shl(self.backoff_step)
            .unwrap_or(u64::MAX)
            .min(BACKOFF_CAP_SECS);
        self.backoff_step = (self.backoff_step + 1).min(8);
        Duration::from_secs(secs)
    }

    fn on_event(&mut self, ev: &DeviceEvent, step: &mut Step) {
        match (ev, self.handshake) {
            (DeviceEvent::ProtocolInfo { table2, .. }, Handshake::AwaitInfo) => {
                self.table2 = *table2;
                self.handshake = Handshake::AwaitSupport;
                self.queue
                    .push_back((None, command::get_support_functions()));
            }
            (DeviceEvent::SupportFunctions(f), Handshake::AwaitSupport) => {
                let caps = Capabilities {
                    probed: probed_for(&self.model_name, f),
                    ..from_support(f, self.table2)
                };
                self.handshake = Handshake::Idle;
                self.backoff_step = 0;
                self.last_refresh = self.now;
                self.state = SessionState::Ready(caps.clone());
                self.queue_reads();
                self.queue
                    .extend(ready_reads(&caps).into_iter().map(|c| (None, c)));
                step.state = Some(SessionState::Ready(caps));
            }
            (DeviceEvent::GeneralSettingInfo { slot, label }, _) => {
                if let SessionState::Ready(caps) = &mut self.state {
                    caps.learn_general_setting(*slot, label);
                    self.queue
                        .push_back((None, command::get_general_setting(*slot)));
                }
            }
            _ => {}
        }
    }

    fn queue_reads(&mut self) {
        if let SessionState::Ready(caps) = &self.state {
            self.queue
                .extend(state_reads(caps).into_iter().map(|c| (None, c)));
        }
    }

    /// Writes the next queued command if none is in flight.
    fn pump(&mut self, write: &mut Vec<Vec<u8>>) {
        if self.in_flight.is_some() {
            return;
        }
        let Some((id, cmd)) = self.queue.pop_front() else {
            return;
        };
        let kind = match cmd.table {
            Table::One => MessageType::DataTable1,
            Table::Two => MessageType::DataTable2,
        };
        let bytes = frame_bytes(kind, self.tx_seq, &cmd.payload);
        write.push(bytes.clone());
        self.in_flight = Some(InFlight {
            id,
            seq: self.tx_seq,
            bytes,
            sent_at: self.now,
            resent: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::BatteryKind;
    use crate::clock::{Clock, FakeClock};
    use crate::command::{self, Command, Table};
    use crate::event::DeviceEvent;
    use crate::frame::{decode, encode, Frame, MessageType};
    use std::time::Duration;

    const PROTO_INFO: [u8; 8] = [0x01, 0x00, 0x03, 0x00, 0x30, 0x32, 0x00, 0x00];
    const XM6_SUPPORT: &str = "07 00 2D 10 FF 12 FF 13 FF 14 FF 15 FF 16 FF 17 FF 23 FF 25 38 27 FF 20 FF 57 13 56 FF 6D 0B 71 02 90 FF A1 14 C1 FF C2 FF E2 1D F1 3A FC 0D 69 2C F9 43 D1 3D D3 2B 94 FF 41 31 42 3C 44 FF FF 2F D2 22 19 FF EB 10 E5 11 E6 0E E7 1C FD 2D B6 19 B0 44 B2 45 B8 21 B7 17 34 41 F4 25";

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect()
    }

    fn data(seq: u8, payload: &[u8]) -> Vec<u8> {
        encode(&Frame {
            kind: MessageType::DataTable1,
            seq,
            payload: payload.to_vec(),
        })
    }

    fn ack(seq: u8) -> Vec<u8> {
        encode(&Frame {
            kind: MessageType::Ack,
            seq,
            payload: vec![],
        })
    }

    fn frames(w: &[Vec<u8>]) -> Vec<Frame> {
        w.iter().map(|b| decode(b).unwrap()).collect()
    }

    fn support(ids: &[u8]) -> Vec<u8> {
        let mut p = vec![0x07, 0x00, ids.len() as u8];
        for &i in ids {
            p.extend([i, 0xFF]);
        }
        p
    }

    fn raw(payload: &[u8]) -> Command {
        Command {
            table: Table::One,
            payload: payload.to_vec(),
            name: "raw",
        }
    }

    /// The ACK the headset sends for the frame in flight: `1 - sent seq`.
    fn ack_in_flight(s: &SessionCore) -> Vec<u8> {
        ack(1 - s.in_flight.as_ref().expect("a frame in flight").seq)
    }

    /// Drives a session through the handshake and acks every state read.
    fn ready(support_payload: &[u8], clock: &FakeClock) -> SessionCore {
        let mut s = SessionCore::new();
        s.on_connected(clock.now());
        s.on_bytes(&ack(1), clock.now());
        s.on_bytes(&data(0, &PROTO_INFO), clock.now());
        s.on_bytes(&ack(0), clock.now());
        let step = s.on_bytes(&data(1, support_payload), clock.now());
        assert!(matches!(step.state, Some(SessionState::Ready(_))));
        while s.in_flight.is_some() {
            s.on_bytes(&ack_in_flight(&s), clock.now());
        }
        s
    }

    #[test]
    fn handshake_then_ready() {
        let clock = FakeClock::new();
        let mut s = SessionCore::new();
        let w = s.on_connected(clock.now());
        assert_eq!(w.len(), 1);
        assert_eq!(
            frames(&w)[0],
            Frame {
                kind: MessageType::DataTable1,
                seq: 0,
                payload: vec![0, 0]
            }
        );
        assert_eq!(s.state(), &SessionState::Connecting);

        s.on_bytes(&ack(1), clock.now());
        let step = s.on_bytes(&data(0, &PROTO_INFO), clock.now());
        let f = frames(&step.write);
        assert_eq!(
            f[0],
            Frame {
                kind: MessageType::Ack,
                seq: 1,
                payload: vec![]
            }
        );
        assert_eq!(
            f[1],
            Frame {
                kind: MessageType::DataTable1,
                seq: 1,
                payload: vec![0x06, 0x00]
            }
        );
        assert_eq!(f.len(), 2);
        assert_eq!(step.state, None);

        s.on_bytes(&ack(0), clock.now());
        let step = s.on_bytes(&data(1, &hex(XM6_SUPPORT)), clock.now());
        let Some(SessionState::Ready(caps)) = step.state.clone() else {
            panic!("not ready: {:?}", step.state)
        };
        assert_eq!(caps.nc_amb_kind, Some(0x19));
        assert_eq!(caps.battery, BatteryKind::Single);
        let f = frames(&step.write);
        assert_eq!(
            f[0],
            Frame {
                kind: MessageType::Ack,
                seq: 0,
                payload: vec![]
            }
        );
        // Wearing goes first, so the model knows the headset is off the head before NC arrives.
        assert_eq!(f[1].payload, vec![0xF2, 0x00]);
        assert_eq!(s.state(), &SessionState::Ready(caps));

        // The rest of the reads follow one per ACK, wearing status on table 2.
        let mut sent = vec![f[1].clone()];
        while s.in_flight.is_some() {
            let step = s.on_bytes(&ack_in_flight(&s), clock.now());
            sent.extend(frames(&step.write));
        }
        let payloads: Vec<Vec<u8>> = sent.iter().map(|f| f.payload.clone()).collect();
        let expected: Vec<Vec<u8>> = [
            command::get_wearing_status(),
            command::get_nc_amb(0x19),
            command::get_eq_status(0x04),
            command::get_eq(0x04),
            command::get_battery(0),
            command::get_speak_to_chat(),
            command::get_auto_pause(),
            command::get_dsee(),
            command::get_auto_power_off(),
            command::get_now_playing(),
            // The XM6 support list alone selects the probed reads (no name given here).
            command::get_codec(),
            command::get_bgm(),
            command::get_cinema(),
            command::get_device_list(),
            command::get_firmware(),
            command::get_colour(),
            command::get_nc_button(),
            command::get_voice_guidance_volume(),
            command::get_general_setting_capability(1),
            command::get_general_setting_capability(2),
            command::get_general_setting_capability(3),
        ]
        .iter()
        .map(|c| c.payload.clone())
        .collect();
        assert_eq!(payloads, expected);
        let table2: Vec<usize> = (0..sent.len())
            .filter(|&i| sent[i].kind == MessageType::DataTable2)
            .collect();
        // Wearing, device list and voice guidance travel on table 2.
        assert_eq!(table2, [0, 13, 17]);
    }

    #[test]
    fn probed_features_reach_ready_caps() {
        let clock = FakeClock::new();
        let mut s = SessionCore::new();
        s.set_model_name("WH-1000XM6");
        s.on_connected(clock.now());
        s.on_bytes(&ack(1), clock.now());
        s.on_bytes(&data(0, &PROTO_INFO), clock.now());
        s.on_bytes(&ack(0), clock.now());
        let step = s.on_bytes(&data(1, &hex(XM6_SUPPORT)), clock.now());
        let Some(SessionState::Ready(caps)) = step.state else {
            panic!("not ready")
        };
        assert!(caps.probed.bgm && caps.probed.colour && !caps.probed.adaptive_volume);
    }

    /// Every internal read the session sends until it goes quiet, acking each one.
    fn drain_reads(s: &mut SessionCore, clock: &FakeClock, first: Vec<Vec<u8>>) -> Vec<Command> {
        let mut sent: Vec<Frame> = frames(&first)
            .into_iter()
            .filter(|f| f.kind != MessageType::Ack)
            .collect();
        while let Some(f) = &s.in_flight {
            let seq = decode(&f.bytes).unwrap().seq;
            let step = s.on_bytes(&ack(1 - seq), clock.now());
            sent.extend(frames(&step.write));
        }
        sent.into_iter()
            .map(|f| Command {
                table: if f.kind == MessageType::DataTable2 {
                    Table::Two
                } else {
                    Table::One
                },
                payload: f.payload,
                name: "sent",
            })
            .collect()
    }

    fn payloads(c: &[Command]) -> Vec<(Table, Vec<u8>)> {
        c.iter().map(|c| (c.table, c.payload.clone())).collect()
    }

    fn probed_xm6(clock: &FakeClock) -> (SessionCore, Vec<Command>) {
        let mut s = SessionCore::new();
        s.set_model_name("WH-1000XM6");
        s.on_connected(clock.now());
        s.on_bytes(&ack(1), clock.now());
        s.on_bytes(&data(0, &PROTO_INFO), clock.now());
        s.on_bytes(&ack(0), clock.now());
        let step = s.on_bytes(&data(1, &hex(XM6_SUPPORT)), clock.now());
        let sent = drain_reads(&mut s, clock, step.write);
        (s, sent)
    }

    #[test]
    fn ready_reads_probed_features_and_discovers_general_settings() {
        let clock = FakeClock::new();
        let (_, sent) = probed_xm6(&clock);
        let tail: Vec<Command> = vec![
            command::get_codec(),
            command::get_bgm(),
            command::get_cinema(),
            command::get_device_list(),
            command::get_firmware(),
            command::get_colour(),
            command::get_nc_button(),
            command::get_voice_guidance_volume(),
            command::get_general_setting_capability(1),
            command::get_general_setting_capability(2),
            command::get_general_setting_capability(3),
        ];
        assert_eq!(sent.len(), 10 + tail.len());
        assert_eq!(payloads(&sent[10..]), payloads(&tail));
    }

    #[test]
    fn general_setting_label_reads_value_and_gates_writes() {
        let clock = FakeClock::new();
        let (mut s, _) = probed_xm6(&clock);
        let info = |slot: u8, label: &str| {
            let mut p = vec![0xD1, 0xD0 + slot, 0x00, 0x01, label.len() as u8];
            p.extend(label.as_bytes());
            p.extend([0x01, b'S']);
            p
        };
        let step = s.on_bytes(&data(0, &info(1, "SIDETONE_SETTING")), clock.now());
        let sent = drain_reads(&mut s, &clock, step.write);
        assert_eq!(
            payloads(&sent),
            payloads(&[command::get_general_setting(1)])
        );
        let step = s.on_bytes(&data(1, &info(3, "TOUCH_PANEL_SETTING")), clock.now());
        drain_reads(&mut s, &clock, step.write);
        let step = s.enqueue(command::set_general_setting(1, false), 1, clock.now());
        assert_eq!(step.write.len(), 1);
        s.on_bytes(&ack(1 - frames(&step.write)[0].seq), clock.now());
        let step = s.enqueue(command::set_general_setting(3, false), 2, clock.now());
        assert_eq!(
            step.outcomes,
            vec![Outcome::Failed(2, SendError::Unsupported)]
        );
        // The 15 s refresh reads no general-setting values, firmware, colour or labels.
        clock.advance(REFRESH_EVERY);
        let first = s.on_tick(clock.now()).write;
        let sent = drain_reads(&mut s, &clock, first);
        let p = payloads(&sent);
        assert!(!p.contains(&(Table::One, vec![0xD6, 0xD1])));
        assert!(!p.contains(&(Table::One, vec![0xD6, 0xD3])));
        assert!(p.contains(&(Table::One, vec![0x12, 0x02])));
        assert!(!p.contains(&(Table::One, vec![0x04, 0x02])));
        assert!(!p.contains(&(Table::One, vec![0x04, 0x03])));
        assert!(!p.iter().any(|(_, b)| b[0] == 0xD0));
    }

    #[test]
    fn xm6_refresh_batch_is_14_reads() {
        let clock = FakeClock::new();
        let (mut s, _) = probed_xm6(&clock);
        clock.advance(REFRESH_EVERY);
        let first = s.on_tick(clock.now()).write;
        let sent = drain_reads(&mut s, &clock, first);
        assert_eq!(sent.len(), 14, "{:02X?}", payloads(&sent));
        let p = payloads(&sent);
        for read in [
            command::get_codec(),
            command::get_bgm(),
            command::get_cinema(),
            command::get_device_list(),
        ] {
            assert!(p.contains(&(read.table, read.payload)));
        }
    }

    #[test]
    fn ack_with_wrong_seq_is_ignored() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        let step = s.enqueue(raw(&[0x10]), 1, clock.now());
        let sent = frames(&step.write)[0].seq;
        let step = s.on_bytes(&ack(sent), clock.now());
        assert!(step.outcomes.is_empty());
        assert!(s.in_flight.is_some());
        let step = s.on_bytes(&ack(1 - sent), clock.now());
        assert_eq!(step.outcomes, vec![Outcome::Done(1)]);
        // A stray ACK with nothing in flight changes nothing.
        let step = s.on_bytes(&ack(sent), clock.now());
        assert!(step.outcomes.is_empty());
        let step = s.enqueue(raw(&[0x11]), 2, clock.now());
        assert_eq!(frames(&step.write)[0].seq, 1 - sent);
    }

    #[test]
    fn user_command_goes_ahead_of_queued_reads() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        clock.advance(REFRESH_EVERY);
        let first = frames(&s.on_tick(clock.now()).write);
        assert_eq!(first[0].payload, vec![0xF2, 0x00]);
        s.enqueue(raw(&[0x10]), 1, clock.now());
        s.enqueue(raw(&[0x11]), 2, clock.now());
        let step = s.on_bytes(&ack(1 - first[0].seq), clock.now());
        let next = frames(&step.write);
        assert_eq!(next[0].payload, vec![0x10]);
        let step = s.on_bytes(&ack(1 - next[0].seq), clock.now());
        assert_eq!(step.outcomes, vec![Outcome::Done(1)]);
        assert_eq!(frames(&step.write)[0].payload, vec![0x11]);
    }

    #[test]
    fn only_ack_sets_next_seq() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        let next = s.tx_seq;
        let step = s.on_bytes(&data(next, &[0xEE, 0xEE]), clock.now());
        assert_eq!(frames(&step.write)[0].seq, 1 - next);
        let step = s.enqueue(command::set_dsee(true), 1, clock.now());
        assert_eq!(frames(&step.write)[0].seq, next);
    }

    #[test]
    fn every_data_frame_is_acked_with_flipped_seq() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        for (seq, kind) in [(1, MessageType::DataTable1), (0, MessageType::DataTable2)] {
            let b = encode(&Frame {
                kind,
                seq,
                payload: vec![0xEE],
            });
            let step = s.on_bytes(&b, clock.now());
            assert_eq!(
                frames(&step.write),
                vec![Frame {
                    kind: MessageType::Ack,
                    seq: 1 - seq,
                    payload: vec![]
                }]
            );
        }
    }

    #[test]
    fn events_returned_in_arrival_order_and_junk_dropped() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        let mut b = data(0, &[0xEE, 0xEE]);
        b.extend(data(1, &hex("67 19 01 01 00 00 14 00 00")));
        b.extend(data(0, &hex("23 00 50 01")));
        let step = s.on_bytes(&b, clock.now());
        assert_eq!(frames(&step.write).len(), 3);
        assert_eq!(step.events.len(), 2);
        assert!(matches!(step.events[0], DeviceEvent::NcAmb { .. }));
        assert!(matches!(step.events[1], DeviceEvent::Battery { .. }));
    }

    #[test]
    fn one_command_in_flight() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        let a = s.enqueue(raw(&[0x10]), 1, clock.now());
        let b = s.enqueue(raw(&[0x11]), 2, clock.now());
        assert_eq!(a.write.len(), 1);
        assert!(b.write.is_empty());
        let first = frames(&a.write)[0].seq;
        let step = s.on_bytes(&ack(1 - first), clock.now());
        assert_eq!(step.outcomes, vec![Outcome::Done(1)]);
        let f = frames(&step.write);
        assert_eq!(
            (f.len(), f[0].payload.clone(), f[0].seq),
            (1, vec![0x11], 1 - first)
        );
        let step = s.on_bytes(&ack(first), clock.now());
        assert_eq!(step.outcomes, vec![Outcome::Done(2)]);
        assert!(step.write.is_empty());
    }

    #[test]
    fn timeout_resends_once_then_fails() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        let first = s.enqueue(raw(&[0x10]), 1, clock.now()).write;
        s.enqueue(raw(&[0x11]), 2, clock.now());
        clock.advance(Duration::from_millis(999));
        assert!(s.on_tick(clock.now()).write.is_empty());
        clock.advance(Duration::from_millis(1));
        let step = s.on_tick(clock.now());
        assert_eq!(step.write, first);
        assert!(step.outcomes.is_empty());
        clock.advance(Duration::from_secs(1));
        let step = s.on_tick(clock.now());
        assert_eq!(step.outcomes, vec![Outcome::Failed(1, SendError::Timeout)]);
        assert_eq!(frames(&step.write)[0].payload, vec![0x11]);
    }

    #[test]
    fn disconnect_mid_command_fails_waiter() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        s.enqueue(raw(&[0x10]), 1, clock.now());
        s.enqueue(raw(&[0x11]), 2, clock.now());
        assert_eq!(
            s.on_disconnected(),
            vec![
                Outcome::Failed(1, SendError::Disconnected),
                Outcome::Failed(2, SendError::Disconnected)
            ]
        );
        assert!(s.on_disconnected().is_empty());
        assert_eq!(s.state(), &SessionState::Disconnected);
    }

    #[test]
    fn unannounced_command_is_unsupported() {
        let clock = FakeClock::new();
        let mut s = ready(&support(&[0x6D]), &clock);
        let step = s.enqueue(command::set_dsee(true), 9, clock.now());
        assert!(step.write.is_empty());
        assert_eq!(
            step.outcomes,
            vec![Outcome::Failed(9, SendError::Unsupported)]
        );
        // The probe tool's raw commands are always allowed.
        assert_eq!(s.enqueue(raw(&[0x10]), 10, clock.now()).write.len(), 1);
    }

    #[test]
    fn enqueue_before_ready_fails_disconnected() {
        let clock = FakeClock::new();
        let mut s = SessionCore::new();
        let step = s.enqueue(raw(&[0x10]), 1, clock.now());
        assert_eq!(
            step.outcomes,
            vec![Outcome::Failed(1, SendError::Disconnected)]
        );
        s.on_connected(clock.now());
        let step = s.enqueue(raw(&[0x10]), 2, clock.now());
        assert_eq!(
            step.outcomes,
            vec![Outcome::Failed(2, SendError::Disconnected)]
        );
        assert!(step.write.is_empty());
    }

    #[test]
    fn refresh_every_15s() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        clock.advance(Duration::from_secs(14));
        let step = s.on_tick(clock.now());
        assert!(step.write.is_empty() && step.outcomes.is_empty());
        clock.advance(Duration::from_secs(1));
        let step = s.on_tick(clock.now());
        assert_eq!(frames(&step.write)[0].payload, vec![0xF2, 0x00]);
    }

    #[test]
    fn backoff_sequence() {
        let clock = FakeClock::new();
        let mut s = SessionCore::new();
        let got: Vec<u64> = (0..7).map(|_| s.next_backoff().as_secs()).collect();
        assert_eq!(got, vec![1, 2, 4, 8, 16, 30, 30]);
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        assert_eq!(s.next_backoff(), Duration::from_secs(1));
        s.next_backoff();
        s.on_disconnected();
        s.on_connected(clock.now());
        s.on_bytes(&ack(1), clock.now());
        s.on_bytes(&data(0, &PROTO_INFO), clock.now());
        s.on_bytes(&ack(0), clock.now());
        s.on_bytes(&data(1, &hex(XM6_SUPPORT)), clock.now());
        assert_eq!(s.next_backoff(), Duration::from_secs(1));
    }

    #[test]
    fn refresh_does_not_pile_up() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        let mut max = 0;
        for _ in 0..60 {
            clock.advance(Duration::from_secs(1));
            s.on_tick(clock.now());
            let internal = s.queue.iter().filter(|(id, _)| id.is_none()).count()
                + usize::from(s.in_flight.as_ref().is_some_and(|f| f.id.is_none()));
            max = max.max(internal);
        }
        // One Ready batch for the XM6: 13 state reads, 4 one-off reads and 3 label reads.
        assert!(max > 0 && max <= 20, "internal reads peaked at {max}");
    }

    #[test]
    fn enqueue_timeout_measured_from_enqueue() {
        let clock = FakeClock::new();
        let mut s = ready(&hex(XM6_SUPPORT), &clock);
        s.on_tick(clock.now());
        clock.advance(Duration::from_millis(900));
        let first = s.enqueue(raw(&[0x10]), 1, clock.now()).write;
        assert_eq!(first.len(), 1);
        clock.advance(Duration::from_millis(100));
        assert!(s.on_tick(clock.now()).write.is_empty());
        clock.advance(Duration::from_millis(900));
        assert_eq!(s.on_tick(clock.now()).write, first);
        clock.advance(Duration::from_secs(1));
        let step = s.on_tick(clock.now());
        assert_eq!(step.outcomes, vec![Outcome::Failed(1, SendError::Timeout)]);
    }

    #[test]
    fn unknown_payload_logged_once() {
        let _g = crate::log::tests::LOG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("xm-core-unknown-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::log::init_log_at(&dir, false);
        let mut s = SessionCore::new();
        let t0 = Instant::now();
        s.on_connected(t0);
        let p = [0xC9, 0x01, 0xAB];
        s.on_bytes(&data(0, &p), t0);
        s.on_bytes(&data(1, &[0xC9, 0x01, 0xCD]), t0);
        let text = std::fs::read_to_string(dir.join("app.log")).unwrap();
        assert_eq!(text.matches("unparsed payload").count(), 1, "{text}");
        assert!(text.contains("C9 01 AB"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
