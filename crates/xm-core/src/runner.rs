//! Session thread: connect, run the session core over the transport, back off, repeat.

use crate::clock::Clock;
use crate::command::Command;
use crate::event::DeviceEvent;
use crate::frame::{MessageType, Reassembler};
use crate::session::{Outcome, SendError, SessionCore, SessionState, Step};
use crate::transport::{ConnectError, Connector, DeviceInfo, Transport};
use crate::{log_debug, log_info};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Longest a read, or a backoff sleep, runs before the loop looks at ticks, commands and stop.
const SLICE: Duration = Duration::from_millis(100);
const READY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    State(SessionState),
    Device(DeviceEvent),
    Outcome(Outcome),
    Connected(DeviceInfo),
}

pub struct SessionHandle {
    pub events: mpsc::Receiver<SessionEvent>,
    commands: mpsc::Sender<(u64, Command)>,
    next_id: AtomicU64,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl SessionHandle {
    /// Queues a command and returns the id its `Outcome` will carry.
    pub fn send(&self, cmd: Command) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        // A send failure means the thread has stopped; the caller simply gets no outcome.
        let _ = self.commands.send((id, cmd));
        id
    }

    /// May block until a pending Bluetooth connect or write completes.
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
    }
}

pub fn start(
    connector: Box<dyn Connector>,
    clock: Arc<dyn Clock>,
    preferred: Option<String>,
) -> SessionHandle {
    let (event_tx, events) = mpsc::channel();
    let (commands, command_rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let mut runner = Runner {
        connector,
        clock,
        preferred,
        core: SessionCore::new(),
        events: event_tx,
        commands: command_rx,
        stop: stop.clone(),
    };
    let thread = std::thread::spawn(move || runner.run());
    SessionHandle {
        events,
        commands,
        next_id: AtomicU64::new(1),
        stop,
        thread,
    }
}

struct Runner {
    connector: Box<dyn Connector>,
    clock: Arc<dyn Clock>,
    preferred: Option<String>,
    core: SessionCore,
    events: mpsc::Sender<SessionEvent>,
    commands: mpsc::Receiver<(u64, Command)>,
    stop: Arc<AtomicBool>,
}

impl Runner {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// A dropped handle disconnects the command channel, which counts as a stop request.
    fn next_command(&self) -> Option<(u64, Command)> {
        match self.commands.try_recv() {
            Ok(c) => Some(c),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.stop.store(true, Ordering::Relaxed);
                None
            }
        }
    }

    fn emit(&self, e: SessionEvent) {
        match &e {
            SessionEvent::State(s) => log_info!("state: {s:?}"),
            SessionEvent::Connected(d) => log_info!("connected: {} ({})", d.name, d.address),
            _ => {}
        }
        // The consumer dropping its receiver is a stop request.
        if self.events.send(e).is_err() {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn run(&mut self) {
        while !self.stopped() {
            log_info!("connecting (preferred {:?})", self.preferred);
            let result = self.connector.connect(self.preferred.as_deref());
            if let Err(e) = &result {
                log_info!("connect failed: {e:?}");
            }
            match result {
                Ok((transport, info)) => {
                    self.core.set_model_name(&info.name);
                    self.emit(SessionEvent::Connected(info));
                    self.run_connected(transport);
                }
                Err(e) => self.emit(SessionEvent::State(match e {
                    ConnectError::BluetoothOff => SessionState::BluetoothOff,
                    ConnectError::NotConnected => SessionState::NotConnected,
                    ConnectError::InUse => SessionState::InUse,
                    ConnectError::NoDevice | ConnectError::Other(_) => SessionState::Disconnected,
                })),
            }
            self.back_off();
        }
    }

    /// Waits out the backoff in short slices; commands sent meanwhile fail as disconnected.
    fn back_off(&mut self) {
        let mut left = self.core.next_backoff();
        while !left.is_zero() && !self.stopped() {
            let d = left.min(SLICE);
            self.clock.sleep(d);
            left -= d;
            while let Some((id, _)) = self.next_command() {
                self.emit(SessionEvent::Outcome(Outcome::Failed(
                    id,
                    SendError::Disconnected,
                )));
            }
        }
    }

    /// Returns when the link fails, the headset never becomes ready, or stop is requested.
    fn run_connected(&mut self, mut transport: Box<dyn Transport>) {
        let started = self.clock.now();
        let mut ready = false;
        self.emit(SessionEvent::State(SessionState::Connecting));
        let hello = Step {
            write: self.core.on_connected(started),
            ..Step::default()
        };
        let mut buf = [0u8; 1024];
        let mut rx_log = Reassembler::default();
        let mut alive = self.apply(hello, &mut *transport, &mut ready);
        while alive && !self.stopped() {
            while let Some((id, cmd)) = self.next_command() {
                let step = self.core.enqueue(cmd, id, self.clock.now());
                alive &= self.apply(step, &mut *transport, &mut ready);
            }
            match transport.read(&mut buf, SLICE) {
                Ok(0) => alive = false,
                Ok(n) => {
                    log_frames("rx", &mut rx_log, &buf[..n]);
                    let step = self.core.on_bytes(&buf[..n], self.clock.now());
                    alive &= self.apply(step, &mut *transport, &mut ready);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(_) => alive = false,
            }
            let now = self.clock.now();
            let step = self.core.on_tick(now);
            alive &= self.apply(step, &mut *transport, &mut ready);
            alive &= ready || now.duration_since(started) < READY_TIMEOUT;
        }
        drop(transport);
        for o in self.core.on_disconnected() {
            self.emit(SessionEvent::Outcome(o));
        }
        self.emit(SessionEvent::State(SessionState::Disconnected));
    }

    /// Writes and publishes a step; false when a write fails (the link is gone).
    fn apply(&mut self, step: Step, transport: &mut dyn Transport, ready: &mut bool) -> bool {
        for e in step.events {
            self.emit(SessionEvent::Device(e));
        }
        for o in step.outcomes {
            self.emit(SessionEvent::Outcome(o));
        }
        if let Some(s) = step.state {
            *ready |= matches!(s, SessionState::Ready(_));
            self.emit(SessionEvent::State(s));
        }
        step.write.iter().all(|w| {
            log_frames("tx", &mut Reassembler::default(), w);
            transport.write_all(w).is_ok()
        })
    }
}

/// Verbose logging only: each complete frame as direction, table, sequence and payload hex.
fn log_frames(dir: &str, re: &mut Reassembler, bytes: &[u8]) {
    if !crate::log::verbose() {
        return;
    }
    for f in re.push(bytes) {
        match f {
            Ok(f) => {
                let table = match f.kind {
                    MessageType::Ack => "ack",
                    MessageType::DataTable1 => "table1",
                    MessageType::DataTable2 => "table2",
                };
                let hex: Vec<String> = f.payload.iter().map(|b| format!("{b:02X}")).collect();
                log_debug!("{dir} {table} seq={} {}", f.seq, hex.join(" "));
            }
            Err(e) => log_debug!("{dir} bad frame: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{FakeClock, SystemClock};
    use crate::command;
    use crate::frame::{encode, Frame, MessageType};
    use crate::session::SendError;
    use crate::transport::{ConnectError, Transport};
    use std::collections::VecDeque;
    use std::io;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    // Payloads from the XM6 connect capture: protocol info, then the support list.
    const PROTO_INFO: [u8; 8] = [0x01, 0x00, 0x03, 0x00, 0x30, 0x32, 0x00, 0x00];
    const CAPTURE: &str =
        include_str!("../../../docs/protocol/captures/2026-10-19-wh-1000xm6-connect.jsonl");

    fn xm6_support() -> Vec<u8> {
        let line = CAPTURE
            .lines()
            .find(|l| l.contains("\"bytes\":\"07 00 2D 10"))
            .unwrap();
        let hex = line
            .split("\"bytes\":\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        hex.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect()
    }

    fn frame(kind: MessageType, seq: u8, payload: &[u8]) -> Vec<u8> {
        encode(&Frame {
            kind,
            seq,
            payload: payload.to_vec(),
        })
    }

    /// The headset side of the handshake, in the order the session consumes it.
    fn handshake_bytes() -> Vec<Vec<u8>> {
        vec![
            frame(MessageType::Ack, 1, &[]),
            frame(MessageType::DataTable1, 0, &PROTO_INFO),
            frame(MessageType::Ack, 0, &[]),
            frame(MessageType::DataTable1, 1, &xm6_support()),
        ]
    }

    /// Reads whatever the test pushes; dropping the sender closes the link.
    struct FakeTransport {
        rx: mpsc::Receiver<Vec<u8>>,
    }

    impl Transport for FakeTransport {
        fn read(&mut self, buf: &mut [u8], _timeout: Duration) -> io::Result<usize> {
            match self.rx.recv_timeout(Duration::from_millis(5)) {
                Ok(b) => {
                    buf[..b.len()].copy_from_slice(&b);
                    Ok(b.len())
                }
                Err(mpsc::RecvTimeoutError::Timeout) => Err(io::ErrorKind::TimedOut.into()),
                Err(mpsc::RecvTimeoutError::Disconnected) => Ok(0),
            }
        }

        fn write_all(&mut self, _bytes: &[u8]) -> io::Result<()> {
            Ok(())
        }
    }

    fn link() -> (mpsc::Sender<Vec<u8>>, Box<dyn Transport>) {
        let (tx, rx) = mpsc::channel();
        (tx, Box::new(FakeTransport { rx }))
    }

    fn info() -> DeviceInfo {
        DeviceInfo {
            name: "WH-1000XM6".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
        }
    }

    /// Hands out scripted results, then `InUse` forever; records when each attempt happened.
    struct FakeConnector {
        script: VecDeque<Result<Box<dyn Transport>, ConnectError>>,
        attempts: Arc<Mutex<Vec<Instant>>>,
        clock: Arc<dyn Clock>,
    }

    impl Connector for FakeConnector {
        fn connect(
            &mut self,
            _preferred: Option<&str>,
        ) -> Result<(Box<dyn Transport>, DeviceInfo), ConnectError> {
            self.attempts.lock().unwrap().push(self.clock.now());
            self.script
                .pop_front()
                .unwrap_or(Err(ConnectError::InUse))
                .map(|t| (t, info()))
        }
    }

    type Attempts = Arc<Mutex<Vec<Instant>>>;

    fn begin(
        clock: Arc<dyn Clock>,
        script: Vec<Result<Box<dyn Transport>, ConnectError>>,
    ) -> (SessionHandle, Attempts) {
        let attempts: Attempts = Arc::default();
        let c = FakeConnector {
            script: script.into(),
            attempts: attempts.clone(),
            clock: clock.clone(),
        };
        (start(Box::new(c), clock, None), attempts)
    }

    fn next(h: &SessionHandle) -> SessionEvent {
        h.events
            .recv_timeout(Duration::from_secs(5))
            .expect("no event within 5 s")
    }

    /// Skips events until `want` matches, returning everything skipped plus the match.
    fn until(h: &SessionHandle, want: impl Fn(&SessionEvent) -> bool) -> Vec<SessionEvent> {
        let mut seen = Vec::new();
        loop {
            let e = next(h);
            let hit = want(&e);
            seen.push(e);
            if hit {
                return seen;
            }
        }
    }

    fn is_ready(e: &SessionEvent) -> bool {
        matches!(e, SessionEvent::State(SessionState::Ready(_)))
    }

    fn is_state(s: SessionState) -> impl Fn(&SessionEvent) -> bool {
        move |e| *e == SessionEvent::State(s.clone())
    }

    #[test]
    fn connects_and_reaches_ready() {
        let clock = Arc::new(FakeClock::new());
        let (tx, t) = link();
        let (h, _) = begin(clock, vec![Ok(t)]);
        assert_eq!(next(&h), SessionEvent::Connected(info()));
        assert_eq!(next(&h), SessionEvent::State(SessionState::Connecting));
        for b in handshake_bytes() {
            tx.send(b).unwrap();
        }
        let seen = until(&h, is_ready);
        assert!(seen
            .iter()
            .any(|e| matches!(e, SessionEvent::Device(DeviceEvent::ProtocolInfo { .. }))));
        assert!(seen
            .iter()
            .any(|e| matches!(e, SessionEvent::Device(DeviceEvent::SupportFunctions(_)))));
        h.stop();
    }

    #[test]
    fn dropped_link_disconnects_then_reconnects_after_backoff() {
        let clock = Arc::new(FakeClock::new());
        let (tx1, t1) = link();
        let (_tx2, t2) = link();
        let (h, attempts) = begin(clock, vec![Ok(t1), Ok(t2)]);
        for b in handshake_bytes() {
            tx1.send(b).unwrap();
        }
        until(&h, is_ready);
        drop(tx1);
        until(&h, is_state(SessionState::Disconnected));
        until(&h, |e| matches!(e, SessionEvent::Connected(_)));
        let a = attempts.lock().unwrap().clone();
        assert_eq!(a.len(), 2);
        assert!(a[1] - a[0] >= Duration::from_secs(1));
        h.stop();
    }

    #[test]
    fn busy_service_maps_to_in_use() {
        let clock = Arc::new(FakeClock::new());
        let (h, attempts) = begin(clock, vec![]);
        assert_eq!(next(&h), SessionEvent::State(SessionState::InUse));
        for _ in 0..4 {
            until(&h, is_state(SessionState::InUse));
        }
        h.stop();
        let a = attempts.lock().unwrap().clone();
        let secs: Vec<u64> = a[..5].iter().map(|t| (*t - a[0]).as_secs()).collect();
        assert_eq!(secs, [0, 1, 3, 7, 15]);
    }

    #[test]
    fn connector_errors_map_to_states() {
        let clock = Arc::new(FakeClock::new());
        let script = vec![
            Err(ConnectError::BluetoothOff),
            Err(ConnectError::NoDevice),
            Err(ConnectError::NotConnected),
            Err(ConnectError::Other("boom".into())),
        ];
        let (h, _) = begin(clock, script);
        let states: Vec<SessionState> = (0..4)
            .map(|_| match next(&h) {
                SessionEvent::State(s) => s,
                e => panic!("{e:?}"),
            })
            .collect();
        assert_eq!(
            states,
            [
                SessionState::BluetoothOff,
                SessionState::Disconnected,
                SessionState::NotConnected,
                SessionState::Disconnected
            ]
        );
        h.stop();
    }

    #[test]
    fn silent_headset_hits_ready_timeout() {
        let clock = Arc::new(FakeClock::new());
        let (_tx, t) = link();
        let (h, _) = begin(clock.clone(), vec![Ok(t)]);
        until(&h, is_state(SessionState::Connecting));
        clock.advance(Duration::from_secs(11));
        until(&h, is_state(SessionState::Disconnected));
        h.stop();
    }

    #[test]
    fn send_while_disconnected_fails_with_a_fresh_id() {
        let clock = Arc::new(FakeClock::new());
        let (h, _) = begin(clock, vec![]);
        let a = h.send(command::power_off());
        let b = h.send(command::power_off());
        assert_eq!((a, b), (1, 2));
        let seen = until(&h, |e| {
            *e == SessionEvent::Outcome(Outcome::Failed(2, SendError::Disconnected))
        });
        assert!(seen.contains(&SessionEvent::Outcome(Outcome::Failed(
            1,
            SendError::Disconnected
        ))));
        h.stop();
    }

    #[test]
    fn dropping_handle_stops_session() {
        struct Watched(FakeTransport, Arc<AtomicBool>);
        impl Transport for Watched {
            fn read(&mut self, buf: &mut [u8], t: Duration) -> io::Result<usize> {
                self.0.read(buf, t)
            }
            fn write_all(&mut self, b: &[u8]) -> io::Result<()> {
                self.0.write_all(b)
            }
        }
        impl Drop for Watched {
            fn drop(&mut self) {
                self.1.store(true, Ordering::Relaxed);
            }
        }
        let closed = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let t: Box<dyn Transport> = Box::new(Watched(FakeTransport { rx }, closed.clone()));
        let (h, _) = begin(Arc::new(FakeClock::new()), vec![Ok(t)]);
        for b in handshake_bytes() {
            tx.send(b).unwrap();
        }
        until(&h, is_ready);
        drop(h);
        let t = Instant::now();
        while !closed.load(Ordering::Relaxed) {
            assert!(t.elapsed() < Duration::from_secs(1), "transport still open");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn stop_returns_promptly_mid_backoff() {
        let (h, _) = begin(Arc::new(SystemClock), vec![]);
        until(&h, is_state(SessionState::InUse));
        let t = Instant::now();
        h.stop();
        assert!(t.elapsed() < Duration::from_secs(1));
    }
}
