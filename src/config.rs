use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    /// Frost amount, as a mip level of the captured desktop (0 = clear, 5 = heavy).
    pub blur: f32,
    /// Max refraction offset at the rim, in dip.
    pub refraction: f32,
    /// Width of the lens band along the edge, in dip.
    pub edge: f32,
    /// 1 bends the backdrop inward, -1 outward.
    pub refraction_sign: f32,
    /// Chromatic dispersion strength at the rim (0..0.4).
    pub aberration: f32,
    pub saturation: f32,
    pub brightness: f32,
    pub tint: [f32; 3],
    pub tint_amount: f32,
    /// Crisp rim line opacity.
    pub rim: f32,
    /// Specular lobe on the lit edge.
    pub specular: f32,
    /// Top-down body sheen.
    pub sheen: f32,
    pub shadow: f32,
    pub shadow_blur: f32,
    pub shadow_y: f32,
    /// Light direction (screen space, y down). Default comes from top-left.
    pub light: [f32; 2],
    pub radius: f32,
    /// Darken bright backdrops so white text stays readable (0..1).
    pub adaptive: f32,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            blur: 3.6,
            refraction: 18.0,
            edge: 16.0,
            refraction_sign: 1.0,
            aberration: 0.18,
            saturation: 1.35,
            brightness: 1.0,
            tint: [0.06, 0.06, 0.08],
            tint_amount: 0.16,
            rim: 0.62,
            specular: 0.10,
            sheen: 1.0,
            shadow: 0.30,
            shadow_blur: 18.0,
            shadow_y: 6.0,
            light: [-0.45, -1.0],
            radius: 28.0,
            adaptive: 0.6,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetCfg {
    /// Only "music" is supported.
    pub kind: String,
    /// Position in physical screen pixels (top-left of the glass).
    pub x: i32,
    pub y: i32,
    /// For music: substring of the app id to prefer (e.g. "cloudmusic", "spotify"). Empty = system current session.
    pub prefer: String,
}

impl Default for WidgetCfg {
    fn default() -> Self {
        WidgetCfg { kind: "music".into(), x: 80, y: 80, prefer: String::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Keep widgets above other windows (refracts windows behind them).
    pub topmost: bool,
    /// Max frames per second while something is changing.
    pub fps: u32,
    pub material: Material,
    #[serde(rename = "widget")]
    pub widgets: Vec<WidgetCfg>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            topmost: true,
            fps: 60,
            material: Material::default(),
            widgets: vec![
                WidgetCfg { kind: "music".into(), x: 120, y: 300, prefer: "cloudmusic".into() },
            ],
        }
    }
}

pub fn path() -> PathBuf {
    let mut p = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("glassdock.exe"));
    p.set_file_name("glassdock.toml");
    p
}

const HEADER: &str = "# GlassDock 配置 —— 保存后自动生效（材质参数热重载）\n\
# material.blur        磨砂强度，0 = 全透，5 = 很糊\n\
# material.refraction  边缘折射最大偏移（dip）\n\
# material.edge        透镜带宽度（dip）\n\
# material.aberration  边缘色散\n\
# material.rim         锐利描边亮度\n\
# material.adaptive    背景太亮时自动压暗，保证白字可读\n\
# material.tint        玻璃底色 RGB，tint_amount 为混合量\n\
# widget.prefer        音乐组件优先绑定的程序（cloudmusic / spotify / qqmusic ...）\n\n";

fn migrate(c: &mut Config) -> bool {
    let changed = c.widgets.is_empty() || c.widgets.iter().any(|w| w.kind != "music");
    c.widgets.retain(|w| w.kind == "music");
    if c.widgets.is_empty() { c.widgets = Config::default().widgets; }
    changed
}

pub fn load() -> Config {
    let p = path();
    match std::fs::read_to_string(&p) {
        Ok(s) => match toml::from_str::<Config>(&s) {
            Ok(mut c) => {
                // Migrate old configurations without recreating the removed clock.
                if migrate(&mut c) { save(&c); }
                c
            },
            Err(e) => {
                crate::log(&format!("config parse error: {e}"));
                Config::default()
            }
        },
        Err(_) => {
            let c = Config::default();
            save(&c);
            c
        }
    }
}

pub fn save_checked(c: &Config) -> std::io::Result<()> {
    let s = toml::to_string_pretty(c).map_err(std::io::Error::other)?;
    std::fs::write(path(), format!("{HEADER}{s}"))
}

pub fn save(c: &Config) {
    if let Err(e) = save_checked(c) { crate::log(&format!("config save error: {e}")); }
}

pub fn mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(path()).ok().and_then(|m| m.modified().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_music_only_and_round_trips() {
        let cfg = Config::default();
        assert_eq!(cfg.widgets.len(), 1);
        assert_eq!(cfg.widgets[0].kind, "music");
        let text = toml::to_string(&cfg).unwrap();
        let restored: Config = toml::from_str(&text).unwrap();
        assert_eq!(restored.widgets[0].prefer, "cloudmusic");
        assert_eq!(restored.material.tint, cfg.material.tint);
        assert_eq!(restored.material.light, cfg.material.light);
    }

    #[test]
    fn migration_preserves_music_position_and_source() {
        let mut cfg = Config::default();
        cfg.widgets[0].x = 842;
        cfg.widgets[0].y = 456;
        cfg.widgets[0].prefer = "spotify".into();
        cfg.widgets.insert(0, WidgetCfg { kind: "clock".into(), ..WidgetCfg::default() });
        assert!(migrate(&mut cfg));
        assert_eq!(cfg.widgets.len(), 1);
        assert_eq!((cfg.widgets[0].x, cfg.widgets[0].y), (842, 456));
        assert_eq!(cfg.widgets[0].prefer, "spotify");
        assert!(!migrate(&mut cfg));
    }

    #[test]
    fn migration_recovers_clock_only_and_empty_configs() {
        let mut cfg = Config::default();
        cfg.widgets[0].kind = "clock".into();
        assert!(migrate(&mut cfg));
        assert_eq!(cfg.widgets[0].kind, "music");
        cfg.widgets.clear();
        assert!(migrate(&mut cfg));
        assert_eq!(cfg.widgets.len(), 1);
    }
}
