//! Sessions audio Windows Core Audio : *Audio Ducking* (baisse du volume des autres
//! applications pendant l'écoute) et détection d'appel visio (micro capturé par une autre
//! application via `IAudioSessionManager2`).

use std::collections::HashMap;

use parking_lot::Mutex;
use windows::core::Interface;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender, AudioSessionStateActive, EDataFlow, ERole,
    IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

struct SessionInfo {
    pid: u32,
    active: bool,
    volume: Option<ISimpleAudioVolume>,
}

fn sessions(flow: EDataFlow, role: ERole) -> windows::core::Result<Vec<SessionInfo>> {
    com_init();
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(flow, role)?;
        let mgr: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let list = mgr.GetSessionEnumerator()?;
        let count = list.GetCount()?;
        let mut out = Vec::new();
        for i in 0..count {
            let Ok(ctl) = list.GetSession(i) else { continue };
            let active = ctl.GetState().map(|s| s == AudioSessionStateActive).unwrap_or(false);
            let pid = ctl
                .cast::<IAudioSessionControl2>()
                .ok()
                .and_then(|c2| c2.GetProcessId().ok())
                .unwrap_or(0);
            let volume = ctl.cast::<ISimpleAudioVolume>().ok();
            out.push(SessionInfo { pid, active, volume });
        }
        Ok(out)
    }
}

#[derive(Default)]
pub struct Ducker {
    saved: Mutex<HashMap<u32, f32>>,
}

impl Ducker {
    pub fn set(&self, on: bool, level: f32) {
        let me = std::process::id();
        let Ok(list) = sessions(eRender, eMultimedia) else { return };
        let mut saved = self.saved.lock();
        for s in list {
            if s.pid == me || s.pid == 0 {
                continue;
            }
            let Some(vol) = s.volume else { continue };
            unsafe {
                if on {
                    if let Ok(v) = vol.GetMasterVolume() {
                        saved.entry(s.pid).or_insert(v);
                        let _ = vol.SetMasterVolume(v * level, std::ptr::null());
                    }
                } else if let Some(v) = saved.get(&s.pid) {
                    let _ = vol.SetMasterVolume(*v, std::ptr::null());
                }
            }
        }
        if !on {
            saved.clear();
        }
    }
}

/// Une autre application capture-t-elle le micro (Teams, Zoom, Discord…) ?
pub fn mic_in_use_by_other_app() -> bool {
    let me = std::process::id();
    [eConsole, eCommunications].iter().any(|role| {
        sessions(eCapture, *role)
            .map(|l| l.iter().any(|s| s.active && s.pid != me && s.pid != 0))
            .unwrap_or(false)
    })
}
