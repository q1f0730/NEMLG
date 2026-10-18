//! One glass widget = one borderless DirectComposition window.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use windows::core::{w, Interface, Result, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::{Matrix3x2, Vector2};

use crate::config::Material;
use crate::gfx::*;
use crate::media::{Cmd, State};

pub const ACT_NONE: u32 = 0;
pub const ACT_TOPMOST: u32 = 1;
pub const ACT_EDIT: u32 = 2;
pub const ACT_RELOAD: u32 = 3;
pub const ACT_QUIT: u32 = 4;
pub static ACTION: AtomicU32 = AtomicU32::new(ACT_NONE);
pub static TOPMOST: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Music,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Btn {
    Prev,
    Play,
    Next,
}

struct Fonts {
    cover_symbol: IDWriteTextFormat,
    tiny: IDWriteTextFormat,
    title: IDWriteTextFormat,
    body: IDWriteTextFormat,
}

pub struct Widget {
    pub kind: Kind,
    pub hwnd: HWND,
    swap: IDXGISwapChain1,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    rtv: ID3D11RenderTargetView,
    d2d_target: ID2D1Bitmap1,
    pub scale: f32,
    w: f32,
    h: f32,
    pub margin: f32,
    px: (u32, u32),
    hover: f32,
    press: f32,
    btn_anim: [f32; 3],
    buttons: Vec<(Btn, D2D_RECT_F)>,
    pressed: Option<Btn>,
    dragging: Option<(POINT, RECT)>,
    cover_rev: u64,
    cover_bmp: Option<ID2D1Bitmap>,
    fonts: Fonts,
    pub media: Option<(Arc<Mutex<State>>, Sender<Cmd>)>,
    last_sig: String,
    last_pos: (i32, i32),
}

const CLASS: PCWSTR = w!("GlassDockWidget");

pub fn register_class() {
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }
}

fn dpi_scale_at(x: i32, y: i32) -> f32 {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTOPRIMARY);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        dx as f32 / 96.0
    }
}

impl Widget {
    pub fn new(g: &Gfx, kind: Kind, x: i32, y: i32, topmost: bool) -> Result<Box<Widget>> {
        unsafe {
            let scale = dpi_scale_at(x, y);
            let (w, h) = match kind {
                Kind::Music => (384.0, 148.0),
            };
            let margin = 30.0;
            let pw = ((w + margin * 2.0) * scale).ceil() as u32;
            let ph = ((h + margin * 2.0) * scale).ceil() as u32;
            let mpx = (margin * scale).round() as i32;

            let mut ex = WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP;
            if topmost {
                ex |= WS_EX_TOPMOST;
            }
            let hwnd = CreateWindowExW(
                ex,
                CLASS,
                w!("GlassDock"),
                WS_POPUP,
                x - mpx,
                y - mpx,
                pw as i32,
                ph as i32,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?;
            // keep our own glass out of the desktop capture (no feedback loop)
            let _ = SetWindowDisplayAffinity(hwnd, WINDOW_DISPLAY_AFFINITY(0x11));

            let swap = g.dxgi_factory.CreateSwapChainForComposition(
                &g.device,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: pw,
                    Height: ph,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                    ..Default::default()
                },
                None,
            )?;
            let target = g.dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = g.dcomp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            g.dcomp.Commit()?;

            let back: ID3D11Texture2D = swap.GetBuffer(0)?;
            let mut rtv = None;
            g.device.CreateRenderTargetView(&back, None, Some(&mut rtv))?;
            let surface: IDXGISurface = swap.GetBuffer(0)?;
            let d2d_target = g.d2d_ctx.CreateBitmapFromDxgiSurface(
                &surface,
                Some(&D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                    dpiX: 96.0 * scale,
                    dpiY: 96.0 * scale,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                    colorContext: std::mem::ManuallyDrop::new(None),
                }),
            )?;

            let dw = &g.dwrite;
            let fonts = Fonts {
                cover_symbol: text_format(dw, "Segoe UI Variable Display", 56.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
                tiny: text_format(dw, "Microsoft YaHei UI", 11.0, DWRITE_FONT_WEIGHT_NORMAL)?,
                title: text_format(dw, "Microsoft YaHei UI", 18.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
                body: text_format(dw, "Microsoft YaHei UI", 13.0, DWRITE_FONT_WEIGHT_NORMAL)?,
            };

            let mut wd = Box::new(Widget {
                kind,
                hwnd,
                swap,
                _target: target,
                _visual: visual,
                rtv: rtv.unwrap(),
                d2d_target,
                scale,
                w,
                h,
                margin,
                px: (pw, ph),
                hover: 0.0,
                press: 0.0,
                btn_anim: [0.0; 3],
                buttons: vec![],
                pressed: None,
                dragging: None,
                cover_rev: u64::MAX,
                cover_bmp: None,
                fonts,
                media: None,
                last_sig: String::new(),
                last_pos: (i32::MIN, 0),
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *wd as *mut Widget as isize);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            Ok(wd)
        }
    }

    /// glass top-left in physical screen px
    pub fn glass_pos(&self) -> (i32, i32) {
        unsafe {
            let mut r = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut r);
            let m = (self.margin * self.scale).round() as i32;
            (r.left + m, r.top + m)
        }
    }

    pub fn set_topmost(&self, on: bool) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(if on { HWND_TOPMOST } else { HWND_NOTOPMOST }),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    fn glass_contains_cursor(&self) -> bool {
        unsafe {
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            let (gx, gy) = self.glass_pos();
            let (w, h) = (self.w * self.scale, self.h * self.scale);
            (p.x as f32) >= gx as f32 && (p.x as f32) < gx as f32 + w && (p.y as f32) >= gy as f32 && (p.y as f32) < gy as f32 + h
        }
    }

    /// Advance animations; returns true if this widget needs a redraw.
    pub fn tick(&mut self, dt: f32) -> bool {
        let k = 1.0 - (-dt * 14.0).exp();
        let hover_t = if self.glass_contains_cursor() || self.dragging.is_some() { 1.0 } else { 0.0 };
        let press_t = if self.dragging.is_some() { 1.0 } else { 0.0 };
        let mut moving = false;
        let step = |v: &mut f32, t: f32, moving: &mut bool| {
            let nv = *v + (t - *v) * k;
            if (nv - t).abs() > 0.002 {
                *moving = true;
                *v = nv;
            } else {
                if *v != t {
                    *moving = true;
                }
                *v = t;
            }
        };
        step(&mut self.hover, hover_t, &mut moving);
        step(&mut self.press, press_t, &mut moving);
        for (i, b) in [Btn::Prev, Btn::Play, Btn::Next].iter().enumerate() {
            let t = if self.pressed == Some(*b) { 1.0 } else { 0.0 };
            let mut v = self.btn_anim[i];
            step(&mut v, t, &mut moving);
            self.btn_anim[i] = v;
        }
        let pos = self.glass_pos();
        if pos != self.last_pos {
            self.last_pos = pos;
            moving = true;
        }
        // content signature (media state)
        let sig = match self.kind {
            Kind::Music => {
                if let Some((st, _)) = &self.media {
                    let s = st.lock().map(|s| s.clone()).unwrap_or_default();
                    format!("{}|{}|{}|{}|{}", s.rev, s.title, s.playing, (s.position_now() * 4.0) as i64, s.has_session)
                } else {
                    String::new()
                }
            }
        };
        if sig != self.last_sig {
            self.last_sig = sig;
            moving = true;
        }
        moving
    }

    pub fn render(&mut self, g: &Gfx, m: &Material) {
        unsafe {
            let s = self.scale;
            let (gx, gy) = self.glass_pos();
            let mpx = (self.margin * s).round();
            let c = Consts {
                rect_px: [mpx, mpx, self.w * s, self.h * s],
                win_screen: [
                    (gx as f32 - mpx) - g.cap.origin.0 as f32,
                    (gy as f32 - mpx) - g.cap.origin.1 as f32,
                    g.cap.size.0 as f32,
                    g.cap.size.1 as f32,
                ],
                p0: [m.radius * s, m.refraction * s, m.edge * s, (m.blur + s.log2()).max(0.0)],
                p1: [m.aberration, m.saturation, m.brightness, m.rim],
                tint: [m.tint[0], m.tint[1], m.tint[2], m.tint_amount],
                p2: [m.shadow, m.shadow_blur * s, m.shadow_y * s, self.hover],
                light: [m.light[0], m.light[1], m.sheen, self.press],
                p3: [s, m.refraction_sign, m.specular, m.adaptive],
            };
            g.upload(&c);
            let ctx = &g.ctx;
            ctx.OMSetRenderTargets(Some(&[Some(self.rtv.clone())]), None);
            ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.px.0 as f32,
                Height: self.px.1 as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.IASetInputLayout(None);
            ctx.VSSetShader(&g.vs, None);
            ctx.PSSetShader(&g.ps, None);
            ctx.PSSetConstantBuffers(0, Some(&[Some(g.cb.clone())]));
            ctx.PSSetShaderResources(0, Some(&[Some(g.cap.srv.clone())]));
            ctx.PSSetSamplers(0, Some(&[Some(g.sampler.clone())]));
            ctx.OMSetBlendState(None, None, 0xffffffff);
            ctx.Draw(3, 0);
            ctx.PSSetShaderResources(0, Some(&[None]));
            ctx.OMSetRenderTargets(None, None);

            // content
            let d2d = &g.d2d_ctx;
            d2d.SetTarget(&self.d2d_target);
            d2d.SetDpi(96.0 * s, 96.0 * s);
            d2d.BeginDraw();
            d2d.SetTransform(&Matrix3x2::translation(self.margin, self.margin));
            let _ = match self.kind {
                Kind::Music => self.draw_music(g),
            };
            let _ = d2d.EndDraw(None, None);
            d2d.SetTarget(None);
            let _ = self.swap.Present(0, DXGI_PRESENT(0));
        }
    }

    fn draw_music(&mut self, g: &Gfx) -> Result<()> {
        let d2d = &g.d2d_ctx;
        let st = self.media.as_ref().and_then(|(s, _)| s.lock().ok().map(|x| x.clone())).unwrap_or_default();
        unsafe {
            let brush = d2d.CreateSolidColorBrush(&rgba(1.0, 1.0, 1.0, 1.0), None)?;
            // cover
            let cov = rect(16.0, 16.0, 116.0, 116.0);
            let rr = D2D1_ROUNDED_RECT { rect: cov, radiusX: 14.0, radiusY: 14.0 };
            if st.rev != self.cover_rev {
                self.cover_rev = st.rev;
                self.cover_bmp = None;
                if let Some(c) = &st.cover {
                    if c.w > 0 && c.h > 0 {
                        self.cover_bmp = d2d
                            .CreateBitmap(
                                D2D_SIZE_U { width: c.w, height: c.h },
                                Some(c.bgra.as_ptr() as _),
                                c.w * 4,
                                &D2D1_BITMAP_PROPERTIES1 {
                                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                                    dpiX: 96.0,
                                    dpiY: 96.0,
                                    bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                                    colorContext: std::mem::ManuallyDrop::new(None),
                                },
                            )
                            .ok()
                            .map(|b| b.cast().unwrap());
                    }
                }
            }
            if let Some(bmp) = &self.cover_bmp {
                let sz = bmp.GetPixelSize();
                let bb = d2d.CreateBitmapBrush(bmp, None, None)?;
                // cover-fit square crop
                let k = 116.0 / (sz.width.min(sz.height) as f32);
                let ox = 16.0 - (sz.width as f32 * k - 116.0) / 2.0;
                let oy = 16.0 - (sz.height as f32 * k - 116.0) / 2.0;
                bb.SetTransform(&(Matrix3x2 { M11: k, M12: 0.0, M21: 0.0, M22: k, M31: ox, M32: oy }));
                // soft shadow under the art
                brush.SetColor(&rgba(0.0, 0.0, 0.0, 0.18));
                d2d.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(16.0, 19.0, 116.0, 116.0), radiusX: 14.0, radiusY: 14.0 }, &brush);
                d2d.FillRoundedRectangle(&rr, &bb);
                brush.SetColor(&rgba(1.0, 1.0, 1.0, 0.18));
                d2d.DrawRoundedRectangle(&rr, &brush, 1.0 / self.scale, None);
            } else {
                brush.SetColor(&rgba(1.0, 1.0, 1.0, 0.10));
                d2d.FillRoundedRectangle(&rr, &brush);
                brush.SetColor(&rgba(1.0, 1.0, 1.0, 0.22));
                d2d.DrawRoundedRectangle(&rr, &brush, 1.0 / self.scale, None);
                text(d2d, &brush, "♪", &self.fonts.cover_symbol, rect(52.0, 34.0, 70.0, 80.0), 0.55, false);
            }

            let x = 148.0;
            let tw = self.w - x - 18.0;
            if st.has_session {
                let label = format!("{} · {}", st.app, if st.playing { "正在播放" } else { "已暂停" });
                text(d2d, &brush, &label, &self.fonts.tiny, rect(x, 16.0, tw, 16.0), 0.58, false);
                let title = if st.title.is_empty() { "未知曲目".to_string() } else { st.title.clone() };
                text(d2d, &brush, &title, &self.fonts.title, rect(x, 32.0, tw, 26.0), 0.98, true);
                text(d2d, &brush, &st.artist, &self.fonts.body, rect(x, 58.0, tw, 20.0), 0.74, true);
            } else {
                text(d2d, &brush, "没有正在播放", &self.fonts.title, rect(x, 30.0, tw, 26.0), 0.95, true);
                text(d2d, &brush, "网易云：设置 → 系统 → 开启SMTC", &self.fonts.tiny, rect(x, 58.0, tw, 18.0), 0.62, false);
            }

            let has_tl = st.end > 0.5;
            let cy = if has_tl { 120.0 } else { 108.0 };
            if has_tl {
                let pos = st.position_now();
                let f = (pos / st.end).clamp(0.0, 1.0) as f32;
                let bar = rect(x, 86.0, tw, 4.0);
                brush.SetColor(&rgba(1.0, 1.0, 1.0, 0.22));
                d2d.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: bar, radiusX: 2.0, radiusY: 2.0 }, &brush);
                brush.SetColor(&rgba(1.0, 1.0, 1.0, 0.92));
                d2d.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x, 86.0, (tw * f).max(4.0), 4.0), radiusX: 2.0, radiusY: 2.0 }, &brush);
                let tf = &self.fonts.tiny;
                text(d2d, &brush, &mmss(pos), tf, rect(x, 92.0, 60.0, 14.0), 0.55, false);
                let _ = tf.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING);
                text(d2d, &brush, &format!("-{}", mmss(st.end - pos)), tf, rect(x + tw - 60.0, 92.0, 60.0, 14.0), 0.55, false);
                let _ = tf.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            }

            // controls
            let cx = x + tw / 2.0;
            self.buttons.clear();
            let factory = d2d.GetFactory()?;
            let items = [(Btn::Prev, cx - 58.0), (Btn::Play, cx), (Btn::Next, cx + 58.0)];
            for (i, (b, bx)) in items.iter().enumerate() {
                let hit = rect(bx - 20.0, cy - 20.0, 40.0, 40.0);
                self.buttons.push((*b, hit));
                let pr = self.btn_anim[i];
                let sc = 1.0 - 0.12 * pr;
                if *b == Btn::Play {
                    brush.SetColor(&rgba(1.0, 1.0, 1.0, 0.16 + 0.10 * pr));
                    d2d.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: *bx, Y: cy }, radiusX: 19.0 * sc, radiusY: 19.0 * sc }, &brush);
                    brush.SetColor(&rgba(1.0, 1.0, 1.0, 0.30));
                    d2d.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: *bx, Y: cy }, radiusX: 19.0 * sc, radiusY: 19.0 * sc }, &brush, 1.0 / self.scale, None);
                } else if pr > 0.01 {
                    brush.SetColor(&rgba(1.0, 1.0, 1.0, 0.14 * pr));
                    d2d.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: *bx, Y: cy }, radiusX: 17.0, radiusY: 17.0 }, &brush);
                }
                brush.SetColor(&rgba(1.0, 1.0, 1.0, 0.95));
                let p = |dx: f32, dy: f32| Vector2 { X: bx + dx * sc, Y: cy + dy * sc };
                match b {
                    Btn::Play => {
                        if st.playing {
                            for ox in [-6.0f32, 2.0] {
                                let r = D2D1_ROUNDED_RECT { rect: rect(bx + ox * sc, cy - 7.5 * sc, 4.0 * sc, 15.0 * sc), radiusX: 1.2, radiusY: 1.2 };
                                d2d.FillRoundedRectangle(&r, &brush);
                            }
                        } else {
                            poly(&factory, d2d, &brush, &[p(-4.5, -8.0), p(-4.5, 8.0), p(8.5, 0.0)])?;
                        }
                    }
                    Btn::Next => {
                        poly(&factory, d2d, &brush, &[p(-8.0, -7.0), p(-8.0, 7.0), p(2.5, 0.0)])?;
                        d2d.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(bx + 3.5 * sc, cy - 7.0 * sc, 2.6 * sc, 14.0 * sc), radiusX: 1.0, radiusY: 1.0 }, &brush);
                    }
                    Btn::Prev => {
                        poly(&factory, d2d, &brush, &[p(8.0, -7.0), p(8.0, 7.0), p(-2.5, 0.0)])?;
                        d2d.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(bx - 6.1 * sc, cy - 7.0 * sc, 2.6 * sc, 14.0 * sc), radiusX: 1.0, radiusY: 1.0 }, &brush);
                    }
                }
            }
        }
        Ok(())
    }

    fn btn_at(&self, cx: i32, cy: i32) -> Option<Btn> {
        let x = cx as f32 / self.scale - self.margin;
        let y = cy as f32 / self.scale - self.margin;
        self.buttons
            .iter()
            .find(|(_, r)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
            .map(|(b, _)| *b)
    }

    fn send(&self, c: Cmd) {
        if let Some((_, tx)) = &self.media {
            let _ = tx.send(c);
        }
    }
}

unsafe fn text(d2d: &ID2D1DeviceContext, brush: &ID2D1SolidColorBrush, s: &str, f: &IDWriteTextFormat, r: D2D_RECT_F, alpha: f32, shadow: bool) {
    let w: Vec<u16> = s.encode_utf16().collect();
    if shadow {
        brush.SetColor(&rgba(0.0, 0.0, 0.0, 0.20 * alpha));
        let r2 = D2D_RECT_F { top: r.top + 1.0, bottom: r.bottom + 1.0, ..r };
        d2d.DrawText(&w, f, &r2, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
    }
    brush.SetColor(&rgba(1.0, 1.0, 1.0, alpha));
    d2d.DrawText(&w, f, &r, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
}

unsafe fn poly(factory: &ID2D1Factory, d2d: &ID2D1DeviceContext, brush: &ID2D1SolidColorBrush, pts: &[Vector2]) -> Result<()> {
    let geo = factory.CreatePathGeometry()?;
    let sink = geo.Open()?;
    sink.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_FILLED);
    sink.AddLines(&pts[1..]);
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    sink.Close()?;
    d2d.FillGeometry(&geo, brush, None);
    Ok(())
}

fn mmss(t: f64) -> String {
    let t = t.max(0.0) as i64;
    format!("{}:{:02}", t / 60, t % 60)
}

fn lparam_pt(l: LPARAM) -> (i32, i32) {
    let x = (l.0 & 0xffff) as i16 as i32;
    let y = ((l.0 >> 16) & 0xffff) as i16 as i32;
    (x, y)
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu().unwrap();
    let top = TOPMOST.load(Ordering::Relaxed) == 1;
    let _ = AppendMenuW(menu, if top { MF_CHECKED } else { MF_UNCHECKED }, ACT_TOPMOST as usize, w!("始终置顶"));
    let _ = AppendMenuW(menu, MF_STRING, ACT_EDIT as usize, w!("设置…"));
    let _ = AppendMenuW(menu, MF_STRING, ACT_RELOAD as usize, w!("重新加载配置"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, ACT_QUIT as usize, w!("退出 GlassDock"));
    let mut p = POINT::default();
    let _ = GetCursorPos(&mut p);
    let _ = SetForegroundWindow(hwnd);
    let id = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, Some(0), hwnd, None);
    let _ = DestroyMenu(menu);
    if id.0 > 0 {
        ACTION.store(id.0 as u32, Ordering::Relaxed);
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Widget;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let wd = &mut *ptr;
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_LBUTTONDOWN => {
                let (x, y) = lparam_pt(lp);
                if let Some(b) = wd.btn_at(x, y) {
                    wd.pressed = Some(b);
                } else {
                    let mut p = POINT::default();
                    let _ = GetCursorPos(&mut p);
                    let mut r = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut r);
                    wd.dragging = Some((p, r));
                }
                SetCapture(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if let Some((p0, r0)) = wd.dragging {
                    let mut p = POINT::default();
                    let _ = GetCursorPos(&mut p);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r0.left + p.x - p0.x,
                        r0.top + p.y - p0.y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = lparam_pt(lp);
                if let Some(b) = wd.pressed.take() {
                    if wd.btn_at(x, y) == Some(b) {
                        wd.send(match b {
                            Btn::Prev => Cmd::Prev,
                            Btn::Play => Cmd::Toggle,
                            Btn::Next => Cmd::Next,
                        });
                    }
                }
                wd.dragging = None;
                let _ = ReleaseCapture();
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                show_menu(hwnd);
                LRESULT(0)
            }
            WM_SETCURSOR => {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let _ = ScreenToClient(hwnd, &mut p);
                let cur = if wd.btn_at(p.x, p.y).is_some() { IDC_HAND } else { IDC_ARROW };
                SetCursor(LoadCursorW(None, cur).ok());
                LRESULT(1)
            }
            WM_DPICHANGED => LRESULT(0),
            WM_CLOSE => {
                ACTION.store(ACT_QUIT, Ordering::Relaxed);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
