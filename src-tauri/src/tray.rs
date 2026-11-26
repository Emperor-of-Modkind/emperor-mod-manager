//! Desktop system tray, window close/minimize-to-tray handling, and OS login autostart.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Window, WindowEvent};

use crate::commands::AppState;
use crate::config::BackgroundSettings;

pub const TRAY_ID: &str = "main-tray";
const MENU_SHOW_ID: &str = "tray-show";
const MENU_QUIT_ID: &str = "tray-quit";
const RESTORE_GUARD_DURATION: Duration = Duration::from_millis(400);

/// Shared runtime state for the system tray and window lifecycle interception.
pub struct TrayState {
    available: AtomicBool,
    quitting: AtomicBool,
    restoring_until: Mutex<Option<Instant>>,
}

impl TrayState {
    pub fn new() -> Self {
        Self {
            available: AtomicBool::new(false),
            quitting: AtomicBool::new(false),
            restoring_until: Mutex::new(None),
        }
    }

    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }

    pub fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::SeqCst);
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }

    pub fn mark_quitting(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    pub fn mark_restoring(&self) {
        if let Ok(mut guard) = self.restoring_until.lock() {
            *guard = Some(Instant::now() + RESTORE_GUARD_DURATION);
        }
    }

    pub fn is_restoring(&self) -> bool {
        self.restoring_until
            .lock()
            .ok()
            .and_then(|g| *g)
            .map(|until| Instant::now() < until)
            .unwrap_or(false)
    }
}

/// Initialize the desktop system tray icon and set its initial visibility.
///
/// Returns `true` when the tray icon was created successfully, or `false` if the
/// desktop environment could not host a tray icon (e.g. missing AppIndicator on Linux).
pub fn init_tray(app: &AppHandle, settings: BackgroundSettings) -> bool {
    let result = (|| -> tauri::Result<()> {
        let show_item = MenuItemBuilder::with_id(MENU_SHOW_ID, "Show Emperor Mod Manager")
            .build(app)?;
        let sep = PredefinedMenuItem::separator(app)?;
        let quit_item = MenuItemBuilder::with_id(MENU_QUIT_ID, "Quit").build(app)?;
        let menu = MenuBuilder::new(app)
            .items(&[&show_item, &sep, &quit_item])
            .build()?;

        let mut builder = TrayIconBuilder::with_id(TRAY_ID)
            .menu(&menu)
            .tooltip("Emperor Mod Manager")
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id().as_ref() {
                MENU_SHOW_ID => show_main_window(app),
                MENU_QUIT_ID => request_quit(app),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    show_main_window(tray.app_handle());
                }
            });

        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }

        let tray = builder.build(app)?;
        let _ = tray.set_visible(settings.run_in_background);
        Ok(())
    })();

    let available = match result {
        Ok(()) => true,
        Err(e) => {
            log::warn!("System tray unavailable: {e}");
            false
        }
    };

    if let Some(tray_state) = app.try_state::<TrayState>() {
        tray_state.set_available(available);
    }
    available
}

/// Show or hide the tray icon dynamically when `run_in_background` changes.
pub fn sync_tray_visibility(app: &AppHandle, visible: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_visible(visible) {
            log::warn!("Failed to update tray visibility ({visible}): {e}");
        }
    }
}

/// Restore, show, and focus the main desktop window.
pub fn show_main_window(app: &AppHandle) {
    if let Some(tray_state) = app.try_state::<TrayState>() {
        tray_state.mark_restoring();
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Exit the application immediately, bypassing close-to-tray interception.
pub fn request_quit(app: &AppHandle) {
    if let Some(tray_state) = app.try_state::<TrayState>() {
        tray_state.mark_quitting();
    }
    app.exit(0);
}

/// Decide whether the main window should start hidden in the system tray.
pub fn should_start_minimized(
    args: &[String],
    settings: BackgroundSettings,
    tray_available: bool,
) -> bool {
    if !tray_available {
        return false;
    }
    let has_flag = args.iter().any(|a| a == "--minimized");
    has_flag || (settings.run_in_background && settings.start_minimized)
}

/// Intercept main-window close and minimize events when tray options are enabled.
pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let app = window.app_handle();
    let Some(tray_state) = app.try_state::<TrayState>() else {
        return;
    };
    if !tray_state.is_available() || tray_state.is_quitting() {
        return;
    }
    let bg = app
        .try_state::<AppState>()
        .and_then(|st| st.config.lock().ok().map(|c| c.background))
        .unwrap_or_default();

    if !bg.run_in_background {
        return;
    }

    match event {
        WindowEvent::CloseRequested { api, .. } => {
            if bg.close_to_tray {
                api.prevent_close();
                let _ = window.hide();
            }
        }
        WindowEvent::Resized(_) | WindowEvent::Focused(false) => {
            if bg.minimize_to_tray
                && !tray_state.is_restoring()
                && window.is_visible().unwrap_or(false)
                && window.is_minimized().unwrap_or(false)
            {
                let _ = window.hide();
            }
        }
        _ => {}
    }
}

/// Resolve the executable path to register for login autostart.
///
/// Prefers `APPIMAGE` on Linux when running from an AppImage so autostart points
/// at the outer `.AppImage` rather than a temporary FUSE mountpoint.
fn autostart_executable() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        if !appimage.is_empty() {
            return Ok(PathBuf::from(appimage));
        }
    }
    std::env::current_exe().context("failed to resolve current executable path")
}

/// Escape an executable path for a freedesktop Desktop Entry `Exec=` value.
#[cfg(any(target_os = "linux", test))]
fn format_desktop_exec(exe: &Path, start_minimized: bool) -> String {
    let raw = exe.to_string_lossy();
    let needs_quotes = raw.chars().any(|c| {
        c.is_whitespace() || matches!(c, '"' | '\\' | '$' | '`' | '\'' | ';' | '&' | '|' | '<' | '>' | '(' | ')')
    });
    let escaped = if needs_quotes {
        let mut out = String::with_capacity(raw.len() + 2);
        out.push('"');
        for ch in raw.chars() {
            if matches!(ch, '"' | '\\' | '$' | '`') {
                out.push('\\');
            }
            out.push(ch);
        }
        out.push('"');
        out
    } else {
        raw.into_owned()
    };

    if start_minimized {
        format!("{escaped} --minimized")
    } else {
        escaped
    }
}

#[cfg(any(target_os = "linux", test))]
fn render_linux_autostart_desktop(exe: &Path, start_minimized: bool) -> String {
    let exec = format_desktop_exec(exe, start_minimized);
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=Emperor Mod Manager\n\
         Comment=Start Emperor Mod Manager in the background\n\
         Exec={exec}\n\
         Icon=emperor-mod-manager\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(any(target_os = "windows", test))]
fn format_windows_run_value(exe: &Path, start_minimized: bool) -> String {
    let raw = exe.to_string_lossy();
    if start_minimized {
        format!("\"{raw}\" --minimized")
    } else {
        format!("\"{raw}\"")
    }
}

#[cfg(any(target_os = "macos", test))]
fn render_macos_launch_agent(exe: &Path, start_minimized: bool) -> String {
    let raw = exe
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let minimized_arg = if start_minimized {
        "\n    <string>--minimized</string>"
    } else {
        ""
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
           <key>Label</key>\n\
           <string>dev.emperormodmanager.desktop</string>\n\
           <key>ProgramArguments</key>\n\
           <array>\n\
             <string>{raw}</string>{minimized_arg}\n\
           </array>\n\
           <key>RunAtLoad</key>\n\
           <true/>\n\
         </dict>\n\
         </plist>\n"
    )
}

/// Create or remove the OS login autostart entry.
pub fn sync_autostart(enabled: bool, start_minimized: bool) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .context("could not resolve XDG autostart directory")?;
        let dir = base.join("autostart");
        let entry_path = dir.join("emperor-mod-manager.desktop");
        if enabled {
            let exe = autostart_executable()?;
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
            let content = render_linux_autostart_desktop(&exe, start_minimized);
            std::fs::write(&entry_path, content)
                .with_context(|| format!("failed to write {}", entry_path.display()))?;
        } else if entry_path.exists() {
            std::fs::remove_file(&entry_path)
                .with_context(|| format!("failed to remove {}", entry_path.display()))?;
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        use winreg::RegKey;

        const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
        const VALUE_NAME: &str = "EmperorModManager";

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if enabled {
            let exe = autostart_executable()?;
            let (run_key, _) = hkcu
                .create_subkey(RUN_SUBKEY)
                .context("failed to open HKCU Run registry key")?;
            let value = format_windows_run_value(&exe, start_minimized);
            run_key
                .set_value(VALUE_NAME, &value)
                .context("failed to write Windows autostart registry value")?;
        } else if let Ok(run_key) = hkcu.open_subkey_with_flags(RUN_SUBKEY, KEY_SET_VALUE) {
            if let Err(e) = run_key.delete_value(VALUE_NAME) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(e).context("failed to remove Windows autostart registry value");
                }
            }
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").context("could not resolve HOME directory")?;
        let dir = PathBuf::from(home).join("Library/LaunchAgents");
        let plist_path = dir.join("dev.emperormodmanager.desktop.plist");
        if enabled {
            let exe = autostart_executable()?;
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
            let content = render_macos_launch_agent(&exe, start_minimized);
            std::fs::write(&plist_path, content)
                .with_context(|| format!("failed to write {}", plist_path.display()))?;
        } else if plist_path.exists() {
            std::fs::remove_file(&plist_path)
                .with_context(|| format!("failed to remove {}", plist_path.display()))?;
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = (enabled, start_minimized);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_minimized_requires_tray_available() {
        let cfg = BackgroundSettings {
            run_in_background: true,
            start_minimized: true,
            ..Default::default()
        };
        assert!(!should_start_minimized(&[], cfg, false));
        assert!(!should_start_minimized(
            &["--minimized".to_string()],
            cfg,
            false
        ));
    }

    #[test]
    fn start_minimized_honors_flag_or_config() {
        let off = BackgroundSettings::default();
        assert!(!should_start_minimized(&[], off, true));
        assert!(should_start_minimized(
            &["--minimized".to_string()],
            off,
            true
        ));

        let on = BackgroundSettings {
            run_in_background: true,
            start_minimized: true,
            ..Default::default()
        };
        assert!(should_start_minimized(&[], on, true));

        let bg_off_min_on = BackgroundSettings {
            run_in_background: false,
            start_minimized: true,
            ..Default::default()
        };
        assert!(!should_start_minimized(&[], bg_off_min_on, true));
    }

    #[test]
    fn linux_desktop_entry_formats_exec_with_and_without_spaces() {
        let plain = render_linux_autostart_desktop(Path::new("/usr/bin/emperor-mod-manager"), false);
        assert!(plain.contains("Exec=/usr/bin/emperor-mod-manager\n"));

        let spaced = render_linux_autostart_desktop(
            Path::new("/home/user/My Apps/Emperor Mod Manager.AppImage"),
            true,
        );
        assert!(spaced.contains(
            "Exec=\"/home/user/My Apps/Emperor Mod Manager.AppImage\" --minimized\n"
        ));
    }

    #[test]
    fn windows_run_value_quotes_path_and_adds_flag() {
        let exe = Path::new(r"C:\Users\Alice\AppData\Local\Emperor\emperor-mod-manager.exe");
        assert_eq!(
            format_windows_run_value(exe, false),
            "\"C:\\Users\\Alice\\AppData\\Local\\Emperor\\emperor-mod-manager.exe\""
        );
        assert_eq!(
            format_windows_run_value(exe, true),
            "\"C:\\Users\\Alice\\AppData\\Local\\Emperor\\emperor-mod-manager.exe\" --minimized"
        );
    }

    #[test]
    fn macos_launch_agent_includes_minimized_arg_when_requested() {
        let exe = Path::new("/Applications/Emperor & Co.app/Contents/MacOS/emperor");
        let normal = render_macos_launch_agent(exe, false);
        assert!(normal.contains("<string>/Applications/Emperor &amp; Co.app/Contents/MacOS/emperor</string>"));
        assert!(!normal.contains("--minimized"));

        let min = render_macos_launch_agent(exe, true);
        assert!(min.contains("<string>--minimized</string>"));
    }
}
