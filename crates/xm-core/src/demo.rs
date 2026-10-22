//! A simulated WH-1000XM6 for `--demo`: answers the handshake, every get, and echoes every set.

use crate::clock::{Clock, SystemClock};
use crate::frame::{encode, Frame, MessageType, Reassembler};
use crate::transport::{ConnectError, Connector, DeviceInfo, Transport};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CAPTURE: &str =
    include_str!("../../../docs/protocol/captures/2026-10-19-wh-1000xm6-connect.jsonl");
const PROTO_INFO: [u8; 8] = [0x01, 0x00, 0x03, 0x00, 0x30, 0x32, 0x00, 0x00];
const DROP_EVERY: Duration = Duration::from_secs(45);
const BATTERY_FLOOR: u8 = 5;
/// A real headset resends a data frame once if the host has not ACKed it by then.
const RESEND_AFTER: Duration = Duration::from_secs(1);
/// After a power-off command the demo stays unreachable this long, like a headset switched off and on.
const OFF_FOR: Duration = Duration::from_secs(10);

pub struct DemoConnector {
    battery: u8,
    name: String,
    clock: Arc<dyn Clock>,
    unacked: Arc<AtomicUsize>,
    /// When the host last turned the demo headset off.
    off_at: Arc<Mutex<Option<Instant>>>,
}

impl DemoConnector {
    pub fn new() -> Self {
        Self {
            battery: 82,
            name: "WH-1000XM6 (demo)".into(),
            clock: Arc::new(SystemClock),
            unacked: Arc::default(),
            off_at: Arc::default(),
        }
    }

    pub fn with_battery(mut self, level: u8) -> Self {
        self.battery = level;
        self
    }

    /// A different device name, to check how long names lay out.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// How many data frames the demo has sent that the host has not ACKed.
    pub fn unacked(&self) -> usize {
        self.unacked.load(Ordering::Relaxed)
    }

    /// The same count as a handle that outlives boxing the connector into a runner.
    pub fn unacked_counter(&self) -> Arc<AtomicUsize> {
        self.unacked.clone()
    }

    /// Times the battery drops on this clock instead of the wall clock.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }
}

impl Default for DemoConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for DemoConnector {
    fn connect(
        &mut self,
        _preferred: Option<&str>,
    ) -> Result<(Box<dyn Transport>, DeviceInfo), ConnectError> {
        let now = self.clock.now();
        if let Ok(mut off) = self.off_at.lock() {
            if off.is_some_and(|t| now.duration_since(t) < OFF_FOR) {
                return Err(ConnectError::NotConnected);
            }
            *off = None;
        }
        let info = DeviceInfo {
            name: self.name.clone(),
            address: "00:00:00:00:00:00".into(),
        };
        let mut t = DemoTransport::new(self.clock.clone(), self.battery);
        t.count = self.unacked.clone();
        t.off_at = self.off_at.clone();
        Ok((Box::new(t), info))
    }
}

/// The support-list payload the real XM6 sent, taken from the connect capture.
fn xm6_support() -> Vec<u8> {
    let line = CAPTURE
        .lines()
        .find(|l| l.contains("\"bytes\":\"07 00 2D 10"))
        .expect("capture has the support list");
    line.split("\"bytes\":\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("capture line has bytes")
        .split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).expect("hex byte"))
        .collect()
}

/// The XM6's nine presets with their curves in dB, 31 Hz to 16 kHz. The built-ins are the factory
/// curves a real XM6 reported on 21 Oct 2026; Manual and the Custom presets start flat and keep
/// what the app writes.
pub const XM6_CURVES: [(u8, [i8; 10]); 9] = [
    (0x00, [0; 10]),
    (0x20, [-6, -2, 2, 3, 1, -1, 1, 3, 0, -2]),
    (0x30, [4, 4, -1, -1, 0, 0, 0, 0, 0, 0]),
    (0x31, [0, 0, 0, 0, 0, 0, 0, 2, 2, 3]),
    (0x32, [4, 4, -1, -1, 0, 0, 0, 2, 2, 3]),
    (0x33, [-3, -2, -1, 1, 1, 1, 0, 0, -2, -3]),
    (0xA0, [0; 10]),
    (0xA1, [0; 10]),
    (0xA2, [0; 10]),
];

/// A data frame sent to the host and not yet ACKed.
struct Sent {
    seq: u8,
    bytes: Vec<u8>,
    at: Instant,
    resent: bool,
}

pub struct DemoTransport {
    unacked: VecDeque<Sent>,
    count: Arc<AtomicUsize>,
    clock: Arc<dyn Clock>,
    rx: Reassembler,
    out: VecDeque<Vec<u8>>,
    /// The headset's own data sequence number, alternating 0 and 1.
    seq: u8,
    /// Settings keyed by (get opcode, sub id); the value is the payload after those two bytes.
    state: HashMap<(u8, u8), Vec<u8>>,
    /// Each preset's bands as wire bytes (dB + 6), so switching back shows its own curve.
    curves: HashMap<u8, Vec<u8>>,
    support: Vec<u8>,
    battery: u8,
    battery_kind: u8,
    last_drop: Instant,
    off_at: Arc<Mutex<Option<Instant>>>,
    /// Set by a power-off command; the link closes once the ACK has gone out.
    powered_off: bool,
}

impl DemoTransport {
    pub fn new(clock: Arc<dyn Clock>, battery: u8) -> Self {
        let curves: HashMap<u8, Vec<u8>> = XM6_CURVES
            .iter()
            .map(|(p, c)| (*p, c.iter().map(|b| (b + 6) as u8).collect()))
            .collect();
        let mut eq = vec![0x20, 10];
        eq.extend(&curves[&0x20]);
        let mut firmware = vec![5];
        firmware.extend(b"3.1.5");
        // One source: connected (state 1) and playing (trailing slot 1).
        let mut devices = vec![1];
        devices.extend(b"02:1D:55:0D:7D:AF");
        devices.extend([0x01, 0x00, 0x00, 0x00, 4]);
        devices.extend(b"Desk");
        devices.push(0x01);
        let label = |name: &str| {
            let summary = format!("{name}_SUMMARY");
            let mut v = vec![0x00, 0x01, name.len() as u8];
            v.extend(name.as_bytes());
            v.push(summary.len() as u8);
            v.extend(summary.as_bytes());
            v
        };
        let mut state = HashMap::from([
            ((0x66, 0x19), vec![0x01, 0x01, 0x00, 0x00, 0x0A, 0x00, 0x00]),
            ((0x52, 0x04), vec![0x00, 0x00]),
            ((0x56, 0x04), eq),
            ((0xF6, 0x0C), vec![0x01, 0x01]),
            ((0xF6, 0x01), vec![0x00]),
            ((0xE6, 0x01), vec![0x01]),
            ((0x26, 0x05), vec![0x10, 0x00]),
            ((0xF2, 0x00), vec![0x00]),
            ((0x04, 0x02), firmware),
            ((0x12, 0x02), vec![0x10]),
            ((0x04, 0x03), vec![0x30, 0x03]),
            ((0xE6, 0x09), vec![0x01, 0x02]),
            ((0xE6, 0x04), vec![0x01]),
            ((0x36, 0x02), devices),
            ((0x66, 0x30), vec![0x02]),
            ((0x46, 0x20), vec![0x00]),
        ]);
        for (slot, name) in [
            (0xD1, "SIDETONE_SETTING"),
            (0xD2, "MULTIPOINT_SETTING"),
            (0xD3, "TOUCH_PANEL_SETTING"),
        ] {
            state.insert((0xD0, slot), label(name));
            state.insert((0xD6, slot), vec![0x00, 0x00]);
        }
        let last_drop = clock.now();
        Self {
            unacked: VecDeque::new(),
            count: Arc::default(),
            clock,
            rx: Reassembler::default(),
            out: VecDeque::new(),
            seq: 0,
            state,
            curves,
            support: xm6_support(),
            battery,
            battery_kind: 0,
            last_drop,
            off_at: Arc::default(),
            powered_off: false,
        }
    }

    pub fn battery_level(&self) -> u8 {
        self.battery
    }

    fn push(&mut self, kind: MessageType, payload: Vec<u8>) {
        let seq = self.seq;
        self.seq ^= 1;
        let bytes = encode(&Frame { kind, seq, payload });
        self.out.push_back(bytes.clone());
        self.unacked.push_back(Sent {
            seq,
            bytes,
            at: self.clock.now(),
            resent: false,
        });
        self.publish_count();
    }

    pub fn unacked(&self) -> usize {
        self.unacked.len()
    }

    fn publish_count(&self) {
        self.count.store(self.unacked.len(), Ordering::Relaxed);
    }

    /// The host's ACK for a data frame carries `1 - seq` of that frame.
    fn on_ack(&mut self, ack_seq: u8) {
        let sent_seq = 1u8.wrapping_sub(ack_seq);
        if let Some(i) = self.unacked.iter().position(|s| s.seq == sent_seq) {
            self.unacked.remove(i);
            self.publish_count();
        }
    }

    fn resend_overdue(&mut self) {
        let now = self.clock.now();
        for s in &mut self.unacked {
            if !s.resent && now.duration_since(s.at) >= RESEND_AFTER {
                s.resent = true;
                s.at = now;
                self.out.push_back(s.bytes.clone());
            }
        }
    }

    fn battery_payload(&self, op: u8) -> Vec<u8> {
        let b = self.battery;
        match self.battery_kind {
            1 => vec![op, 1, b, 0, b, 0],
            k => vec![op, k, b, 0],
        }
    }

    fn handle(&mut self, kind: MessageType, seq: u8, p: &[u8]) {
        let (Some(&op), Some(&sub)) = (p.first(), p.get(1)) else {
            return;
        };
        self.out.push_back(encode(&Frame {
            kind: MessageType::Ack,
            seq: 1u8.wrapping_sub(seq),
            payload: vec![],
        }));
        // A get's reply opcode is get + 1, its set is get + 2, and the set's echo is get + 3.
        let reply = match (op, sub) {
            (0x00, 0x00) => Some(PROTO_INFO.to_vec()),
            (0x06, 0x00) => Some(self.support.clone()),
            (0x24, 0x03) => {
                self.powered_off = true;
                if let Ok(mut off) = self.off_at.lock() {
                    *off = Some(self.clock.now());
                }
                None
            }
            (0x22, k) => {
                self.battery_kind = k;
                Some(self.battery_payload(0x23))
            }
            (
                0x52 | 0x56 | 0x66 | 0xF6 | 0xE6 | 0x26 | 0xF2 | 0x04 | 0x12 | 0x36 | 0x46 | 0xD0
                | 0xD6,
                _,
            ) => self.state.get(&(op, sub)).map(|v| {
                let mut r = vec![op + 1, sub];
                r.extend(v);
                r
            }),
            (0x58 | 0x68 | 0xF8 | 0xE8 | 0x28 | 0xD8, _) => {
                let mut v = p[2..].to_vec();
                if op == 0x58 && v.len() > 2 {
                    self.curves.insert(v[0], v[2..].to_vec());
                }
                // A preset-only set (no bands) takes that preset's curve, or else keeps the bands
                // already held, for the next read.
                if (op, v.get(1)) == (0x58, Some(&0)) {
                    if let Some(c) = self.curves.get(&v[0]) {
                        v = [&[v[0], c.len() as u8][..], c].concat();
                    } else if let Some(old) = self.state.get(&(0x56, sub)).filter(|o| o.len() > 2) {
                        v = [&v[..1], &old[1..]].concat();
                    }
                }
                let mut echo = vec![op + 1, sub];
                echo.extend(&v);
                self.state.insert((op - 2, sub), v);
                Some(echo)
            }
            _ => None,
        };
        if let Some(r) = reply {
            self.push(kind, r);
        }
    }

    fn drop_battery(&mut self) {
        let now = self.clock.now();
        while now.duration_since(self.last_drop) >= DROP_EVERY && self.battery > BATTERY_FLOOR {
            self.last_drop += DROP_EVERY;
            self.battery -= 1;
            let p = self.battery_payload(0x25);
            self.push(MessageType::DataTable1, p);
        }
    }
}

impl Transport for DemoTransport {
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.drop_battery();
        self.resend_overdue();
        if let Some(f) = self.out.pop_front() {
            buf[..f.len()].copy_from_slice(&f);
            return Ok(f.len());
        }
        if self.powered_off {
            return Ok(0);
        }
        // Stands in for waiting on a socket; the fake clock never advances by itself.
        std::thread::sleep(timeout.min(Duration::from_millis(10)));
        Err(io::ErrorKind::TimedOut.into())
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        for f in self.rx.push(bytes).into_iter().flatten() {
            if f.kind == MessageType::Ack {
                self.on_ack(f.seq);
            } else {
                self.handle(f.kind, f.seq, &f.payload);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::command::{set_nc_amb, NcAmb, NcAmbMode};
    use crate::event::DeviceEvent;
    use crate::runner::{start, SessionEvent};
    use crate::session::SessionState;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::Duration;

    fn next(h: &crate::runner::SessionHandle) -> SessionEvent {
        h.events
            .recv_timeout(Duration::from_secs(5))
            .expect("no event")
    }

    #[test]
    fn demo_answers_the_probed_reads() {
        use crate::command::{set_general_setting, BgmRoom};
        use crate::event::{Codec, HeadsetColourCode};
        let clock = Arc::new(FakeClock::new());
        let h = start(
            Box::new(DemoConnector::new().with_clock(clock.clone())),
            clock,
            None,
        );
        let want = |e: &DeviceEvent| match e {
            DeviceEvent::Firmware(v) => v == "3.1.5",
            DeviceEvent::Codec(c) => *c == Codec::Ldac,
            DeviceEvent::Colour { series, colour } => {
                (*series, *colour) == (0x30, HeadsetColourCode::Silver)
            }
            DeviceEvent::Bgm { enabled, room } => !enabled && *room == BgmRoom::Cafe,
            DeviceEvent::Cinema { enabled } => !enabled,
            DeviceEvent::DeviceList(d) => d.len() == 1 && d[0].connected && d[0].playing,
            DeviceEvent::NcButton(b) => *b == 2,
            DeviceEvent::VoiceGuidanceVolume(v) => *v == 0,
            DeviceEvent::GeneralSettingInfo { .. }
            | DeviceEvent::GeneralSetting { on: true, .. } => true,
            _ => false,
        };
        let mut seen = Vec::new();
        let mut labels = Vec::new();
        while seen.len() < 14 {
            if let SessionEvent::Device(e) = next(&h) {
                if let DeviceEvent::GeneralSettingInfo { slot, label } = &e {
                    labels.push((*slot, label.clone()));
                }
                if want(&e) && !seen.contains(&e) {
                    seen.push(e);
                }
            }
        }
        labels.sort();
        assert_eq!(
            labels,
            [
                (1, "SIDETONE_SETTING".to_string()),
                (2, "MULTIPOINT_SETTING".to_string()),
                (3, "TOUCH_PANEL_SETTING".to_string())
            ]
        );
        h.send(set_general_setting(1, false));
        while next(&h) != SessionEvent::Device(DeviceEvent::GeneralSetting { slot: 1, on: false }) {
        }
        h.stop();
    }

    #[test]
    fn demo_power_off_closes_then_reconnects_later() {
        let clock = Arc::new(FakeClock::new());
        let mut c = DemoConnector::new().with_clock(clock.clone());
        let (mut t, _) = c.connect(None).unwrap();
        t.write_all(&encode(&Frame {
            kind: MessageType::DataTable1,
            seq: 0,
            payload: crate::command::power_off().payload,
        }))
        .unwrap();
        let mut buf = [0u8; 64];
        // The ACK still goes out, then the link reads as closed.
        assert!(t.read(&mut buf, Duration::ZERO).unwrap() > 0);
        assert_eq!(t.read(&mut buf, Duration::ZERO).unwrap(), 0);
        assert_eq!(c.connect(None).err(), Some(ConnectError::NotConnected));
        clock.advance(OFF_FOR);
        assert!(c.connect(None).is_ok());
    }

    #[test]
    fn demo_preset_keeps_bands() {
        let mut d = DemoTransport::new(Arc::new(FakeClock::new()), 80);
        d.handle(MessageType::DataTable1, 0, &[0x58, 0x04, 0x11, 0x00]);
        let v = &d.state[&(0x56, 0x04)];
        assert_eq!(v[0], 0x11);
        assert_eq!(v.len(), 12);
        assert_eq!(v[1], 10);
    }

    #[test]
    fn demo_presets_have_the_xm6_curves() {
        let ids: Vec<u8> = XM6_CURVES.iter().map(|(p, _)| *p).collect();
        assert_eq!(ids, [0x00, 0x20, 0x30, 0x31, 0x32, 0x33, 0xA0, 0xA1, 0xA2]);
        assert!(XM6_CURVES
            .iter()
            .all(|(_, c)| c.len() == 10 && c.iter().all(|b| b.abs() <= 6)));
        let mut d = DemoTransport::new(Arc::new(FakeClock::new()), 80);
        let wire = |c: &[i8; 10]| c.iter().map(|b| (b + 6) as u8).collect::<Vec<u8>>();
        // Each preset switch reads back that preset's curve.
        for (p, c) in XM6_CURVES {
            d.handle(MessageType::DataTable1, 0, &[0x58, 0x04, p, 0x00]);
            assert_eq!(d.state[&(0x56, 0x04)], [vec![p, 10], wire(&c)].concat());
        }
        // A custom preset keeps what was written to it across a switch away and back.
        let mine = [vec![0x58, 0x04, 0xA1, 10], vec![9; 10]].concat();
        d.handle(MessageType::DataTable1, 0, &mine);
        d.handle(MessageType::DataTable1, 0, &[0x58, 0x04, 0x30, 0x00]);
        d.handle(MessageType::DataTable1, 0, &[0x58, 0x04, 0xA1, 0x00]);
        assert_eq!(d.state[&(0x56, 0x04)][2..], [9; 10]);
    }

    #[test]
    fn demo_reaches_ready_and_echoes_sets() {
        let clock = Arc::new(FakeClock::new());
        let t0 = clock.now();
        let h = start(
            Box::new(DemoConnector::new().with_clock(clock.clone())),
            clock.clone(),
            None,
        );
        loop {
            if matches!(next(&h), SessionEvent::State(SessionState::Ready(_))) {
                break;
            }
        }
        h.send(set_nc_amb(
            0x19,
            &NcAmb {
                mode: NcAmbMode::Ambient,
                ambient_level: 14,
                focus_on_voice: false,
                adaptive: Some((0, 0)),
            },
        ));
        loop {
            if let SessionEvent::Device(DeviceEvent::NcAmb {
                mode,
                ambient_level,
                ..
            }) = next(&h)
            {
                if ambient_level == 14 {
                    assert_eq!(mode, NcAmbMode::Ambient);
                    break;
                }
            }
        }
        assert!(clock.now() - t0 < Duration::from_millis(200));
        h.stop();
    }

    #[test]
    fn session_acks_demo_notifies() {
        let clock = Arc::new(FakeClock::new());
        let demo = DemoConnector::new().with_clock(clock.clone());
        let counter = demo.unacked_counter();
        let h = start(Box::new(demo), clock.clone(), None);
        loop {
            if matches!(next(&h), SessionEvent::State(SessionState::Ready(_))) {
                break;
            }
        }
        h.send(set_nc_amb(
            0x19,
            &NcAmb {
                mode: NcAmbMode::Ambient,
                ambient_level: 14,
                focus_on_voice: false,
                adaptive: Some((0, 0)),
            },
        ));
        loop {
            if let SessionEvent::Device(DeviceEvent::NcAmb {
                ambient_level: 14, ..
            }) = next(&h)
            {
                break;
            }
        }
        // The host writes its ACK just after publishing the event.
        let t = std::time::Instant::now();
        while counter.load(Ordering::Relaxed) != 0 {
            assert!(
                t.elapsed() < Duration::from_secs(2),
                "demo frames left unACKed"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        h.stop();
    }

    #[test]
    fn demo_resends_an_unacked_frame_once_after_1_s() {
        let clock = Arc::new(FakeClock::new());
        let mut t = DemoTransport::new(clock.clone(), 82);
        let mut buf = [0u8; 1024];
        let hello = encode(&Frame {
            kind: MessageType::DataTable1,
            seq: 0,
            payload: vec![0x00, 0x00],
        });
        t.write_all(&hello).unwrap();
        // ACK, then the protocol info, which nobody ACKs.
        for _ in 0..2 {
            t.read(&mut buf, Duration::ZERO).unwrap();
        }
        assert_eq!(t.unacked(), 1);
        assert!(t.read(&mut buf, Duration::ZERO).is_err());
        clock.advance(Duration::from_secs(1));
        let n = t.read(&mut buf, Duration::ZERO).unwrap();
        assert_eq!(crate::frame::decode(&buf[..n]).unwrap().payload, PROTO_INFO);
        clock.advance(Duration::from_secs(5));
        assert!(t.read(&mut buf, Duration::ZERO).is_err());
        let ack = encode(&Frame {
            kind: MessageType::Ack,
            seq: 1,
            payload: vec![],
        });
        t.write_all(&ack).unwrap();
        assert_eq!(t.unacked(), 0);
    }

    #[test]
    fn demo_battery_starts_at_82_and_drops_every_45_s_to_a_floor_of_5() {
        let clock = Arc::new(FakeClock::new());
        let mut t = DemoTransport::new(clock.clone(), 82);
        let mut buf = [0u8; 1024];
        assert_eq!(t.battery_level(), 82);
        clock.advance(Duration::from_secs(44));
        let _ = t.read(&mut buf, Duration::ZERO);
        assert_eq!(t.battery_level(), 82);
        clock.advance(Duration::from_secs(46));
        let n = t.read(&mut buf, Duration::ZERO).unwrap();
        assert_eq!(t.battery_level(), 80);
        let f = crate::frame::decode(&buf[..n]).unwrap();
        assert_eq!(f.payload, [0x25, 0x00, 81, 0x00]);
        clock.advance(Duration::from_secs(45 * 200));
        while t.read(&mut buf, Duration::ZERO).is_ok() {}
        assert_eq!(t.battery_level(), 5);
    }
}
