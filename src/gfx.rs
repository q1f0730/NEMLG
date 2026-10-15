//! D3D11 device, desktop capture (DXGI Desktop Duplication), shader, D2D/DWrite.

use windows::core::{s, Interface, Result, PCSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::Fxc::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

const SHADER: &str = include_str!("glass.hlsl");

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Consts {
    pub rect_px: [f32; 4],
    pub win_screen: [f32; 4],
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    pub tint: [f32; 4],
    pub p2: [f32; 4],
    pub light: [f32; 4],
    pub p3: [f32; 4],
}

pub struct Capture {
    output: IDXGIOutput1,
    dup: Option<IDXGIOutputDuplication>,
    pub origin: (i32, i32),
    pub size: (u32, u32),
    pub tex: ID3D11Texture2D,
    pub srv: ID3D11ShaderResourceView,
    pub ok: bool,
    retry_at: std::time::Instant,
}

pub struct Gfx {
    pub device: ID3D11Device,
    pub ctx: ID3D11DeviceContext,
    pub dxgi_factory: IDXGIFactory2,
    pub dcomp: IDCompositionDevice,
    pub d2d_ctx: ID2D1DeviceContext,
    pub dwrite: IDWriteFactory,
    pub vs: ID3D11VertexShader,
    pub ps: ID3D11PixelShader,
    pub cb: ID3D11Buffer,
    pub sampler: ID3D11SamplerState,
    pub cap: Capture,
}

unsafe fn compile(entry: PCSTR, target: PCSTR) -> Result<ID3DBlob> {
    let mut blob = None;
    let mut err = None;
    let r = D3DCompile(
        SHADER.as_ptr() as _,
        SHADER.len(),
        s!("glass.hlsl"),
        None,
        None,
        entry,
        target,
        D3DCOMPILE_OPTIMIZATION_LEVEL3,
        0,
        &mut blob,
        Some(&mut err),
    );
    if let Err(e) = r {
        if let Some(err) = err {
            let msg = std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize());
            crate::log(&format!("shader: {}", String::from_utf8_lossy(msg)));
        }
        return Err(e);
    }
    Ok(blob.unwrap())
}

unsafe fn blob_bytes(b: &ID3DBlob) -> &[u8] {
    std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())
}

impl Gfx {
    pub fn new() -> Result<Gfx> {
        unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory1()?;
            // first adapter that drives an output (the primary monitor)
            let mut adapter: Option<IDXGIAdapter1> = None;
            let mut output: Option<IDXGIOutput> = None;
            let mut i = 0;
            while let Ok(a) = factory.EnumAdapters1(i) {
                if let Ok(o) = a.EnumOutputs(0) {
                    adapter = Some(a);
                    output = Some(o);
                    break;
                }
                i += 1;
            }
            let adapter = adapter.ok_or(windows::core::Error::from(E_FAIL))?;
            let output: IDXGIOutput1 = output.unwrap().cast()?;

            let mut device = None;
            let mut ctx = None;
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )?;
            let device: ID3D11Device = device.unwrap();
            let ctx = ctx.unwrap();
            let dxgi_device: IDXGIDevice = device.cast()?;

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;

            let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
            let d2d_ctx = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            d2d_ctx.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let vsb = compile(s!("vs"), s!("vs_5_0"))?;
            let psb = compile(s!("ps"), s!("ps_5_0"))?;
            let mut vs = None;
            device.CreateVertexShader(blob_bytes(&vsb), None, Some(&mut vs))?;
            let mut ps = None;
            device.CreatePixelShader(blob_bytes(&psb), None, Some(&mut ps))?;

            let mut cb = None;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: std::mem::size_of::<Consts>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut cb),
            )?;
            let mut sampler = None;
            device.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                },
                Some(&mut sampler),
            )?;

            let cap = Capture::new(&device, output)?;
            Ok(Gfx {
                device,
                ctx,
                dxgi_factory: factory,
                dcomp,
                d2d_ctx,
                dwrite,
                vs: vs.unwrap(),
                ps: ps.unwrap(),
                cb: cb.unwrap(),
                sampler: sampler.unwrap(),
                cap,
            })
        }
    }

    pub fn upload(&self, c: &Consts) {
        unsafe {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            if self.ctx.Map(&self.cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)).is_ok() {
                std::ptr::copy_nonoverlapping(c as *const Consts as *const u8, m.pData as *mut u8, std::mem::size_of::<Consts>());
                self.ctx.Unmap(&self.cb, 0);
            }
        }
    }

    /// Pull the newest desktop frame. Returns true when the backdrop changed.
    pub fn update_capture(&mut self) -> bool {
        unsafe { self.cap.update(&self.device, &self.ctx) }
    }
}

impl Capture {
    unsafe fn new(device: &ID3D11Device, output: IDXGIOutput1) -> Result<Capture> {
        let desc = output.GetDesc()?;
        let r = desc.DesktopCoordinates;
        let (w, h) = ((r.right - r.left) as u32, (r.bottom - r.top) as u32);
        let mut tex = None;
        device.CreateTexture2D(
            &D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: 0,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
            },
            None,
            Some(&mut tex),
        )?;
        let tex: ID3D11Texture2D = tex.unwrap();
        let mut srv = None;
        device.CreateShaderResourceView(&tex, None, Some(&mut srv))?;
        let mut c = Capture {
            output,
            dup: None,
            origin: (r.left, r.top),
            size: (w, h),
            tex,
            srv: srv.unwrap(),
            ok: false,
            retry_at: std::time::Instant::now(),
        };
        c.reopen(device);
        Ok(c)
    }

    unsafe fn reopen(&mut self, device: &ID3D11Device) {
        self.dup = None;
        match self.output.DuplicateOutput(device) {
            Ok(d) => {
                self.dup = Some(d);
                self.ok = true;
            }
            Err(e) => {
                crate::log(&format!("DuplicateOutput failed: {e:?}"));
                self.ok = false;
                self.retry_at = std::time::Instant::now() + std::time::Duration::from_secs(2);
            }
        }
    }

    unsafe fn update(&mut self, device: &ID3D11Device, ctx: &ID3D11DeviceContext) -> bool {
        if self.dup.is_none() {
            if std::time::Instant::now() >= self.retry_at {
                self.reopen(device);
            }
            if self.dup.is_none() {
                return false;
            }
        }
        let dup = self.dup.clone().unwrap();
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res: Option<IDXGIResource> = None;
        match dup.AcquireNextFrame(0, &mut info, &mut res) {
            Ok(()) => {
                let mut changed = false;
                if info.LastPresentTime != 0 {
                    if let Some(res) = res {
                        if let Ok(t) = res.cast::<ID3D11Texture2D>() {
                            ctx.CopySubresourceRegion(&self.tex, 0, 0, 0, 0, &t, 0, None);
                            ctx.GenerateMips(&self.srv);
                            changed = true;
                        }
                    }
                }
                let _ = dup.ReleaseFrame();
                changed
            }
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => false,
            Err(e) => {
                crate::log(&format!("AcquireNextFrame: {e:?}"));
                self.dup = None;
                self.retry_at = std::time::Instant::now() + std::time::Duration::from_millis(500);
                false
            }
        }
    }
}

// ---------- D2D helpers ----------

pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }
}

pub fn text_format(dw: &IDWriteFactory, family: &str, size: f32, weight: DWRITE_FONT_WEIGHT) -> Result<IDWriteTextFormat> {
    unsafe {
        let fam: Vec<u16> = family.encode_utf16().chain(Some(0)).collect();
        let loc: Vec<u16> = "zh-cn".encode_utf16().chain(Some(0)).collect();
        let f = dw.CreateTextFormat(
            windows::core::PCWSTR(fam.as_ptr()),
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            windows::core::PCWSTR(loc.as_ptr()),
        )?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        let sign = dw.CreateEllipsisTrimmingSign(&f)?;
        f.SetTrimming(
            &DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 },
            &sign,
        )?;
        Ok(f)
    }
}
