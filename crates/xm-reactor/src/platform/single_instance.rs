//! One copy per sign-in session. The first copy owns a named mutex until it exits; a later
//! launch, the toast and the uninstall cleanup reach it with a registered window message that its
//! flyout window handles (see `flyout::on_request`).

use std::sync::OnceLock;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM,
    WAIT_FAILED, WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenMutexW, WaitForSingleObject, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, PostMessageW, RegisterWindowMessageW, ASFW_ANY, HWND_BROADCAST,
};
use xm_core::log_info;

const MUTEX: &str = r"Local\app.xmreactor.windows";

/// The message's wParam: open the flyout, or quit.
pub const OPEN: usize = 0;
pub const QUIT: usize = 1;

/// The registered `XMReactorActivate` message.
pub fn message() -> u32 {
    static MSG: OnceLock<u32> = OnceLock::new();
    // SAFETY: `w!` gives a static NUL-terminated name.
    *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(w!("XMReactorActivate")) })
}

/// True for the first copy, which keeps the mutex for its whole life. A later copy asks the first
/// to open its flyout and returns false. A request sent while the first copy is still starting,
/// before its flyout window exists, is lost; the user can launch again.
pub fn claim() -> bool {
    let first = claim_at(MUTEX);
    if !first {
        // SAFETY: no pointers; passing on this launch's foreground right lets the running copy
        // bring its flyout to the front.
        unsafe { _ = AllowSetForegroundWindow(ASFW_ANY) };
        post(HWND_BROADCAST, OPEN);
    }
    first
}

/// Creates and owns the mutex `name`; false when another copy already has it.
fn claim_at(name: &str) -> bool {
    let name = HSTRING::from(name);
    // SAFETY: `name` outlives the call, and GetLastError is read straight after it.
    let (handle, err) = unsafe { (CreateMutexW(None, true, &name), GetLastError()) };
    match handle {
        Ok(h) if err == ERROR_ALREADY_EXISTS => {
            // SAFETY: `h` is this process's own handle, closed once.
            unsafe { _ = CloseHandle(h) };
            false
        }
        // The handle stays open until the process exits, so the mutex lives that long.
        Ok(_) => true,
        // The mutex exists but belongs to a copy running elevated.
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => false,
        Err(e) => {
            log_info!("could not create the single instance mutex: {e}");
            true
        }
    }
}

/// Posts the request to `hwnd` (`HWND_BROADCAST`: every top-level window, where only ours knows
/// the message). Safe from any thread.
pub fn post(hwnd: HWND, request: usize) {
    // SAFETY: posting copies two plain integers; a stale handle only makes the call fail.
    if let Err(e) = unsafe { PostMessageW(Some(hwnd), message(), WPARAM(request), LPARAM(0)) } {
        log_info!("could not post request {request} to the running copy: {e}");
    }
}

/// Asks a running copy to quit and waits up to 5 s for it to exit. True when none is running now.
pub fn stop_running() -> bool {
    let Some(h) = open(MUTEX) else {
        return true;
    };
    log_info!("asking the running copy to quit");
    post(HWND_BROADCAST, QUIT);
    wait_and_close(h, 5_000)
}

/// The mutex `name` when some copy holds it.
fn open(name: &str) -> Option<HANDLE> {
    // SAFETY: the name is a live HSTRING for the duration of the call.
    unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(name)) }.ok()
}

/// True when the owner let go within `ms`: the running copy owns the mutex, so the wait ends
/// (abandoned) when its process exits.
fn wait_and_close(h: HANDLE, ms: u32) -> bool {
    // SAFETY: `h` is a live handle with SYNCHRONIZE access, closed once.
    let r = unsafe {
        let r = WaitForSingleObject(h, ms);
        _ = CloseHandle(h);
        r
    };
    r != WAIT_TIMEOUT && r != WAIT_FAILED
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn claim_and_stop_on_a_test_mutex() {
        let name = format!(r"Local\app.xmreactor.windows.test-{}", std::process::id());
        assert_ne!(name, MUTEX);
        assert!(open(&name).is_none());

        // The owner thread stands in for the running copy.
        let (claimed_tx, claimed) = mpsc::channel();
        let (quit, quit_rx) = mpsc::channel::<()>();
        let n = name.clone();
        let owner = std::thread::spawn(move || {
            claimed_tx.send((claim_at(&n), claim_at(&n))).unwrap();
            quit_rx.recv().unwrap();
        });
        assert_eq!(claimed.recv().unwrap(), (true, false));

        // While the owner lives, the wait times out. Windows rounds a wait to its scheduler tick,
        // so it can return a few milliseconds early; the point is that it waited at all.
        let start = Instant::now();
        assert!(!wait_and_close(open(&name).unwrap(), 200));
        assert!(start.elapsed() >= Duration::from_millis(150));

        // Once the owner is gone, as when the app exits, the wait ends at once.
        quit.send(()).unwrap();
        owner.join().unwrap();
        let start = Instant::now();
        assert!(wait_and_close(open(&name).unwrap(), 5_000));
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
