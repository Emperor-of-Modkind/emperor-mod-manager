//! Optional PZ3D & ZombieBuddy installer managed by the Project Zomboid plugin.
//!
//! Follows the official `pz3d.dev/install/guide.html` workflow:
//! - Downloads ZombieBuddy 2.3.4+ (`ZombieBuddy.jar`, plus `zbNative.dll` on Windows/Proton)
//!   from `zed-0xff/ZombieBuddy` GitHub Releases (skipping `windows_installer*` tags).
//! - Places the `ZombieBuddy` mod folder into `Zomboid/mods/ZombieBuddy/` and copies the
//!   loader binary(ies) beside `ProjectZomboid64.json`.
//! - Patches `ProjectZomboid64.json` `vmArgs` with `-agentlib:zbNative` (Windows/Proton)
//!   or `-javaagent:ZombieBuddy.jar` (native Linux), removing legacy `pz3dLoader` args and
//!   optionally bumping a standard 3 GiB `-Xmx` heap limit to 6 GiB (`-Xmx6144m`) for PZ3D.
//! - Downloads the latest PZ3D standalone archive from `https://pz3d.dev/install/releases.json`,
//!   extracts `pz3d-standalone/Contents/mods/*` into `Zomboid/mods/`, and enables `ZombieBuddy`
//!   and `pz3d` in `Zomboid/mods/default.txt`.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::projectzomboid::{
    is_windows_build, mods_dir, update_default_list_entries, zomboid_user_dir,
};
use super::{PluginToolActionResult, PluginToolStatus};
use crate::config::Paths;
use crate::github_release::{self, GhRelease};
use crate::mods;

const TOOL_PZ3D: &str = "pz3d";
const TOOL_ZOMBIEBUDDY: &str = "zombiebuddy";

const ZOMBIEBUDDY_REPO: &str = "zed-0xff/ZombieBuddy";
const ZOMBIEBUDDY_HOMEPAGE: &str = "https://github.com/zed-0xff/ZombieBuddy";
const ZOMBIEBUDDY_MIN_VERSION: (u32, u32, u32) = (2, 3, 4);

const PZ3D_BASE_URL: &str = "https://pz3d.dev/install/";
const PZ3D_RELEASES_URL: &str = "https://pz3d.dev/install/releases.json";
const PZ3D_HOMEPAGE: &str = "https://pz3d.dev/install/guide.html";

const STATE_FILE: &str = ".emperor-pz3d.json";
const JSON_BACKUP_SUFFIX: &str = ".emperor-pz3d.bak";

const FALLBACK_ZOMBIEBUDDY_MOD_INFO: &str = "\
name=ZombieBuddy
id=ZombieBuddy
author=Zed
url=https://github.com/zed-0xff/ZombieBuddy
description=Java bytecode manipulation framework for Project Zomboid.

javaJarFile=../libs/ZombieBuddy.jar
javaPkgName=me.zed_0xff.zombie_buddy
";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Pz3dState {
    #[serde(default)]
    zombiebuddy_version: Option<String>,
    #[serde(default)]
    pz3d_version: Option<String>,
    #[serde(default)]
    pz3d_mod_dirs: Vec<String>,
    #[serde(default)]
    copied_game_files: Vec<String>,
    #[serde(default)]
    patched_vm_arg: Option<String>,
    #[serde(default)]
    previous_xmx: Option<String>,
    #[serde(default)]
    previous_bat_xmx: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Pz3dReleaseEntry {
    version: String,
    #[serde(default)]
    download: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedZombieBuddyAssets {
    version: String,
    jar_url: String,
    native_dll_url: Option<String>,
}

pub(super) fn tools_status(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
) -> Vec<PluginToolStatus> {
    build_tools_status(paths, game_id, install_path, None, None)
}

pub(super) async fn check_tool_updates(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
) -> Result<Vec<PluginToolStatus>> {
    let (pz3d_res, zb_res) =
        tokio::join!(fetch_latest_pz3d_release(), fetch_zombiebuddy_assets(false));
    let latest_pz3d = pz3d_res.ok().map(|r| r.version);
    let latest_zb = zb_res.ok().map(|a| a.version);
    if latest_pz3d.is_none() && latest_zb.is_none() {
        anyhow::bail!("Could not reach pz3d.dev or GitHub to check for updates");
    }
    Ok(build_tools_status(
        paths,
        game_id,
        install_path,
        latest_pz3d.as_deref(),
        latest_zb.as_deref(),
    ))
}

pub(super) async fn install_tool(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
    tool_id: &str,
) -> Result<PluginToolActionResult> {
    match tool_id {
        TOOL_ZOMBIEBUDDY => install_zombiebuddy(paths, game_id, install_path).await,
        TOOL_PZ3D => install_pz3d(paths, game_id, install_path).await,
        other => Err(anyhow!("unknown Project Zomboid tool: {other}")),
    }
}

pub(super) async fn uninstall_tool(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
    tool_id: &str,
) -> Result<PluginToolActionResult> {
    match tool_id {
        TOOL_ZOMBIEBUDDY => uninstall_zombiebuddy(paths, game_id, install_path),
        TOOL_PZ3D => uninstall_pz3d(paths, game_id, install_path),
        other => Err(anyhow!("unknown Project Zomboid tool: {other}")),
    }
}

pub(super) fn adopt_legacy_installs(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
) -> Result<bool> {
    let mods = mods_dir(install_path);
    adopt_legacy_installs_in(paths, game_id, &mods)
}

fn adopt_legacy_installs_in(paths: &Paths, game_id: &str, mods_root: &Path) -> Result<bool> {
    let state = load_state(mods_root);
    if state.zombiebuddy_version.is_none() && state.pz3d_version.is_none() {
        return Ok(false);
    }
    let has_deploy = mods::has_active_deployment(paths, game_id);
    let mut adopted = false;

    if state.zombiebuddy_version.is_some()
        && mods::find_plugin_tool_mod(paths, game_id, TOOL_ZOMBIEBUDDY).is_none()
    {
        let zb_dir = mods_root.join("ZombieBuddy");
        if zb_dir.join("42").join("mod.info").is_file() {
            let tmp = StageTempDir::new(paths, "adopt-zb")?;
            let dst = tmp.path().join("mods").join("ZombieBuddy");
            copy_dir_recursive(&zb_dir, &dst)?;
            mods::stage_plugin_tool_mod(
                paths,
                game_id,
                TOOL_ZOMBIEBUDDY,
                "ZombieBuddy",
                "projectzomboid",
                state.zombiebuddy_version.clone(),
                tmp.path(),
                &[],
            )?;
            if !has_deploy {
                let _ = std::fs::remove_dir_all(&zb_dir);
                let _ = update_default_list_entries(mods_root, &[], &["ZombieBuddy"]);
            }
            adopted = true;
        }
    }

    if state.pz3d_version.is_some()
        && mods::find_plugin_tool_mod(paths, game_id, TOOL_PZ3D).is_none()
    {
        let dirs: Vec<String> = if state.pz3d_mod_dirs.is_empty() {
            vec!["pz3d".into(), "pz3d_chainlink".into()]
        } else {
            state.pz3d_mod_dirs.clone()
        };
        if mods_root.join("pz3d").is_dir() {
            let tmp = StageTempDir::new(paths, "adopt-pz3d")?;
            let mut copied_any = false;
            for dir_name in &dirs {
                let src = mods_root.join(dir_name);
                if src.is_dir() {
                    let dst = tmp.path().join("mods").join(dir_name);
                    copy_dir_recursive(&src, &dst)?;
                    copied_any = true;
                }
            }
            if copied_any {
                mods::stage_plugin_tool_mod(
                    paths,
                    game_id,
                    TOOL_PZ3D,
                    "PZ3D",
                    "projectzomboid",
                    state.pz3d_version.clone(),
                    tmp.path(),
                    &[mods::plugin_tool_mod_id(TOOL_ZOMBIEBUDDY)],
                )?;
                if !has_deploy {
                    for dir_name in &dirs {
                        let target = mods_root.join(dir_name);
                        if target.exists() {
                            let _ = std::fs::remove_dir_all(&target);
                        }
                    }
                    let _ = update_default_list_entries(
                        mods_root,
                        &[],
                        &["pz3d", "pz3d_chainlink"],
                    );
                }
                adopted = true;
            }
        }
    }

    Ok(adopted)
}

struct StageTempDir {
    path: PathBuf,
}

impl StageTempDir {
    fn new(paths: &Paths, label: &str) -> Result<Self> {
        let root = paths.cache_dir.join("pz3d-stage-tmp");
        std::fs::create_dir_all(&root)
            .with_context(|| format!("create {}", root.display()))?;
        let unique = format!(
            "{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = root.join(unique);
        std::fs::create_dir_all(&path)
            .with_context(|| format!("create {}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StageTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
    for entry in std::fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Automatically heal ZombieBuddy / PZ3D loader binaries and `ProjectZomboid64.json` /
/// `ProjectZomboid64.bat` patches during deploy if a Steam update or file verification
/// overwrote them.
pub(super) fn repair_after_deploy(
    install_path: &Path,
    enabled_mod_folders: &[String],
) -> Result<Vec<String>> {
    let mods = mods_dir(install_path);
    repair_after_deploy_in(install_path, &mods, enabled_mod_folders)
}

fn repair_after_deploy_in(
    install_path: &Path,
    mods: &Path,
    enabled_mod_folders: &[String],
) -> Result<Vec<String>> {
    let state = load_state(mods);
    if state.zombiebuddy_version.is_none() && state.pz3d_version.is_none() {
        return Ok(Vec::new());
    }

    let zb_enabled = enabled_mod_folders
        .iter()
        .any(|id| id.eq_ignore_ascii_case("ZombieBuddy"));
    let pz3d_enabled = enabled_mod_folders
        .iter()
        .any(|id| id.eq_ignore_ascii_case("pz3d"));
    if !zb_enabled && !pz3d_enabled {
        return Ok(Vec::new());
    }

    let windows = is_windows_build(install_path);
    let Some(json_path) = find_game_json(install_path, windows) else {
        return Ok(vec![
            "PZ3D / ZombieBuddy is installed, but ProjectZomboid64.json could not be found to verify loader settings."
                .into(),
        ]);
    };
    let Some(jvm_dir) = json_path.parent() else {
        return Ok(Vec::new());
    };

    let mut repaired = Vec::new();
    let zb_libs = mods.join("ZombieBuddy").join("libs");
    if state.zombiebuddy_version.is_some() && zb_enabled {
        let jar_src = zb_libs.join("ZombieBuddy.jar");
        let jar_dst = jvm_dir.join("ZombieBuddy.jar");
        if !jar_dst.is_file() && jar_src.is_file() {
            std::fs::copy(&jar_src, &jar_dst)
                .with_context(|| format!("restore {}", jar_dst.display()))?;
            repaired.push("ZombieBuddy.jar");
        }
        if windows {
            let dll_src = zb_libs.join("zbNative.dll");
            let dll_dst = jvm_dir.join("zbNative.dll");
            if !dll_dst.is_file() && dll_src.is_file() {
                std::fs::copy(&dll_src, &dll_dst)
                    .with_context(|| format!("restore {}", dll_dst.display()))?;
                repaired.push("zbNative.dll");
            }
        }

        let need_6gb = state.pz3d_version.is_some() && pz3d_enabled;
        let json_missing_agent = !json_has_zombiebuddy_arg(&json_path, windows);
        let json_needs_ram = need_6gb
            && read_xmx_arg(&json_path)
                .as_deref()
                .is_some_and(is_default_3gb_xmx);
        if json_missing_agent || json_needs_ram {
            patch_game_json(&json_path, windows, need_6gb)?;
            repaired.push("ProjectZomboid64.json");
        }

        if let Some(bat_path) = find_game_bat(install_path, windows) {
            let bat_missing_agent = !bat_has_zombiebuddy_arg(&bat_path);
            let bat_needs_ram = need_6gb && bat_has_default_3gb_xmx(&bat_path);
            if bat_missing_agent || bat_needs_ram {
                patch_game_bat(&bat_path, need_6gb)?;
                repaired.push("ProjectZomboid64.bat");
            }
        }
    }

    if repaired.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![format!(
            "Re-applied ZombieBuddy / PZ3D configuration after game update ({}).",
            repaired.join(", ")
        )])
    }
}

fn staged_has_zombiebuddy(staging_path: &Path) -> bool {
    staging_path
        .join("mods")
        .join("ZombieBuddy")
        .join("42")
        .join("mod.info")
        .is_file()
        || staging_path
            .join("ZombieBuddy")
            .join("42")
            .join("mod.info")
            .is_file()
}

fn staged_has_pz3d(staging_path: &Path) -> bool {
    staging_path.join("mods").join("pz3d").is_dir() || staging_path.join("pz3d").is_dir()
}

fn is_linux_proton_build(install_path: &Path) -> bool {
    cfg!(target_os = "linux") && is_windows_build(install_path)
}

fn build_tools_status(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
    latest_pz3d: Option<&str>,
    latest_zb: Option<&str>,
) -> Vec<PluginToolStatus> {
    let mods = mods_dir(install_path);
    let state = load_state(&mods);
    let windows = is_windows_build(install_path);
    let json_path = find_game_json(install_path, windows);
    let jvm_dir = json_path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf));

    let zb_staged = mods::find_plugin_tool_mod(paths, game_id, TOOL_ZOMBIEBUDDY);
    let pz3d_staged = mods::find_plugin_tool_mod(paths, game_id, TOOL_PZ3D);

    let zb_files_ok = jvm_dir.as_ref().is_some_and(|dir| {
        dir.join("ZombieBuddy.jar").is_file()
            && (!windows || dir.join("zbNative.dll").is_file())
    });
    let zb_arg_ok = json_path
        .as_ref()
        .is_some_and(|p| json_has_zombiebuddy_arg(p, windows));
    let zb_staged_ok = zb_staged
        .as_ref()
        .is_some_and(|m| staged_has_zombiebuddy(Path::new(&m.staging_path)));
    let zb_mod_ok = zb_staged_ok || mods.join("ZombieBuddy").join("42").join("mod.info").is_file();
    let zb_installed = zb_files_ok && zb_arg_ok && zb_mod_ok;
    let zb_version = if zb_installed {
        zb_staged
            .as_ref()
            .and_then(|m| m.version.clone())
            .or_else(|| state.zombiebuddy_version.clone())
    } else {
        None
    };
    let zb_update = match (zb_version.as_deref(), latest_zb) {
        (Some(cur), Some(lat)) => !versions_equal(cur, lat),
        _ => false,
    };

    let pz3d_staged_ok = pz3d_staged
        .as_ref()
        .is_some_and(|m| staged_has_pz3d(Path::new(&m.staging_path)));
    let pz3d_installed = (pz3d_staged_ok || mods.join("pz3d").is_dir()) && zb_installed;
    let pz3d_version = if pz3d_installed {
        pz3d_staged
            .as_ref()
            .and_then(|m| m.version.clone())
            .or_else(|| state.pz3d_version.clone())
    } else {
        None
    };
    let pz3d_update = match (pz3d_version.as_deref(), latest_pz3d) {
        (Some(cur), Some(lat)) => !versions_equal(cur, lat),
        _ => false,
    };

    let ram_label = json_path
        .as_ref()
        .and_then(|p| read_xmx_arg(p))
        .map(|arg| format!("JVM heap: {arg}"));

    let mut pz3d_detail_parts = Vec::new();
    if pz3d_staged.as_ref().is_some_and(|m| !m.enabled) {
        pz3d_detail_parts.push("Disabled in staged mods".into());
    }
    if zb_installed {
        let v = zb_version.as_deref().unwrap_or("configured");
        pz3d_detail_parts.push(format!("ZombieBuddy {v}"));
    }
    if let Some(ram) = ram_label {
        pz3d_detail_parts.push(ram);
    }
    let pz3d_detail = if pz3d_detail_parts.is_empty() {
        None
    } else {
        Some(pz3d_detail_parts.join(" · "))
    };

    let mut pz3d_notes = Vec::new();
    if pz3d_installed {
        pz3d_notes.push(
            "On first launch, approve PZ3D when ZombieBuddy prompts you, then restart Project Zomboid."
                .into(),
        );
    }
    if let Some(user) = zomboid_user_dir(install_path) {
        if user.join("Workshop").join("pz3d-standalone").is_dir() {
            pz3d_notes.push(
                "Detected an older local copy at Zomboid/Workshop/pz3d-standalone; remove it if it conflicts with the Emperor-managed copy."
                    .into(),
            );
        }
    }

    let zb_detail = if zb_installed {
        let mut parts = Vec::new();
        if zb_staged.as_ref().is_some_and(|m| !m.enabled) {
            parts.push("Disabled in staged mods".to_string());
        }
        parts.push(if windows {
            "Configured in ProjectZomboid64.json (-agentlib:zbNative)".into()
        } else {
            "Configured in ProjectZomboid64.json (-javaagent:ZombieBuddy.jar)".into()
        });
        Some(parts.join(" · "))
    } else if json_path.is_none() {
        Some("ProjectZomboid64.json not found in game folder".into())
    } else {
        None
    };

    let mut zb_notes = Vec::new();
    if zb_installed {
        zb_notes.push(
            "If you previously added -agentlib:zbNative or -javaagent:ZombieBuddy.jar to Steam Launch Options, remove it so ZombieBuddy is not loaded twice."
                .into(),
        );
    } else if state.zombiebuddy_version.is_some() {
        zb_notes.push(
            "A Project Zomboid update or Steam file verification appears to have reset ProjectZomboid64.json or removed the loader binary. Deploy mods or click Install to restore it."
                .into(),
        );
    }
    if is_linux_proton_build(install_path) {
        let proton_note = "Detected the Windows/Proton build on Linux. PZ3D and ZombieBuddy target the native Linux build (zbNative.dll may fail under Proton/Wine); switching to the native Linux version in Steam is recommended.";
        zb_notes.push(proton_note.into());
        pz3d_notes.push(proton_note.into());
    }

    vec![
        PluginToolStatus {
            id: TOOL_PZ3D.into(),
            name: "PZ3D (3D Mod)".into(),
            description: "Installs the standalone PZ3D 3D camera & models mod from pz3d.dev, automatically sets up ZombieBuddy 2.3.4+, enables both mods, and bumps a default 3 GB JVM heap limit to 6 GB.".into(),
            homepage_url: Some(PZ3D_HOMEPAGE.into()),
            installed: pz3d_installed,
            installed_version: pz3d_version,
            latest_version: latest_pz3d.map(str::to_string),
            update_available: pz3d_update,
            status_detail: pz3d_detail,
            notes: pz3d_notes,
            can_uninstall: true,
        },
        PluginToolStatus {
            id: TOOL_ZOMBIEBUDDY.into(),
            name: "ZombieBuddy Loader".into(),
            description: "Java agent loader required by PZ3D and other Project Zomboid Java mods. Installed from GitHub Releases and wired into ProjectZomboid64.json.".into(),
            homepage_url: Some(ZOMBIEBUDDY_HOMEPAGE.into()),
            installed: zb_installed,
            installed_version: zb_version,
            latest_version: latest_zb.map(str::to_string),
            update_available: zb_update,
            status_detail: zb_detail,
            notes: zb_notes,
            can_uninstall: true,
        },
    ]
}

async fn install_zombiebuddy(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
) -> Result<PluginToolActionResult> {
    let windows = is_windows_build(install_path);
    let json_path = find_game_json(install_path, windows).ok_or_else(|| {
        anyhow!(
            "Could not find ProjectZomboid64.json in {}. Verify your Project Zomboid installation path.",
            install_path.display()
        )
    })?;
    let jvm_dir = json_path
        .parent()
        .ok_or_else(|| anyhow!("ProjectZomboid64.json has no parent directory"))?;
    let mods = mods_dir(install_path);
    std::fs::create_dir_all(&mods).with_context(|| format!("create {}", mods.display()))?;
    let bat_path = find_game_bat(install_path, windows);

    ensure_files_not_locked(&collect_zombiebuddy_target_files(
        jvm_dir,
        &mods,
        &json_path,
        bat_path.as_deref(),
        windows,
    ))?;

    let assets = fetch_zombiebuddy_assets(windows).await?;
    let jar_bytes = github_release::download_bytes(&assets.jar_url)
        .await
        .context("download ZombieBuddy.jar")?;
    let dll_bytes = match &assets.native_dll_url {
        Some(url) => Some(
            github_release::download_bytes(url)
                .await
                .context("download zbNative.dll")?,
        ),
        None => None,
    };

    let mod_info = fetch_zombiebuddy_mod_info(&assets.version)
        .await
        .unwrap_or_else(|| FALLBACK_ZOMBIEBUDDY_MOD_INFO.to_string());

    let stage_tmp = StageTempDir::new(paths, "zb")?;
    let zb_mod_dir = stage_tmp.path().join("mods").join("ZombieBuddy");
    let zb_libs_dir = zb_mod_dir.join("libs");
    let zb_42_dir = zb_mod_dir.join("42");
    std::fs::create_dir_all(&zb_libs_dir)?;
    std::fs::create_dir_all(&zb_42_dir)?;

    std::fs::write(zb_libs_dir.join("ZombieBuddy.jar"), &jar_bytes)?;
    std::fs::write(zb_42_dir.join("mod.info"), &mod_info)?;
    if let Some(dll) = &dll_bytes {
        std::fs::write(zb_libs_dir.join("zbNative.dll"), dll)?;
    }

    mods::stage_plugin_tool_mod(
        paths,
        game_id,
        TOOL_ZOMBIEBUDDY,
        "ZombieBuddy",
        "projectzomboid",
        Some(assets.version.clone()),
        stage_tmp.path(),
        &[],
    )?;

    std::fs::write(jvm_dir.join("ZombieBuddy.jar"), &jar_bytes)
        .with_context(|| format!("write {}", jvm_dir.join("ZombieBuddy.jar").display()))?;

    let mut copied = vec!["ZombieBuddy.jar".to_string()];
    if let Some(dll) = &dll_bytes {
        std::fs::write(jvm_dir.join("zbNative.dll"), dll)
            .with_context(|| format!("write {}", jvm_dir.join("zbNative.dll").display()))?;
        copied.push("zbNative.dll".to_string());
    }

    let patch = patch_game_json(&json_path, windows, false)?;
    let bat_patch = match &bat_path {
        Some(path) => Some(patch_game_bat(path, false)?),
        None => None,
    };

    // Clean any legacy unmanaged copy so Emperor's deploy owns Zomboid/mods/ZombieBuddy.
    if !mods::has_active_deployment(paths, game_id) {
        let legacy_dir = mods.join("ZombieBuddy");
        if legacy_dir.exists() {
            let _ = std::fs::remove_dir_all(&legacy_dir);
        }
        let _ = update_default_list_entries(&mods, &[], &["ZombieBuddy"]);
    }

    let mut state = load_state(&mods);
    state.zombiebuddy_version = Some(assets.version.clone());
    state.copied_game_files = copied;
    state.patched_vm_arg = Some(patch.agent_arg.clone());
    save_state(&mods, &state)?;

    let mut warnings = Vec::new();
    if patch.removed_legacy_pz3d_loader
        || bat_patch
            .as_ref()
            .is_some_and(|b| b.removed_legacy_pz3d_loader)
    {
        warnings.push(
            "Removed legacy pz3dLoader argument from game launch config. If you also added one to Steam Launch Options, remove it there too."
                .into(),
        );
    }
    if is_linux_proton_build(install_path) {
        warnings.push(
            "Installed for the Windows/Proton build on Linux (best-effort). ZombieBuddy's zbNative.dll may fail under Proton/Wine; switching to the native Linux build in Steam is recommended."
                .into(),
        );
    }

    let tool = build_tools_status(paths, game_id, install_path, None, Some(&assets.version))
        .into_iter()
        .find(|t| t.id == TOOL_ZOMBIEBUDDY)
        .unwrap();

    Ok(PluginToolActionResult {
        tool,
        message: format!("Installed ZombieBuddy {} as a staged mod.", assets.version),
        warnings,
    })
}

async fn install_pz3d(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
) -> Result<PluginToolActionResult> {
    let windows = is_windows_build(install_path);
    let json_path = find_game_json(install_path, windows).ok_or_else(|| {
        anyhow!(
            "Could not find ProjectZomboid64.json in {}. Verify your Project Zomboid installation path.",
            install_path.display()
        )
    })?;
    let mods = mods_dir(install_path);
    std::fs::create_dir_all(&mods).with_context(|| format!("create {}", mods.display()))?;
    let bat_path = find_game_bat(install_path, windows);

    let mut lock_targets = collect_pz3d_target_files(&mods, &json_path, bat_path.as_deref());
    if let Some(jvm_dir) = json_path.parent() {
        lock_targets.extend(collect_zombiebuddy_target_files(
            jvm_dir,
            &mods,
            &json_path,
            bat_path.as_deref(),
            windows,
        ));
    }
    ensure_files_not_locked(&lock_targets)?;

    let mut warnings = Vec::new();

    // Step 1: Ensure ZombieBuddy 2.3.4+ is installed, staged, and wired up.
    let current_status = build_tools_status(paths, game_id, install_path, None, None);
    let zb_staged = mods::find_plugin_tool_mod(paths, game_id, TOOL_ZOMBIEBUDDY)
        .is_some_and(|m| staged_has_zombiebuddy(Path::new(&m.staging_path)));
    let zb_ready = zb_staged
        && current_status
            .iter()
            .find(|t| t.id == TOOL_ZOMBIEBUDDY)
            .is_some_and(|t| {
                t.installed
                    && t.installed_version
                        .as_deref()
                        .and_then(parse_semver)
                        .is_some_and(|v| v >= ZOMBIEBUDDY_MIN_VERSION)
            });
    if !zb_ready {
        let zb_result = install_zombiebuddy(paths, game_id, install_path).await?;
        warnings.extend(zb_result.warnings);
    }

    // Step 2: Fetch latest PZ3D release metadata and stream-download archive.
    let release = fetch_latest_pz3d_release().await?;
    let download_rel = release
        .download
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            anyhow!(
                "PZ3D {} does not have a standalone download link published yet",
                release.version
            )
        })?;
    let download_url = resolve_pz3d_download_url(download_rel);

    let cache_dir = paths.cache_dir.join("pz3d");
    std::fs::create_dir_all(&cache_dir)
        .with_context(|| format!("create {}", cache_dir.display()))?;
    let zip_path = cache_dir.join(format!("pz3d-{}.zip", sanitize_tag(&release.version)));
    github_release::download_to_path(&download_url, &zip_path)
        .await
        .with_context(|| format!("download PZ3D {} from {download_url}", release.version))?;

    // Step 3: Extract `pz3d-standalone/Contents/mods/*` into a temp dir and stage as a real mod.
    let stage_tmp = StageTempDir::new(paths, "pz3d")?;
    let stage_mods = stage_tmp.path().join("mods");
    std::fs::create_dir_all(&stage_mods)?;
    let extract_res = extract_pz3d_mods_from_zip(&zip_path, &stage_mods);
    let _ = std::fs::remove_file(&zip_path);
    let extracted_dirs = extract_res?;

    mods::stage_plugin_tool_mod(
        paths,
        game_id,
        TOOL_PZ3D,
        "PZ3D",
        "projectzomboid",
        Some(release.version.clone()),
        stage_tmp.path(),
        &[mods::plugin_tool_mod_id(TOOL_ZOMBIEBUDDY)],
    )?;

    // Step 4: Ensure 6 GB heap bump in ProjectZomboid64.json (and .bat on Windows) if still at default 3 GB.
    let patch = patch_game_json(&json_path, windows, true)?;
    let bat_patch = match &bat_path {
        Some(path) => Some(patch_game_bat(path, true)?),
        None => None,
    };
    if patch.removed_legacy_pz3d_loader
        || bat_patch
            .as_ref()
            .is_some_and(|b| b.removed_legacy_pz3d_loader)
    {
        warnings.push(
            "Removed legacy pz3dLoader argument from game launch config.".into(),
        );
    }

    // Clean any legacy unmanaged copies so Emperor's deploy owns Zomboid/mods/pz3d*.
    if !mods::has_active_deployment(paths, game_id) {
        for dir_name in &extracted_dirs {
            let legacy_dir = mods.join(dir_name);
            if legacy_dir.exists() {
                let _ = std::fs::remove_dir_all(&legacy_dir);
            }
        }
        let _ = update_default_list_entries(&mods, &[], &["pz3d", "pz3d_chainlink"]);
    }

    let mut state = load_state(&mods);
    state.pz3d_version = Some(release.version.clone());
    state.pz3d_mod_dirs = extracted_dirs;
    if state.previous_xmx.is_none() {
        state.previous_xmx = patch.previous_xmx;
    }
    if state.previous_bat_xmx.is_none() {
        state.previous_bat_xmx = bat_patch.and_then(|b| b.previous_xmx);
    }
    save_state(&mods, &state)?;

    if let Some(user) = zomboid_user_dir(install_path) {
        if user.join("Workshop").join("pz3d-standalone").is_dir() {
            warnings.push(
                "An older local copy exists at Zomboid/Workshop/pz3d-standalone; consider removing it to avoid conflicts."
                    .into(),
            );
        }
    }

    let tool = build_tools_status(paths, game_id, install_path, Some(&release.version), None)
        .into_iter()
        .find(|t| t.id == TOOL_PZ3D)
        .unwrap();

    Ok(PluginToolActionResult {
        tool,
        message: format!(
            "Installed PZ3D {} and ZombieBuddy as staged mods. Launch Project Zomboid from Emperor, approve PZ3D when ZombieBuddy prompts you, then restart the game.",
            release.version
        ),
        warnings,
    })
}

fn uninstall_pz3d(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
) -> Result<PluginToolActionResult> {
    let windows = is_windows_build(install_path);
    let mods = mods_dir(install_path);
    let json_path = find_game_json(install_path, windows);
    let bat_path = find_game_bat(install_path, windows);
    if let Some(jp) = json_path.as_deref() {
        ensure_files_not_locked(&collect_pz3d_target_files(&mods, jp, bat_path.as_deref()))?;
    }
    let mut state = load_state(&mods);

    let _ = mods::remove_mod(paths, game_id, &mods::plugin_tool_mod_id(TOOL_PZ3D));

    let dirs_to_remove: Vec<String> = if state.pz3d_mod_dirs.is_empty() {
        vec!["pz3d".into(), "pz3d_chainlink".into()]
    } else {
        state.pz3d_mod_dirs.clone()
    };
    for dir_name in &dirs_to_remove {
        let target = mods.join(dir_name);
        if target.exists() {
            std::fs::remove_dir_all(&target)
                .with_context(|| format!("remove {}", target.display()))?;
        }
    }

    let _ = update_default_list_entries(&mods, &[], &["pz3d", "pz3d_chainlink"]);

    if let (Some(prev_xmx), Some(jp)) = (state.previous_xmx.take(), json_path.as_deref()) {
        let _ = restore_xmx_in_game_json(jp, &prev_xmx);
    }
    if let (Some(prev_bat_xmx), Some(bp)) = (state.previous_bat_xmx.take(), bat_path.as_deref()) {
        let _ = restore_xmx_in_game_bat(bp, &prev_bat_xmx);
    }

    state.pz3d_version = None;
    state.pz3d_mod_dirs.clear();
    save_or_clean_state(&mods, &state)?;

    let tool = build_tools_status(paths, game_id, install_path, None, None)
        .into_iter()
        .find(|t| t.id == TOOL_PZ3D)
        .unwrap();

    Ok(PluginToolActionResult {
        tool,
        message: "Removed PZ3D staged mod and restored launch settings.".into(),
        warnings: Vec::new(),
    })
}

fn uninstall_zombiebuddy(
    paths: &Paths,
    game_id: &str,
    install_path: &Path,
) -> Result<PluginToolActionResult> {
    let windows = is_windows_build(install_path);
    let mods = mods_dir(install_path);
    let json_path = find_game_json(install_path, windows);
    let bat_path = find_game_bat(install_path, windows);
    if let (Some(jp), Some(jvm_dir)) = (
        json_path.as_deref(),
        json_path.as_deref().and_then(Path::parent),
    ) {
        ensure_files_not_locked(&collect_zombiebuddy_target_files(
            jvm_dir,
            &mods,
            jp,
            bat_path.as_deref(),
            windows,
        ))?;
    }
    let mut state = load_state(&mods);

    let _ = mods::remove_mod(paths, game_id, &mods::plugin_tool_mod_id(TOOL_ZOMBIEBUDDY));

    let zb_mod_dir = mods.join("ZombieBuddy");
    if zb_mod_dir.exists() {
        std::fs::remove_dir_all(&zb_mod_dir)
            .with_context(|| format!("remove {}", zb_mod_dir.display()))?;
    }

    if let Some(jp) = json_path.as_deref() {
        if let Some(jvm_dir) = jp.parent() {
            let files: Vec<&str> = if state.copied_game_files.is_empty() {
                vec!["ZombieBuddy.jar", "zbNative.dll"]
            } else {
                state.copied_game_files.iter().map(String::as_str).collect()
            };
            for file in files {
                let path = jvm_dir.join(file);
                if path.is_file() {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("remove {}", path.display()))?;
                }
            }
        }
        unpatch_zombiebuddy_from_game_json(jp)?;
    }
    if let Some(bp) = bat_path.as_deref() {
        unpatch_zombiebuddy_from_game_bat(bp)?;
    }

    let _ = update_default_list_entries(&mods, &[], &["ZombieBuddy"]);

    state.zombiebuddy_version = None;
    state.copied_game_files.clear();
    state.patched_vm_arg = None;
    save_or_clean_state(&mods, &state)?;

    let tool = build_tools_status(paths, game_id, install_path, None, None)
        .into_iter()
        .find(|t| t.id == TOOL_ZOMBIEBUDDY)
        .unwrap();

    Ok(PluginToolActionResult {
        tool,
        message: "Removed ZombieBuddy staged mod, loader files, and game launch config.".into(),
        warnings: Vec::new(),
    })
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("emperor-mod-manager")
        .timeout(Duration::from_secs(20))
        .build()
        .context("build HTTP client")
}

async fn fetch_latest_pz3d_release() -> Result<Pz3dReleaseEntry> {
    let client = http_client()?;
    let resp = client
        .get(PZ3D_RELEASES_URL)
        .send()
        .await
        .context("fetch pz3d releases.json")?;
    if !resp.status().is_success() {
        anyhow::bail!("pz3d.dev returned HTTP {}", resp.status());
    }
    let releases: Vec<Pz3dReleaseEntry> = resp
        .json()
        .await
        .context("parse pz3d releases.json")?;
    pick_latest_pz3d_release(&releases)
        .cloned()
        .ok_or_else(|| anyhow!("pz3d releases.json contained no downloadable release"))
}

fn pick_latest_pz3d_release(releases: &[Pz3dReleaseEntry]) -> Option<&Pz3dReleaseEntry> {
    releases
        .iter()
        .find(|entry| entry.download.as_deref().is_some_and(|d| !d.trim().is_empty()))
        .or_else(|| releases.first())
}

fn resolve_pz3d_download_url(download: &str) -> String {
    let trimmed = download.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        trimmed.to_string()
    } else {
        format!("{PZ3D_BASE_URL}{}", trimmed.trim_start_matches('/'))
    }
}

async fn fetch_zombiebuddy_assets(need_windows_dll: bool) -> Result<ResolvedZombieBuddyAssets> {
    let releases = github_release::fetch_releases(ZOMBIEBUDDY_REPO, 20)
        .await
        .context("fetch ZombieBuddy GitHub releases")?;
    select_zombiebuddy_assets(&releases, need_windows_dll)
}

fn select_zombiebuddy_assets(
    releases: &[GhRelease],
    need_windows_dll: bool,
) -> Result<ResolvedZombieBuddyAssets> {
    let mod_releases: Vec<(&GhRelease, (u32, u32, u32))> = releases
        .iter()
        .filter(|r| !r.prerelease && !r.tag_name.to_ascii_lowercase().contains("installer"))
        .filter_map(|r| parse_semver(&r.tag_name).map(|v| (r, v)))
        .collect();

    let (jar_release, jar_ver) = mod_releases
        .iter()
        .filter(|(r, _)| r.assets.iter().any(|a| a.name.eq_ignore_ascii_case("ZombieBuddy.jar")))
        .max_by_key(|(_, v)| *v)
        .copied()
        .ok_or_else(|| {
            anyhow!("Could not find a ZombieBuddy GitHub release containing ZombieBuddy.jar")
        })?;

    if jar_ver < ZOMBIEBUDDY_MIN_VERSION {
        anyhow::bail!(
            "Latest ZombieBuddy release ({}) is older than the minimum required 2.3.4",
            jar_release.tag_name
        );
    }

    let jar_url = jar_release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("ZombieBuddy.jar"))
        .unwrap()
        .browser_download_url
        .clone();

    let native_dll_url = if need_windows_dll {
        let same_release_dll = jar_release
            .assets
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case("zbNative.dll"))
            .map(|a| a.browser_download_url.clone());

        let fallback_dll = same_release_dll.or_else(|| {
            mod_releases
                .iter()
                .filter(|(r, _)| {
                    r.assets
                        .iter()
                        .any(|a| a.name.eq_ignore_ascii_case("zbNative.dll"))
                })
                .max_by_key(|(_, v)| *v)
                .and_then(|(r, _)| {
                    r.assets
                        .iter()
                        .find(|a| a.name.eq_ignore_ascii_case("zbNative.dll"))
                        .map(|a| a.browser_download_url.clone())
                })
        });

        Some(fallback_dll.ok_or_else(|| {
            anyhow!("Could not find zbNative.dll in ZombieBuddy GitHub releases")
        })?)
    } else {
        None
    };

    Ok(ResolvedZombieBuddyAssets {
        version: jar_release.tag_name.clone(),
        jar_url,
        native_dll_url,
    })
}

async fn fetch_zombiebuddy_mod_info(tag: &str) -> Option<String> {
    let url = format!("https://raw.githubusercontent.com/{ZOMBIEBUDDY_REPO}/{tag}/42/mod.info");
    let client = http_client().ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().await.ok()?;
    if text.contains("id=ZombieBuddy") {
        Some(text)
    } else {
        None
    }
}

fn find_game_json(install_path: &Path, windows: bool) -> Option<PathBuf> {
    if windows {
        let direct = install_path.join("ProjectZomboid64.json");
        if direct.is_file() {
            return Some(direct);
        }
        return None;
    }
    let nested = install_path
        .join("projectzomboid")
        .join("ProjectZomboid64.json");
    if nested.is_file() {
        return Some(nested);
    }
    let direct = install_path.join("ProjectZomboid64.json");
    if direct.is_file() {
        return Some(direct);
    }
    None
}

fn find_game_bat(install_path: &Path, windows: bool) -> Option<PathBuf> {
    if !windows {
        return None;
    }
    let bat = install_path.join("ProjectZomboid64.bat");
    if bat.is_file() {
        Some(bat)
    } else {
        None
    }
}

fn collect_zombiebuddy_target_files(
    jvm_dir: &Path,
    mods_root: &Path,
    json_path: &Path,
    bat_path: Option<&Path>,
    windows: bool,
) -> Vec<PathBuf> {
    let mut files = vec![
        json_path.to_path_buf(),
        jvm_dir.join("ZombieBuddy.jar"),
        mods_root.join("ZombieBuddy").join("libs").join("ZombieBuddy.jar"),
    ];
    if windows {
        files.push(jvm_dir.join("zbNative.dll"));
        files.push(mods_root.join("ZombieBuddy").join("libs").join("zbNative.dll"));
    }
    if let Some(bp) = bat_path {
        files.push(bp.to_path_buf());
    }
    files
}

fn collect_pz3d_target_files(
    mods_root: &Path,
    json_path: &Path,
    bat_path: Option<&Path>,
) -> Vec<PathBuf> {
    let mut files = vec![json_path.to_path_buf()];
    if let Some(bp) = bat_path {
        files.push(bp.to_path_buf());
    }
    collect_jar_files(&mods_root.join("pz3d"), 4, &mut files);
    files
}

fn collect_jar_files(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth == 0 || !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jar_files(&path, depth - 1, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("jar"))
        {
            out.push(path);
        }
    }
}

fn ensure_files_not_locked(paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        if !path.is_file() {
            continue;
        }
        if let Err(err) = OpenOptions::new().read(true).write(true).open(path) {
            anyhow::bail!(
                "Close Project Zomboid before installing, updating, or uninstalling PZ3D / ZombieBuddy (cannot write {}: {err})",
                path.display()
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BatPatchOutcome {
    previous_xmx: Option<String>,
    removed_legacy_pz3d_loader: bool,
}

fn bat_has_zombiebuddy_arg(bat_path: &Path) -> bool {
    std::fs::read_to_string(bat_path)
        .ok()
        .is_some_and(|text| text.contains("-agentlib:zbNative"))
}

fn bat_has_default_3gb_xmx(bat_path: &Path) -> bool {
    std::fs::read_to_string(bat_path).ok().is_some_and(|text| {
        text.split_whitespace().any(is_default_3gb_xmx)
    })
}

fn patch_game_bat(bat_path: &Path, ensure_6gb_ram: bool) -> Result<BatPatchOutcome> {
    let raw = std::fs::read_to_string(bat_path)
        .with_context(|| format!("read {}", bat_path.display()))?;
    let backup = PathBuf::from(format!("{}{JSON_BACKUP_SUFFIX}", bat_path.display()));
    if !backup.exists() {
        std::fs::write(&backup, &raw)
            .with_context(|| format!("write backup {}", backup.display()))?;
    }

    let line_ending = if raw.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let mut removed_legacy = false;
    let mut previous_xmx = None;
    let mut found_java_opts = false;

    for line in &mut lines {
        let trimmed = line.trim_start();
        if trimmed.to_ascii_uppercase().starts_with("SET _JAVA_OPTIONS=") {
            found_java_opts = true;
            let eq_pos = line.find('=').unwrap();
            let prefix = &line[..=eq_pos];
            let rest = &line[eq_pos + 1..];
            let mut tokens: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
            let before = tokens.len();
            tokens.retain(|t| !is_legacy_pz3d_loader_arg(t));
            if tokens.len() != before {
                removed_legacy = true;
            }
            if !tokens.iter().any(|t| t.starts_with("-agentlib:zbNative")) {
                tokens.push("-agentlib:zbNative".into());
            }
            *line = format!("{prefix}{}", tokens.join(" "));
        } else if trimmed.to_ascii_lowercase().contains("java.exe")
            || trimmed.to_ascii_lowercase().contains("jre64")
        {
            let mut tokens: Vec<String> = line.split_whitespace().map(str::to_string).collect();
            let before = tokens.len();
            tokens.retain(|t| !is_legacy_pz3d_loader_arg(t));
            if tokens.len() != before {
                removed_legacy = true;
            }
            if ensure_6gb_ram {
                let xmx_positions: Vec<usize> = tokens
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| {
                        t.to_ascii_lowercase().starts_with("-xmx").then_some(i)
                    })
                    .collect();
                if xmx_positions.len() == 1 {
                    let idx = xmx_positions[0];
                    if is_default_3gb_xmx(&tokens[idx]) {
                        previous_xmx = Some(tokens[idx].clone());
                        tokens[idx] = "-Xmx6144m".into();
                    }
                }
            }
            if tokens.len() != before || previous_xmx.is_some() {
                *line = tokens.join(" ");
            }
        }
    }

    if !found_java_opts {
        let insert_at = lines
            .iter()
            .position(|l| {
                let lower = l.to_ascii_lowercase();
                lower.contains("java.exe") || lower.contains("jre64")
            })
            .unwrap_or(0);
        lines.insert(insert_at, "SET _JAVA_OPTIONS=-agentlib:zbNative".into());
    }

    let mut out = lines.join(line_ending);
    if raw.ends_with('\n') {
        out.push_str(line_ending);
    }
    std::fs::write(bat_path, out).with_context(|| format!("write {}", bat_path.display()))?;

    Ok(BatPatchOutcome {
        previous_xmx,
        removed_legacy_pz3d_loader: removed_legacy,
    })
}

fn unpatch_zombiebuddy_from_game_bat(bat_path: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(bat_path)
        .with_context(|| format!("read {}", bat_path.display()))?;
    let line_ending = if raw.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let mut changed = false;

    for line in &mut lines {
        let trimmed = line.trim_start();
        if trimmed.to_ascii_uppercase().starts_with("SET _JAVA_OPTIONS=") {
            let eq_pos = line.find('=').unwrap();
            let prefix = &line[..=eq_pos];
            let rest = &line[eq_pos + 1..];
            let mut tokens: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
            let before = tokens.len();
            tokens.retain(|t| !is_any_zombiebuddy_arg(t));
            if tokens.len() != before {
                changed = true;
                *line = format!("{prefix}{}", tokens.join(" "));
            }
        }
    }

    if changed {
        let mut out = lines.join(line_ending);
        if raw.ends_with('\n') {
            out.push_str(line_ending);
        }
        std::fs::write(bat_path, out).with_context(|| format!("write {}", bat_path.display()))?;
    }
    Ok(())
}

fn restore_xmx_in_game_bat(bat_path: &Path, previous_xmx: &str) -> Result<()> {
    let raw = std::fs::read_to_string(bat_path)
        .with_context(|| format!("read {}", bat_path.display()))?;
    if !raw.contains("-Xmx6144m") {
        return Ok(());
    }
    let updated = raw.replace("-Xmx6144m", previous_xmx);
    std::fs::write(bat_path, updated).with_context(|| format!("write {}", bat_path.display()))?;
    Ok(())
}

fn desired_agent_arg(windows: bool) -> &'static str {
    if windows {
        "-agentlib:zbNative"
    } else {
        "-javaagent:ZombieBuddy.jar"
    }
}

fn is_zombiebuddy_arg(arg: &str, windows: bool) -> bool {
    if windows {
        arg.starts_with("-agentlib:zbNative")
    } else {
        arg.starts_with("-javaagent:ZombieBuddy.jar")
    }
}

fn is_any_zombiebuddy_arg(arg: &str) -> bool {
    arg.starts_with("-agentlib:zbNative") || arg.starts_with("-javaagent:ZombieBuddy.jar")
}

fn is_legacy_pz3d_loader_arg(arg: &str) -> bool {
    arg.to_ascii_lowercase().contains("pz3dloader")
}

fn is_default_3gb_xmx(arg: &str) -> bool {
    let lower = arg.trim().to_ascii_lowercase();
    lower == "-xmx3g" || lower == "-xmx3072m"
}

fn json_has_zombiebuddy_arg(json_path: &Path, windows: bool) -> bool {
    let Ok(text) = std::fs::read_to_string(json_path) else {
        return false;
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    doc.get("vmArgs")
        .and_then(Value::as_array)
        .is_some_and(|args| {
            args.iter()
                .filter_map(Value::as_str)
                .any(|arg| is_zombiebuddy_arg(arg, windows))
        })
}

fn read_xmx_arg(json_path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(json_path).ok()?;
    let doc = serde_json::from_str::<Value>(&text).ok()?;
    doc.get("vmArgs")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .find(|arg| arg.to_ascii_lowercase().starts_with("-xmx"))
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JsonPatchOutcome {
    agent_arg: String,
    previous_xmx: Option<String>,
    removed_legacy_pz3d_loader: bool,
}

fn patch_game_json(
    json_path: &Path,
    windows: bool,
    ensure_6gb_ram: bool,
) -> Result<JsonPatchOutcome> {
    let raw = std::fs::read_to_string(json_path)
        .with_context(|| format!("read {}", json_path.display()))?;
    let mut doc: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parse {}", json_path.display()))?;

    let backup = PathBuf::from(format!("{}{JSON_BACKUP_SUFFIX}", json_path.display()));
    if !backup.exists() {
        std::fs::write(&backup, &raw)
            .with_context(|| format!("write backup {}", backup.display()))?;
    }

    let vm_args = doc
        .get_mut("vmArgs")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("{} has no vmArgs array", json_path.display()))?;

    let before_len = vm_args.len();
    vm_args.retain(|v| v.as_str().is_none_or(|s| !is_legacy_pz3d_loader_arg(s)));
    let removed_legacy_pz3d_loader = vm_args.len() != before_len;

    let agent_arg = desired_agent_arg(windows).to_string();
    let has_agent = vm_args
        .iter()
        .filter_map(Value::as_str)
        .any(|s| is_zombiebuddy_arg(s, windows));
    if !has_agent {
        vm_args.insert(0, Value::String(agent_arg.clone()));
    }

    let mut previous_xmx = None;
    if ensure_6gb_ram {
        let xmx_indices: Vec<usize> = vm_args
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                v.as_str()
                    .filter(|s| s.to_ascii_lowercase().starts_with("-xmx"))
                    .map(|_| i)
            })
            .collect();
        if xmx_indices.len() == 1 {
            let idx = xmx_indices[0];
            if let Some(current) = vm_args[idx].as_str().map(str::to_string) {
                if is_default_3gb_xmx(&current) {
                    previous_xmx = Some(current);
                    vm_args[idx] = Value::String("-Xmx6144m".into());
                }
            }
        }
    }

    let formatted = serde_json::to_string_pretty(&doc)?;
    std::fs::write(json_path, format!("{formatted}\n"))
        .with_context(|| format!("write {}", json_path.display()))?;

    Ok(JsonPatchOutcome {
        agent_arg,
        previous_xmx,
        removed_legacy_pz3d_loader,
    })
}

fn unpatch_zombiebuddy_from_game_json(json_path: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(json_path)
        .with_context(|| format!("read {}", json_path.display()))?;
    let mut doc: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parse {}", json_path.display()))?;
    if let Some(vm_args) = doc.get_mut("vmArgs").and_then(Value::as_array_mut) {
        vm_args.retain(|v| v.as_str().is_none_or(|s| !is_any_zombiebuddy_arg(s)));
        let formatted = serde_json::to_string_pretty(&doc)?;
        std::fs::write(json_path, format!("{formatted}\n"))
            .with_context(|| format!("write {}", json_path.display()))?;
    }
    Ok(())
}

fn restore_xmx_in_game_json(json_path: &Path, previous_xmx: &str) -> Result<()> {
    let raw = std::fs::read_to_string(json_path)
        .with_context(|| format!("read {}", json_path.display()))?;
    let mut doc: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parse {}", json_path.display()))?;
    if let Some(vm_args) = doc.get_mut("vmArgs").and_then(Value::as_array_mut) {
        let mut changed = false;
        for entry in vm_args.iter_mut() {
            if entry
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("-xmx6144m"))
            {
                *entry = Value::String(previous_xmx.to_string());
                changed = true;
            }
        }
        if changed {
            let formatted = serde_json::to_string_pretty(&doc)?;
            std::fs::write(json_path, format!("{formatted}\n"))
                .with_context(|| format!("write {}", json_path.display()))?;
        }
    }
    Ok(())
}

fn extract_pz3d_mods_from_zip(zip_path: &Path, mods_root: &Path) -> Result<Vec<String>> {
    let file = File::open(zip_path).with_context(|| format!("open {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("open PZ3D zip archive")?;

    let staging = mods_root.join(".emperor-pz3d-staging");
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    std::fs::create_dir_all(&staging)?;

    let mut top_dirs = BTreeSet::new();
    let extract_result: Result<()> = (|| {
        for idx in 0..archive.len() {
            let mut entry = archive.by_index(idx)?;
            let raw_name = entry.name().replace('\\', "/");
            let Some(rel_under_mods) = strip_pz3d_mods_prefix(&raw_name) else {
                continue;
            };
            let rel_path = safe_relative_path(rel_under_mods)?;
            if rel_path.as_os_str().is_empty() {
                continue;
            }
            if let Some(Component::Normal(first)) = rel_path.components().next() {
                top_dirs.insert(first.to_string_lossy().into_owned());
            }
            let out_path = staging.join(&rel_path);
            if entry.is_dir() {
                std::fs::create_dir_all(&out_path)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out_file = File::create(&out_path)
                    .with_context(|| format!("create {}", out_path.display()))?;
                io::copy(&mut entry, &mut out_file)?;
            }
        }
        Ok(())
    })();

    if let Err(err) = extract_result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }

    if top_dirs.is_empty() {
        let _ = std::fs::remove_dir_all(&staging);
        anyhow::bail!("PZ3D archive did not contain any mod folders under Contents/mods/");
    }

    let mut installed = Vec::new();
    for dir_name in top_dirs {
        let src = staging.join(&dir_name);
        if !src.is_dir() {
            continue;
        }
        let dst = mods_root.join(&dir_name);
        if dst.exists() {
            std::fs::remove_dir_all(&dst)
                .with_context(|| format!("remove existing {}", dst.display()))?;
        }
        std::fs::rename(&src, &dst)
            .with_context(|| format!("move {} to {}", src.display(), dst.display()))?;
        installed.push(dir_name);
    }

    let _ = std::fs::remove_dir_all(&staging);
    Ok(installed)
}

fn strip_pz3d_mods_prefix(entry_name: &str) -> Option<&str> {
    let trimmed = entry_name.trim_start_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    for needle in ["contents/mods/", "mods/"] {
        if let Some(pos) = lower.find(needle) {
            let after = &trimmed[pos + needle.len()..];
            if !after.is_empty() {
                return Some(after);
            }
        }
    }
    None
}

fn safe_relative_path(raw: &str) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for comp in Path::new(raw).components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("unsafe archive entry path: {raw}");
            }
        }
    }
    Ok(out)
}

fn parse_semver(raw: &str) -> Option<(u32, u32, u32)> {
    let cleaned = raw.trim().trim_start_matches(['v', 'V']);
    let core = cleaned.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().unwrap_or("0").parse().ok()?;
    let patch: u32 = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

fn versions_equal(a: &str, b: &str) -> bool {
    a.trim().trim_start_matches(['v', 'V']).eq_ignore_ascii_case(
        b.trim().trim_start_matches(['v', 'V']),
    )
}

fn sanitize_tag(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect()
}

fn load_state(mods_root: &Path) -> Pz3dState {
    let path = mods_root.join(STATE_FILE);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(mods_root: &Path, state: &Pz3dState) -> Result<()> {
    let path = mods_root.join(STATE_FILE);
    let body = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, format!("{body}\n"))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn save_or_clean_state(mods_root: &Path, state: &Pz3dState) -> Result<()> {
    let path = mods_root.join(STATE_FILE);
    if state.pz3d_version.is_none() && state.zombiebuddy_version.is_none() {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    save_state(mods_root, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_release::GhAsset;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn sample_json(xmx: &str) -> String {
        format!(
            r#"{{
  "mainClass": "zombie/gameStates/MainScreenState",
  "vmArgs": [
    "-Djava.awt.headless=true",
    "{xmx}",
    "-Dzomboid.steam=1"
  ]
}}
"#
        )
    }

    #[test]
    fn selects_zombiebuddy_mod_release_skipping_windows_installer() {
        let releases = vec![
            GhRelease {
                tag_name: "windows_installer_4.4".into(),
                prerelease: false,
                html_url: None,
                assets: vec![GhAsset {
                    name: "ZombieBuddyInstaller_v4.4.exe".into(),
                    browser_download_url: "https://example.invalid/installer.exe".into(),
                }],
            },
            GhRelease {
                tag_name: "v2.3.4".into(),
                prerelease: false,
                html_url: None,
                assets: vec![GhAsset {
                    name: "ZombieBuddy.jar".into(),
                    browser_download_url: "https://example.invalid/v2.3.4/ZombieBuddy.jar".into(),
                }],
            },
            GhRelease {
                tag_name: "v2.3.3".into(),
                prerelease: false,
                html_url: None,
                assets: vec![
                    GhAsset {
                        name: "ZombieBuddy.jar".into(),
                        browser_download_url: "https://example.invalid/v2.3.3/ZombieBuddy.jar".into(),
                    },
                    GhAsset {
                        name: "zbNative.dll".into(),
                        browser_download_url: "https://example.invalid/v2.3.3/zbNative.dll".into(),
                    },
                ],
            },
        ];

        let linux = select_zombiebuddy_assets(&releases, false).unwrap();
        assert_eq!(linux.version, "v2.3.4");
        assert_eq!(linux.jar_url, "https://example.invalid/v2.3.4/ZombieBuddy.jar");
        assert_eq!(linux.native_dll_url, None);

        let windows = select_zombiebuddy_assets(&releases, true).unwrap();
        assert_eq!(windows.version, "v2.3.4");
        assert_eq!(
            windows.native_dll_url.as_deref(),
            Some("https://example.invalid/v2.3.3/zbNative.dll")
        );
    }

    #[test]
    fn patches_and_restores_linux_and_windows_game_json() {
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("ProjectZomboid64.json");
        std::fs::write(
            &json_path,
            r#"{
  "vmArgs": [
    "-javaagent:pz3dLoader.jar",
    "-Xmx3g",
    "-Dzomboid.steam=1"
  ]
}"#,
        )
        .unwrap();

        let outcome = patch_game_json(&json_path, false, true).unwrap();
        assert_eq!(outcome.agent_arg, "-javaagent:ZombieBuddy.jar");
        assert_eq!(outcome.previous_xmx.as_deref(), Some("-Xmx3g"));
        assert!(outcome.removed_legacy_pz3d_loader);
        assert!(json_has_zombiebuddy_arg(&json_path, false));
        assert_eq!(read_xmx_arg(&json_path).as_deref(), Some("-Xmx6144m"));
        assert!(PathBuf::from(format!("{}{JSON_BACKUP_SUFFIX}", json_path.display())).is_file());

        // Patching again is idempotent.
        let again = patch_game_json(&json_path, false, true).unwrap();
        assert_eq!(again.previous_xmx, None);
        assert!(!again.removed_legacy_pz3d_loader);

        restore_xmx_in_game_json(&json_path, "-Xmx3g").unwrap();
        assert_eq!(read_xmx_arg(&json_path).as_deref(), Some("-Xmx3g"));

        unpatch_zombiebuddy_from_game_json(&json_path).unwrap();
        assert!(!json_has_zombiebuddy_arg(&json_path, false));
    }

    #[test]
    fn preserves_custom_higher_xmx_limit() {
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("ProjectZomboid64.json");
        std::fs::write(&json_path, sample_json("-Xmx8g")).unwrap();

        let outcome = patch_game_json(&json_path, true, true).unwrap();
        assert_eq!(outcome.agent_arg, "-agentlib:zbNative");
        assert_eq!(outcome.previous_xmx, None);
        assert_eq!(read_xmx_arg(&json_path).as_deref(), Some("-Xmx8g"));
        assert!(json_has_zombiebuddy_arg(&json_path, true));
    }

    #[test]
    fn extracts_pz3d_standalone_mods_from_zip() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("pz3d.zip");
        let file = File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();

        zip.start_file("pz3d-0.10.4-windows-linux/README.txt", opts)
            .unwrap();
        zip.write_all(b"readme").unwrap();
        zip.start_file(
            "pz3d-0.10.4-windows-linux/pz3d-standalone/Contents/mods/pz3d/42.20.4/mod.info",
            opts,
        )
        .unwrap();
        zip.write_all(b"name=pz3d\nid=pz3d\n").unwrap();
        zip.start_file(
            "pz3d-0.10.4-windows-linux/pz3d-standalone/Contents/mods/pz3d_chainlink/42.20.4/mod.info",
            opts,
        )
        .unwrap();
        zip.write_all(b"name=pz3d_chainlink\nid=pz3d_chainlink\n")
            .unwrap();
        zip.finish().unwrap();

        let mods = dir.path().join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let extracted = extract_pz3d_mods_from_zip(&zip_path, &mods).unwrap();
        assert_eq!(extracted, vec!["pz3d".to_string(), "pz3d_chainlink".to_string()]);
        assert!(mods.join("pz3d").join("42.20.4").join("mod.info").is_file());
        assert!(mods
            .join("pz3d_chainlink")
            .join("42.20.4")
            .join("mod.info")
            .is_file());
        assert!(!mods.join("README.txt").exists());
    }

    #[test]
    fn resolves_pz3d_download_urls() {
        assert_eq!(
            resolve_pz3d_download_url("downloads/pz3d-0.10.4-windows-linux.zip"),
            "https://pz3d.dev/install/downloads/pz3d-0.10.4-windows-linux.zip"
        );
        assert_eq!(
            resolve_pz3d_download_url("https://example.com/pz3d.zip"),
            "https://example.com/pz3d.zip"
        );
    }

    #[test]
    fn default_list_helper_preserves_workshop_and_emperor_entries() {
        let mods = tempfile::tempdir().unwrap();
        std::fs::write(
            mods.path().join("default.txt"),
            "VERSION = 1,\n\nmods\n{\n\tmod = WorkshopMod,\n}\n\nmaps\n{\n}\n",
        )
        .unwrap();

        update_default_list_entries(mods.path(), &["ZombieBuddy", "pz3d"], &[]).unwrap();
        let body = std::fs::read_to_string(mods.path().join("default.txt")).unwrap();
        assert!(body.contains("mod = WorkshopMod,"));
        assert!(body.contains("mod = ZombieBuddy,"));
        assert!(body.contains("mod = pz3d,"));

        update_default_list_entries(mods.path(), &[], &["pz3d"]).unwrap();
        let body = std::fs::read_to_string(mods.path().join("default.txt")).unwrap();
        assert!(body.contains("mod = WorkshopMod,"));
        assert!(body.contains("mod = ZombieBuddy,"));
        assert!(!body.contains("mod = pz3d,"));
    }

    #[test]
    fn patches_and_restores_windows_alternate_launch_bat() {
        let dir = tempfile::tempdir().unwrap();
        let bat_path = dir.path().join("ProjectZomboid64.bat");
        std::fs::write(
            &bat_path,
            "@setlocal enableextensions\r\nSET _JAVA_OPTIONS=-javaagent:pz3dLoader.jar\r\n\".\\jre64\\bin\\java.exe\" -Xmx3072m -Dzomboid.steam=1 zombie.gameStates.MainScreenState\r\n",
        )
        .unwrap();

        let outcome = patch_game_bat(&bat_path, true).unwrap();
        assert!(outcome.removed_legacy_pz3d_loader);
        assert_eq!(outcome.previous_xmx.as_deref(), Some("-Xmx3072m"));
        assert!(bat_has_zombiebuddy_arg(&bat_path));

        let patched = std::fs::read_to_string(&bat_path).unwrap();
        assert!(patched.contains("\r\n"));
        assert!(patched.contains("SET _JAVA_OPTIONS=-agentlib:zbNative"));
        assert!(!patched.contains("pz3dLoader"));
        assert!(patched.contains("-Xmx6144m"));

        restore_xmx_in_game_bat(&bat_path, "-Xmx3072m").unwrap();
        unpatch_zombiebuddy_from_game_bat(&bat_path).unwrap();
        let reverted = std::fs::read_to_string(&bat_path).unwrap();
        assert!(!reverted.contains("-agentlib:zbNative"));
        assert!(reverted.contains("-Xmx3072m"));
    }

    #[test]
    fn repair_after_deploy_restores_wiped_json_bat_and_binaries() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("ProjectZomboid");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("ProjectZomboid64.exe"), b"exe").unwrap();
        std::fs::write(install.join("ProjectZomboid64.json"), sample_json("-Xmx3g")).unwrap();
        std::fs::write(
            install.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=\r\n\".\\jre64\\bin\\java.exe\" -Xmx3072m Main\r\n",
        )
        .unwrap();

        let mods = tmp.path().join("zomboid_mods");
        let libs = mods.join("ZombieBuddy").join("libs");
        std::fs::create_dir_all(&libs).unwrap();
        std::fs::write(libs.join("ZombieBuddy.jar"), b"jar").unwrap();
        std::fs::write(libs.join("zbNative.dll"), b"dll").unwrap();
        save_state(
            &mods,
            &Pz3dState {
                zombiebuddy_version: Some("v2.3.4".into()),
                pz3d_version: Some("0.10.4".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let warnings = repair_after_deploy_in(
            &install,
            &mods,
            &["ZombieBuddy".into(), "pz3d".into()],
        )
        .unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Re-applied ZombieBuddy / PZ3D"));
        assert!(install.join("ZombieBuddy.jar").is_file());
        assert!(install.join("zbNative.dll").is_file());
        assert!(json_has_zombiebuddy_arg(
            &install.join("ProjectZomboid64.json"),
            true
        ));
        assert_eq!(
            read_xmx_arg(&install.join("ProjectZomboid64.json")).as_deref(),
            Some("-Xmx6144m")
        );
        assert!(bat_has_zombiebuddy_arg(&install.join("ProjectZomboid64.bat")));
    }

    #[test]
    fn staged_tool_mods_appear_in_status_and_deploy_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("emperor");
        let paths = Paths::from_dirs(
            root.join("config"),
            root.join("data"),
            root.join("cache"),
        )
        .unwrap();
        let game_id = "pz-game-1";

        let install = tmp.path().join("ProjectZomboid");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("ProjectZomboid64.json"), sample_json("-Xmx3g")).unwrap();
        std::fs::write(install.join("ZombieBuddy.jar"), b"jar").unwrap();
        patch_game_json(&install.join("ProjectZomboid64.json"), false, true).unwrap();

        // Stage ZombieBuddy as a real staged mod.
        let zb_src = tmp.path().join("zb_src");
        std::fs::create_dir_all(zb_src.join("mods/ZombieBuddy/42")).unwrap();
        std::fs::create_dir_all(zb_src.join("mods/ZombieBuddy/libs")).unwrap();
        std::fs::write(
            zb_src.join("mods/ZombieBuddy/42/mod.info"),
            FALLBACK_ZOMBIEBUDDY_MOD_INFO,
        )
        .unwrap();
        std::fs::write(zb_src.join("mods/ZombieBuddy/libs/ZombieBuddy.jar"), b"jar").unwrap();
        let zb_staged = mods::stage_plugin_tool_mod(
            &paths,
            game_id,
            TOOL_ZOMBIEBUDDY,
            "ZombieBuddy",
            "projectzomboid",
            Some("v2.3.4".into()),
            &zb_src,
            &[],
        )
        .unwrap();
        assert_eq!(zb_staged.plugin_tool_id.as_deref(), Some(TOOL_ZOMBIEBUDDY));
        assert_eq!(zb_staged.id, "tool:zombiebuddy");

        // Stage PZ3D as a real staged mod depending on ZombieBuddy.
        let pz_src = tmp.path().join("pz_src");
        std::fs::create_dir_all(pz_src.join("mods/pz3d/42")).unwrap();
        std::fs::write(
            pz_src.join("mods/pz3d/42/mod.info"),
            "name=pz3d\nid=pz3d\n",
        )
        .unwrap();
        let pz_staged = mods::stage_plugin_tool_mod(
            &paths,
            game_id,
            TOOL_PZ3D,
            "PZ3D",
            "projectzomboid",
            Some("0.10.4".into()),
            &pz_src,
            &[mods::plugin_tool_mod_id(TOOL_ZOMBIEBUDDY)],
        )
        .unwrap();
        assert_eq!(pz_staged.plugin_tool_id.as_deref(), Some(TOOL_PZ3D));
        assert_eq!(pz_staged.depends_on, vec!["tool:zombiebuddy"]);

        // Both appear in loadorder.json and tools_status recognizes them as installed.
        let order = mods::load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods.len(), 2);

        let statuses = build_tools_status(&paths, game_id, &install, None, None);
        let pz_status = statuses.iter().find(|t| t.id == TOOL_PZ3D).unwrap();
        let zb_status = statuses.iter().find(|t| t.id == TOOL_ZOMBIEBUDDY).unwrap();
        assert!(zb_status.installed);
        assert_eq!(zb_status.installed_version.as_deref(), Some("v2.3.4"));
        assert!(pz_status.installed);
        assert_eq!(pz_status.installed_version.as_deref(), Some("0.10.4"));

        // Disabling PZ3D in staged mods surfaces in tools_status detail.
        mods::set_enabled(&paths, game_id, &pz_staged.id, false).unwrap();
        let disabled_statuses = build_tools_status(&paths, game_id, &install, None, None);
        let pz_disabled = disabled_statuses
            .iter()
            .find(|t| t.id == TOOL_PZ3D)
            .unwrap();
        assert!(pz_disabled.installed);
        assert!(pz_disabled
            .status_detail
            .as_deref()
            .is_some_and(|d| d.contains("Disabled in staged mods")));
    }

    #[test]
    fn adopt_legacy_installs_migrates_unmanaged_tool_dirs_into_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("emperor");
        let paths = Paths::from_dirs(
            root.join("config"),
            root.join("data"),
            root.join("cache"),
        )
        .unwrap();
        let game_id = "pz-legacy";
        let mods_root = tmp.path().join("zomboid_mods");

        std::fs::create_dir_all(mods_root.join("ZombieBuddy/42")).unwrap();
        std::fs::create_dir_all(mods_root.join("ZombieBuddy/libs")).unwrap();
        std::fs::write(
            mods_root.join("ZombieBuddy/42/mod.info"),
            FALLBACK_ZOMBIEBUDDY_MOD_INFO,
        )
        .unwrap();
        std::fs::write(mods_root.join("ZombieBuddy/libs/ZombieBuddy.jar"), b"jar").unwrap();

        std::fs::create_dir_all(mods_root.join("pz3d/42")).unwrap();
        std::fs::write(mods_root.join("pz3d/42/mod.info"), "name=pz3d\nid=pz3d\n").unwrap();

        save_state(
            &mods_root,
            &Pz3dState {
                zombiebuddy_version: Some("v2.3.4".into()),
                pz3d_version: Some("0.10.4".into()),
                pz3d_mod_dirs: vec!["pz3d".into()],
                ..Default::default()
            },
        )
        .unwrap();

        let adopted = adopt_legacy_installs_in(&paths, game_id, &mods_root).unwrap();
        assert!(adopted);
        // Second call is a no-op once staged.
        assert!(!adopt_legacy_installs_in(&paths, game_id, &mods_root).unwrap());

        let order = mods::load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods.len(), 2);
        assert!(order
            .mods
            .iter()
            .any(|m| m.plugin_tool_id.as_deref() == Some(TOOL_ZOMBIEBUDDY)));
        assert!(order
            .mods
            .iter()
            .any(|m| m.plugin_tool_id.as_deref() == Some(TOOL_PZ3D)));
        // Unmanaged copies were cleaned up since there was no active deployment.
        assert!(!mods_root.join("ZombieBuddy").exists());
        assert!(!mods_root.join("pz3d").exists());
    }

    #[test]
    fn preflight_lock_check_reports_clear_error_for_unwritable_file() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("ZombieBuddy.jar");
        std::fs::write(&jar, b"jar").unwrap();
        let mut perms = std::fs::metadata(&jar).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&jar, perms).unwrap();

        let err = ensure_files_not_locked(&[jar.clone()]).unwrap_err();
        assert!(err.to_string().contains("Close Project Zomboid"));

        // Restore writable bit for cleanup.
        let mut perms = std::fs::metadata(&jar).unwrap().permissions();
        perms.set_readonly(false);
        std::fs::set_permissions(&jar, perms).unwrap();
    }
}
