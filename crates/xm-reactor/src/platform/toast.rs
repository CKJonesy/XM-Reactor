//! The low-battery notification: a classic toast under the app's own AUMID, registered in the
//! current user's classes. The app is always running while it shows, so a click reaches it through
//! the toast's in-process Activated event, which asks the flyout to open.

use super::{flyout, single_instance, startup};
use crate::strings;
use std::cell::RefCell;
use std::path::Path;
use windows::core::{IInspectable, HSTRING};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::HWND;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use xm_core::log_info;

pub const AUMID: &str = "app.xmreactor.windows";
pub const AUMID_KEY: &str = r"Software\Classes\AppUserModelId\app.xmreactor.windows";
const APP_ICON: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.ico"));

thread_local! {
    /// The last toast shown, kept so its Activated handler stays registered.
    static SHOWN: RefCell<Option<ToastNotification>> = const { RefCell::new(None) };
}

/// Names the AUMID "XM Reactor" with the app icon (when there is a file for it) under `key`.
pub fn register(key: &str, icon: Option<&Path>) -> std::io::Result<()> {
    startup::write_string(key, "DisplayName", strings::WINDOW_TITLE)?;
    match icon {
        Some(p) => startup::write_string(key, "IconUri", &p.display().to_string()),
        // A path left from an earlier start may point at a file that is gone.
        None => startup::delete_value(key, "IconUri"),
    }
}

/// Shows the notification; failures are logged, never raised.
pub fn show_low_battery(device: &str, level: u8) {
    match show(device, level) {
        Ok(()) => log_info!("low battery notification shown: {level} %"),
        Err(e) => log_info!("could not show the low battery notification: {e}"),
    }
}

fn show(device: &str, level: u8) -> windows::core::Result<()> {
    if let Err(e) = register(AUMID_KEY, app_icon().as_deref()) {
        log_info!("could not register the notification name: {e}");
    }
    let xml = XmlDocument::new()?;
    xml.LoadXml(&HSTRING::from(toast_xml(device, level)))?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
        |_, _| {
            log_info!("low battery notification clicked");
            match flyout::current() {
                0 => log_info!("no flyout window to open yet"),
                hwnd => single_instance::post(HWND(hwnd as _), single_instance::OPEN),
            }
            Ok(())
        },
    ))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)?;
    SHOWN.with(|s| *s.borrow_mut() = Some(toast));
    Ok(())
}

/// Removes the app's notifications from the notification centre.
pub fn clear_history() {
    let r = ToastNotificationManager::History().and_then(|h| h.ClearWithId(&HSTRING::from(AUMID)));
    if let Err(e) = r {
        log_info!("could not clear the notifications: {e}");
    }
}

/// The app icon written beside the tray icons, since the shell reads the icon from a file.
fn app_icon() -> Option<std::path::PathBuf> {
    let path = crate::ui::tray::icon_dir()?.join("app.ico");
    if std::fs::read(&path).ok().as_deref() != Some(APP_ICON) {
        if let Err(e) = std::fs::write(&path, APP_ICON) {
            log_info!("could not write {}: {e}", path.display());
            return None;
        }
    }
    Some(path)
}

fn toast_xml(device: &str, level: u8) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text>\
         </binding></visual></toast>",
        escape(&strings::low_battery_title(device)),
        escape(&strings::low_battery_body(level)),
    )
}

/// The device name is user-chosen, so it may hold XML's special characters.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_texts() {
        assert_eq!(
            strings::low_battery_title("WH-1000XM6"),
            "WH-1000XM6 battery is low"
        );
        assert_eq!(
            strings::low_battery_body(19),
            "19 % left. Charge them soon."
        );
        let xml = toast_xml("Jo's <XM6> & \"co\"", 19);
        assert!(
            xml.contains("<text>Jo&apos;s &lt;XM6&gt; &amp; &quot;co&quot; battery is low</text>")
        );
        assert!(xml.contains("<text>19 % left. Charge them soon.</text>"));
    }
}
