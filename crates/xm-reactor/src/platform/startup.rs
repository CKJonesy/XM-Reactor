//! "Open at sign-in": a value named "XM Reactor" under the current user's Run key holding the
//! quoted path of this exe. The key is a parameter so tests can use a throwaway key.

use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use xm_core::log_info;

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "XM Reactor";

fn quoted_exe() -> std::io::Result<String> {
    Ok(format!("\"{}\"", std::env::current_exe()?.display()))
}

/// On only when the value starts this copy of the app; one left by a copy elsewhere is off.
pub fn is_enabled(key: &str) -> bool {
    let Ok(want) = quoted_exe() else {
        return false;
    };
    read(key).is_some_and(|v| v.to_lowercase() == want.to_lowercase())
}

/// Adds or removes the value; removing one that is not there is not an error.
pub fn set(key: &str, on: bool) -> std::io::Result<()> {
    if on {
        return write_string(key, VALUE_NAME, &quoted_exe()?);
    }
    delete_value(key, VALUE_NAME)
}

/// Removes a value; one that is not there (or a missing key) is not an error.
pub fn delete_value(key: &str, name: &str) -> std::io::Result<()> {
    // SAFETY: both names are live HSTRINGs for the duration of the call.
    let err =
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name)) };
    if err != ERROR_FILE_NOT_FOUND {
        err.ok()?;
    }
    Ok(())
}

/// Sets a string value, creating the key if it is missing.
pub fn write_string(key: &str, name: &str, value: &str) -> std::io::Result<()> {
    let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
    // SAFETY: `data` is a NUL-terminated UTF-16 buffer of exactly the byte length passed, and it
    // outlives the call. The call creates the key if it is missing.
    let err: WIN32_ERROR = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        )
    };
    Ok(err.ok()?)
}

/// The stored command line, or None when there is none; other failures are logged.
fn read(key: &str) -> Option<String> {
    let mut buf = vec![0u16; 4096];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: `buf` holds `len` bytes and outlives the call, which writes at most that many.
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(VALUE_NAME),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if err == ERROR_FILE_NOT_FOUND {
        return None;
    }
    if let Err(e) = err.ok() {
        log_info!("could not read the open at sign-in setting: {e}");
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::{RegDeleteKeyW, RegDeleteTreeW};

    const TEST_ROOT: &str = r"Software\XM Reactor\Test";

    /// Removes the test key, then the app's key if nothing else is in it.
    fn clean_up() {
        // SAFETY: the names are live HSTRINGs; RegDeleteKeyW refuses a key that still has subkeys.
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(TEST_ROOT));
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(TEST_ROOT));
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(r"Software\XM Reactor"));
        }
    }

    #[test]
    fn set_and_clear_on_a_throwaway_key() {
        let key = format!(r"{TEST_ROOT}\startup-{}", std::process::id());
        assert_ne!(key, RUN_KEY);
        assert!(!is_enabled(&key));
        set(&key, true).unwrap();
        let result = std::panic::catch_unwind(|| {
            assert!(is_enabled(&key));
            let exe = std::env::current_exe().unwrap();
            assert_eq!(read(&key), Some(format!("\"{}\"", exe.display())));
            set(&key, false).unwrap();
            assert!(!is_enabled(&key));
            // Turning it off twice is fine.
            set(&key, false).unwrap();
            // A value left by a copy of the app somewhere else is not this app's.
            write_string(&key, VALUE_NAME, r#""C:\Elsewhere\XMReactor.exe""#).unwrap();
            assert!(!is_enabled(&key));
            write_string(&key, VALUE_NAME, &quoted_exe().unwrap().to_uppercase()).unwrap();
            assert!(is_enabled(&key));
        });
        clean_up();
        result.unwrap();
    }
}
