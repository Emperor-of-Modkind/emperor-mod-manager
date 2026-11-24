//! Uninstalled game installation options, native launcher URI handoffs,
//! and direct Heroic CLI (`legendary` / `gogdl` / `nile`) installation with live progress.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sysinfo::Disks;
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::detection::libraries::steam_library::{
    free_space_for_path, steam_library_folders_with_space, SteamLibraryFolderInfo,
};
use crate::detection::libraries::{LibraryDlcOption, LibraryGame, LibraryGameMeta, LibrarySource};
use crate::tools::heroic_launch::heroic_config_dirs;

pub const LAUNCHER_INSTALL_EVENT: &str = "launcher-install-progress";

#[derive(Debug, Clone, Serialize)]
pub struct LauncherInstallOptions {
    pub source: LibrarySource,
    pub external_id: String,
    pub title: String,
    pub default_install_dir: Option<String>,
    pub default_dir_free_bytes: Option<u64>,
    pub steam_libraries: Vec<SteamLibraryFolderInfo>,
    pub platforms: Vec<String>,
    pub dlcs: Vec<LibraryDlcOption>,
    pub languages: Vec<String>,
    pub cli_available: bool,
    pub cli_name: Option<String>,
    pub handoff_url: Option<String>,
    pub handoff_note: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallMethod {
    Handoff,
    HeroicCli,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LauncherInstallRequest {
    pub game_id: String,
    pub source: LibrarySource,
    pub external_id: String,
    pub title: String,
    pub method: InstallMethod,
    #[serde(default)]
    pub install_dir: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub dlc_ids: Vec<String>,
    #[serde(default)]
    pub metadata: LibraryGameMeta,
}

#[derive(Debug, Clone, Serialize)]
pub struct LauncherInstallResult {
    pub job_id: Option<String>,
    pub method: InstallMethod,
    pub handoff_url: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LauncherInstallProgress {
    pub job_id: String,
    pub game_id: String,
    pub title: String,
    pub source: LibrarySource,
    pub phase: String,
    pub percent: Option<f32>,
    pub speed_bps: Option<u64>,
    pub message: Option<String>,
    pub error: Option<String>,
}

/// Find a Heroic CLI helper (`legendary`, `gogdl`, or `nile`) in Heroic's bundled
/// resources or system `PATH`.
pub fn find_heroic_cli_binary(bin_name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let file_name = format!("{bin_name}.exe");
    #[cfg(not(target_os = "windows"))]
    let file_name = bin_name.to_string();

    let mut candidates: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "linux")]
    {
        for base in [
            "/opt/Heroic/resources/app.asar.unpacked/build/bin/x64/linux",
            "/usr/lib/heroic/resources/app.asar.unpacked/build/bin/x64/linux",
            "/usr/share/heroic/resources/app.asar.unpacked/build/bin/x64/linux",
            "/var/lib/flatpak/app/com.heroicgameslauncher.hgl/current/active/files/bin/heroic/resources/app.asar.unpacked/build/bin/x64/linux",
        ] {
            candidates.push(PathBuf::from(base).join(&file_name));
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            candidates.push(
                home.join(".local/share/flatpak/app/com.heroicgameslauncher.hgl/current/active/files/bin/heroic/resources/app.asar.unpacked/build/bin/x64/linux")
                    .join(&file_name),
            );
            candidates.push(home.join(".local/bin").join(&file_name));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local)
                    .join("Programs/heroic/resources/app.asar.unpacked/build/bin/x64/win32")
                    .join(&file_name),
            );
        }
        candidates.push(
            PathBuf::from(r"C:\Program Files\Heroic\resources\app.asar.unpacked\build\bin\x64\win32")
                .join(&file_name),
        );
    }

    for cand in candidates {
        if cand.is_file() {
            return Some(cand);
        }
    }

    // Check PATH
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let full = dir.join(&file_name);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

pub fn heroic_cli_name_for_source(source: LibrarySource) -> Option<&'static str> {
    match source {
        LibrarySource::HeroicEpic => Some("legendary"),
        LibrarySource::HeroicGog => Some("gogdl"),
        LibrarySource::HeroicAmazon => Some("nile"),
        _ => None,
    }
}

pub fn heroic_default_install_dir() -> Option<String> {
    for dir in heroic_config_dirs() {
        let cfg = dir.join("config.json");
        if let Ok(raw) = fs::read_to_string(&cfg) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(p) = val
                    .get("defaultSettings")
                    .and_then(|d| d.get("defaultInstallPath"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    return Some(p.to_string());
                }
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Some(PathBuf::from(home).join("Games").join("Heroic").to_string_lossy().to_string());
    }
    #[cfg(target_os = "windows")]
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return Some(
            PathBuf::from(profile)
                .join("Games")
                .join("Heroic")
                .to_string_lossy()
                .to_string(),
        );
    }
    None
}

/// Build the native launcher URI for installing `source` + `external_id`.
pub fn build_handoff_url(
    source: LibrarySource,
    external_id: &str,
    meta: &LibraryGameMeta,
) -> Option<String> {
    match source {
        LibrarySource::Steam => Some(format!("steam://install/{external_id}")),
        LibrarySource::HeroicEpic | LibrarySource::HeroicGog | LibrarySource::HeroicAmazon => {
            let runner = meta.heroic_runner.as_deref().unwrap_or(match source {
                LibrarySource::HeroicEpic => "legendary",
                LibrarySource::HeroicGog => "gog",
                LibrarySource::HeroicAmazon => "nile",
                _ => "legendary",
            });
            Some(format!(
                "heroic://launch?appName={external_id}&runner={runner}"
            ))
        }
        LibrarySource::EpicLauncher => {
            let (ns, item, app) = match (
                meta.epic_namespace.as_deref(),
                meta.epic_catalog_item_id.as_deref(),
                meta.epic_app_name.as_deref(),
            ) {
                (Some(n), Some(i), Some(a)) => (n, i, a),
                _ => {
                    let parts: Vec<&str> = external_id.split(':').collect();
                    if parts.len() == 3 {
                        (parts[0], parts[1], parts[2])
                    } else {
                        return None;
                    }
                }
            };
            Some(format!(
                "com.epicgames.launcher://apps/{ns}%3A{item}%3A{app}?action=install"
            ))
        }
        LibrarySource::Lutris => {
            let slug = meta.lutris_slug.as_deref().unwrap_or(external_id);
            Some(format!("lutris:install/{slug}"))
        }
    }
}

/// Assemble install options for the frontend modal.
pub fn build_install_options(game: &LibraryGame) -> LauncherInstallOptions {
    let disks = Disks::new_with_refreshed_list();
    let handoff_url = build_handoff_url(game.source, &game.external_id, &game.metadata);
    let cli_name = heroic_cli_name_for_source(game.source);
    let cli_available = cli_name
        .and_then(find_heroic_cli_binary)
        .is_some();

    let (default_install_dir, steam_libraries, handoff_note) = match game.source {
        LibrarySource::Steam => (
            None,
            steam_library_folders_with_space(),
            "Steam's native install dialog will open so you can choose your target Steam library folder and shortcuts."
                .to_string(),
        ),
        LibrarySource::HeroicEpic | LibrarySource::HeroicGog | LibrarySource::HeroicAmazon => (
            heroic_default_install_dir(),
            Vec::new(),
            "Opening in Heroic triggers Heroic's native install dialog; direct CLI installs in the background using Heroic's existing login."
                .to_string(),
        ),
        LibrarySource::EpicLauncher => (
            None,
            Vec::new(),
            "Epic Games Launcher will open its install dialog so you can choose the install folder and shortcuts."
                .to_string(),
        ),
        LibrarySource::Lutris => (
            None,
            Vec::new(),
            "Lutris will open its install wizard for this title.".to_string(),
        ),
    };

    let default_dir_free_bytes = default_install_dir
        .as_deref()
        .and_then(|p| free_space_for_path(Path::new(p), &disks));

    LauncherInstallOptions {
        source: game.source,
        external_id: game.external_id.clone(),
        title: game.title.clone(),
        default_install_dir,
        default_dir_free_bytes,
        steam_libraries,
        platforms: game.platforms.clone(),
        dlcs: game.metadata.dlcs.clone(),
        languages: game.metadata.languages.clone(),
        cli_available,
        cli_name: cli_name.map(str::to_string),
        handoff_url,
        handoff_note,
    }
}

/// Trigger a native launcher URI handoff (or Lutris CLI fallback).
pub fn open_launcher_handoff(
    app: &tauri::AppHandle,
    req: &LauncherInstallRequest,
) -> Result<LauncherInstallResult> {
    let url = build_handoff_url(req.source, &req.external_id, &req.metadata)
        .ok_or_else(|| anyhow!("Could not construct install URL for {}", req.title))?;

    let open_res = app.opener().open_url(&url, None::<&str>);
    if let Err(err) = open_res {
        #[cfg(target_os = "linux")]
        if req.source == LibrarySource::Lutris {
            std::process::Command::new("lutris")
                .arg(&url)
                .spawn()
                .map_err(|e| anyhow!("Failed to open Lutris ({err}); CLI fallback failed: {e}"))?;
            return Ok(LauncherInstallResult {
                job_id: None,
                method: InstallMethod::Handoff,
                handoff_url: Some(url),
                message: format!("Requested Lutris to install {}.", req.title),
            });
        }
        return Err(anyhow!(
            "Failed to hand off install to {}: {err}",
            req.source.label()
        ));
    }

    Ok(LauncherInstallResult {
        job_id: None,
        method: InstallMethod::Handoff,
        handoff_url: Some(url),
        message: format!("Requested {} to install {}.", req.source.label(), req.title),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeroicCliInvocation {
    pub args: Vec<String>,
    pub envs: Vec<(String, String)>,
}

/// Build Heroic CLI arguments and environment variables for `legendary`, `gogdl`, or `nile`.
pub fn build_heroic_cli_invocation(
    req: &LauncherInstallRequest,
    heroic_config_dir: &Path,
) -> Result<HeroicCliInvocation> {
    let install_dir = req
        .install_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Install directory is required for direct Heroic CLI install"))?;

    match req.source {
        LibrarySource::HeroicEpic => {
            let mut args = vec![
                "install".to_string(),
                req.external_id.clone(),
                "--yes".to_string(),
                "--base-path".to_string(),
                install_dir.to_string(),
            ];
            if let Some(plat) = req.platform.as_deref().filter(|p| !p.trim().is_empty()) {
                let mapped = match plat.to_ascii_lowercase().as_str() {
                    "windows" => "Windows",
                    "macos" | "mac" => "Mac",
                    _ => "Windows",
                };
                args.push("--platform".to_string());
                args.push(mapped.to_string());
            }
            if req.dlc_ids.is_empty() {
                args.push("--skip-dlcs".to_string());
            } else {
                args.push("--with-dlcs".to_string());
            }
            let leg_cfg = heroic_config_dir.join("legendaryConfig").join("legendary");
            Ok(HeroicCliInvocation {
                args,
                envs: vec![(
                    "LEGENDARY_CONFIG_PATH".to_string(),
                    leg_cfg.to_string_lossy().to_string(),
                )],
            })
        }
        LibrarySource::HeroicGog => {
            let auth_path = heroic_config_dir.join("gog_store").join("auth.json");
            let platform = req
                .platform
                .as_deref()
                .map(|p| p.to_ascii_lowercase())
                .unwrap_or_else(|| "windows".to_string());
            let mut args = vec![
                "--auth-config-path".to_string(),
                auth_path.to_string_lossy().to_string(),
                "download".to_string(),
                req.external_id.clone(),
                "--path".to_string(),
                install_dir.to_string(),
                "--platform".to_string(),
                platform,
            ];
            if let Some(lang) = req.language.as_deref().filter(|l| !l.trim().is_empty()) {
                args.push("--lang".to_string());
                args.push(lang.to_string());
            }
            if req.dlc_ids.is_empty() {
                args.push("--skip-dlcs".to_string());
            } else {
                args.push("--with-dlcs".to_string());
                for dlc in &req.dlc_ids {
                    args.push("--dlcs".to_string());
                    args.push(dlc.clone());
                }
            }
            Ok(HeroicCliInvocation {
                args,
                envs: Vec::new(),
            })
        }
        LibrarySource::HeroicAmazon => {
            let args = vec![
                "install".to_string(),
                req.external_id.clone(),
                "--base-path".to_string(),
                install_dir.to_string(),
            ];
            let nile_cfg = heroic_config_dir.join("nile_config").join("nile");
            Ok(HeroicCliInvocation {
                args,
                envs: vec![(
                    "NILE_CONFIG_PATH".to_string(),
                    nile_cfg.to_string_lossy().to_string(),
                )],
            })
        }
        _ => Err(anyhow!(
            "Direct CLI install is only supported for Heroic libraries"
        )),
    }
}

/// Parse progress percentage and optional speed (bytes/sec) from legendary / gogdl / nile output.
pub fn parse_cli_progress_line(line: &str) -> Option<(f32, Option<u64>)> {
    let pct_re = Regex::new(r"(\d{1,3}(?:\.\d+)?)\s*%").ok()?;
    let pct = pct_re
        .captures(line)?
        .get(1)?
        .as_str()
        .parse::<f32>()
        .ok()?
        .clamp(0.0, 100.0);

    let speed_re = Regex::new(r"(?i)(\d+(?:\.\d+)?)\s*(KiB|MiB|GiB|KB|MB|GB)/s").ok()?;
    let speed_bps = speed_re.captures(line).and_then(|cap| {
        let num = cap.get(1)?.as_str().parse::<f64>().ok()?;
        let unit = cap.get(2)?.as_str().to_ascii_lowercase();
        let mult = match unit.as_str() {
            "kib" => 1024.0,
            "mib" => 1024.0 * 1024.0,
            "gib" => 1024.0 * 1024.0 * 1024.0,
            "kb" => 1_000.0,
            "mb" => 1_000_000.0,
            "gb" => 1_000_000_000.0,
            _ => 1.0,
        };
        Some((num * mult) as u64)
    });

    Some((pct, speed_bps))
}

/// Spawn a background Heroic CLI install job and stream `launcher-install-progress` events.
pub fn start_heroic_cli_install(
    app: tauri::AppHandle,
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    req: LauncherInstallRequest,
) -> Result<LauncherInstallResult> {
    let cli_name = heroic_cli_name_for_source(req.source)
        .ok_or_else(|| anyhow!("No Heroic CLI for {}", req.source.label()))?;
    let cli_bin = find_heroic_cli_binary(cli_name).ok_or_else(|| {
        anyhow!(
            "Heroic CLI `{cli_name}` was not found. Use 'Open in Heroic Launcher' instead."
        )
    })?;
    let heroic_dir = heroic_config_dirs()
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("Heroic configuration directory not found"))?;
    let invocation = build_heroic_cli_invocation(&req, &heroic_dir)?;

    if let Some(dir) = req.install_dir.as_deref() {
        let _ = fs::create_dir_all(dir);
    }

    let job_id = uuid::Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut map) = jobs.lock() {
        map.insert(job_id.clone(), cancel.clone());
    }

    let job_id_clone = job_id.clone();
    let game_id = req.game_id.clone();
    let title = req.title.clone();
    let source = req.source;

    tauri::async_runtime::spawn(async move {
        let emit = |phase: &str,
                    percent: Option<f32>,
                    speed_bps: Option<u64>,
                    message: Option<String>,
                    error: Option<String>| {
            let _ = app.emit(
                LAUNCHER_INSTALL_EVENT,
                LauncherInstallProgress {
                    job_id: job_id_clone.clone(),
                    game_id: game_id.clone(),
                    title: title.clone(),
                    source,
                    phase: phase.to_string(),
                    percent,
                    speed_bps,
                    message,
                    error,
                },
            );
        };

        emit("starting", Some(0.0), None, Some(format!("Starting {cli_name}…")), None);

        let mut cmd = Command::new(&cli_bin);
        cmd.args(&invocation.args)
            .envs(invocation.envs.iter().map(|(k, v)| (k, v)))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                emit("failed", None, None, None, Some(format!("Failed to start {cli_name}: {e}")));
                if let Ok(mut map) = jobs.lock() {
                    map.remove(&job_id_clone);
                }
                return;
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(128);

        if let Some(out) = stdout {
            let tx_out = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(out).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if tx_out.send(line).await.is_err() {
                        break;
                    }
                }
            });
        }
        if let Some(err) = stderr {
            let tx_err = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if tx_err.send(line).await.is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let mut last_line = String::new();
        let mut was_cancelled = false;
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));

        loop {
            tokio::select! {
                maybe_line = rx.recv() => {
                    match maybe_line {
                        Some(line) => {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                last_line = trimmed.to_string();
                            }
                            if let Some((pct, speed)) = parse_cli_progress_line(&line) {
                                let phase = if line.to_ascii_lowercase().contains("verif") {
                                    "verifying"
                                } else {
                                    "downloading"
                                };
                                emit(phase, Some(pct), speed, Some(last_line.clone()), None);
                            }
                        }
                        None => break,
                    }
                }
                _ = tick.tick() => {
                    if cancel.load(Ordering::Relaxed) {
                        was_cancelled = true;
                        let _ = child.kill().await;
                        break;
                    }
                }
            }
        }

        let status = child.wait().await;
        if let Ok(mut map) = jobs.lock() {
            map.remove(&job_id_clone);
        }

        if was_cancelled || cancel.load(Ordering::Relaxed) {
            emit("cancelled", None, None, Some("Installation cancelled.".into()), None);
            return;
        }

        match status {
            Ok(s) if s.success() => {
                emit(
                    "completed",
                    Some(100.0),
                    None,
                    Some(format!("Finished installing {title}.")),
                    None,
                );
            }
            Ok(s) => {
                let msg = if last_line.is_empty() {
                    format!("{cli_name} exited with status {s}")
                } else {
                    format!("{cli_name} failed ({s}): {last_line}")
                };
                emit("failed", None, None, None, Some(msg));
            }
            Err(e) => {
                emit("failed", None, None, None, Some(format!("Process error: {e}")));
            }
        }
    });

    Ok(LauncherInstallResult {
        job_id: Some(job_id),
        method: InstallMethod::HeroicCli,
        handoff_url: None,
        message: format!("Started installing {} via {cli_name}.", req.title),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_handoff_urls_for_all_sources() {
        assert_eq!(
            build_handoff_url(LibrarySource::Steam, "489830", &LibraryGameMeta::default()).as_deref(),
            Some("steam://install/489830")
        );
        assert_eq!(
            build_handoff_url(
                LibrarySource::HeroicEpic,
                "Crab",
                &LibraryGameMeta {
                    heroic_runner: Some("legendary".into()),
                    ..Default::default()
                }
            )
            .as_deref(),
            Some("heroic://launch?appName=Crab&runner=legendary")
        );
        assert_eq!(
            build_handoff_url(
                LibrarySource::EpicLauncher,
                "ns1:item1:Crab",
                &LibraryGameMeta {
                    epic_namespace: Some("ns1".into()),
                    epic_catalog_item_id: Some("item1".into()),
                    epic_app_name: Some("Crab".into()),
                    ..Default::default()
                }
            )
            .as_deref(),
            Some("com.epicgames.launcher://apps/ns1%3Aitem1%3ACrab?action=install")
        );
        assert_eq!(
            build_handoff_url(
                LibrarySource::Lutris,
                "witcher-3",
                &LibraryGameMeta {
                    lutris_slug: Some("witcher-3".into()),
                    ..Default::default()
                }
            )
            .as_deref(),
            Some("lutris:install/witcher-3")
        );
    }

    #[test]
    fn builds_heroic_cli_args_for_legendary_and_gogdl() {
        let heroic_dir = PathBuf::from("/home/user/.config/heroic");
        let req = LauncherInstallRequest {
            game_id: "g1".into(),
            source: LibrarySource::HeroicEpic,
            external_id: "Crab".into(),
            title: "Control".into(),
            method: InstallMethod::HeroicCli,
            install_dir: Some("/games/heroic".into()),
            platform: Some("windows".into()),
            language: None,
            dlc_ids: vec!["CrabDLC1".into()],
            metadata: LibraryGameMeta::default(),
        };
        let inv = build_heroic_cli_invocation(&req, &heroic_dir).unwrap();
        assert!(inv.args.contains(&"install".to_string()));
        assert!(inv.args.contains(&"Crab".to_string()));
        assert!(inv.args.contains(&"--with-dlcs".to_string()));
        assert_eq!(inv.envs[0].0, "LEGENDARY_CONFIG_PATH");

        let gog_req = LauncherInstallRequest {
            source: LibrarySource::HeroicGog,
            external_id: "1207664643".into(),
            language: Some("en-US".into()),
            dlc_ids: Vec::new(),
            ..req
        };
        let gog_inv = build_heroic_cli_invocation(&gog_req, &heroic_dir).unwrap();
        assert!(gog_inv.args.contains(&"download".to_string()));
        assert!(gog_inv.args.contains(&"--skip-dlcs".to_string()));
        assert!(gog_inv.args.contains(&"en-US".to_string()));
    }

    #[test]
    fn parses_cli_progress_and_speed() {
        let (pct, speed) =
            parse_cli_progress_line("[DLManager] INFO: = Progress: 42.5% (123/290), 25.0 MiB/s")
                .unwrap();
        assert!((pct - 42.5).abs() < 0.01);
        assert_eq!(speed, Some((25.0 * 1024.0 * 1024.0) as u64));
    }
}
