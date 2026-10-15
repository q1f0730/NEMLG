#![windows_subsystem = "windows"]
//! GlassDock — liquid-glass desktop widgets for Windows.
//! Real-time desktop capture → Studio Glass shader → DirectComposition windows.

mod config;
mod gfx;
mod media;
mod widget;

use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{w, Result, HSTRING};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::System::Com::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

use widget::*;

pub fn log(s: &str) {
    let mut p = std::env::current_exe().unwrap_or_default();
    p.set_file_name("glassdock.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "[{t}] {s}");
    }
}

fn main() {
    if let Err(e) = run() {
        log(&format!("fatal: {e:?}"));
        unsafe {
            MessageBoxW(None, &HSTRING::from(format!("GlassDock 启动失败：\n{e}\n\n详情见 glassdock.log")), w!("GlassDock"), MB_ICONERROR);
        }
    }
}

fn run() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    log("start");
    let mut cfg = config::load();
    TOPMOST.store(cfg.topmost as u32, Ordering::Relaxed);
    let mut g = gfx::Gfx::new()?;
    log(&format!("capture {}x{} at {:?}, ok={}", g.cap.size.0, g.cap.size.1, g.cap.origin, g.cap.ok));
    register_class();

    let mut widgets: Vec<Box<Widget>> = Vec::new();
    for wc in &cfg.widgets {
        let kind = match wc.kind.as_str() {
            "music" => Kind::Music,
            _ => Kind::Clock,
        };
        let mut wd = Widget::new(&g, kind, wc.x, wc.y, cfg.topmost)?;
        if kind == Kind::Music {
            let st = Arc::new(Mutex::new(media::State::default()));
            let (tx, rx) = mpsc::channel();
            media::spawn(wc.prefer.clone(), st.clone(), rx);
            wd.media = Some((st, tx));
        }
        widgets.push(wd);
    }

    let mut mtime = config::mtime();
    let mut last_cfg_check = Instant::now();
    let mut last = Instant::now();
    let mut first = true;
    'main: loop {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'main;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        match ACTION.swap(ACT_NONE, Ordering::Relaxed) {
            ACT_QUIT => break 'main,
            ACT_TOPMOST => {
                cfg.topmost = !cfg.topmost;
                TOPMOST.store(cfg.topmost as u32, Ordering::Relaxed);
                for w in &widgets {
                    w.set_topmost(cfg.topmost);
                }
                save_positions(&mut cfg, &widgets);
                mtime = config::mtime();
            }
            ACT_EDIT => unsafe {
                let p = HSTRING::from(config::path().to_string_lossy().to_string());
                ShellExecuteW(None, w!("open"), w!("notepad.exe"), &p, None, SW_SHOWNORMAL);
            },
            ACT_RELOAD => {
                cfg.material = config::load().material;
                first = true;
            }
            _ => {}
        }

        // hot-reload material when the file changes
        if last_cfg_check.elapsed() > Duration::from_millis(500) {
            last_cfg_check = Instant::now();
            let m = config::mtime();
            if m != mtime {
                mtime = m;
                let n = config::load();
                cfg.material = n.material;
                if n.topmost != cfg.topmost {
                    cfg.topmost = n.topmost;
                    TOPMOST.store(cfg.topmost as u32, Ordering::Relaxed);
                    for w in &widgets {
                        w.set_topmost(cfg.topmost);
                    }
                }
                first = true;
                log("config reloaded");
            }
        }

        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.1);
        last = now;
        let cap_changed = g.update_capture();
        let mut drew = false;
        for w in widgets.iter_mut() {
            let need = w.tick(dt);
            if need || cap_changed || first {
                w.render(&g, &cfg.material);
                drew = true;
            }
        }
        first = false;
        unsafe {
            if drew {
                let _ = DwmFlush();
            } else {
                let frame = 1000 / cfg.fps.clamp(10, 240);
                MsgWaitForMultipleObjects(None, false, frame, QS_ALLINPUT);
            }
        }
    }
    save_positions(&mut cfg, &widgets);
    log("exit");
    Ok(())
}

fn save_positions(cfg: &mut config::Config, widgets: &[Box<Widget>]) {
    for (c, w) in cfg.widgets.iter_mut().zip(widgets) {
        let (x, y) = w.glass_pos();
        c.x = x;
        c.y = y;
    }
    // keep material edits made on disk
    let disk = config::load();
    let mut out = cfg.clone();
    out.material = disk.material;
    config::save(&out);
}
