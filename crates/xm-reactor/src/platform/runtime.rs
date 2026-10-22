use crate::strings;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows::Win32::Storage::Packaging::Appx::GetPackagesByPackageFamily;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDOK, MB_ICONINFORMATION, MB_OKCANCEL};
use xm_core::log_info;

const FAMILY: PCWSTR = w!("Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe");
pub const DOWNLOAD_URL: &str =
    "https://aka.ms/windowsappsdk/2.5/latest/windowsappruntimeinstall-x64.exe";

/// True when a full package name (`Name_Version_Arch__PublisherId`) is an x64 build of version 2.0 or later.
fn is_x64_v2(full_name: &str) -> bool {
    let mut parts = full_name.split('_');
    let (name, version, arch) = (parts.next(), parts.next(), parts.next());
    let major = version
        .and_then(|v| v.split('.').next())
        .and_then(|m| m.parse::<u32>().ok());
    name == Some("Microsoft.WindowsAppRuntime.2")
        && arch == Some("x64")
        && major.is_some_and(|m| m >= 2)
}

pub fn any_x64_v2<S: AsRef<str>>(full_names: &[S]) -> bool {
    full_names.iter().any(|n| is_x64_v2(n.as_ref()))
}

/// `None` when the OS lookup itself failed, so the caller can fail open.
fn installed_full_names() -> Option<Vec<String>> {
    let (mut count, mut buf_len) = (0u32, 0u32);
    // SAFETY: standard two-call size query, then fill; buffers are sized from the first call.
    unsafe {
        let rc = GetPackagesByPackageFamily(FAMILY, &mut count, None, &mut buf_len, None);
        if rc == ERROR_SUCCESS && count == 0 {
            return Some(Vec::new());
        }
        if rc != ERROR_INSUFFICIENT_BUFFER {
            log_info!("runtime lookup failed (size query): {}", rc.0);
            return None;
        }
        let mut ptrs = vec![PWSTR::null(); count as usize];
        let mut buf = vec![0u16; buf_len as usize];
        let rc = GetPackagesByPackageFamily(
            FAMILY,
            &mut count,
            Some(ptrs.as_mut_ptr()),
            &mut buf_len,
            Some(PWSTR(buf.as_mut_ptr())),
        );
        if rc != ERROR_SUCCESS {
            log_info!("runtime lookup failed (fill): {}", rc.0);
            return None;
        }
        Some(
            ptrs.iter()
                .take(count as usize)
                .filter_map(|p| p.to_string().ok())
                .collect(),
        )
    }
}

/// Unknown (lookup failed) counts as available: the reactor itself reports a real load failure.
fn decide(names: Option<Vec<String>>) -> bool {
    names.is_none_or(|n| any_x64_v2(&n))
}

pub fn runtime_available() -> bool {
    decide(installed_full_names())
}
pub fn show_runtime_missing() {
    let text: Vec<u16> = strings::RUNTIME_MISSING
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let title: Vec<u16> = strings::RUNTIME_MISSING_TITLE
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: both buffers are NUL-terminated and outlive the call.
    let choice = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OKCANCEL | MB_ICONINFORMATION,
        )
    };
    if choice == IDOK {
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", DOWNLOAD_URL])
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decides_on_arch_and_version() {
        assert!(any_x64_v2(&[
            "Microsoft.WindowsAppRuntime.2_2.5.1.0_x64__8wekyb3d8bbwe"
        ]));
        assert!(!any_x64_v2(&[
            "Microsoft.WindowsAppRuntime.2_2.5.1.0_x86__8wekyb3d8bbwe"
        ]));
        assert!(!any_x64_v2(&[
            "Microsoft.WindowsAppRuntime.1.8_8000.994.2142.0_x64__8wekyb3d8bbwe"
        ]));
        assert!(!any_x64_v2::<&str>(&[]));
    }

    #[test]
    fn lookup_failure_fails_open() {
        let real = "Microsoft.WindowsAppRuntime.2_2.5.1.0_x64__8wekyb3d8bbwe".to_string();
        assert!(decide(None));
        assert!(!decide(Some(vec![])));
        assert!(decide(Some(vec![real])));
    }
}
