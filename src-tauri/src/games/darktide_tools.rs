//! Optional Darktide Mod Loader (GitHub) and Darktide Mod Framework (Nexus) tools.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::warhammer40kdarktide::{
    bundle_already_patched, is_loader_or_framework_root, resolve_patcher_command,
    run_darktide_patcher, PatcherCommand, DARKTIDE_ROOT_DIRS,
};
use super::{
    PluginToolActionResult, PluginToolInstallContext, PluginToolInstallOutcome,
    PluginToolRecommendation, PluginToolStatus, ToolAssistRequest,
};
use crate::config::Paths;
use crate::github_release;
use crate::nexus::NexusClient;
use crate::tools::install as tool_install;

pub const TOOL_DML: &str = "darktide_mod_loader";
pub const TOOL_DMF: &str = "darktide_mod_framework";

const DML_REPO: &str = "Darktide-Mod-Framework/Darktide-Mod-Loader";
const DML_HOMEPAGE: &str = "https://github.com/Darktide-Mod-Framework/Darktide-Mod-Loader";

const DMF_NEXUS_DOMAIN: &str = "warhammer40kdarktide";
const DMF_NEXUS_MOD_ID: u64 = 8;
const DMF_HOMEPAGE: &str = "https://www.nexusmods.com/warhammer40kdarktide/mods/8";

const STATE_FILE: &str = ".emperor-darktide-tools.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DarktideToolsState {
    #[serde(default)]
    dml_version: Option<String>,
    #[serde(default)]
    dml_files: Vec<String>,
    #[serde(default)]
    dmf_version: Option<String>,
}

fn load_state(install_path: &Path) -> DarktideToolsState {
    let path = install_path.join(STATE_FILE);
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(install_path: &Path, state: &DarktideToolsState) -> Result<()> {
    let path = install_path.join(STATE_FILE);
    fs::write(&path, serde_json::to_string_pretty(state)?)
        .with_context(|| format!("write {}", path.display()))
}

pub fn dml_present(install_path: &Path) -> bool {
    let tools = install_path.join("tools");
    tools.join("dtkit-patch.exe").is_file() || tools.join("dtkit-patch").is_file()
}

pub fn dmf_present(install_path: &Path) -> bool {
    let dmf = install_path.join("mods").join("dmf");
    if !dmf.is_dir() {
        return false;
    }
    fs::read_dir(&dmf)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
}

fn build_dml_status(install_path: &Path, latest_version: Option<&str>) -> PluginToolStatus {
    let installed = dml_present(install_path);
    let patched = bundle_already_patched(install_path);
    let state = load_state(install_path);
    let installed_version = if installed {
        state.dml_version.clone()
    } else {
        None
    };
    let update_available = match (installed, installed_version.as_deref(), latest_version) {
        (true, Some(cur), Some(latest)) => cur.trim() != latest.trim(),
        _ => false,
    };
    let status_detail = if installed && patched {
        Some("Mod Loader detected and bundle_database.data is patched.".into())
    } else if installed {
        Some("Mod Loader files detected (bundle_database.data not patched yet; deploy or reinstall to patch).".into())
    } else {
        None
    };
    PluginToolStatus {
        id: TOOL_DML.into(),
        name: "Darktide Mod Loader".into(),
        description: "Patches Darktide's bundle database and loads mods from mods/mod_load_order.txt.".into(),
        homepage_url: Some(DML_HOMEPAGE.into()),
        installed,
        installed_version,
        latest_version: latest_version.map(str::to_string),
        update_available,
        status_detail,
        notes: Vec::new(),
        can_uninstall: installed,
    }
}

fn build_dmf_status(install_path: &Path, latest_version: Option<&str>) -> PluginToolStatus {
    let installed = dmf_present(install_path);
    let state = load_state(install_path);
    let installed_version = if installed {
        state.dmf_version.clone()
    } else {
        None
    };
    let update_available = match (installed, installed_version.as_deref(), latest_version) {
        (true, Some(cur), Some(latest)) => cur.trim() != latest.trim(),
        _ => false,
    };
    PluginToolStatus {
        id: TOOL_DMF.into(),
        name: "Darktide Mod Framework (DMF)".into(),
        description: "Core modding framework providing hooks, mod options menus, keybinds, and chat commands.".into(),
        homepage_url: Some(DMF_HOMEPAGE.into()),
        installed,
        installed_version,
        latest_version: latest_version.map(str::to_string),
        update_available,
        status_detail: if installed {
            Some("mods/dmf detected in the game folder.".into())
        } else {
            None
        },
        notes: vec!["Downloaded from Nexus Mods (Premium direct download, or Download Assist for free accounts).".into()],
        can_uninstall: installed,
    }
}

pub fn tools_status(_paths: &Paths, install_path: &Path) -> Vec<PluginToolStatus> {
    vec![
        build_dml_status(install_path, None),
        build_dmf_status(install_path, None),
    ]
}

pub async fn check_tool_updates(
    _paths: &Paths,
    install_path: &Path,
) -> Result<Vec<PluginToolStatus>> {
    let latest_dml = github_release::fetch_latest_release(DML_REPO)
        .await?
        .map(|r| r.tag_name);
    Ok(vec![
        build_dml_status(install_path, latest_dml.as_deref()),
        build_dmf_status(install_path, None),
    ])
}

pub async fn install_tool_ctx(
    ctx: &PluginToolInstallContext<'_>,
    tool_id: &str,
) -> Result<PluginToolInstallOutcome> {
    match tool_id {
        TOOL_DML => {
            let res = install_dml(ctx.paths, ctx.install_path).await?;
            Ok(PluginToolInstallOutcome::Done(res))
        }
        TOOL_DMF => install_dmf_ctx(ctx).await,
        other => Err(anyhow!("unknown Darktide tool: {other}")),
    }
}

async fn install_dml(paths: &Paths, install_path: &Path) -> Result<PluginToolActionResult> {
    let release = github_release::fetch_latest_release(DML_REPO)
        .await?
        .ok_or_else(|| anyhow!("Darktide Mod Loader has no GitHub releases"))?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.to_ascii_lowercase().ends_with(".zip"))
        .ok_or_else(|| anyhow!("release {} has no .zip asset", release.tag_name))?;
    let bytes = github_release::download_bytes(&asset.browser_download_url).await?;

    let tmp_dir = paths
        .cache_dir
        .join("darktide-dml-extract")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&tmp_dir)?;
    let extract_res = tool_install::extract_archive(&bytes, &asset.name, &tmp_dir);
    if let Err(e) = extract_res {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(e);
    }

    let source_root = peel_dml_wrapper(&tmp_dir);
    let copied = copy_tree_tracked(&source_root, install_path)?;
    let _ = fs::remove_dir_all(&tmp_dir);

    let mut state = load_state(install_path);
    state.dml_version = Some(release.tag_name.clone());
    state.dml_files = copied;
    save_state(install_path, &state)?;

    let mut warnings = Vec::new();
    match run_darktide_patcher(install_path) {
        Ok(None) => {}
        Ok(Some(msg)) => warnings.push(msg),
        Err(e) => warnings.push(format!("Darktide patcher failed: {e:#}")),
    }

    let tool = build_dml_status(install_path, Some(&release.tag_name));
    Ok(PluginToolActionResult {
        tool,
        message: format!("Installed Darktide Mod Loader ({}).", release.tag_name),
        warnings,
    })
}

async fn install_dmf_ctx(ctx: &PluginToolInstallContext<'_>) -> Result<PluginToolInstallOutcome> {
    match (ctx.nexus, ctx.premium) {
        (Some(client), true) => {
            let res = download_and_install_dmf_nexus(client, ctx.paths, ctx.install_path).await?;
            Ok(PluginToolInstallOutcome::Done(res))
        }
        (Some(client), false) => {
            let files = client
                .list_mod_files(DMF_NEXUS_DOMAIN, DMF_NEXUS_MOD_ID)
                .await?;
            let file = files
                .iter()
                .find(|f| f.is_primary)
                .or_else(|| files.first())
                .ok_or_else(|| anyhow!("Darktide Mod Framework has no Nexus files"))?;
            Ok(PluginToolInstallOutcome::NeedsAssist {
                request: ToolAssistRequest {
                    domain: DMF_NEXUS_DOMAIN.to_string(),
                    mod_id: DMF_NEXUS_MOD_ID,
                    file_id: file.file_id,
                    plugin_id: "warhammer40kdarktide".to_string(),
                    tool_id: TOOL_DMF.to_string(),
                    install_path: ctx.install_path.to_path_buf(),
                    label: "Darktide Mod Framework".to_string(),
                },
            })
        }
        (None, _) => Err(anyhow!(
            "No Nexus API key configured. Sign in to Nexus in Settings or download Darktide Mod Framework manually from {DMF_HOMEPAGE}."
        )),
    }
}

async fn download_and_install_dmf_nexus(
    client: &NexusClient,
    paths: &Paths,
    install_path: &Path,
) -> Result<PluginToolActionResult> {
    let files = client
        .list_mod_files(DMF_NEXUS_DOMAIN, DMF_NEXUS_MOD_ID)
        .await?;
    let file = files
        .iter()
        .find(|f| f.is_primary)
        .or_else(|| files.first())
        .ok_or_else(|| anyhow!("Darktide Mod Framework has no Nexus files"))?;
    let version = file.version.clone();
    let archive_name = file
        .file_name
        .clone()
        .unwrap_or_else(|| "dmf.zip".to_string());
    let dl_dir = paths
        .cache_dir
        .join("darktide-dmf-download")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&dl_dir)?;
    let archive_path = dl_dir.join(&archive_name);
    let dl_res = client
        .download_file(
            DMF_NEXUS_DOMAIN,
            DMF_NEXUS_MOD_ID,
            file.file_id,
            &archive_path,
            true,
            None,
            None,
            0,
            None,
            None,
        )
        .await;
    if let Err(e) = dl_res {
        let _ = fs::remove_dir_all(&dl_dir);
        return Err(e);
    }
    let install_res = install_dmf_from_archive_inner(paths, install_path, &archive_path, version);
    let _ = fs::remove_dir_all(&dl_dir);
    install_res
}

pub fn install_tool_from_archive(
    paths: &Paths,
    install_path: &Path,
    tool_id: &str,
    archive: &Path,
) -> Result<PluginToolActionResult> {
    if tool_id != TOOL_DMF {
        anyhow::bail!("unknown Darktide archive tool: {tool_id}");
    }
    install_dmf_from_archive_inner(paths, install_path, archive, None)
}

fn install_dmf_from_archive_inner(
    paths: &Paths,
    install_path: &Path,
    archive: &Path,
    version: Option<String>,
) -> Result<PluginToolActionResult> {
    let tmp_dir = paths
        .cache_dir
        .join("darktide-dmf-extract")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&tmp_dir)?;
    if let Err(e) = crate::mods::extract_archive(archive, &tmp_dir) {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(e);
    }

    let dmf_src = find_dmf_dir(&tmp_dir).ok_or_else(|| {
        let _ = fs::remove_dir_all(&tmp_dir);
        anyhow!("archive did not contain a dmf/ folder")
    })?;
    let dmf_dest = install_path.join("mods").join("dmf");
    if dmf_dest.exists() {
        let _ = fs::remove_dir_all(&dmf_dest);
    }
    copy_tree_tracked(&dmf_src, &dmf_dest)?;
    let _ = fs::remove_dir_all(&tmp_dir);

    let mut state = load_state(install_path);
    state.dmf_version = version.or_else(|| Some("installed".into()));
    save_state(install_path, &state)?;

    let tool = build_dmf_status(install_path, None);
    Ok(PluginToolActionResult {
        tool,
        message: "Installed Darktide Mod Framework (DMF) into mods/dmf.".into(),
        warnings: Vec::new(),
    })
}

pub fn tool_assist_ready(install_path: &Path, tool_id: &str) -> bool {
    tool_id == TOOL_DMF && dmf_present(install_path)
}

pub fn uninstall_tool(
    _paths: &Paths,
    install_path: &Path,
    tool_id: &str,
) -> Result<PluginToolActionResult> {
    let mut state = load_state(install_path);
    match tool_id {
        TOOL_DML => {
            let mut warnings = Vec::new();
            if let Some(warn) = try_unpatch_darktide(install_path) {
                warnings.push(warn);
            }
            if !state.dml_files.is_empty() {
                for rel in &state.dml_files {
                    let lower = rel.to_ascii_lowercase();
                    // Never delete user mods or mod_load_order.txt or mods/dmf.
                    if lower.starts_with("mods/") && lower != "mods/base" {
                        continue;
                    }
                    let target = install_path.join(rel);
                    if target.is_file() {
                        let _ = fs::remove_file(&target);
                    }
                }
            } else {
                for name in [
                    "toggle_darktide_mods.bat",
                    "tools/dtkit-patch.exe",
                    "tools/dtkit-patch",
                    "binaries/mod_loader",
                ] {
                    let target = install_path.join(name);
                    if target.is_file() {
                        let _ = fs::remove_file(&target);
                    }
                }
            }
            state.dml_version = None;
            state.dml_files.clear();
            let _ = save_state(install_path, &state);
            let tool = build_dml_status(install_path, None);
            Ok(PluginToolActionResult {
                tool,
                message: "Uninstalled Darktide Mod Loader (preserved your mods/ folder).".into(),
                warnings,
            })
        }
        TOOL_DMF => {
            let dmf_dest = install_path.join("mods").join("dmf");
            if dmf_dest.is_dir() {
                fs::remove_dir_all(&dmf_dest)
                    .with_context(|| format!("remove {}", dmf_dest.display()))?;
            }
            state.dmf_version = None;
            let _ = save_state(install_path, &state);
            let tool = build_dmf_status(install_path, None);
            Ok(PluginToolActionResult {
                tool,
                message: "Uninstalled Darktide Mod Framework (DMF).".into(),
                warnings: Vec::new(),
            })
        }
        other => Err(anyhow!("unknown Darktide tool: {other}")),
    }
}

pub fn recommended_tools(
    install_path: &Path,
    content_roots: &[&Path],
) -> Vec<PluginToolRecommendation> {
    let mut has_regular_mod = false;
    let mut staged_has_dml = false;
    let mut staged_has_dmf = false;

    for root in content_roots {
        if looks_like_dml_root(root) {
            staged_has_dml = true;
        }
        if looks_like_dmf_root(root) {
            staged_has_dmf = true;
        }
        if !is_loader_or_framework_root(root) {
            has_regular_mod = true;
        }
    }

    if !has_regular_mod {
        return Vec::new();
    }

    let mut recs = Vec::new();
    if !dml_present(install_path) && !staged_has_dml {
        recs.push(PluginToolRecommendation {
            tool_id: TOOL_DML.into(),
            tool_name: "Darktide Mod Loader".into(),
            reason: "Required to patch Darktide's bundle database and load enabled mods.".into(),
        });
    }
    if !dmf_present(install_path) && !staged_has_dmf {
        recs.push(PluginToolRecommendation {
            tool_id: TOOL_DMF.into(),
            tool_name: "Darktide Mod Framework (DMF)".into(),
            reason: "Required by Darktide mods for hooks, options menus, and keybinds.".into(),
        });
    }
    recs
}

fn looks_like_dml_root(root: &Path) -> bool {
    root.join("toggle_darktide_mods.bat").is_file()
        || root.join("tools").join("dtkit-patch.exe").is_file()
        || root.join("tools").join("dtkit-patch").is_file()
}

fn looks_like_dmf_root(root: &Path) -> bool {
    root.join("dmf").is_dir()
        || root
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("dmf"))
}

fn peel_dml_wrapper(root: &Path) -> PathBuf {
    let mut cur = root.to_path_buf();
    for _ in 0..3 {
        if cur.join("toggle_darktide_mods.bat").is_file()
            || DARKTIDE_ROOT_DIRS.iter().any(|d| cur.join(d).is_dir())
        {
            return cur;
        }
        let Ok(entries) = fs::read_dir(&cur) else {
            break;
        };
        let dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        if dirs.len() == 1 {
            cur = dirs[0].clone();
        } else {
            break;
        }
    }
    cur
}

fn find_dmf_dir(root: &Path) -> Option<PathBuf> {
    if root.join("dmf.mod").is_file() || root.join("dmf.lua").is_file() {
        return Some(root.to_path_buf());
    }
    for entry in WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.eq_ignore_ascii_case("dmf"))
        {
            return Some(entry.path().to_path_buf());
        }
    }
    None
}

fn copy_tree_tracked(src: &Path, dest: &Path) -> Result<Vec<String>> {
    let mut copied = Vec::new();
    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(src)?;
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), &out)
            .with_context(|| format!("copy {} to {}", entry.path().display(), out.display()))?;
        copied.push(rel.to_string_lossy().replace('\\', "/"));
    }
    Ok(copied)
}

fn try_unpatch_darktide(install_path: &Path) -> Option<String> {
    if !bundle_already_patched(install_path) {
        return None;
    }
    let cmd = resolve_patcher_command(install_path)?;
    let mut command = match &cmd {
        PatcherCommand::Native(bin) => {
            let mut c = Command::new(bin);
            c.arg("--unpatch").arg("bundle");
            c
        }
        PatcherCommand::Wine(bin) => {
            let mut c = Command::new("wine");
            c.arg(bin).arg("--unpatch").arg("bundle");
            c
        }
    };
    match command
        .current_dir(install_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(s) if s.success() => None,
        Ok(s) => Some(format!(
            "dtkit-patch --unpatch exited with {s}; verify game files in Steam if needed."
        )),
        Err(e) => Some(format!("Could not run dtkit-patch --unpatch: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths(dir: &Path) -> Paths {
        Paths {
            config_dir: dir.join("cfg"),
            data_dir: dir.join("data"),
            cache_dir: dir.join("cache"),
        }
    }

    #[test]
    fn recommends_dml_and_dmf_for_regular_mod_when_missing() {
        let install = tempfile::tempdir().unwrap();
        let mod_root = tempfile::tempdir().unwrap();
        fs::write(mod_root.path().join("scoreboard.mod"), b"return {}").unwrap();

        let recs = recommended_tools(install.path(), &[mod_root.path()]);
        let ids: Vec<&str> = recs.iter().map(|r| r.tool_id.as_str()).collect();
        assert_eq!(ids, vec![TOOL_DML, TOOL_DMF]);
    }

    #[test]
    fn skips_recommendations_when_installed_or_staged() {
        let install = tempfile::tempdir().unwrap();
        fs::create_dir_all(install.path().join("tools")).unwrap();
        fs::write(install.path().join("tools/dtkit-patch.exe"), b"x").unwrap();

        let dmf_staged = tempfile::tempdir().unwrap();
        fs::create_dir_all(dmf_staged.path().join("dmf")).unwrap();
        fs::write(dmf_staged.path().join("dmf/dmf.lua"), b"x").unwrap();

        let mod_root = tempfile::tempdir().unwrap();
        fs::write(mod_root.path().join("healthbars.mod"), b"return {}").unwrap();

        let recs = recommended_tools(install.path(), &[dmf_staged.path(), mod_root.path()]);
        assert!(recs.is_empty());
    }

    #[test]
    fn uninstall_dml_preserves_user_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = test_paths(tmp.path());
        let install = tmp.path().join("game");
        fs::create_dir_all(install.join("tools")).unwrap();
        fs::create_dir_all(install.join("mods/scoreboard")).unwrap();
        fs::write(install.join("tools/dtkit-patch.exe"), b"x").unwrap();
        fs::write(install.join("toggle_darktide_mods.bat"), b"x").unwrap();
        fs::write(install.join("mods/scoreboard/scoreboard.mod"), b"x").unwrap();
        save_state(
            &install,
            &DarktideToolsState {
                dml_version: Some("v1.0".into()),
                dml_files: vec![
                    "tools/dtkit-patch.exe".into(),
                    "toggle_darktide_mods.bat".into(),
                    "mods/scoreboard/scoreboard.mod".into(),
                ],
                dmf_version: None,
            },
        )
        .unwrap();

        let res = uninstall_tool(&paths, &install, TOOL_DML).unwrap();
        assert!(!res.tool.installed);
        assert!(!install.join("tools/dtkit-patch.exe").exists());
        assert!(install.join("mods/scoreboard/scoreboard.mod").is_file());
    }
}
