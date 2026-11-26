//! État système Windows : fenêtre active (titre, processus, niveau d'intégrité / UIPI),
//! plein écran exclusif, GPU, démarrage automatique, ouverture d'URL.

use std::ffi::c_void;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, ShellExecuteW, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, SW_SHOWNORMAL,
};

use crate::platform::WindowInfo;

pub fn hwnd_from(v: isize) -> HWND {
    HWND(v as *mut c_void)
}

pub fn hwnd_to(h: HWND) -> isize {
    h.0 as isize
}

fn token_elevated(process: HANDLE) -> Option<bool> {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
            return None;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut TOKEN_ELEVATION as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok.then_some(elev.TokenIsElevated != 0)
    }
}

pub fn self_elevated() -> bool {
    token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

pub fn foreground_window() -> Option<WindowInfo> {
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() {
            return None;
        }
        let len = GetWindowTextLengthW(h).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let n = GetWindowTextW(h, &mut buf).max(0) as usize;
        let title = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let mut process_name = String::new();
        let mut elevated = false;
        if let Ok(ph) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut path = vec![0u16; 1024];
            let mut size = path.len() as u32;
            if QueryFullProcessImageNameW(ph, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut size).is_ok() {
                let full = String::from_utf16_lossy(&path[..size as usize]);
                process_name = full.rsplit('\\').next().unwrap_or(&full).to_string();
            }
            // Échec d'accès au jeton d'un autre processus : niveau d'intégrité supérieur (Admin).
            elevated = token_elevated(ph).unwrap_or(true);
            let _ = CloseHandle(ph);
        } else if pid != 0 {
            elevated = true;
        }
        Some(WindowInfo {
            hwnd: hwnd_to(h),
            title,
            process_name,
            elevated,
        })
    }
}

/// Jeu / présentation en plein écran exclusif : l'overlay doit rester masqué.
pub fn fullscreen_exclusive() -> bool {
    match unsafe { SHQueryUserNotificationState() } {
        Ok(s) => s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE,
        Err(_) => false,
    }
}

/// GPU dédié détecté via DXGI (nom + mémoire vidéo).
pub fn gpu_info() -> Option<String> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut best: Option<(usize, String)> = None;
        let mut i = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                continue;
            }
            let end = desc.Description.iter().position(|c| *c == 0).unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..end]);
            let mem = desc.DedicatedVideoMemory;
            if best.as_ref().map(|(m, _)| mem > *m).unwrap_or(true) {
                best = Some((mem, name));
            }
        }
        best.filter(|(m, _)| *m >= 1 << 30)
            .map(|(m, n)| format!("{n} ({} Go)", m >> 30))
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn open_url(url: &str) {
    let u = wide(url);
    unsafe {
        ShellExecuteW(
            HWND::default(),
            w!("open"),
            PCWSTR(u.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Démarrage automatique avec Windows (clé `Run` de l'utilisateur, sans élévation).
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY)?;
    if enabled {
        let exe = std::env::current_exe()?;
        key.set_value("DumonVoice", &format!("\"{}\" --background", exe.display()))?;
    } else {
        let _ = key.delete_value("DumonVoice");
    }
    Ok(())
}
