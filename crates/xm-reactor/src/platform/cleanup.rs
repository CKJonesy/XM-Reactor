//! `--uninstall-cleanup`: stops a running copy, removes what the app registered for the current
//! user and its notifications, then, when asked and confirmed, its settings and logs.

use super::{single_instance, startup, toast};
use crate::strings;
use std::path::Path;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{RegDeleteKeyW, RegDeleteTreeW, HKEY_CURRENT_USER};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO};
use xm_core::log_info;

pub fn run(quiet: bool) {
    log_info!("uninstall cleanup requested");
    // First, so no running copy loses its icon folder or rewrites what is removed below.
    let stopped = single_instance::stop_running();
    toast::clear_history();
    remove_registrations(startup::RUN_KEY, toast::AUMID_KEY);
    if !stopped {
        log_info!("the running copy did not quit; its folders are left in place");
        return;
    }
    remove_dir(&std::env::temp_dir().join(crate::ui::tray::TEMP_ICON_DIR));
    if !quiet && confirm(strings::CLEANUP_DELETE_DATA) {
        remove_dir(&xm_core::settings::app_dir());
    }
    log_info!("uninstall cleanup finished");
}

fn remove_registrations(run_key: &str, aumid_key: &str) {
    if let Err(e) = startup::set(run_key, false) {
        log_info!("could not remove the open at sign-in value: {e}");
    }
    delete_tree(aumid_key);
}

fn delete_tree(key: &str) {
    let name = HSTRING::from(key);
    // SAFETY: `name` is a live HSTRING for both calls. RegDeleteTreeW empties the key and
    // RegDeleteKeyW then removes the key itself.
    let err = unsafe {
        let err = RegDeleteTreeW(HKEY_CURRENT_USER, &name);
        if err.is_ok() {
            RegDeleteKeyW(HKEY_CURRENT_USER, &name)
        } else {
            err
        }
    };
    if err != ERROR_FILE_NOT_FOUND {
        if let Err(e) = err.ok() {
            log_info!("could not remove {key}: {e}");
        }
    }
}

fn remove_dir(dir: &Path) {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => log_info!("removed {}", dir.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log_info!("could not remove {}: {e}", dir.display()),
    }
}

fn confirm(question: &str) -> bool {
    let text: Vec<u16> = question.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = strings::WINDOW_TITLE
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: both buffers are NUL-terminated and outlive the call.
    let choice = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONQUESTION,
        )
    };
    choice == IDYES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{startup, toast};
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_ANY,
    };

    fn has_value(key: &str, name: &str) -> bool {
        // SAFETY: both names are live HSTRINGs; no data is read.
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(key),
                &HSTRING::from(name),
                RRF_RT_ANY,
                None,
                None,
                None,
            )
        }
        .is_ok()
    }

    fn key_exists(key: &str) -> bool {
        let mut h = HKEY::default();
        // SAFETY: the name is a live HSTRING and `h` outlives the call; the handle is leaked in a test.
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(key),
                None,
                KEY_READ,
                &mut h,
            )
        }
        .is_ok()
    }

    #[test]
    fn cleanup_removes_values() {
        // Its own top-level key: the startup test deletes `Software\XM Reactor\Test` as it ends.
        let root = format!(r"Software\XM Reactor cleanup test {}", std::process::id());
        let (run, aumid) = (format!(r"{root}\Run"), format!(r"{root}\Aumid"));
        assert_ne!(run, startup::RUN_KEY);
        assert_ne!(aumid, toast::AUMID_KEY);
        let result = std::panic::catch_unwind(|| {
            startup::set(&run, true).unwrap();
            toast::register(&aumid, Some(std::path::Path::new(r"C:\x\app.ico"))).unwrap();
            assert!(startup::is_enabled(&run) && has_value(&aumid, "IconUri"));
            // Registering again without an icon file drops the stale path.
            toast::register(&aumid, None).unwrap();
            assert!(has_value(&aumid, "DisplayName") && !has_value(&aumid, "IconUri"));

            remove_registrations(&run, &aumid);
            assert!(!startup::is_enabled(&run));
            assert!(!key_exists(&aumid));
            // A second run with nothing left to remove is fine.
            remove_registrations(&run, &aumid);
        });
        delete_tree(&root);
        result.unwrap();

        // The app's own log stays open while the cleanup deletes the folder it is in.
        let dir = std::env::temp_dir().join(format!("xm-cleanup-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        let open = std::fs::File::create(dir.join("logs").join("app.log")).unwrap();
        remove_dir(&dir);
        drop(open);
        let gone = !dir.exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(gone);
        remove_dir(&dir); // a folder that is already gone is fine
    }
}
