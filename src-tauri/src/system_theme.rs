//! Read OS window / accent colors for the System theme style.

use serde::Serialize;

use crate::config::{normalize_accent_color, DEFAULT_ACCENT_COLOR};

#[derive(Debug, Clone, Serialize)]
pub struct SystemPalette {
    pub source: String,
    pub dark: bool,
    pub bg: String,
    pub bg_2: String,
    pub panel: String,
    pub ink: String,
    pub muted: String,
    pub line: String,
    pub accent: String,
    /// Toolkit selection foreground, when the theme defines one.
    /// The UI keeps it only if it contrasts with `accent`.
    pub on_accent: Option<String>,
    pub input_bg: String,
}

impl SystemPalette {
    fn app_fallback(accent: &str, dark: bool) -> Self {
        let accent = normalize_accent_color(accent);
        if dark {
            Self {
                source: "app-fallback".into(),
                dark: true,
                bg: "#202020".into(),
                bg_2: "#2d2d2d".into(),
                panel: "#2b2b2b".into(),
                ink: "#ffffff".into(),
                muted: "#b3b3b3".into(),
                line: "#3d3d3d".into(),
                accent,
                on_accent: None,
                input_bg: "#1f1f1f".into(),
            }
        } else {
            Self {
                source: "app-fallback".into(),
                dark: false,
                bg: "#f3f3f3".into(),
                bg_2: "#fafafa".into(),
                panel: "#ffffff".into(),
                ink: "#1a1a1a".into(),
                muted: "#5c5c5c".into(),
                line: "#e0e0e0".into(),
                accent,
                on_accent: None,
                input_bg: "#ffffff".into(),
            }
        }
    }
}

/// Preferred luminance when the OS palette does not imply dark/light clearly.
#[derive(Debug, Clone, Copy)]
pub enum PreferredLuminance {
    Light,
    Dark,
    System,
}

pub fn get_system_palette(
    preferred: PreferredLuminance,
    accent_fallback: &str,
    #[cfg(target_os = "linux")] backend: crate::config::LinuxSystemBackend,
) -> SystemPalette {
    let prefer_dark = match preferred {
        PreferredLuminance::Light => false,
        PreferredLuminance::Dark => true,
        PreferredLuminance::System => system_prefers_dark(),
    };

    #[cfg(target_os = "windows")]
    {
        if let Some(p) = windows_palette(prefer_dark, accent_fallback) {
            return p;
        }
    }

    #[cfg(target_os = "linux")]
    {
        match backend {
            crate::config::LinuxSystemBackend::Gtk => {
                if let Some(p) = gtk_palette(prefer_dark, accent_fallback) {
                    return p;
                }
            }
            crate::config::LinuxSystemBackend::Qt => {
                if let Some(p) = kde_palette(prefer_dark, accent_fallback) {
                    return p;
                }
            }
        }
    }

    SystemPalette::app_fallback(accent_fallback, prefer_dark)
}

fn system_prefers_dark() -> bool {
    #[cfg(target_os = "windows")]
    {
        return windows_apps_use_dark();
    }
    #[cfg(target_os = "linux")]
    {
        return linux_prefers_dark();
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        false
    }
}

fn luminance(r: u8, g: u8, b: u8) -> f32 {
    // Relative luminance (sRGB, approximate).
    0.2126 * (r as f32) + 0.7152 * (g as f32) + 0.0722 * (b as f32)
}

fn is_dark_bg(r: u8, g: u8, b: u8) -> bool {
    luminance(r, g, b) < 128.0
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn parse_rgb_triple(v: &str) -> Option<(u8, u8, u8)> {
    let v = v.trim();
    if let Some(h) = v.strip_prefix('#') {
        if h.len() == 6 {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            return Some((r, g, b));
        }
    }
    let parts: Vec<&str> = v.split(',').map(str::trim).collect();
    if parts.len() == 3 {
        let r: u32 = parts[0].parse().ok()?;
        let g: u32 = parts[1].parse().ok()?;
        let b: u32 = parts[2].parse().ok()?;
        if r <= 255 && g <= 255 && b <= 255 {
            return Some((r as u8, g as u8, b as u8));
        }
    }
    None
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let lerp = |x: u8, y: u8| ((x as f32) * (1.0 - t) + (y as f32) * t).round() as u8;
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

fn muted_from(ink: (u8, u8, u8), bg: (u8, u8, u8)) -> (u8, u8, u8) {
    mix(ink, bg, 0.45)
}

fn build_palette(
    source: &str,
    bg: (u8, u8, u8),
    panel: (u8, u8, u8),
    ink: (u8, u8, u8),
    line: (u8, u8, u8),
    accent: (u8, u8, u8),
    input_bg: (u8, u8, u8),
    on_accent: Option<(u8, u8, u8)>,
) -> SystemPalette {
    let dark = is_dark_bg(bg.0, bg.1, bg.2);
    let bg_2 = if dark {
        mix(bg, (255, 255, 255), 0.08)
    } else {
        mix(bg, (255, 255, 255), 0.55)
    };
    let muted = muted_from(ink, bg);
    SystemPalette {
        source: source.into(),
        dark,
        bg: hex(bg.0, bg.1, bg.2),
        bg_2: hex(bg_2.0, bg_2.1, bg_2.2),
        panel: hex(panel.0, panel.1, panel.2),
        ink: hex(ink.0, ink.1, ink.2),
        muted: hex(muted.0, muted.1, muted.2),
        line: hex(line.0, line.1, line.2),
        accent: hex(accent.0, accent.1, accent.2),
        on_accent: on_accent.map(|c| hex(c.0, c.1, c.2)),
        input_bg: hex(input_bg.0, input_bg.1, input_bg.2),
    }
}

fn accent_or_fallback(accent: Option<(u8, u8, u8)>, fallback: &str) -> (u8, u8, u8) {
    accent.unwrap_or_else(|| {
        parse_rgb_triple(&normalize_accent_color(fallback))
            .or_else(|| parse_rgb_triple(DEFAULT_ACCENT_COLOR))
            .unwrap_or((0, 120, 212))
    })
}

// —— Windows ——————————————————————————————————————————————————————————————

#[cfg(target_os = "windows")]
fn windows_apps_use_dark() -> bool {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
    else {
        return false;
    };
    let apps_light: u32 = key.get_value("AppsUseLightTheme").unwrap_or(1);
    apps_light == 0
}

#[cfg(target_os = "windows")]
fn windows_accent() -> Option<(u8, u8, u8)> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key = hkcu.open_subkey(r"Software\Microsoft\Windows\DWM").ok()?;
    // AccentColor is ABGR DWORD.
    let abgr: u32 = key.get_value("AccentColor").ok()?;
    let r = (abgr & 0xff) as u8;
    let g = ((abgr >> 8) & 0xff) as u8;
    let b = ((abgr >> 16) & 0xff) as u8;
    Some((r, g, b))
}

#[cfg(target_os = "windows")]
fn windows_palette(prefer_dark: bool, accent_fallback: &str) -> Option<SystemPalette> {
    use windows_sys::Win32::Graphics::Gdi::{
        GetSysColor, COLOR_3DFACE, COLOR_3DSHADOW, COLOR_BTNFACE, COLOR_HIGHLIGHT,
        COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT,
    };

    let sys = |idx: i32| -> (u8, u8, u8) {
        let c = unsafe { GetSysColor(idx) };
        (
            (c & 0xff) as u8,
            ((c >> 8) & 0xff) as u8,
            ((c >> 16) & 0xff) as u8,
        )
    };

    let window = sys(COLOR_WINDOW as i32);
    let text = sys(COLOR_WINDOWTEXT as i32);
    let face = sys(COLOR_BTNFACE as i32);
    let face3d = sys(COLOR_3DFACE as i32);
    let shadow = sys(COLOR_3DSHADOW as i32);
    let accent = accent_or_fallback(windows_accent(), accent_fallback);
    // Highlight text is paired with COLOR_HIGHLIGHT, so only keep it when that
    // color is the accent fill. A DWM accent would make the pairing meaningless.
    let highlight = sys(COLOR_HIGHLIGHT as i32);
    let on_accent = if accent == highlight {
        Some(sys(COLOR_HIGHLIGHTTEXT as i32))
    } else {
        None
    };

    // Classic GetSysColor stays "light window" even in dark mode on modern Windows.
    // Prefer AppsUseLightTheme for luminance; still use sys colors for borders when light.
    if prefer_dark || windows_apps_use_dark() {
        let dark = SystemPalette::app_fallback(accent_fallback, true);
        return Some(SystemPalette {
            source: "windows".into(),
            accent: hex(accent.0, accent.1, accent.2),
            line: hex(shadow.0, shadow.1, shadow.2),
            on_accent: on_accent.map(|c| hex(c.0, c.1, c.2)),
            ..dark
        });
    }

    let panel = if is_dark_bg(face.0, face.1, face.2) {
        window
    } else {
        face
    };
    Some(build_palette(
        "windows", face3d, panel, text, shadow, accent, window, on_accent,
    ))
}

// —— Linux ————————————————————————————————————————————————————————————————

#[cfg(target_os = "linux")]
fn linux_prefers_dark() -> bool {
    // XDG Settings portal color-scheme: 0 default, 1 prefer-dark, 2 prefer-light
    if let Some(v) = portal_read_u32("org.freedesktop.appearance", "color-scheme") {
        return v == 1;
    }
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
            if s.contains("prefer-dark") {
                return true;
            }
            if s.contains("prefer-light") {
                return false;
            }
        }
    }
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
            if s.contains("dark") {
                return true;
            }
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn portal_read_u32(namespace: &str, key: &str) -> Option<u32> {
    let run = |method: &str| {
        std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
                "--method",
                method,
                namespace,
                key,
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let out = run("org.freedesktop.portal.Settings.ReadOne")
        .or_else(|| run("org.freedesktop.portal.Settings.Read"))?;
    out.split(|c: char| !c.is_ascii_digit())
        .find_map(|tok| tok.parse::<u32>().ok())
}

#[cfg(target_os = "linux")]
fn portal_accent() -> Option<(u8, u8, u8)> {
    let run = |method: &str| {
        std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
                "--method",
                method,
                "org.freedesktop.appearance",
                "accent-color",
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let out = run("org.freedesktop.portal.Settings.ReadOne")
        .or_else(|| run("org.freedesktop.portal.Settings.Read"))?;
    let mut comps = out
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .filter_map(|tok| tok.parse::<f64>().ok());
    let (r, g, b) = (comps.next()?, comps.next()?, comps.next()?);
    if r < 0.0 || g < 0.0 || b < 0.0 {
        return None;
    }
    Some((
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    ))
}

#[cfg(target_os = "linux")]
fn gnome_named_accent() -> Option<(u8, u8, u8)> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "accent-color"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    let name = raw
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_lowercase();
    let hex = match name.as_str() {
        "blue" => "#3584e4",
        "teal" => "#2190a4",
        "green" => "#3a944a",
        "yellow" => "#c88800",
        "orange" => "#ed5b00",
        "red" => "#e62d42",
        "pink" => "#d56199",
        "purple" => "#9141ac",
        "slate" => "#6f8396",
        _ => return None,
    };
    parse_rgb_triple(hex)
}

#[cfg(target_os = "linux")]
fn linux_accent(fallback: &str) -> (u8, u8, u8) {
    // GTK path only: portal / GNOME named accent, then kdeglobals as last resort.
    accent_or_fallback(
        portal_accent()
            .or_else(gnome_named_accent)
            .or_else(kde_accent_only),
        fallback,
    )
}

type ColorIniMap = std::collections::HashMap<String, std::collections::HashMap<String, String>>;

#[cfg(any(target_os = "linux", test))]
fn parse_color_ini_str(contents: &str) -> ColorIniMap {
    let mut map = ColorIniMap::new();
    let mut section = String::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            map.entry(section.clone()).or_default();
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.entry(section.clone())
                .or_default()
                .insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

#[cfg(target_os = "linux")]
fn parse_color_ini_path(path: &std::path::Path) -> Option<ColorIniMap> {
    let contents = std::fs::read_to_string(path).ok()?;
    Some(parse_color_ini_str(&contents))
}

#[cfg(target_os = "linux")]
fn kde_config_path() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().join("kdeglobals"))
}

#[cfg(target_os = "linux")]
fn kde_section_color(map: &ColorIniMap, section: &str, key: &str) -> Option<(u8, u8, u8)> {
    let v = map.get(section)?.get(key)?;
    parse_rgb_triple(v)
}

#[cfg(target_os = "linux")]
fn kde_accent_only() -> Option<(u8, u8, u8)> {
    let map = parse_color_ini_path(&kde_config_path()?)?;
    qt_scheme_accent(&map)
}

/// Accent from a Plasma/qtct `.colors` map — never GNOME portal/gsettings.
#[cfg(any(target_os = "linux", test))]
fn qt_scheme_accent(map: &ColorIniMap) -> Option<(u8, u8, u8)> {
    kde_section_color_cfg(map, "Colors:Selection", "BackgroundNormal")
        .or_else(|| kde_section_color_cfg(map, "Colors:Window", "DecorationFocus"))
        .or_else(|| kde_section_color_cfg(map, "Colors:Button", "DecorationFocus"))
        .or_else(|| kde_section_color_cfg(map, "General", "AccentColor"))
}

/// Selection label from a Plasma/qtct `.colors` map.
#[cfg(any(target_os = "linux", test))]
fn qt_scheme_on_accent(map: &ColorIniMap) -> Option<(u8, u8, u8)> {
    kde_section_color_cfg(map, "Colors:Selection", "ForegroundNormal")
}

#[cfg(any(target_os = "linux", test))]
fn kde_section_color_cfg(map: &ColorIniMap, section: &str, key: &str) -> Option<(u8, u8, u8)> {
    let v = map.get(section)?.get(key)?;
    parse_rgb_triple(v)
}

#[cfg(any(target_os = "linux", test))]
fn qtct_color_scheme_path_from_conf(contents: &str) -> Option<String> {
    let map = parse_color_ini_str(contents);
    let path = map.get("Appearance")?.get("color_scheme_path")?;
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

#[cfg(target_os = "linux")]
fn expand_user_path(raw: &str) -> std::path::PathBuf {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = directories::BaseDirs::new() {
            return home.home_dir().join(rest);
        }
    }
    if raw == "~" {
        if let Some(home) = directories::BaseDirs::new() {
            return home.home_dir().to_path_buf();
        }
    }
    std::path::PathBuf::from(raw)
}

#[cfg(target_os = "linux")]
fn qtct_scheme_map() -> Option<ColorIniMap> {
    let base = directories::BaseDirs::new()?.config_dir().to_path_buf();
    for name in ["qt6ct/qt6ct.conf", "qt5ct/qt5ct.conf"] {
        let conf_path = base.join(name);
        let Ok(contents) = std::fs::read_to_string(&conf_path) else {
            continue;
        };
        let Some(scheme) = qtct_color_scheme_path_from_conf(&contents) else {
            continue;
        };
        let scheme_path = expand_user_path(&scheme);
        if let Some(map) = parse_color_ini_path(&scheme_path) {
            if map.keys().any(|k| k.starts_with("Colors:")) {
                return Some(map);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn plasma_named_scheme_map() -> Option<ColorIniMap> {
    let name = {
        let from_globals = parse_color_ini_path(&kde_config_path()?)
            .and_then(|m| m.get("General")?.get("ColorScheme").cloned());
        let from_defaults = directories::BaseDirs::new()
            .map(|b| b.config_dir().join("kdedefaults").join("kdeglobals"))
            .and_then(|p| parse_color_ini_path(&p))
            .and_then(|m| m.get("General")?.get("ColorScheme").cloned());
        from_globals.or(from_defaults)?
    };
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let file_name = if name.ends_with(".colors") {
        name.to_string()
    } else {
        format!("{name}.colors")
    };
    let mut candidates = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        candidates.push(base.data_dir().join("color-schemes").join(&file_name));
        candidates.push(
            base.home_dir()
                .join(".local/share/color-schemes")
                .join(&file_name),
        );
    }
    candidates.push(std::path::PathBuf::from("/usr/share/color-schemes").join(&file_name));
    candidates.push(std::path::PathBuf::from("/usr/local/share/color-schemes").join(&file_name));
    for path in candidates {
        if let Some(map) = parse_color_ini_path(&path) {
            if map.keys().any(|k| k.starts_with("Colors:")) {
                return Some(map);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn resolve_qt_color_map() -> Option<ColorIniMap> {
    if let Some(map) = qtct_scheme_map() {
        return Some(map);
    }
    if let Some(map) = plasma_named_scheme_map() {
        return Some(map);
    }
    let globals = parse_color_ini_path(&kde_config_path()?)?;
    if globals.keys().any(|k| k.starts_with("Colors:")) {
        Some(globals)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn kde_palette(prefer_dark: bool, accent_fallback: &str) -> Option<SystemPalette> {
    let map = resolve_qt_color_map()?;
    let bg = kde_section_color(&map, "Colors:Window", "BackgroundNormal")
        .or_else(|| kde_section_color(&map, "Colors:View", "BackgroundNormal"))?;
    let ink = kde_section_color(&map, "Colors:Window", "ForegroundNormal")
        .or_else(|| kde_section_color(&map, "Colors:View", "ForegroundNormal"))
        .unwrap_or(if prefer_dark || is_dark_bg(bg.0, bg.1, bg.2) {
            (255, 255, 255)
        } else {
            (26, 26, 26)
        });
    let panel = kde_section_color(&map, "Colors:View", "BackgroundNormal").unwrap_or(bg);
    let line = kde_section_color(&map, "Colors:Window", "ForegroundInactive")
        .or_else(|| kde_section_color(&map, "Colors:Button", "BackgroundNormal"))
        .unwrap_or(mix(bg, ink, 0.2));
    let input_bg = kde_section_color(&map, "Colors:View", "BackgroundNormal").unwrap_or(panel);
    // Qt/Plasma scheme only — do not use GNOME portal/gsettings accents.
    let accent = accent_or_fallback(qt_scheme_accent(&map), accent_fallback);
    let on_accent = qt_scheme_on_accent(&map);
    Some(build_palette(
        "qt", bg, panel, ink, line, accent, input_bg, on_accent,
    ))
}

#[cfg(target_os = "linux")]
fn gtk_lookup_color(name: &str) -> Option<(u8, u8, u8)> {
    use gtk::prelude::*;
    if !gtk::is_initialized() {
        return None;
    }
    // StyleContext lookups need a widget; a plain Label is enough once GTK is up.
    let label = gtk::Label::new(None);
    let ctx = label.style_context();
    let rgba = ctx.lookup_color(name)?;
    Some((
        (rgba.red().clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgba.green().clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgba.blue().clamp(0.0, 1.0) * 255.0).round() as u8,
    ))
}

#[cfg(target_os = "linux")]
fn gtk_palette(prefer_dark: bool, accent_fallback: &str) -> Option<SystemPalette> {
    if !gtk::is_initialized() {
        return None;
    }
    let bg = gtk_lookup_color("theme_bg_color").or_else(|| gtk_lookup_color("theme_base_color"))?;
    let ink = gtk_lookup_color("theme_fg_color")
        .or_else(|| gtk_lookup_color("theme_text_color"))
        .unwrap_or(if prefer_dark || is_dark_bg(bg.0, bg.1, bg.2) {
            (255, 255, 255)
        } else {
            (26, 26, 26)
        });
    let panel = gtk_lookup_color("theme_base_color").unwrap_or(bg);
    let line = gtk_lookup_color("borders")
        .or_else(|| gtk_lookup_color("unfocused_borders"))
        .unwrap_or(mix(bg, ink, 0.22));
    let input_bg = gtk_lookup_color("theme_base_color").unwrap_or(panel);
    let selected = gtk_lookup_color("theme_selected_bg_color");
    let accent = accent_or_fallback(
        selected.or_else(portal_accent).or_else(gnome_named_accent),
        accent_fallback,
    );
    // Only the GTK selection color is paired with theme_selected_fg_color.
    // A portal or named accent has no matching foreground.
    let on_accent = if selected.is_some() {
        gtk_lookup_color("theme_selected_fg_color")
    } else {
        None
    };
    Some(build_palette(
        "gtk", bg, panel, ink, line, accent, input_bg, on_accent,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_fallback_light_and_dark() {
        let light = SystemPalette::app_fallback("#0078d4", false);
        assert!(!light.dark);
        assert_eq!(light.bg, "#f3f3f3");
        assert_eq!(light.accent, "#0078d4");
        let dark = SystemPalette::app_fallback("bad", true);
        assert!(dark.dark);
        assert_eq!(dark.accent, DEFAULT_ACCENT_COLOR);
    }

    #[test]
    fn parse_hex_and_csv() {
        assert_eq!(parse_rgb_triple("#ff8800"), Some((255, 136, 0)));
        assert_eq!(parse_rgb_triple("12,34,56"), Some((12, 34, 56)));
    }

    #[test]
    fn parse_colors_scheme_accent_and_window() {
        let ini = r#"
[Colors:Window]
BackgroundNormal=10,22,17
ForegroundNormal=216,230,222
DecorationFocus=0,224,177

[Colors:View]
BackgroundNormal=10,22,17
ForegroundNormal=216,230,222

[Colors:Selection]
BackgroundNormal=0,224,177
ForegroundNormal=0,56,42
"#;
        let map = parse_color_ini_str(ini);
        assert_eq!(
            kde_section_color_cfg(&map, "Colors:Window", "BackgroundNormal"),
            Some((10, 22, 17))
        );
        assert_eq!(qt_scheme_accent(&map), Some((0, 224, 177)));
        assert_eq!(qt_scheme_on_accent(&map), Some((0, 56, 42)));
        let palette = build_palette(
            "qt",
            (10, 22, 17),
            (10, 22, 17),
            (216, 230, 222),
            (80, 90, 85),
            (0, 224, 177),
            (10, 22, 17),
            qt_scheme_on_accent(&map),
        );
        assert_eq!(palette.on_accent.as_deref(), Some("#00382a"));
    }

    #[test]
    fn qtct_conf_reads_color_scheme_path() {
        let conf = r#"
[Appearance]
color_scheme_path=/home/user/.local/share/color-schemes/DankMatugen.colors
custom_palette=true
style=Breeze
"#;
        assert_eq!(
            qtct_color_scheme_path_from_conf(conf).as_deref(),
            Some("/home/user/.local/share/color-schemes/DankMatugen.colors")
        );
    }
}
