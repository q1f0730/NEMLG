//! System media (SMTC) bridge: reads what's playing and sends play/pause/skip.
//! Works with any player that publishes a Windows media session
//! (网易云音乐 3.x: 设置 → 系统 → 开启SMTC; Spotify, QQ音乐, browsers, ...).

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::core::{Interface, HSTRING};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::IRandomAccessStreamReference;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::WinRT::CreateStreamOverRandomAccessStream;

#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    Toggle,
    Next,
    Prev,
}

#[derive(Clone, Default)]
pub struct Cover {
    pub w: u32,
    pub h: u32,
    pub bgra: Arc<Vec<u8>>, // premultiplied BGRA
}

#[derive(Clone, Default)]
pub struct State {
    pub has_session: bool,
    pub app: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// seconds; end == 0 means the player gives no timeline
    pub pos: f64,
    pub end: f64,
    /// FILETIME (100ns) when pos was sampled, for extrapolation
    pub pos_stamp: i64,
    pub cover: Option<Cover>,
    /// bumps whenever title/cover changes
    pub rev: u64,
}

impl State {
    pub fn position_now(&self) -> f64 {
        if !self.playing || self.end <= 0.0 {
            return self.pos;
        }
        let dt = (now_filetime() - self.pos_stamp) as f64 / 1e7;
        (self.pos + dt.max(0.0)).min(self.end)
    }
}

pub fn now_filetime() -> i64 {
    unsafe {
        let ft = GetSystemTimeAsFileTime();
        ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
    }
}

pub fn spawn(prefer: String, shared: Arc<Mutex<State>>, rx: Receiver<Cmd>) {
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        loop {
            match run(&prefer, &shared, &rx) {
                Ok(()) => return,
                Err(e) => {
                    crate::log(&format!("media: {e:?}"));
                    if let Ok(mut s) = shared.lock() {
                        s.has_session = false;
                    }
                    std::thread::sleep(Duration::from_secs(3));
                }
            }
        }
    });
}

fn pick(mgr: &Manager, prefer: &str) -> Option<Session> {
    if !prefer.is_empty() {
        if let Ok(list) = mgr.GetSessions() {
            let n = list.Size().unwrap_or(0);
            for i in 0..n {
                if let Ok(s) = list.GetAt(i) {
                    let id = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
                    if id.to_lowercase().contains(&prefer.to_lowercase()) {
                        return Some(s);
                    }
                }
            }
        }
    }
    mgr.GetCurrentSession().ok()
}

fn app_name(id: &str) -> String {
    let l = id.to_lowercase();
    if l.contains("cloudmusic") || l.contains("netease") {
        "网易云音乐".into()
    } else if l.contains("spotify") {
        "Spotify".into()
    } else if l.contains("qqmusic") {
        "QQ音乐".into()
    } else if l.contains("kugou") {
        "酷狗音乐".into()
    } else if l.contains("chrome") || l.contains("msedge") || l.contains("firefox") {
        "浏览器".into()
    } else {
        id.rsplit(['\\', '!']).next().unwrap_or(id).trim_end_matches(".exe").to_string()
    }
}

fn run(prefer: &str, shared: &Arc<Mutex<State>>, rx: &Receiver<Cmd>) -> windows::core::Result<()> {
    let mgr = Manager::RequestAsync()?.join()?;
    let mut last_key = String::new();
    let mut rev = 0u64;
    loop {
        let session = pick(&mgr, prefer);
        // commands
        while let Ok(cmd) = rx.try_recv() {
            if let Some(s) = &session {
                let r = match cmd {
                    Cmd::Toggle => s.TryTogglePlayPauseAsync().and_then(|a| a.join()),
                    Cmd::Next => s.TrySkipNextAsync().and_then(|a| a.join()),
                    Cmd::Prev => s.TrySkipPreviousAsync().and_then(|a| a.join()),
                };
                if let Err(e) = r {
                    crate::log(&format!("media cmd {cmd:?}: {e:?}"));
                }
            }
        }

        let mut st = State::default();
        if let Some(s) = &session {
            st.has_session = true;
            let id = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
            st.app = app_name(&id);
            if let Ok(p) = s.TryGetMediaPropertiesAsync().and_then(|a| a.join()) {
                st.title = p.Title().map(|h| h.to_string()).unwrap_or_default();
                st.artist = p.Artist().map(|h| h.to_string()).unwrap_or_default();
                let key = format!("{id}|{}|{}", st.title, st.artist);
                if key != last_key {
                    last_key = key;
                    rev += 1;
                    st.cover = p.Thumbnail().ok().and_then(|t| decode_cover(&t).ok());
                } else if let Ok(old) = shared.lock() {
                    st.cover = old.cover.clone();
                }
            }
            if let Ok(info) = s.GetPlaybackInfo() {
                st.playing = info.PlaybackStatus().map(|x| x == Status::Playing).unwrap_or(false);
            }
            if let Ok(t) = s.GetTimelineProperties() {
                let end = t.EndTime().map(|d| d.Duration).unwrap_or(0) as f64 / 1e7;
                let pos = t.Position().map(|d| d.Duration).unwrap_or(0) as f64 / 1e7;
                let stamp = t.LastUpdatedTime().map(|d| d.UniversalTime).unwrap_or(0);
                st.end = end;
                st.pos = pos;
                st.pos_stamp = if stamp > 0 { stamp } else { now_filetime() };
            }
        } else {
            last_key.clear();
        }
        st.rev = rev;
        if let Ok(mut g) = shared.lock() {
            *g = st;
        }
        // wake quickly after a command
        match rx.recv_timeout(Duration::from_millis(400)) {
            Ok(cmd) => {
                if let Some(s) = &session {
                    let _ = match cmd {
                        Cmd::Toggle => s.TryTogglePlayPauseAsync().and_then(|a| a.join()),
                        Cmd::Next => s.TrySkipNextAsync().and_then(|a| a.join()),
                        Cmd::Prev => s.TrySkipPreviousAsync().and_then(|a| a.join()),
                    };
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(_) => {}
        }
    }
}

fn decode_cover(r: &IRandomAccessStreamReference) -> windows::core::Result<Cover> {
    unsafe {
        let ras = r.OpenReadAsync()?.join()?;
        let stream: IStream = CreateStreamOverRandomAccessStream(&ras)?;
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = dec.GetFrame(0)?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h)?;
        // scale down to at most 256px
        let src: IWICBitmapSource = if w > 256 || h > 256 {
            let sc = wic.CreateBitmapScaler()?;
            let k = 256.0 / (w.max(h) as f32);
            let (nw, nh) = (((w as f32) * k) as u32, ((h as f32) * k) as u32);
            sc.Initialize(&frame, nw, nh, WICBitmapInterpolationModeHighQualityCubic)?;
            w = nw;
            h = nh;
            sc.cast()?
        } else {
            frame.cast()?
        };
        let conv = wic.CreateFormatConverter()?;
        conv.Initialize(&src, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut buf)?;
        let _ = HSTRING::new();
        Ok(Cover { w, h, bgra: Arc::new(buf) })
    }
}
