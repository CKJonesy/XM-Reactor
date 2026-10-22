//! Probe tool: lists headsets, watches events, captures frames and sends raw commands.
//!
//! Exit codes: 0 ok, 1 usage, 2 no headset Ready within 20 s, 3 command failed.

use std::fmt::Write as _;
use xm_core::command::Table;

const USAGE: &str = "usage: xm-probe scan\n\
       xm-probe watch [--verbose]\n\
       xm-probe capture <file.jsonl>\n\
       xm-probe raw <hex...> [--table2] [--wait <ms>] [--confirm]\n\
\n\
hex is space-, dash- or un-separated byte pairs (04 02, 04-02, 0402).\n\
raw refuses any payload starting 24 (power off family) unless --confirm is given.";

#[derive(Debug, PartialEq, Eq)]
enum Cli {
    Scan,
    Watch {
        verbose: bool,
    },
    Capture {
        file: String,
    },
    Raw {
        table: Table,
        payload: Vec<u8>,
        wait_ms: u64,
    },
}

#[derive(Debug, PartialEq, Eq)]
enum ArgError {
    Usage(String),
    NeedsConfirm,
}

fn usage(msg: &str) -> ArgError {
    ArgError::Usage(msg.to_string())
}

/// Accepts `04 02`, `04-02` and `0402`; each token is split on dashes, then into byte pairs.
fn parse_hex(tokens: &[String]) -> Result<Vec<u8>, ArgError> {
    let mut out = Vec::new();
    for part in tokens.iter().flat_map(|t| t.split('-')) {
        if part.is_empty() || part.len() % 2 != 0 || !part.is_ascii() {
            return Err(ArgError::Usage(format!("bad hex: {part:?}")));
        }
        for i in (0..part.len()).step_by(2) {
            let byte = u8::from_str_radix(&part[i..i + 2], 16)
                .map_err(|_| ArgError::Usage(format!("bad hex: {part:?}")))?;
            out.push(byte);
        }
    }
    if out.is_empty() {
        return Err(usage("raw needs at least one byte"));
    }
    Ok(out)
}

fn parse_args<S: AsRef<str>>(args: &[S]) -> Result<Cli, ArgError> {
    let args: Vec<String> = args.iter().map(|a| a.as_ref().to_string()).collect();
    let Some((cmd, rest)) = args.split_first() else {
        return Err(usage("missing command"));
    };
    match cmd.as_str() {
        "scan" if rest.is_empty() => Ok(Cli::Scan),
        "watch" => match rest {
            [] => Ok(Cli::Watch { verbose: false }),
            [f] if f == "--verbose" => Ok(Cli::Watch { verbose: true }),
            _ => Err(usage("watch takes only --verbose")),
        },
        "capture" => match rest {
            [file] if !file.starts_with("--") => Ok(Cli::Capture { file: file.clone() }),
            _ => Err(usage("capture takes one file path")),
        },
        "raw" => {
            let (mut table, mut wait_ms, mut confirm) = (Table::One, 800, false);
            let mut hex = Vec::new();
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--table2" => table = Table::Two,
                    "--confirm" => confirm = true,
                    "--wait" => {
                        wait_ms = it
                            .next()
                            .and_then(|v| v.parse().ok())
                            .ok_or_else(|| usage("--wait needs milliseconds"))?;
                    }
                    f if f.starts_with("--") => {
                        return Err(ArgError::Usage(format!("unknown flag {f}")))
                    }
                    _ => hex.push(a.clone()),
                }
            }
            let payload = parse_hex(&hex)?;
            if payload[0] == 0x24 && !confirm {
                return Err(ArgError::NeedsConfirm);
            }
            Ok(Cli::Raw {
                table,
                payload,
                wait_ms,
            })
        }
        other => Err(ArgError::Usage(format!("unknown command {other}"))),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        if !s.is_empty() {
            s.push(' ');
        }
        let _ = write!(s, "{b:02X}");
        s
    })
}

/// UTC RFC 3339 with millisecond precision, from days-since-epoch civil conversion.
fn rfc3339(since_epoch: std::time::Duration) -> String {
    let secs = since_epoch.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        since_epoch.subsec_millis()
    )
}

fn json_line(dir: &str, table: Table, bytes: &[u8], at: &str, model: &str) -> String {
    let t = if table == Table::One { 1 } else { 2 };
    let model = model.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "{{\"dir\":\"{dir}\",\"table\":{t},\"bytes\":\"{}\",\"at\":\"{at}\",\"model\":\"{model}\"}}",
        hex(bytes)
    )
}

#[cfg(windows)]
mod app {
    use super::*;
    use std::fs::File;
    use std::io::{self, Write};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use xm_core::bluetooth::{list_devices, RfcommConnector};
    use xm_core::clock::SystemClock;
    use xm_core::command::Command;
    use xm_core::frame::{decode, MessageType, Reassembler};
    use xm_core::runner::{start, SessionEvent, SessionHandle};
    use xm_core::session::{Outcome, SendError, SessionState};
    use xm_core::transport::{ConnectError, Connector, DeviceInfo, Transport};

    const READY_WAIT: Duration = Duration::from_secs(20);
    const ACK_WAIT: Duration = Duration::from_secs(5);

    /// Sees every data frame crossing the link: echoes it and appends it to the capture file.
    struct Observer {
        echo: bool,
        file: Mutex<Option<File>>,
    }

    impl Observer {
        fn frame(&self, dir: &str, kind: MessageType, payload: &[u8], model: &str) {
            let table = if kind == MessageType::DataTable2 {
                Table::Two
            } else {
                Table::One
            };
            let line = json_line(dir, table, payload, &rfc3339(now()), model);
            if self.echo {
                println!("{line}");
            }
            if let Some(f) = self.file.lock().unwrap().as_mut() {
                let _ = writeln!(f, "{line}").and_then(|_| f.flush());
            }
        }
    }

    fn now() -> Duration {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
    }

    struct Observed {
        inner: RfcommConnector,
        obs: Arc<Observer>,
    }

    impl Connector for Observed {
        fn connect(
            &mut self,
            preferred: Option<&str>,
        ) -> Result<(Box<dyn Transport>, DeviceInfo), ConnectError> {
            let (inner, info) = self.inner.connect(preferred)?;
            let t = ObservedTransport {
                inner,
                obs: self.obs.clone(),
                model: info.name.clone(),
                rx: Reassembler::default(),
            };
            Ok((Box::new(t), info))
        }
    }

    struct ObservedTransport {
        inner: Box<dyn Transport>,
        obs: Arc<Observer>,
        model: String,
        rx: Reassembler,
    }

    impl Transport for ObservedTransport {
        fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
            let n = self.inner.read(buf, timeout)?;
            for f in self.rx.push(&buf[..n]).into_iter().flatten() {
                if f.kind != MessageType::Ack {
                    self.obs.frame("rx", f.kind, &f.payload, &self.model);
                }
            }
            Ok(n)
        }

        fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.inner.write_all(bytes)?;
            if let Ok(f) = decode(bytes) {
                if f.kind != MessageType::Ack {
                    self.obs.frame("tx", f.kind, &f.payload, &self.model);
                }
            }
            Ok(())
        }
    }

    fn session(echo: bool, file: Option<File>) -> SessionHandle {
        let obs = Arc::new(Observer {
            echo,
            file: Mutex::new(file),
        });
        let connector = Observed {
            inner: RfcommConnector,
            obs,
        };
        start(Box::new(connector), Arc::new(SystemClock), None)
    }

    /// Prints events until `done` says so or the wait passes; true when `done` fired.
    fn pump(
        h: &SessionHandle,
        wait: Duration,
        mut done: impl FnMut(&SessionEvent) -> bool,
    ) -> bool {
        let end = Instant::now() + wait;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match h.events.recv_timeout(left) {
                Ok(e) => {
                    println!("{e:?}");
                    if done(&e) {
                        return true;
                    }
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    fn wait_ready(h: &SessionHandle) -> bool {
        pump(h, READY_WAIT, |e| {
            matches!(e, SessionEvent::State(SessionState::Ready(_)))
        })
    }

    fn watch(h: SessionHandle) -> i32 {
        if !wait_ready(&h) {
            eprintln!("no headset Ready within 20 s");
            h.stop();
            return 2;
        }
        println!("watching; press Enter to stop");
        let (tx, enter) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = io::stdin().read_line(&mut String::new());
            let _ = tx.send(());
        });
        while enter.try_recv().is_err() {
            pump(&h, Duration::from_millis(200), |_| false);
        }
        h.stop();
        0
    }

    fn raw(h: SessionHandle, table: Table, payload: Vec<u8>, wait_ms: u64) -> i32 {
        if !wait_ready(&h) {
            eprintln!("no headset Ready within 20 s");
            h.stop();
            return 2;
        }
        let id = h.send(Command {
            table,
            payload,
            name: "raw",
        });
        let mut result = None;
        pump(&h, ACK_WAIT, |e| match e {
            SessionEvent::Outcome(Outcome::Done(i)) if *i == id => {
                result = Some(Ok(()));
                true
            }
            SessionEvent::Outcome(Outcome::Failed(i, err)) if *i == id => {
                result = Some(Err(*err));
                true
            }
            _ => false,
        });
        let code = match result {
            Some(Ok(())) => {
                println!("ACK ok");
                0
            }
            Some(Err(SendError::Timeout)) | None => {
                println!("ACK timeout");
                3
            }
            Some(Err(e)) => {
                println!("command failed: {e:?}");
                3
            }
        };
        pump(&h, Duration::from_millis(wait_ms), |_| false);
        h.stop();
        code
    }

    pub fn run(cli: Cli) -> i32 {
        match cli {
            Cli::Scan => match list_devices() {
                Ok(list) => {
                    for (d, up) in list {
                        let up = if up { "yes" } else { "no" };
                        println!("{}  {}  connected: {up}", d.name, d.address);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("scan failed: {e:?}");
                    2
                }
            },
            Cli::Watch { verbose } => watch(session(verbose, None)),
            Cli::Capture { file } => {
                let opened = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file);
                match opened {
                    Ok(f) => watch(session(false, Some(f))),
                    Err(e) => {
                        eprintln!("cannot open {file}: {e}");
                        1
                    }
                }
            }
            Cli::Raw {
                table,
                payload,
                wait_ms,
            } => raw(session(true, None), table, payload, wait_ms),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    let cli = match parse_args(&args) {
        Ok(c) => c,
        Err(ArgError::NeedsConfirm) => {
            eprintln!("refused: payloads starting 24 (power off family) need --confirm");
            std::process::exit(1);
        }
        Err(ArgError::Usage(m)) => {
            eprintln!("{m}\n{USAGE}");
            std::process::exit(1);
        }
    };
    #[cfg(windows)]
    std::process::exit(app::run(cli));
    #[cfg(not(windows))]
    {
        let _ = cli;
        eprintln!("xm-probe needs Windows");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Cli, ArgError> {
        parse_args(a)
    }

    #[test]
    fn power_off_needs_confirm() {
        assert_eq!(p(&["raw", "24", "03", "01"]), Err(ArgError::NeedsConfirm));
        assert_eq!(
            p(&["raw", "24", "03", "01", "--confirm"]),
            Ok(Cli::Raw {
                table: Table::One,
                payload: vec![0x24, 0x03, 0x01],
                wait_ms: 800
            })
        );
    }

    #[test]
    fn hex_accepts_dashes_spaces_and_contiguous() {
        let want = Ok(Cli::Raw {
            table: Table::One,
            payload: vec![0x04, 0x02],
            wait_ms: 800,
        });
        assert_eq!(p(&["raw", "04-02"]), want);
        assert_eq!(p(&["raw", "04", "02"]), want);
        assert_eq!(p(&["raw", "0402"]), want);
    }

    #[test]
    fn raw_flags_and_errors() {
        assert_eq!(
            p(&["raw", "--table2", "36", "02", "--wait", "100"]),
            Ok(Cli::Raw {
                table: Table::Two,
                payload: vec![0x36, 0x02],
                wait_ms: 100
            })
        );
        assert!(matches!(p(&["raw"]), Err(ArgError::Usage(_))));
        assert!(matches!(p(&["raw", "0"]), Err(ArgError::Usage(_))));
        assert!(matches!(p(&["raw", "zz"]), Err(ArgError::Usage(_))));
        assert!(matches!(
            p(&["raw", "04", "--wait"]),
            Err(ArgError::Usage(_))
        ));
        assert!(matches!(p(&["bogus"]), Err(ArgError::Usage(_))));
    }

    #[test]
    fn other_commands_parse() {
        assert_eq!(p(&["scan"]), Ok(Cli::Scan));
        assert_eq!(p(&["watch", "--verbose"]), Ok(Cli::Watch { verbose: true }));
        assert_eq!(
            p(&["capture", "a.jsonl"]),
            Ok(Cli::Capture {
                file: "a.jsonl".into()
            })
        );
        assert!(matches!(p(&["capture"]), Err(ArgError::Usage(_))));
    }

    #[test]
    fn capture_line_matches_existing_format() {
        let at = rfc3339(std::time::Duration::from_millis(1_000_000_000_123));
        assert_eq!(at, "2001-09-09T01:46:40.123Z");
        assert_eq!(
            json_line("rx", Table::Two, &[0xAA, 0x0B], &at, "WH-1000XM6"),
            "{\"dir\":\"rx\",\"table\":2,\"bytes\":\"AA 0B\",\"at\":\"2001-09-09T01:46:40.123Z\",\"model\":\"WH-1000XM6\"}"
        );
    }
}
