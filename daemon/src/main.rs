// Pas de console en version finale sous Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let background = args.iter().any(|a| a == "--background");
    if let Some(v) = args.iter().find_map(|a| a.strip_prefix("--autostart=")) {
        dumonvoice::app::apply_cli_autostart(v == "1" || v == "true")?;
    }
    if args.iter().any(|a| a == "--configure-only") {
        return Ok(());
    }
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(16)
        .thread_name("dumonvoice")
        .enable_all()
        .build()?;
    let app = match rt.block_on(dumonvoice::app::start(background)) {
        Ok(a) => a,
        Err(e) if e.is::<dumonvoice::app::AlreadyRunning>() => return Ok(()),
        Err(e) => {
            eprintln!("Démarrage impossible : {e:#}");
            return Err(e);
        }
    };

    #[cfg(windows)]
    {
        let _guard = rt.enter();
        if let Err(e) = app.run_tray() {
            tracing::error!("System Tray : {e}");
            rt.block_on(async {
                let _ = tokio::signal::ctrl_c().await;
            });
        }
    }
    #[cfg(not(windows))]
    {
        println!("DumonVoice démarré (mode développement hors Windows).");
        println!("Réglages : {}", app.ctx.local_url(None));
        rt.block_on(async {
            let _ = tokio::signal::ctrl_c().await;
        });
    }

    app.shutdown();
    rt.shutdown_timeout(Duration::from_secs(2));
    Ok(())
}
