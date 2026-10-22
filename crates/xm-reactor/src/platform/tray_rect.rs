use super::placement::{Rect, TaskbarEdge};
use windows::core::w;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForSystem, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, Shell_NotifyIconGetRect, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS,
    APPBARDATA, NOTIFYICONIDENTIFIER,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// The monitor that holds the taskbar, with what `place` needs to know about it.
pub struct TaskbarScreen {
    pub edge: TaskbarEdge,
    pub work_area: Rect,
    pub monitor: Rect,
    pub dpi_scale: f64,
}

fn rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

#[allow(dead_code)] // reactor keeps the notify icon's window private, so the tray uses the click point
/// Screen rectangle of a notification-area icon registered by `owner` with id `uid`.
pub fn tray_icon_rect(owner: isize, uid: u32) -> Option<Rect> {
    let id = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: HWND(owner as _),
        uID: uid,
        ..Default::default()
    };
    // SAFETY: `id` is a fully initialised identifier that lives for the call.
    unsafe { Shell_NotifyIconGetRect(&id) }.ok().map(rect)
}

/// Edge, work area, bounds and scale of the monitor holding the primary taskbar.
pub fn taskbar_screen() -> Option<TaskbarScreen> {
    let mut bar = APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (mut dpi, mut unused) = (96u32, 0u32);
    // SAFETY: every out-parameter is a sized, initialised local that outlives its call.
    unsafe {
        if SHAppBarMessage(ABM_GETTASKBARPOS, &mut bar) == 0 {
            return None;
        }
        let centre = POINT {
            x: (bar.rc.left + bar.rc.right) / 2,
            y: (bar.rc.top + bar.rc.bottom) / 2,
        };
        let monitor = MonitorFromPoint(centre, MONITOR_DEFAULTTOPRIMARY);
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        // Falls back to 96 (100 %) if the lookup fails.
        _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
    }
    Some(TaskbarScreen {
        edge: match bar.uEdge {
            ABE_TOP => TaskbarEdge::Top,
            ABE_LEFT => TaskbarEdge::Left,
            ABE_RIGHT => TaskbarEdge::Right,
            _ => TaskbarEdge::Bottom,
        },
        work_area: rect(info.rcWork),
        monitor: rect(info.rcMonitor),
        dpi_scale: f64::from(dpi.max(96)) / 96.0,
    })
}

/// Whether the taskbar uses the light theme; dark (the Windows default) when the value is missing.
pub fn light_taskbar() -> bool {
    let (mut value, mut len) = (0u32, size_of::<u32>() as u32);
    // SAFETY: the names are static wide strings and `value` holds the `len` bytes written.
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut len),
        )
    };
    err.is_ok() && value != 0
}

/// Fallback when the taskbar cannot be queried: the primary monitor's work area, bottom edge.
pub fn primary_screen() -> TaskbarScreen {
    let mut work = RECT::default();
    // SAFETY: SPI_GETWORKAREA writes one RECT into `work`, which outlives the call.
    let dpi = unsafe {
        _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut work).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        GetDpiForSystem()
    };
    TaskbarScreen {
        edge: TaskbarEdge::Bottom,
        work_area: rect(work),
        monitor: rect(work),
        dpi_scale: f64::from(dpi.max(96)) / 96.0,
    }
}
