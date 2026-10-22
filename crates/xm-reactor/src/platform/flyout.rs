use super::placement::{resized, Rect};
use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::Instant;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, GA_ROOTOWNER, GWL_EXSTYLE, GWL_STYLE,
    HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SW_HIDE, WA_INACTIVE, WM_ACTIVATE, WM_KEYDOWN, WM_NCDESTROY, WM_SETTINGCHANGE,
    WS_CAPTION, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP,
    WS_SYSMENU, WS_THICKFRAME,
};
use xm_core::log_info;

const REOPEN_GUARD_MS: u64 = 300;

/// When the flyout last hid because the user clicked away, in ms since `now_ms` started counting
/// (0 = never).
static HIDDEN_AT_MS: AtomicU64 = AtomicU64::new(0);

/// The flyout window made by `make_flyout` (0 = none yet).
static FLYOUT_HWND: AtomicIsize = AtomicIsize::new(0);

type OnRequest = Box<dyn Fn(usize)>;

thread_local! {
    /// Run when Windows broadcasts a settings change, such as the taskbar theme.
    static ON_SETTING_CHANGE: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    /// Run with the request (`single_instance::OPEN` or `QUIT`) when another process or the
    /// toast posts the app's registered message.
    static ON_REQUEST: RefCell<Option<OnRequest>> = const { RefCell::new(None) };
}

/// Calls `f` on the UI thread whenever Windows broadcasts a settings change.
pub fn on_setting_change(f: impl Fn() + 'static) {
    ON_SETTING_CHANGE.with(|c| *c.borrow_mut() = Some(Box::new(f)));
}

/// Calls `f` on the UI thread with each request posted by `single_instance`.
pub fn on_request(f: impl Fn(usize) + 'static) {
    ON_REQUEST.with(|c| *c.borrow_mut() = Some(Box::new(f)));
}

/// The flyout's HWND, or 0 before `make_flyout` has run.
pub fn current() -> isize {
    FLYOUT_HWND.load(Relaxed)
}

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// True while a tray click that arrives just after a click-away hide should not reopen the flyout.
fn reopen_blocked(now_ms: u64, hidden_at_ms: Option<u64>) -> bool {
    hidden_at_ms.is_some_and(|t| now_ms >= t && now_ms - t < REOPEN_GUARD_MS)
}

/// Turns the window into a borderless, rounded tool window (no taskbar button) that hides on
/// click-away and Esc. Leaves it hidden; `show_at` opens it.
pub fn make_flyout(hwnd: isize) {
    let h = HWND(hwnd as _);
    let chrome =
        (WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0 as isize;
    // SAFETY: `h` is this thread's live window; the subclass proc is a static fn and the corner
    // preference value outlives its call.
    unsafe {
        // The taskbar only drops the button if the extended style changes while the window is hidden.
        _ = ShowWindow(h, SW_HIDE);
        let style = GetWindowLongPtrW(h, GWL_STYLE);
        SetWindowLongPtrW(h, GWL_STYLE, (style & !chrome) | WS_POPUP.0 as isize);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(
            h,
            GWL_EXSTYLE,
            (ex & !(WS_EX_APPWINDOW.0 as isize)) | WS_EX_TOOLWINDOW.0 as isize,
        );
        _ = SetWindowPos(
            h,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        let round = DWMWCP_ROUND;
        _ = DwmSetWindowAttribute(
            h,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const round).cast(),
            size_of_val(&round) as u32,
        );
        if !SetWindowSubclass(h, Some(subclass_proc), 1, 0).as_bool() {
            log_info!("flyout: SetWindowSubclass failed; click-away and Esc will not hide it");
        }
    }
    FLYOUT_HWND.store(hwnd, Relaxed);
}

/// Lets the next `show_at` open the flyout even just after a click-away hide: an explicit request
/// (a second launch, the toast) is not the tray click that closed it.
pub fn clear_reopen_guard() {
    HIDDEN_AT_MS.store(0, Relaxed);
}

/// Shows the flyout at `rect` (physical pixels), on top and focused, unless it hid on a click-away
/// within the last 300 ms (that click was the tray click meant to close it).
pub fn show_at(hwnd: isize, rect: Rect) {
    let hidden_at = HIDDEN_AT_MS.load(Relaxed);
    if reopen_blocked(now_ms(), (hidden_at != 0).then_some(hidden_at)) {
        return;
    }
    let h = HWND(hwnd as _);
    let (w, ht) = (rect.right - rect.left, rect.bottom - rect.top);
    // SAFETY: `h` is this thread's live window.
    unsafe {
        // Twice: moving onto a monitor with another DPI makes WinUI rescale the first placement.
        for _ in 0..2 {
            _ = SetWindowPos(
                h,
                Some(HWND_TOPMOST),
                rect.left,
                rect.top,
                w,
                ht,
                SWP_SHOWWINDOW,
            );
        }
        _ = SetForegroundWindow(h);
    }
}

/// Changes the open flyout's height without activating it or moving it sideways (see
/// `placement::resized`); a hidden one waits for `show_at`.
pub fn resize(hwnd: isize, height: i32, work_area: Rect, keep_top: bool) {
    let h = HWND(hwnd as _);
    let mut cur = RECT::default();
    // SAFETY: all three calls accept any handle (a null or stale one fails or is a no-op), and
    // `cur` outlives GetWindowRect.
    unsafe {
        if hwnd == 0 || !IsWindowVisible(h).as_bool() || GetWindowRect(h, &mut cur).is_err() {
            return;
        }
        let current = Rect {
            left: cur.left,
            top: cur.top,
            right: cur.right,
            bottom: cur.bottom,
        };
        let r = resized(current, height, work_area, keep_top);
        _ = SetWindowPos(
            h,
            None,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

pub fn is_open(hwnd: isize) -> bool {
    // SAFETY: IsWindowVisible accepts any handle, including null.
    hwnd != 0 && unsafe { IsWindowVisible(HWND(hwnd as _)) }.as_bool()
}

pub fn hide(hwnd: isize) {
    // SAFETY: hiding a window handle has no memory-safety requirements.
    unsafe {
        _ = ShowWindow(HWND(hwnd as _), SW_HIDE);
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            // Focus moving to one of our own popups (a menu or drop-down) is not a click-away.
            let other = HWND(lparam.0 as _);
            // SAFETY: GetAncestor accepts any handle, including null or another process's window.
            let ours = !other.is_invalid() && unsafe { GetAncestor(other, GA_ROOTOWNER) } == hwnd;
            if !ours {
                HIDDEN_AT_MS.store(now_ms().max(1), Relaxed);
                hide(hwnd.0 as isize);
            }
        }
        // Only posted messages land here: real keyboard focus is in the XAML island's child window,
        // whose Esc is handled by the panel's root key handler.
        WM_KEYDOWN if wparam.0 == usize::from(VK_ESCAPE.0) => {
            hide(hwnd.0 as isize);
            return LRESULT(0);
        }
        WM_SETTINGCHANGE => ON_SETTING_CHANGE.with(|c| {
            if let Some(f) = c.borrow().as_ref() {
                f();
            }
        }),
        _ if msg == super::single_instance::message() => {
            ON_REQUEST.with(|c| {
                if let Some(f) = c.borrow().as_ref() {
                    f(wparam.0);
                }
            });
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            FLYOUT_HWND.store(0, Relaxed);
            // SAFETY: removes the subclass this module installed on `hwnd`, with the same proc and id.
            unsafe {
                _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), 1);
            }
        }
        _ => {}
    }
    // SAFETY: forwards the untouched message to the next procedure in the subclass chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopen_guard_300ms() {
        assert!(!reopen_blocked(5_000, None));
        assert!(reopen_blocked(5_000, Some(5_000)));
        assert!(reopen_blocked(5_299, Some(5_000)));
        assert!(!reopen_blocked(5_300, Some(5_000)));
        assert!(!reopen_blocked(60_000, Some(5_000)));
        // A clock that appears to go backwards never locks the flyout shut.
        assert!(!reopen_blocked(4_000, Some(5_000)));
    }
}
