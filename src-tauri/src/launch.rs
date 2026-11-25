//! Game launching, process lifecycle monitoring, and automatic deployment reversion.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, Signal, System, UpdateKind};
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

use crate::{
    config::{ManagedGame, Paths},
    games, mods,
    tools::{
        executables::{find_launch_executables, steam_app_id_from_install},
        heroic_launch::resolve_heroic_app,
    },
};

pub const LAUNCH_STATE_EVENT: &str = "game-launch-state";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(1500);
const RUNNING_POLL_INTERVAL: Duration = Duration::from_millis(2000);
const EXIT_DEBOUNCE_POLLS: u8 = 5;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    Modded,
    Unmodded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchPhase {
    Idle,
    Deploying,
    Starting,
    Running,
    Reverting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameLaunchState {
    pub game_id: String,
    pub mode: LaunchMode,
    pub phase: LaunchPhase,
    pub pids: Vec<u32>,
    pub started_at: u64,
    pub error: Option<String>,
}

impl GameLaunchState {
    pub fn idle(game_id: String) -> Self {
        Self {
            game_id,
            mode: LaunchMode::Modded,
            phase: LaunchPhase::Idle,
            pids: Vec::new(),
            started_at: 0,
            error: None,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.phase != LaunchPhase::Idle
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchResult {
    pub state: GameLaunchState,
    pub deploy_result: Option<mods::DeployResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    Steam {
        app_id: String,
    },
    Heroic {
        runner: String,
        app_name: String,
    },
    Executable {
        path: PathBuf,
    },
    Me3 {
        binary: PathBuf,
        windows_binaries_dir: Option<PathBuf>,
        game: String,
        profile: PathBuf,
        cwd: PathBuf,
        log_file: PathBuf,
    },
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Resolve the modded launch target, preferring Mod Engine 3 (`me3`) for modern FromSoft titles
/// when no custom launch executable is configured and `emperor.me3` + `me3` are available.
pub fn resolve_modded_launch_target(paths: &Paths, game: &ManagedGame) -> Result<LaunchTarget> {
    let has_custom = game
        .launch_executable
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if !has_custom {
        if let Some(me3_game) = games::me3_game_name(&game.plugin_id) {
            let game_root = games::fromsoft_resolve_game_root(Path::new(&game.install_path));
            let profile = game_root.join("emperor.me3");
            if profile.is_file() {
                if let Some(me3) = games::resolve_me3_installation(paths) {
                    return Ok(LaunchTarget::Me3 {
                        binary: me3.binary,
                        windows_binaries_dir: me3.windows_binaries_dir,
                        game: me3_game.to_string(),
                        profile,
                        cwd: game_root.clone(),
                        log_file: game_root.join("emperor_me3.log"),
                    });
                }
            }
        }
    }
    resolve_launch_target(game)
}

pub fn resolve_launch_target(game: &ManagedGame) -> Result<LaunchTarget> {
    if let Some(custom) = game
        .launch_executable
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let p = PathBuf::from(custom);
        let full = if p.is_absolute() {
            p
        } else {
            Path::new(&game.install_path).join(p)
        };
        if !full.is_file() {
            return Err(anyhow!(
                "Configured launch executable not found: {}",
                full.display()
            ));
        }
        return Ok(LaunchTarget::Executable { path: full });
    }

    if let Some(app_id) = steam_app_id_from_install(&game.install_path) {
        return Ok(LaunchTarget::Steam { app_id });
    }

    if game.launcher.to_ascii_lowercase().contains("heroic") {
        if let Some(heroic) = resolve_heroic_app(&game.install_path) {
            return Ok(LaunchTarget::Heroic {
                runner: heroic.runner,
                app_name: heroic.app_name,
            });
        }
    }

    let candidates = find_launch_executables(&game.install_path, 20);
    let first = candidates.into_iter().next().ok_or_else(|| {
        anyhow!(
            "Could not find a launchable executable in {}",
            game.install_path
        )
    })?;
    Ok(LaunchTarget::Executable {
        path: Path::new(&game.install_path).join(first),
    })
}

pub fn launch_target(
    app: &tauri::AppHandle,
    target: &LaunchTarget,
    env_vars: &HashMap<String, String>,
    wrapper: Option<&str>,
    extra_args: Option<&str>,
) -> Result<Option<u32>> {
    match target {
        LaunchTarget::Steam { app_id } => {
            let url = format!("steam://rungameid/{app_id}");
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| anyhow!("Failed to open Steam launch URL: {e}"))?;
            Ok(None)
        }
        LaunchTarget::Heroic { runner, app_name } => {
            let url = format!("heroic://launch?appName={app_name}&runner={runner}");
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| anyhow!("Failed to open Heroic launch URL: {e}"))?;
            Ok(None)
        }
        LaunchTarget::Executable { path } => {
            let cwd = path.parent().unwrap_or_else(|| Path::new("."));
            let mut argv: Vec<String> = Vec::new();
            let mut program = path.as_os_str().to_os_string();

            #[cfg(target_os = "linux")]
            let is_win_exe = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
            #[cfg(not(target_os = "linux"))]
            let is_win_exe = false;

            if let Some(w) = wrapper.map(str::trim).filter(|s| !s.is_empty()) {
                let mut w_tokens = w.split_whitespace();
                if let Some(first_w) = w_tokens.next() {
                    program = std::ffi::OsString::from(first_w);
                    for rest in w_tokens {
                        argv.push(rest.to_string());
                    }
                    if is_win_exe {
                        argv.push("wine".to_string());
                    }
                    argv.push(path.to_string_lossy().into_owned());
                } else if is_win_exe {
                    program = std::ffi::OsString::from("wine");
                    argv.push(path.to_string_lossy().into_owned());
                }
            } else if is_win_exe {
                program = std::ffi::OsString::from("wine");
                argv.push(path.to_string_lossy().into_owned());
            }

            if let Some(extra) = extra_args.map(str::trim).filter(|s| !s.is_empty()) {
                for tok in extra.split_whitespace() {
                    argv.push(tok.to_string());
                }
            }

            let child = Command::new(&program)
                .args(&argv)
                .current_dir(cwd)
                .envs(env_vars)
                .spawn()
                .with_context(|| format!("launching {}", path.display()))?;
            Ok(Some(child.id()))
        }
        LaunchTarget::Me3 {
            binary,
            windows_binaries_dir,
            game,
            profile,
            cwd,
            log_file,
        } => {
            let mut argv: Vec<String> = Vec::new();
            let program = if let Some(w) = wrapper.map(str::trim).filter(|s| !s.is_empty()) {
                let mut w_tokens = w.split_whitespace();
                if let Some(first_w) = w_tokens.next() {
                    for rest in w_tokens {
                        argv.push(rest.to_string());
                    }
                    argv.push(binary.to_string_lossy().into_owned());
                    std::ffi::OsString::from(first_w)
                } else {
                    binary.as_os_str().to_os_string()
                }
            } else {
                binary.as_os_str().to_os_string()
            };

            if let Some(win_dir) = windows_binaries_dir {
                argv.push("--windows-binaries-dir".to_string());
                argv.push(win_dir.to_string_lossy().into_owned());
            }
            argv.push("launch".to_string());
            argv.push("--game".to_string());
            argv.push(game.clone());
            argv.push("--profile".to_string());
            argv.push(profile.to_string_lossy().into_owned());

            if let Some(extra) = extra_args.map(str::trim).filter(|s| !s.is_empty()) {
                for tok in extra.split_whitespace() {
                    argv.push(tok.to_string());
                }
            }

            let mut cmd = Command::new(&program);
            cmd.args(&argv).current_dir(cwd).envs(env_vars);
            if let Ok(file) = std::fs::File::create(log_file) {
                if let Ok(err_file) = file.try_clone() {
                    cmd.stdout(std::process::Stdio::from(file))
                        .stderr(std::process::Stdio::from(err_file));
                }
            }
            let child = cmd
                .spawn()
                .with_context(|| format!("launching me3 ({})", binary.display()))?;
            Ok(Some(child.id()))
        }
    }
}

fn normalize_for_match(raw: &str) -> String {
    raw.replace('\\', "/").to_lowercase()
}

fn is_ignored_helper_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    let stem = lower.rsplit('/').next().unwrap_or(&lower);
    let stem = stem.rsplit('\\').next().unwrap_or(stem);
    matches!(
        stem,
        "wineserver"
            | "wine64-preloader"
            | "wine-preloader"
            | "services.exe"
            | "winedevice.exe"
            | "plugplay.exe"
            | "rpcss.exe"
            | "svchost.exe"
            | "explorer.exe"
            | "conhost.exe"
            | "reaper"
            | "srt-bwrap"
            | "pv-bwrap"
            | "pressure-vessel-wrap"
            | "proton"
    ) || stem.contains("crashreporter")
        || stem.contains("crashpad")
        || stem.contains("bugsplat")
}

/// Pure process-matching predicate for unit testing.
#[cfg(test)]
pub fn process_matches_game(
    pid: u32,
    exe: Option<&Path>,
    cmd: &[String],
    install_path: &str,
    steam_app_id: Option<&str>,
    direct_pid: Option<u32>,
    include_wrappers: bool,
) -> bool {
    process_matches_game_with_context(
        pid,
        exe,
        cmd,
        None,
        &[],
        install_path,
        steam_app_id,
        direct_pid,
        include_wrappers,
    )
}

fn known_game_exes(install_path: &str) -> Vec<String> {
    if install_path.trim().is_empty() {
        return Vec::new();
    }
    find_launch_executables(install_path, 20)
        .into_iter()
        .filter_map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
        })
        .collect()
}

pub fn process_matches_game_with_context(
    pid: u32,
    exe: Option<&Path>,
    cmd: &[String],
    cwd: Option<&Path>,
    known_exes: &[String],
    install_path: &str,
    steam_app_id: Option<&str>,
    direct_pid: Option<u32>,
    include_wrappers: bool,
) -> bool {
    if pid == std::process::id() {
        return false;
    }

    if include_wrappers && direct_pid == Some(pid) {
        return true;
    }

    if !include_wrappers {
        if let Some(exe_path) = exe {
            if exe_path
                .file_name()
                .is_some_and(|n| is_ignored_helper_name(&n.to_string_lossy()))
            {
                // Allow wine64-preloader / proton ONLY if cmd[0] is the actual game .exe
                let is_wine_loader = exe_path
                    .file_name()
                    .is_some_and(|n| {
                        let s = n.to_string_lossy().to_lowercase();
                        s.contains("preloader") || s == "proton"
                    });
                if !is_wine_loader {
                    return false;
                }
            }
        }
        if let Some(first) = cmd.first() {
            if is_ignored_helper_name(first) {
                let first_lower = first.to_lowercase();
                let first_stem = first_lower.rsplit('/').next().unwrap_or(&first_lower);
                let is_wine_loader =
                    first_stem.contains("preloader") || first_stem == "proton";
                if !is_wine_loader || cmd.len() <= 1 {
                    return false;
                }
            }
        }
    }

    let norm_install = normalize_for_match(install_path.trim_end_matches(['/', '\\']));
    if !norm_install.is_empty() {
        if let Some(exe_path) = exe {
            let norm_exe = normalize_for_match(&exe_path.to_string_lossy());
            if norm_exe.starts_with(&format!("{norm_install}/")) || norm_exe == norm_install {
                let is_helper = exe_path
                    .file_name()
                    .is_some_and(|n| is_ignored_helper_name(&n.to_string_lossy()));
                if include_wrappers || !is_helper {
                    return true;
                }
            }
        }
        for arg in cmd {
            let norm_arg = normalize_for_match(arg);
            if norm_arg.contains(&norm_install) {
                if !include_wrappers && is_ignored_helper_name(&norm_arg) {
                    continue;
                }
                return true;
            }
        }
        // Match Wine/Proton processes whose working directory is inside the game install path
        // (e.g. `Cyberpunk2077.exe` spawned with a relative or `C:\...` command line).
        if let Some(cwd_path) = cwd {
            let norm_cwd = normalize_for_match(&cwd_path.to_string_lossy());
            if norm_cwd == norm_install || norm_cwd.starts_with(&format!("{norm_install}/")) {
                return true;
            }
        }
    }

    // Match Wine/Proton processes whose cmd args reference a known executable in install_path
    // (handles Wine's space-padded argv[0], embedded flags, or `.exe` in cmd[1]).
    if !known_exes.is_empty() {
        for arg in cmd {
            let norm_arg = normalize_for_match(arg);
            for known in known_exes {
                if let Some(pos) = norm_arg.find(known.as_str()) {
                    let before_ok = pos == 0
                        || matches!(
                            norm_arg.as_bytes()[pos - 1],
                            b'/' | b' ' | b'\t' | b'"' | b'\''
                        );
                    let after_pos = pos + known.len();
                    let after_ok = after_pos == norm_arg.len()
                        || matches!(
                            norm_arg.as_bytes()[after_pos],
                            b' ' | b'\t' | b'"' | b'\'' | b'\0'
                        );
                    if before_ok && after_ok && !is_ignored_helper_name(known) {
                        return true;
                    }
                }
            }
        }
    }

    if include_wrappers {
        if let Some(app_id) = steam_app_id {
            let needle = format!("appid={app_id}").to_lowercase();
            for arg in cmd {
                if arg.to_lowercase().contains(&needle) {
                    return true;
                }
            }
        }
    }

    false
}

fn refresh_processes_for_launch(sys: &mut System, include_environ: bool) {
    let mut kind = ProcessRefreshKind::nothing()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::Always)
        .with_cwd(UpdateKind::OnlyIfNotSet);
    if include_environ {
        kind = kind.with_environ(UpdateKind::OnlyIfNotSet);
    }
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
}

fn collect_matching_pids(
    sys: &System,
    install_path: &str,
    steam_app_id: Option<&str>,
    direct_pid: Option<u32>,
    include_wrappers: bool,
) -> Vec<u32> {
    let known_exes = known_game_exes(install_path);
    let mut pids = Vec::new();
    for (pid, proc_) in sys.processes() {
        if matches!(
            proc_.status(),
            ProcessStatus::Zombie | ProcessStatus::Dead
        ) {
            continue;
        }
        let pid_u32 = pid.as_u32();
        let cmd: Vec<String> = proc_
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        if process_matches_game_with_context(
            pid_u32,
            proc_.exe(),
            &cmd,
            proc_.cwd(),
            &known_exes,
            install_path,
            steam_app_id,
            direct_pid,
            include_wrappers,
        ) {
            pids.push(pid_u32);
        }
    }
    pids.sort_unstable();
    pids
}

pub fn find_matching_pids(
    sys: &mut System,
    install_path: &str,
    steam_app_id: Option<&str>,
    direct_pid: Option<u32>,
    include_wrappers: bool,
) -> Vec<u32> {
    refresh_processes_for_launch(sys, false);
    collect_matching_pids(sys, install_path, steam_app_id, direct_pid, include_wrappers)
}

pub async fn close_running_game(
    install_path: &str,
    steam_app_id: Option<&str>,
    tracked_pids: &[u32],
    heroic_wine_prefix: Option<&str>,
) -> Result<()> {
    let mut sys = System::new();
    refresh_processes_for_launch(&mut sys, true);
    let mut pids = collect_matching_pids(&sys, install_path, steam_app_id, None, true);
    for &p in tracked_pids {
        if p != std::process::id() && !pids.contains(&p) {
            pids.push(p);
        }
    }

    // Collect WINEPREFIX / STEAM_COMPAT_DATA_PATH from Heroic config and matched processes
    // so reparented Wine/Proton processes (Cyberpunk2077.exe, wineserver, services.exe) are also terminated.
    let mut wine_prefixes: Vec<String> = Vec::new();
    if let Some(prefix) = heroic_wine_prefix.map(str::trim).filter(|s| !s.is_empty()) {
        wine_prefixes.push(prefix.to_string());
    }
    for &pid_u32 in &pids {
        if let Some(proc_) = sys.process(Pid::from_u32(pid_u32)) {
            for env_entry in proc_.environ() {
                let s = env_entry.to_string_lossy();
                if let Some(val) = s
                    .strip_prefix("WINEPREFIX=")
                    .or_else(|| s.strip_prefix("STEAM_COMPAT_DATA_PATH="))
                {
                    let trimmed = val.trim();
                    if !trimmed.is_empty() && !wine_prefixes.iter().any(|p| p == trimmed) {
                        wine_prefixes.push(trimmed.to_string());
                    }
                }
            }
        }
    }

    // Expand to multi-level descendants (up to 8 hops) and processes sharing the same Wine prefix.
    for _ in 0..8 {
        let mut added = false;
        for (pid, proc_) in sys.processes() {
            let pid_u32 = pid.as_u32();
            if pid_u32 == std::process::id() || pids.contains(&pid_u32) {
                continue;
            }
            let is_descendant = proc_
                .parent()
                .is_some_and(|parent| pids.contains(&parent.as_u32()));
            let shares_prefix = !wine_prefixes.is_empty()
                && proc_.environ().iter().any(|env_entry| {
                    let s = env_entry.to_string_lossy();
                    s.strip_prefix("WINEPREFIX=")
                        .or_else(|| s.strip_prefix("STEAM_COMPAT_DATA_PATH="))
                        .is_some_and(|val| wine_prefixes.iter().any(|p| p == val.trim()))
                });
            if is_descendant || shares_prefix {
                pids.push(pid_u32);
                added = true;
            }
        }
        if !added {
            break;
        }
    }

    for &pid_u32 in &pids {
        if let Some(proc_) = sys.process(Pid::from_u32(pid_u32)) {
            if proc_.kill_with(Signal::Term).is_none() {
                let _ = proc_.kill();
            }
        }
    }
    if pids.is_empty() {
        return Ok(());
    }

    // Wait up to 1.5s for graceful exit before escalating to SIGKILL on any surviving PIDs.
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let any_alive = pids.iter().any(|&p| {
            sys.process(Pid::from_u32(p))
                .is_some_and(|proc_| !matches!(proc_.status(), ProcessStatus::Zombie | ProcessStatus::Dead))
        });
        if !any_alive {
            return Ok(());
        }
    }

    let extra = find_matching_pids(&mut sys, install_path, steam_app_id, None, true);
    for p in extra {
        if !pids.contains(&p) {
            pids.push(p);
        }
    }
    for &pid_u32 in &pids {
        if let Some(proc_) = sys.process(Pid::from_u32(pid_u32)) {
            if !matches!(proc_.status(), ProcessStatus::Zombie | ProcessStatus::Dead) {
                let _ = proc_.kill();
            }
        }
    }
    Ok(())
}

pub fn revert_game_deployment(
    paths: &Paths,
    game_id: &str,
    install_path: &str,
    plugin_id: &str,
) -> Result<()> {
    let install = Path::new(install_path);
    mods::purge_deploy(paths, game_id, Some(install))?;
    if let Some(plugin) = games::plugin_by_id(plugin_id) {
        mods::prune_empty_loader_dirs(plugin, install);
        plugin.on_revert(install)?;
    }
    #[cfg(target_os = "linux")]
    {
        let backup = paths.steam_launch_backup_file(game_id);
        let _ = crate::tools::steam_launch::restore_steam_launch_options(&backup);
    }
    Ok(())
}

pub fn set_and_emit_state(
    app: &tauri::AppHandle,
    launch_states: &Arc<Mutex<HashMap<String, GameLaunchState>>>,
    state: GameLaunchState,
) {
    if let Ok(mut map) = launch_states.lock() {
        if state.phase == LaunchPhase::Idle && state.error.is_none() {
            map.remove(&state.game_id);
        } else {
            map.insert(state.game_id.clone(), state.clone());
        }
    }
    let _ = app.emit(LAUNCH_STATE_EVENT, &state);
}

pub fn spawn_game_watcher(
    app: tauri::AppHandle,
    paths: Paths,
    launch_states: Arc<Mutex<HashMap<String, GameLaunchState>>>,
    close_flag: Arc<AtomicBool>,
    game: ManagedGame,
    mode: LaunchMode,
    steam_app_id: Option<String>,
    direct_pid: Option<u32>,
    already_running: bool,
) {
    tauri::async_runtime::spawn(async move {
        let mut sys = System::new();
        let started_at = now_unix_secs();

        if !already_running {
            let start_deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
            let mut found_pids = Vec::new();
            while tokio::time::Instant::now() < start_deadline {
                if close_flag.load(Ordering::Relaxed) {
                    break;
                }
                found_pids = find_matching_pids(
                    &mut sys,
                    &game.install_path,
                    steam_app_id.as_deref(),
                    direct_pid,
                    false,
                );
                if !found_pids.is_empty() {
                    break;
                }
                tokio::time::sleep(STARTUP_POLL_INTERVAL).await;
            }

            if close_flag.load(Ordering::Relaxed) {
                if mode == LaunchMode::Modded {
                    let _ = revert_game_deployment(
                        &paths,
                        &game.id,
                        &game.install_path,
                        &game.plugin_id,
                    );
                }
                set_and_emit_state(&app, &launch_states, GameLaunchState::idle(game.id));
                return;
            }

            if found_pids.is_empty() {
                set_and_emit_state(
                    &app,
                    &launch_states,
                    GameLaunchState {
                        game_id: game.id.clone(),
                        mode,
                        phase: LaunchPhase::Idle,
                        pids: Vec::new(),
                        started_at,
                        error: Some(
                            "Game process was not detected within 90s. Use Revert deployment if needed."
                                .to_string(),
                        ),
                    },
                );
                return;
            }

            set_and_emit_state(
                &app,
                &launch_states,
                GameLaunchState {
                    game_id: game.id.clone(),
                    mode,
                    phase: LaunchPhase::Running,
                    pids: found_pids,
                    started_at,
                    error: None,
                },
            );
        }

        // Running phase: wait until no matching process is observed for EXIT_DEBOUNCE_POLLS
        // (or immediately if user clicked Close game).
        let mut missing_streak: u8 = 0;
        loop {
            tokio::time::sleep(RUNNING_POLL_INTERVAL).await;
            let pids = find_matching_pids(
                &mut sys,
                &game.install_path,
                steam_app_id.as_deref(),
                direct_pid,
                false,
            );
            if pids.is_empty() {
                missing_streak += 1;
                if close_flag.load(Ordering::Relaxed) || missing_streak >= EXIT_DEBOUNCE_POLLS {
                    break;
                }
            } else {
                missing_streak = 0;
                set_and_emit_state(
                    &app,
                    &launch_states,
                    GameLaunchState {
                        game_id: game.id.clone(),
                        mode,
                        phase: LaunchPhase::Running,
                        pids,
                        started_at,
                        error: None,
                    },
                );
            }
        }

        if mode == LaunchMode::Modded {
            set_and_emit_state(
                &app,
                &launch_states,
                GameLaunchState {
                    game_id: game.id.clone(),
                    mode,
                    phase: LaunchPhase::Reverting,
                    pids: Vec::new(),
                    started_at,
                    error: None,
                },
            );

            let err = revert_game_deployment(
                &paths,
                &game.id,
                &game.install_path,
                &game.plugin_id,
            )
            .err()
            .map(|e| format!("Failed to revert deployment: {e:#}"));

            set_and_emit_state(
                &app,
                &launch_states,
                GameLaunchState {
                    game_id: game.id,
                    mode,
                    phase: LaunchPhase::Idle,
                    pids: Vec::new(),
                    started_at: 0,
                    error: err,
                },
            );
        } else {
            set_and_emit_state(&app, &launch_states, GameLaunchState::idle(game.id));
        }
    });
}

/// On app startup, revert any leftover active deployments whose game is no longer running,
/// or attach a watcher if the game is still running.
pub fn reconcile_orphaned_deployments(
    app: &tauri::AppHandle,
    paths: &Paths,
    games: &[ManagedGame],
    launch_states: &Arc<Mutex<HashMap<String, GameLaunchState>>>,
) {
    let mut sys = System::new();
    for game in games {
        if !mods::has_active_deployment(paths, &game.id) {
            continue;
        }
        let steam_app_id = steam_app_id_from_install(&game.install_path);
        let pids = find_matching_pids(
            &mut sys,
            &game.install_path,
            steam_app_id.as_deref(),
            None,
            false,
        );
        if pids.is_empty() {
            if let Err(e) =
                revert_game_deployment(paths, &game.id, &game.install_path, &game.plugin_id)
            {
                log::warn!(
                    "Startup deployment revert failed for {}: {e:#}",
                    game.title
                );
            } else {
                log::info!("Reverted orphaned deployment for {}", game.title);
            }
        } else {
            set_and_emit_state(
                app,
                launch_states,
                GameLaunchState {
                    game_id: game.id.clone(),
                    mode: LaunchMode::Modded,
                    phase: LaunchPhase::Running,
                    pids,
                    started_at: now_unix_secs(),
                    error: None,
                },
            );
            spawn_game_watcher(
                app.clone(),
                paths.clone(),
                launch_states.clone(),
                Arc::new(AtomicBool::new(false)),
                game.clone(),
                LaunchMode::Modded,
                steam_app_id,
                None,
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn matches_direct_pid() {
        assert!(process_matches_game(
            4242,
            None,
            &[],
            "/games/Skyrim",
            None,
            Some(4242),
            true,
        ));
    }

    #[test]
    fn matches_native_exe_inside_install_path() {
        assert!(process_matches_game(
            100,
            Some(Path::new("/games/Valheim/valheim.x86_64")),
            &[],
            "/games/Valheim",
            None,
            None,
            false,
        ));
        assert!(!process_matches_game(
            101,
            Some(Path::new("/games/ValheimOther/valheim.x86_64")),
            &[],
            "/games/Valheim",
            None,
            None,
            false,
        ));
    }

    #[test]
    fn matches_proton_wine_cmdline_with_backslashes() {
        let cmd = vec![
            "Z:\\home\\user\\.steam\\steamapps\\common\\Risk of Rain 2\\Risk of Rain 2.exe"
                .to_string(),
        ];
        assert!(process_matches_game(
            200,
            Some(Path::new("/usr/bin/wine64-preloader")),
            &cmd,
            "/home/user/.steam/steamapps/common/Risk of Rain 2",
            Some("632360"),
            None,
            false,
        ));
    }

    #[test]
    fn matches_steam_reaper_appid_only_with_wrappers() {
        let cmd = vec![
            "/home/user/.steam/ubuntu12_32/reaper".to_string(),
            "SteamLaunch".to_string(),
            "AppId=632360".to_string(),
        ];
        assert!(process_matches_game(
            300,
            Some(Path::new("/home/user/.steam/ubuntu12_32/reaper")),
            &cmd,
            "/other/path",
            Some("632360"),
            None,
            true,
        ));
        assert!(!process_matches_game(
            300,
            Some(Path::new("/home/user/.steam/ubuntu12_32/reaper")),
            &cmd,
            "/other/path",
            Some("632360"),
            None,
            false,
        ));
    }

    #[test]
    fn ignores_wine_helpers_and_crash_reporters_when_running() {
        assert!(!process_matches_game(
            400,
            Some(Path::new("/usr/bin/wineserver")),
            &["/games/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe".to_string()],
            "/games/Cyberpunk 2077",
            Some("1091500"),
            None,
            false,
        ));
        assert!(!process_matches_game(
            401,
            Some(Path::new("/games/Cyberpunk 2077/bin/x64/CrashReporter.exe")),
            &[],
            "/games/Cyberpunk 2077",
            Some("1091500"),
            None,
            false,
        ));
        assert!(process_matches_game(
            402,
            Some(Path::new("/usr/bin/wine64-preloader")),
            &["Z:\\games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe".to_string()],
            "/games/Cyberpunk 2077",
            Some("1091500"),
            None,
            false,
        ));
    }

    #[test]
    fn revert_clears_deployed_files_and_manifest() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let config_dir = tmp.path().join("config");
        let cache_dir = tmp.path().join("cache");
        let install_dir = tmp.path().join("game");
        fs::create_dir_all(&install_dir).unwrap();

        let paths = Paths {
            config_dir,
            data_dir,
            cache_dir,
        };
        let game_id = "test-game";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();

        let deployed_file = install_dir.join("mod_test.txt");
        fs::write(&deployed_file, "hello").unwrap();

        let manifest = mods::DeployManifest {
            paths: vec![deployed_file.to_string_lossy().into_owned()],
        };
        fs::write(
            paths.deploy_manifest(game_id),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        assert!(mods::has_active_deployment(&paths, game_id));
        revert_game_deployment(
            &paths,
            game_id,
            &install_dir.to_string_lossy(),
            "valheim",
        )
        .unwrap();
        assert!(!deployed_file.exists());
        assert!(!mods::has_active_deployment(&paths, game_id));
    }

    #[test]
    fn matches_process_by_cwd_and_known_exe_filename() {
        // Wine/Proton process launched with relative cmdline but cwd inside install_path
        assert!(process_matches_game_with_context(
            500,
            Some(Path::new("/usr/bin/wine64-preloader")),
            &["bin\\x64\\Cyberpunk2077.exe".to_string()],
            Some(Path::new("/games/Cyberpunk 2077/bin/x64")),
            &[],
            "/games/Cyberpunk 2077",
            None,
            None,
            false,
        ));
        // Wine/Proton process launched with C:\... cmdline matching known_exes
        assert!(process_matches_game_with_context(
            501,
            Some(Path::new("/usr/bin/wine64-preloader")),
            &["C:\\Games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe".to_string()],
            None,
            &["cyberpunk2077.exe".to_string()],
            "/games/Cyberpunk 2077",
            None,
            None,
            false,
        ));
        // Wine space-padded argv[0] and .exe passed in cmd[1]
        assert!(process_matches_game_with_context(
            502,
            Some(Path::new("/usr/bin/wine64-preloader")),
            &["C:\\Games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe   --launcher-skip".to_string()],
            None,
            &["cyberpunk2077.exe".to_string()],
            "/games/Cyberpunk 2077",
            None,
            None,
            false,
        ));
        assert!(process_matches_game_with_context(
            503,
            Some(Path::new("/usr/bin/wine64-preloader")),
            &[
                "/usr/bin/wine64-preloader".to_string(),
                "Cyberpunk2077.exe".to_string(),
            ],
            None,
            &["cyberpunk2077.exe".to_string()],
            "/games/Cyberpunk 2077",
            None,
            None,
            false,
        ));
    }

    #[test]
    fn resolve_modded_launch_target_uses_me3_for_elden_ring() {
        let tmp = tempdir().unwrap();
        let paths = Paths {
            config_dir: tmp.path().join("cfg"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        let bin_dir = paths.me3_install_dir().join("bin");
        let win_dir = bin_dir.join("win64");
        fs::create_dir_all(&win_dir).unwrap();
        fs::write(bin_dir.join("me3"), b"x").unwrap();
        fs::write(win_dir.join("me3_mod_host.dll"), b"dll").unwrap();

        let install = tmp.path().join("ELDEN RING");
        let game_dir = install.join("Game");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(game_dir.join("eldenring.exe"), b"x").unwrap();
        fs::write(game_dir.join("emperor.me3"), b"profileVersion = \"v1\"\n").unwrap();

        let mut game = ManagedGame {
            id: "er-1".into(),
            title: "ELDEN RING".into(),
            nexus_domain: "eldenring".into(),
            install_path: install.to_string_lossy().into_owned(),
            launcher: "Steam".into(),
            plugin_id: "eldenring".into(),
            cover_path: None,
            cover_url: None,
            project_name: None,
            thunderstore_community: None,
            modio_game_id: None,
            tool_overrides: None,
            game_version: None,
            ignored_requirements: Vec::new(),
            launch_executable: None,
            launch_command_source: crate::config::LaunchCommandSource::Emperor,
            custom_env_vars: Vec::new(),
            custom_wrapper: None,
            custom_launch_args: None,
            custom_raw_command: None,
            use_global_launch_options: true,
            wine_overrides_dismissed: false,
        };

        let target = resolve_modded_launch_target(&paths, &game).unwrap();
        assert_eq!(
            target,
            LaunchTarget::Me3 {
                binary: bin_dir.join("me3"),
                windows_binaries_dir: Some(win_dir),
                game: "eldenring".into(),
                profile: game_dir.join("emperor.me3"),
                cwd: game_dir.clone(),
                log_file: game_dir.join("emperor_me3.log"),
            }
        );

        // Explicit custom launch_executable still overrides me3.
        game.launch_executable = Some("Game/eldenring.exe".into());
        let custom = resolve_modded_launch_target(&paths, &game).unwrap();
        assert_eq!(
            custom,
            LaunchTarget::Executable {
                path: game_dir.join("eldenring.exe"),
            }
        );
    }

    #[test]
    fn refresh_processes_for_launch_populates_cmd_cwd_and_environ() {
        let mut sys = System::new();
        refresh_processes_for_launch(&mut sys, true);
        let me = sys
            .process(Pid::from_u32(std::process::id()))
            .expect("current test process should be visible in sysinfo");
        assert!(!me.cmd().is_empty(), "cmd() must be populated by refresh_processes_for_launch");
        assert!(me.cwd().is_some(), "cwd() must be populated by refresh_processes_for_launch");
        assert!(
            !me.environ().is_empty(),
            "environ() must be populated when include_environ is true"
        );
    }
}
