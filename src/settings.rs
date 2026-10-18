//! Modeless native settings; the normal message loop keeps the music card alive.
use std::cell::RefCell;
use windows::core::{w, HSTRING, Result};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use crate::config::Config;

thread_local! {
    static WINDOW: RefCell<Option<HWND>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<Config>> = const { RefCell::new(None) };
}
pub fn take() -> Option<Config> { PENDING.with(|p| p.borrow_mut().take()) }
pub fn translate(msg: &MSG) -> bool {
    let hwnd = WINDOW.with(|p| *p.borrow());
    hwnd.map(|hwnd| unsafe { IsDialogMessageW(hwnd, msg).as_bool() }).unwrap_or(false)
}

struct Field { path: String, edit: HWND, min: f64, max: f64 }
struct State { cfg: Config, fields: Vec<Field>, top: HWND, prefer: HWND }

unsafe fn control(parent: HWND, class: &str, title: &str, style: WINDOW_STYLE,
                  x: i32, y: i32, width: i32, id: isize) -> Result<HWND> {
    CreateWindowExW(WINDOW_EX_STYLE::default(), &HSTRING::from(class), &HSTRING::from(title),
        WS_CHILD | WS_VISIBLE | style, x, y, width, 25, Some(parent),
        Some(HMENU(id as _)), Some(GetModuleHandleW(None)?.into()), None)
}
unsafe fn value(hwnd: HWND) -> String {
    let mut buf = vec![0u16; GetWindowTextLengthW(hwnd) as usize + 1];
    let n = GetWindowTextW(hwnd, &mut buf);
    String::from_utf16_lossy(&buf[..n as usize])
}

pub fn open(cfg: &Config) -> Result<()> {
    unsafe {
        if let Some(hwnd) = WINDOW.with(|p| *p.borrow()) {
            let _ = SetForegroundWindow(hwnd);
            return Ok(());
        }
        let instance = GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(),
            lpszClassName: w!("GlassDockSettings"), hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(6 as _), ..Default::default() });
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE::default(), w!("GlassDockSettings"),
            w!("GlassDock 设置"), WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT, CW_USEDEFAULT, 760, 590, None, None, Some(instance.into()), None)?;
        let top = control(hwnd, "BUTTON", "始终置顶", WINDOW_STYLE(0x00000003) | WS_TABSTOP, 20, 15, 170, 10)?;
        SendMessageW(top, BM_SETCHECK, Some(WPARAM(cfg.topmost as usize)), None);
        control(hwnd, "STATIC", "音乐来源（留空跟随系统）：", WINDOW_STYLE::default(), 210, 15, 260, 0)?;
        let prefer = control(hwnd, "EDIT", cfg.widgets.first().map(|w| w.prefer.as_str()).unwrap_or(""),
            WS_BORDER | WS_TABSTOP | WINDOW_STYLE(0x80), 480, 15, 240, 11)?;
        let mut state = Box::new(State { cfg: cfg.clone(), fields: vec![], top, prefer });
        let specs = [
            ("fps", "帧率 (10–240)", 10., 240.),
            ("blur", "磨砂 (0–5)", 0., 5.),
            ("refraction", "折射 (0–80)", 0., 80.),
            ("edge", "透镜带宽 (1–80)", 1., 80.),
            ("refraction_sign", "折射方向 (-1–1)", -1., 1.),
            ("aberration", "色散 (0–0.4)", 0., 0.4),
            ("saturation", "饱和度 (0–3)", 0., 3.),
            ("brightness", "亮度 (0–3)", 0., 3.),
            ("tint.0", "底色红 (0–1)", 0., 1.),
            ("tint.1", "底色绿 (0–1)", 0., 1.),
            ("tint.2", "底色蓝 (0–1)", 0., 1.),
            ("tint_amount", "底色混合 (0–1)", 0., 1.),
            ("rim", "描边 (0–1)", 0., 1.),
            ("specular", "高光 (0–1)", 0., 1.),
            ("sheen", "光泽 (0–2)", 0., 2.),
            ("shadow", "阴影 (0–1)", 0., 1.),
            ("shadow_blur", "阴影模糊 (0–60)", 0., 60.),
            ("shadow_y", "阴影偏移 (-30–30)", -30., 30.),
            ("light.0", "光向 X (-1–1)", -1., 1.),
            ("light.1", "光向 Y (-1–1)", -1., 1.),
            ("radius", "圆角 (0–74)", 0., 74.),
            ("adaptive", "自适应压暗 (0–1)", 0., 1.),
        ];
        let material = toml::Value::try_from(&cfg.material).expect("material serialization");
        for (i, (path, label, min, max)) in specs.iter().enumerate() {
            let x = 20 + (i / 11) as i32 * 365;
            let y = 60 + (i % 11) as i32 * 36;
            let initial = if *path == "fps" { cfg.fps.to_string() } else {
                let mut parts = path.split('.');
                let mut v = &material[parts.next().unwrap()];
                if let Some(index) = parts.next() { v = &v[index.parse::<usize>().unwrap()]; }
                v.to_string()
            };
            control(hwnd, "STATIC", label, WINDOW_STYLE::default(), x, y + 3, 235, 0)?;
            let edit = control(hwnd, "EDIT", &initial, WS_BORDER | WS_TABSTOP | WINDOW_STYLE(0x80), x + 235, y, 95, 100 + i as isize)?;
            state.fields.push(Field { path: path.to_string(), edit, min: *min, max: *max });
        }
        control(hwnd, "STATIC", "修改后点击保存，立即生效。音乐来源示例：cloudmusic / spotify / qqmusic", WINDOW_STYLE::default(), 20, 465, 710, 0)?;
        control(hwnd, "BUTTON", "保存并应用", WS_TABSTOP | WINDOW_STYLE(1), 450, 505, 130, 1)?;
        control(hwnd, "BUTTON", "取消", WS_TABSTOP, 600, 505, 120, 2)?;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        WINDOW.with(|p| *p.borrow_mut() = Some(hwnd));
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    Ok(())
}

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if msg == WM_NCDESTROY && !ptr.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
            WINDOW.with(|p| *p.borrow_mut() = None);
        } else if msg == WM_COMMAND && !ptr.is_null() {
            match wp.0 & 0xffff {
                2 => { let _ = DestroyWindow(hwnd); return LRESULT(0); }
                1 => {
                    let state = &*ptr;
                    let mut cfg = state.cfg.clone();
                    let mut mat = toml::Value::try_from(&cfg.material).unwrap();
                    for f in &state.fields {
                        let n = value(f.edit).trim().parse::<f64>().ok();
                        let valid = n.filter(|n| n.is_finite() && *n >= f.min && *n <= f.max
                            && (f.path != "fps" || n.fract() == 0.));
                        let Some(n) = valid else {
                            MessageBoxW(Some(hwnd), w!("请输入标注范围内的有效数字；帧率必须是整数。"), w!("设置未保存"), MB_ICONWARNING);
                            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(f.edit));
                            return LRESULT(0);
                        };
                        if f.path == "fps" { cfg.fps = n as u32; } else {
                            let mut parts = f.path.split('.');
                            let key = parts.next().unwrap();
                            if let Some(index) = parts.next() {
                                mat[key][index.parse::<usize>().unwrap()] = toml::Value::Float(n);
                            } else { mat[key] = toml::Value::Float(n); }
                        }
                    }
                    cfg.material = mat.try_into().unwrap();
                    cfg.topmost = SendMessageW(state.top, BM_GETCHECK, None, None).0 == 1;
                    for widget in &mut cfg.widgets { widget.prefer = value(state.prefer).trim().to_string(); }
                    PENDING.with(|p| *p.borrow_mut() = Some(cfg));
                    let _ = DestroyWindow(hwnd);
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}
