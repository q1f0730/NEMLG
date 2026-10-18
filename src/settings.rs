//! Settings share the music card's live desktop-refraction renderer.
use std::cell::RefCell;
use windows::core::Result;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use crate::config::{Config, Material};
use crate::gfx::*;
use crate::widget::{Kind, Widget};

pub const WIDTH: f32 = 820.;
pub const HEIGHT: f32 = 744.;
const TRACK_W: f32 = 312.;
#[derive(Clone, Copy)]
struct Spec { key: &'static str, label: &'static str, min: f64, max: f64, step: f64 }
const SPECS: [Spec; 22] = [
    Spec { key: "blur", label: "磨砂", min: 0., max: 5., step: 0.05 },
    Spec { key: "refraction", label: "折射", min: 0., max: 80., step: 0.5 },
    Spec { key: "edge", label: "透镜带宽", min: 1., max: 80., step: 0.5 },
    Spec { key: "refraction_sign", label: "折射方向", min: -1., max: 1., step: 1. },
    Spec { key: "aberration", label: "色散", min: 0., max: 0.4, step: 0.01 },
    Spec { key: "saturation", label: "饱和度", min: 0., max: 3., step: 0.01 },
    Spec { key: "brightness", label: "亮度", min: 0., max: 3., step: 0.01 },
    Spec { key: "radius", label: "圆角", min: 0., max: 74., step: 1. },
    Spec { key: "adaptive", label: "自适应压暗", min: 0., max: 1., step: 0.01 },
    Spec { key: "fps", label: "帧率", min: 10., max: 240., step: 1. },
    Spec { key: "tint_amount", label: "底色混合", min: 0., max: 1., step: 0.01 },
    Spec { key: "tint.0", label: "底色 · 红", min: 0., max: 1., step: 0.01 },
    Spec { key: "tint.1", label: "底色 · 绿", min: 0., max: 1., step: 0.01 },
    Spec { key: "tint.2", label: "底色 · 蓝", min: 0., max: 1., step: 0.01 },
    Spec { key: "rim", label: "边缘描光", min: 0., max: 1., step: 0.01 },
    Spec { key: "specular", label: "高光", min: 0., max: 1., step: 0.01 },
    Spec { key: "sheen", label: "光泽", min: 0., max: 2., step: 0.01 },
    Spec { key: "shadow", label: "阴影", min: 0., max: 1., step: 0.01 },
    Spec { key: "shadow_blur", label: "阴影柔和度", min: 0., max: 60., step: 0.5 },
    Spec { key: "shadow_y", label: "阴影偏移", min: -30., max: 30., step: 0.5 },
    Spec { key: "light.0", label: "光向 · 水平", min: -1., max: 1., step: 0.01 },
    Spec { key: "light.1", label: "光向 · 垂直", min: -1., max: 1., step: 0.01 },
];
thread_local! {
    static WINDOW: RefCell<Option<Box<Widget>>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<Config>> = const { RefCell::new(None) };
}
pub fn take() -> Option<Config> { PENDING.with(|p| p.borrow_mut().take()) }
pub fn preview() -> Option<Material> {
    WINDOW.with(|p| p.borrow().as_ref().and_then(|w| w.panel.as_ref())
        .filter(|s| !s.closed).map(|s| s.config().material))
}
pub fn open(g: &Gfx, cfg: &Config) -> Result<()> {
    let existing = WINDOW.with(|p| p.borrow().as_ref().map(|w| w.hwnd));
    if let Some(hwnd) = existing { unsafe { let _ = SetForegroundWindow(hwnd); } return Ok(()); }
    let mut pos = POINT::default();
    unsafe { let _ = GetCursorPos(&mut pos); }
    let panel = Panel::new(g, cfg)?;
    let mut window = Widget::new(g, Kind::Settings, pos.x, pos.y, false)?;
    window.panel = Some(panel);
    unsafe {
        use windows::Win32::Graphics::Gdi::*;
        let monitor = MonitorFromPoint(pos, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let mut r = RECT::default(); let _ = GetWindowRect(window.hwnd, &mut r);
            let x = info.rcWork.left + ((info.rcWork.right - info.rcWork.left) - (r.right - r.left)) / 2;
            let y = info.rcWork.top + ((info.rcWork.bottom - info.rcWork.top) - (r.bottom - r.top)) / 2;
            let _ = SetWindowPos(window.hwnd, Some(HWND_TOP), x, y, 0, 0, SWP_NOSIZE);
        }
        let _ = SetForegroundWindow(window.hwnd); let _ = SetFocus(Some(window.hwnd));
    }
    WINDOW.with(|p| *p.borrow_mut() = Some(window));
    Ok(())
}
pub fn render(g: &Gfx, dt: f32) -> bool {
    // Destroy after returning from the input callback, never while it holds the state.
    let closed = WINDOW.with(|p| p.borrow().as_ref().is_some_and(|w| w.panel.as_ref().is_some_and(|s| s.closed)));
    if closed {
        let old = WINDOW.with(|p| p.borrow_mut().take());
        if let Some(w) = old { unsafe { SetWindowLongPtrW(w.hwnd, GWLP_USERDATA, 0); let _ = DestroyWindow(w.hwnd); } }
        return false;
    }
    WINDOW.with(|p| {
        let mut p = p.borrow_mut(); let Some(w) = p.as_mut() else { return false; };
        w.tick(dt);
        let material = Material { blur: 4.6, refraction: 12., tint: [0.055, 0.065, 0.12],
            tint_amount: 0.48, brightness: 0.72, adaptive: 0.88, radius: 30., rim: 0.46,
            shadow: 0.4, ..Material::default() };
        w.render(g, &material); true
    })
}
fn quantize(s: Spec, n: f64) -> f64 {
    if !n.is_finite() { return s.min; }
    (s.min + ((n.clamp(s.min, s.max) - s.min) / s.step).round() * s.step).clamp(s.min, s.max)
}
fn display(s: Spec, value: f64) -> String {
    if s.key == "refraction_sign" { return if value < 0. { "向外" } else if value > 0. { "向内" } else { "平直" }.into(); }
    let value = quantize(s, value); let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if s.key == "fps" { format!("{text} fps") } else { text.into() }
}
fn track(i: usize) -> (f32, f32) { (50. + (i / 11) as f32 * 384., 263. + (i % 11) as f32 * 37.) }
fn inside(x: f32, y: f32, r: D2D_RECT_F) -> bool { x >= r.left && x <= r.right && y >= r.top && y <= r.bottom }
fn values(cfg: &Config) -> Vec<f64> {
    let mat = toml::Value::try_from(&cfg.material).unwrap();
    SPECS.iter().map(|s| {
        if s.key == "fps" { return cfg.fps as f64; }
        let mut parts = s.key.split('.'); let mut v = &mat[parts.next().unwrap()];
        if let Some(i) = parts.next() { v = &v[i.parse::<usize>().unwrap()]; }
        v.as_float().unwrap_or(s.min)
    }).collect()
}
pub struct Panel {
    cfg: Config, values: Vec<f64>, prefer: String, all_selected: bool,
    focus: usize, dragging: Option<usize>, hover: Option<usize>, pub closed: bool,
    title: IDWriteTextFormat, label: IDWriteTextFormat, small: IDWriteTextFormat, number: IDWriteTextFormat,
}
impl Panel {
    fn new(g: &Gfx, cfg: &Config) -> Result<Self> {
        Ok(Self { cfg: cfg.clone(), values: values(cfg), prefer: cfg.widgets.first().map(|w| w.prefer.clone()).unwrap_or_default(),
            all_selected: false, focus: 27, dragging: None, hover: None, closed: false,
            title: text_format(&g.dwrite, "Microsoft YaHei UI", 28., DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
            label: text_format(&g.dwrite, "Microsoft YaHei UI", 13., DWRITE_FONT_WEIGHT_NORMAL)?,
            small: text_format(&g.dwrite, "Microsoft YaHei UI", 11., DWRITE_FONT_WEIGHT_NORMAL)?,
            number: text_format(&g.dwrite, "Segoe UI", 12., DWRITE_FONT_WEIGHT_MEDIUM)?,
        })
    }
    fn config(&self) -> Config {
        let mut cfg = self.cfg.clone(); let mut mat = toml::Value::try_from(&cfg.material).unwrap();
        for (s, n) in SPECS.iter().zip(&self.values) {
            let n = quantize(*s, *n);
            if s.key == "fps" { cfg.fps = n as u32; continue; }
            let mut parts = s.key.split('.'); let key = parts.next().unwrap();
            if let Some(i) = parts.next() { mat[key][i.parse::<usize>().unwrap()] = toml::Value::Float(n); }
            else { mat[key] = toml::Value::Float(n); }
        }
        cfg.material = mat.try_into().unwrap();
        for w in &mut cfg.widgets { w.prefer = self.prefer.trim().to_string(); } cfg
    }
    fn save(&mut self) { PENDING.with(|p| *p.borrow_mut() = Some(self.config())); self.closed = true; }
    fn restore(&mut self) {
        let defaults = Config::default();
        self.values = values(&defaults);
        self.cfg.topmost = defaults.topmost;
        self.prefer = defaults.widgets[0].prefer.clone();
    }
    fn slider_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..SPECS.len()).find(|&i| { let (tx, ty) = track(i); inside(x, y, rect(tx - 10., ty - 10., TRACK_W + 20., 22.)) })
    }
    fn slide(&mut self, i: usize, x: f32) {
        let (tx, _) = track(i); let s = SPECS[i];
        self.values[i] = quantize(s, s.min + ((x - tx) / TRACK_W).clamp(0., 1.) as f64 * (s.max - s.min));
    }
    pub fn wants_drag(&self, x: f32, y: f32) -> bool { y < 88. && x < 750. }
    pub fn pointer(&mut self, hwnd: HWND, msg: u32, x: f32, y: f32) {
        unsafe { match msg {
            WM_LBUTTONDOWN => {
                let _ = SetFocus(Some(hwnd));
                if let Some(i) = self.slider_at(x, y) { self.focus = i + 2; self.dragging = Some(i); self.slide(i, x); SetCapture(hwnd); }
                else if inside(x, y, rect(42., 120., 260., 37.)) { self.focus = 0; self.cfg.topmost = !self.cfg.topmost; }
                else if inside(x, y, rect(338., 110., 438., 32.)) { self.focus = 1; self.all_selected = true; }
                else if y >= 150. && y <= 174. && x >= 338. && x < 774. {
                    let i = ((x - 338.) / 109.) as usize;
                    self.prefer = ["", "cloudmusic", "spotify", "qqmusic"][i.min(3)].into(); self.focus = 1; self.all_selected = true;
                }
                else if inside(x, y, rect(42., 680., 120., 38.)) { self.focus = 24; self.restore(); }
                else if inside(x, y, rect(514., 680., 110., 38.)) { self.focus = 25; self.closed = true; }
                else if inside(x, y, rect(642., 680., 136., 38.)) { self.focus = 26; self.save(); }
                else if inside(x, y, rect(756., 24., 32., 32.)) { self.closed = true; }
            }
            WM_MOUSEMOVE => { self.hover = self.slider_at(x, y); if let Some(i) = self.dragging { self.slide(i, x); } }
            WM_LBUTTONUP | WM_CAPTURECHANGED => { self.dragging = None; if msg == WM_LBUTTONUP { let _ = ReleaseCapture(); } }
            _ => {}
        } }
    }
    pub fn key(&mut self, key: u16) {
        let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
        match VIRTUAL_KEY(key) {
            VK_ESCAPE => self.closed = true,
            VK_TAB => {
                self.focus = if self.focus >= 27 { if shift { 26 } else { 0 } }
                    else if shift { (self.focus + 26) % 27 } else { (self.focus + 1) % 27 };
                self.all_selected = self.focus == 1;
            }
            VK_RETURN => { if self.focus == 24 { self.restore(); } else if self.focus == 25 { self.closed = true; } else { self.save(); } }
            VK_SPACE if self.focus == 0 => self.cfg.topmost = !self.cfg.topmost,
            VK_SPACE if self.focus == 24 => self.restore(),
            VK_SPACE if self.focus == 25 => self.closed = true,
            VK_SPACE if self.focus == 26 => self.save(),
            VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_HOME | VK_END if (2..24).contains(&self.focus) => {
                let i = self.focus - 2; let s = SPECS[i];
                let sign = if key == VK_LEFT.0 || key == VK_DOWN.0 { -1. } else { 1. };
                self.values[i] = if key == VK_HOME.0 { s.min } else if key == VK_END.0 { s.max }
                    else { quantize(s, self.values[i] + sign * s.step * if shift { 10. } else { 1. }) };
            }
            _ if self.focus == 1 && ctrl && key == b'A' as u16 => self.all_selected = true,
            _ if self.focus == 1 && ctrl && key == b'V' as u16 => {
                if let Some(text) = clipboard() { self.prefer = text.chars().filter(|c| !c.is_control()).take(160).collect(); self.all_selected = false; }
            }
            _ => {}
        }
    }
    pub fn character(&mut self, c: u32) {
        if self.focus != 1 { return; }
        if c == 8 { if self.all_selected { self.prefer.clear(); } else { self.prefer.pop(); } self.all_selected = false; }
        else if let Some(c) = char::from_u32(c).filter(|c| !c.is_control()) {
            if self.all_selected { self.prefer.clear(); self.all_selected = false; }
            if self.prefer.len() < 160 { self.prefer.push(c); }
        }
    }
    pub fn draw(&self, g: &Gfx) -> Result<()> {
        unsafe {
            let d = &g.d2d_ctx; let b = d.CreateSolidColorBrush(&rgba(1., 1., 1., 1.), None)?;
            let accent = rgba(0.66, 0.74, 1., 1.);
            let round = |x, y, w, h, radius| D2D1_ROUNDED_RECT { rect: rect(x, y, w, h), radiusX: radius, radiusY: radius };
            let fill = |r: D2D1_ROUNDED_RECT, color| { b.SetColor(&color); d.FillRoundedRectangle(&r, &b); };
            let outline = |r: D2D1_ROUNDED_RECT, color, width| { b.SetColor(&color); d.DrawRoundedRectangle(&r, &b, width, None); };
            let text = |s: &str, font: &IDWriteTextFormat, r: D2D_RECT_F, color| {
                b.SetColor(&color); let s: Vec<u16> = s.encode_utf16().collect();
                d.DrawText(&s, font, &r, &b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            };
            let white = rgba(0.96, 0.97, 1., 1.); let muted = rgba(0.78, 0.81, 0.9, 0.8);
            text("设置", &self.title, rect(34., 24., 400., 42.), white);
            text("调出属于你的玻璃质感", &self.label, rect(35., 67., 520., 23.), muted);
            fill(round(756., 24., 32., 32., 16.), rgba(1., 1., 1., 0.07));
            text("×", &self.label, rect(767., 30., 20., 24.), white);
            fill(round(30., 100., 760., 84., 18.), rgba(1., 1., 1., 0.055));
            outline(round(30., 100., 760., 84., 18.), rgba(1., 1., 1., 0.10), 1.);
            text("始终置顶", &self.label, rect(48., 119., 150., 23.), white);
            text("让音乐卡片浮在窗口上方", &self.small, rect(48., 148., 240., 18.), muted);
            fill(round(248., 123., 40., 22., 11.), if self.cfg.topmost { accent } else { rgba(1., 1., 1., 0.15) });
            fill(round(if self.cfg.topmost { 268. } else { 251. }, 126., 16., 16., 8.), white);
            if self.focus == 0 { outline(round(244., 119., 48., 30., 15.), accent, 1.); }
            let source = round(338., 110., 438., 32., 10.);
            fill(source, rgba(0.02, 0.025, 0.07, 0.24));
            outline(source, if self.focus == 1 { accent } else { rgba(1., 1., 1., 0.14) }, 1.);
            if self.focus == 1 && self.all_selected { fill(round(347., 115., 415., 21., 4.), rgba(0.55, 0.64, 1., 0.22)); }
            let source_text = if self.prefer.is_empty() { "跟随系统媒体" } else { &self.prefer };
            let source_text = if self.focus == 1 && !self.all_selected { format!("{source_text}│") } else { source_text.to_string() };
            text(&source_text, &self.label, rect(349., 116., 414., 24.), white);
            for (i, name) in ["跟随系统", "网易云", "Spotify", "QQ 音乐"].iter().enumerate() {
                let active = self.prefer == ["", "cloudmusic", "spotify", "qqmusic"][i]; let x = 338. + i as f32 * 109.;
                if active { fill(round(x, 149., 102., 25., 8.), rgba(0.6, 0.7, 1., 0.16)); }
                text(name, &self.small, rect(x + 12., 154., 92., 19.), if active { white } else { muted });
            }
            text("玻璃质感", &self.label, rect(48., 203., 300., 24.), white);
            text("光影与色彩", &self.label, rect(432., 203., 300., 24.), white);
            for col in 0..2 { fill(round(30. + col as f32 * 384., 231., 376., 431., 18.), rgba(1., 1., 1., 0.045)); }
            self.number.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)?;
            for (i, s) in SPECS.iter().enumerate() {
                let (x, y) = track(i); let value = quantize(*s, self.values[i]);
                text(s.label, &self.label, rect(x, y - 25., 220., 23.), white);
                text(&display(*s, value), &self.number, rect(x + 222., y - 25., 90., 22.), muted);
                fill(round(x, y, TRACK_W, 3., 1.5), rgba(1., 1., 1., 0.14));
                let progress = ((value - s.min) / (s.max - s.min)) as f32 * TRACK_W;
                if progress > 0. { fill(round(x, y, progress, 3., 1.5), accent); }
                if self.focus == i + 2 || self.hover == Some(i) { fill(round(x + progress - 9., y - 7.5, 18., 18., 9.), rgba(0.65, 0.74, 1., 0.2)); }
                fill(round(x + progress - 5., y - 3.5, 10., 10., 5.), white);
            }
            fill(round(42., 680., 120., 38., 12.), rgba(1., 1., 1., 0.07));
            text("恢复默认", &self.label, rect(65., 690., 100., 24.), white);
            text("拖动实时预览 · 保存后记住设置", &self.small, rect(181., 692., 310., 24.), muted);
            fill(round(514., 680., 110., 38., 12.), rgba(1., 1., 1., 0.08));
            text("取消", &self.label, rect(554., 690., 70., 24.), white);
            fill(round(642., 680., 136., 38., 12.), accent);
            text("保存并应用", &self.label, rect(674., 690., 110., 24.), rgba(0.07, 0.10, 0.20, 1.));
            for (id, x, width) in [(24, 42., 120.), (25, 514., 110.), (26, 642., 136.)] {
                if self.focus == id { outline(round(x - 3., 677., width + 6., 44., 15.), accent, 1.); }
            }
        } Ok(())
    }
}
fn clipboard() -> Option<String> {
    use windows::Win32::System::{DataExchange::*, Memory::*};
    unsafe {
        OpenClipboard(None).ok()?;
        let result = (|| {
            let handle = GetClipboardData(13).ok()?; let global = HGLOBAL(handle.0);
            let ptr = GlobalLock(global) as *const u16; if ptr.is_null() { return None; }
            let capacity = GlobalSize(global) / 2; let slice = std::slice::from_raw_parts(ptr, capacity);
            let len = slice.iter().position(|c| *c == 0).unwrap_or(capacity);
            let value = String::from_utf16_lossy(&slice[..len]); let _ = GlobalUnlock(global); Some(value)
        })();
        let _ = CloseClipboard(); result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slider_limits_and_steps() {
        for s in SPECS {
            assert_eq!(quantize(s, -1000.), s.min); assert_eq!(quantize(s, 1000.), s.max);
            assert!(quantize(s, f64::NAN).is_finite());
        }
        assert_eq!(quantize(SPECS[9], 60.4), 60.); assert_eq!(quantize(SPECS[3], -0.8), -1.);
    }
    #[test]
    fn labels_hide_float_noise() {
        assert_eq!(display(SPECS[0], 3.5999999), "3.6");
        assert_eq!(display(SPECS[9], 60.), "60 fps"); assert_eq!(display(SPECS[3], -1.), "向外");
    }
}
