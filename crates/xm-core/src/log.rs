//! File log at `%LOCALAPPDATA%\XM Reactor\logs\app.log`. Failures are swallowed: logging must never hurt the app.

use std::fmt::Arguments;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1024 * 1024;
/// After a failed rotation, wait for this much more growth before trying again.
const RETRY_GROWTH: u64 = 256 * 1024;

pub struct Logger {
    dir: PathBuf,
    verbose: bool,
    file: File,
    size: u64,
    /// Rotation is skipped until the file reaches this size (0 when it is not backing off).
    retry_at: u64,
}

static LOGGER: Mutex<Option<Logger>> = Mutex::new(None);

impl Logger {
    pub fn open(dir: &Path, verbose: bool) -> std::io::Result<Logger> {
        std::fs::create_dir_all(dir)?;
        // std opens with FILE_SHARE_READ | WRITE | DELETE, so the log can be viewed while open.
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("app.log"))?;
        let size = file.metadata()?.len();
        Ok(Logger {
            dir: dir.to_path_buf(),
            verbose,
            file,
            size,
            retry_at: 0,
        })
    }

    pub fn write(&mut self, level: &str, msg: &str) {
        let line = format!("{} {level} {msg}\n", timestamp());
        if self.size + line.len() as u64 > MAX_BYTES && self.size >= self.retry_at {
            self.rotate();
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.size += line.len() as u64;
        }
    }

    /// Keeps one old file. The rename replaces any backup in one step, so a failure (say a viewer
    /// holds app.log) leaves both files as they were and the log backs off before trying again.
    fn rotate(&mut self) {
        let renamed = std::fs::rename(self.dir.join("app.log"), self.dir.join("app.1.log"));
        match renamed
            .is_ok()
            .then(|| Logger::open(&self.dir, self.verbose))
        {
            Some(Ok(fresh)) => {
                self.file = fresh.file;
                self.size = 0;
                self.retry_at = 0;
            }
            _ => self.retry_at = self.size + RETRY_GROWTH,
        }
    }
}

/// Local time to the millisecond with its UTC offset (`2026-10-20T16:05:01.123+11:00`);
/// UTC with a `Z` if the local offset cannot be read.
pub fn timestamp() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_timestamp(
        d.as_secs() as i64,
        d.subsec_millis(),
        local_offset_minutes(),
    )
}

/// Minutes east of UTC at this moment (daylight saving included).
#[cfg(windows)]
fn local_offset_minutes() -> Option<i32> {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    // 0 unknown, 1 standard, 2 daylight; Bias is minutes west of UTC.
    let bias = match unsafe { GetTimeZoneInformation(&mut tz) } {
        1 => tz.Bias + tz.StandardBias,
        2 => tz.Bias + tz.DaylightBias,
        0 => tz.Bias,
        _ => return None,
    };
    Some(-bias)
}

#[cfg(not(windows))]
fn local_offset_minutes() -> Option<i32> {
    None
}

fn format_timestamp(utc_secs: i64, ms: u32, offset_min: Option<i32>) -> String {
    let off = offset_min.unwrap_or(0);
    let secs = utc_secs + i64::from(off) * 60;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let t = secs.rem_euclid(86_400);
    let zone = match offset_min {
        Some(m) => format!(
            "{}{:02}:{:02}",
            if m < 0 { '-' } else { '+' },
            m.abs() / 60,
            m.abs() % 60
        ),
        None => "Z".to_string(),
    };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}{zone}",
        t / 3600,
        t % 3600 / 60,
        t % 60
    )
}

pub fn init_log(verbose: bool) {
    init_log_at(&crate::settings::app_dir().join("logs"), verbose);
}

pub fn init_log_at(dir: &Path, verbose: bool) {
    if let Ok(mut g) = LOGGER.lock() {
        *g = Logger::open(dir, verbose).ok();
    }
}

/// True when `init_log(true)` was used; callers check it before building expensive debug text.
pub fn verbose() -> bool {
    LOGGER
        .lock()
        .ok()
        .is_some_and(|g| g.as_ref().is_some_and(|l| l.verbose))
}

#[doc(hidden)]
pub fn write(debug: bool, args: Arguments) {
    if let Ok(mut g) = LOGGER.lock() {
        if let Some(l) = g.as_mut().filter(|l| l.verbose || !debug) {
            l.write(if debug { "DEBUG" } else { "INFO" }, &args.to_string());
        }
    }
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::log::write(false, format_args!($($arg)*)) };
}

/// Written only when the log was started verbose.
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => { $crate::log::write(true, format_args!($($arg)*)) };
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The logger is process-wide, so tests that start one take turns.
    pub(crate) static LOG_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn timestamp_formats_offset_and_utc_fallback() {
        // 2026-10-20T05:05:01Z is 16:05:01 at +11:00.
        let t = 1_792_472_701;
        assert_eq!(
            format_timestamp(t, 123, Some(660)),
            "2026-10-20T16:05:01.123+11:00"
        );
        assert_eq!(
            format_timestamp(t, 7, Some(-210)),
            "2026-10-20T01:35:01.007-03:30"
        );
        assert_eq!(format_timestamp(t, 0, None), "2026-10-20T05:05:01.000Z");
        // The offset can roll the date.
        assert_eq!(
            format_timestamp(t + 19 * 3600, 0, Some(660)),
            "2026-10-21T11:05:01.000+11:00"
        );
    }

    #[test]
    fn log_survives_unwritable_dir() {
        let _g = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A file where the directory should be makes every open fail.
        let f = std::env::temp_dir().join(format!("xm-core-notadir-{}", std::process::id()));
        std::fs::write(&f, "x").unwrap();
        init_log_at(&f.join("logs"), true);
        log_info!("hello {}", 1);
        log_debug!("quiet");
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn failed_rotation_keeps_files_and_backs_off() {
        let dir = std::env::temp_dir().join(format!("xm-core-logfail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut l = Logger::open(&dir, false).unwrap();
        // A directory as the backup makes the rename fail.
        std::fs::create_dir(dir.join("app.1.log")).unwrap();
        let big = "x".repeat(1000);
        for _ in 0..1100 {
            l.write("INFO", &big);
        }
        assert!(dir.join("app.1.log").is_dir());
        assert!(l.size > MAX_BYTES, "kept appending to app.log");
        assert!(l.retry_at > MAX_BYTES && l.retry_at <= l.size + RETRY_GROWTH);
        let first_retry = l.retry_at;
        l.write("INFO", "one more");
        assert_eq!(l.retry_at, first_retry, "no retry on every line");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_writes_a_line_and_rotates_at_1_mb() {
        let dir = std::env::temp_dir().join(format!("xm-core-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut l = Logger::open(&dir, false).unwrap();
        l.write("INFO", "first line");
        let text = std::fs::read_to_string(dir.join("app.log")).unwrap();
        assert!(text.contains("INFO first line"), "{text}");

        let big = "x".repeat(1000);
        for _ in 0..1100 {
            l.write("INFO", &big);
        }
        assert!(dir.join("app.1.log").exists());
        assert!(std::fs::metadata(dir.join("app.log")).unwrap().len() < MAX_BYTES);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
