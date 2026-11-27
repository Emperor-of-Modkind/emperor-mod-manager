//! Optional BepInEx loader managed via the Plugin Tool system.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use super::unity_bepinex::{
    bepinex_cache_ready, bepinex_present, copy_bepinex_tree, should_auto_install_bepinex,
    BepInExBuild,
};
use super::{PluginToolActionResult, PluginToolRecommendation, PluginToolStatus};
use crate::config::Paths;
use crate::github_release;
use crate::thunderstore::ThunderstoreClient;
use crate::tools::install as tool_install;

pub const TOOL_BEPINEX: &str = "bepinex";
const STATE_FILE: &str = ".emperor-bepinex.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BepInExState {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    build_id: Option<String>,
    #[serde(default)]
    installed_files: Vec<String>,
}

fn load_state(install_path: &Path) -> BepInExState {
    let path = install_path.join(STATE_FILE);
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(install_path: &Path, state: &BepInExState) -> Result<()> {
    let path = install_path.join(STATE_FILE);
    let body = serde_json::to_string_pretty(state)?;
    fs::write(&path, body).with_context(|| format!("write {}", path.display()))
}

fn remove_state(install_path: &Path) {
    let _ = fs::remove_file(install_path.join(STATE_FILE));
}

fn build_display_name(build: BepInExBuild) -> String {
    match build {
        BepInExBuild::Github { label, .. } => label.to_string(),
        BepInExBuild::Thunderstore {
            namespace, name, ..
        } => format!("BepInEx ({namespace}-{name})"),
    }
}

fn build_homepage(build: BepInExBuild) -> String {
    match build {
        BepInExBuild::Github { .. } => format!("https://github.com/{}", build.repo()),
        BepInExBuild::Thunderstore {
            community,
            namespace,
            name,
        } => format!("https://thunderstore.io/c/{community}/p/{namespace}/{name}/"),
    }
}

fn build_status(
    install_path: &Path,
    build: BepInExBuild,
    latest_version: Option<&str>,
) -> PluginToolStatus {
    let installed = bepinex_present(install_path);
    let state = load_state(install_path);
    let installed_version = if installed {
        state.version.clone().or_else(|| match build {
            BepInExBuild::Github { tag, .. } => Some(tag.to_string()),
            BepInExBuild::Thunderstore { .. } => None,
        })
    } else {
        None
    };
    let update_available = match (installed, installed_version.as_deref(), latest_version) {
        (true, Some(cur), Some(latest)) => cur.trim() != latest.trim(),
        _ => false,
    };
    let description = match build {
        BepInExBuild::Github { il2cpp: true, .. } => {
            "Unity IL2CPP mod loader and plugin framework.".to_string()
        }
        BepInExBuild::Github { il2cpp: false, .. } => {
            "Unity Mono mod loader and plugin framework.".to_string()
        }
        BepInExBuild::Thunderstore {
            namespace, name, ..
        } => format!("Community Thunderstore BepInEx pack ({namespace}-{name})."),
    };
    let mut notes = Vec::new();
    if matches!(build, BepInExBuild::Github { il2cpp: true, .. }) {
        notes.push("First launch generates IL2CPP interop assemblies.".into());
    }
    if installed && state.installed_files.is_empty() {
        notes.push(
            "Detected an existing BepInEx install. Reinstalling or uninstalling will preserve BepInEx/plugins and BepInEx/config."
                .into(),
        );
    }
    PluginToolStatus {
        id: TOOL_BEPINEX.into(),
        name: build_display_name(build),
        description,
        homepage_url: Some(build_homepage(build)),
        installed,
        installed_version,
        latest_version: latest_version.map(str::to_string),
        update_available,
        status_detail: if installed {
            Some("BepInEx core detected in the game folder.".into())
        } else {
            None
        },
        notes,
        can_uninstall: installed,
    }
}

pub fn tools_status(
    _paths: &Paths,
    install_path: &Path,
    build: BepInExBuild,
) -> Vec<PluginToolStatus> {
    vec![build_status(install_path, build, None)]
}

pub async fn check_tool_updates(
    _paths: &Paths,
    install_path: &Path,
    build: BepInExBuild,
) -> Result<Vec<PluginToolStatus>> {
    let latest = match build {
        BepInExBuild::Github { tag, .. } => {
            let release = github_release::fetch_release_by_tag(build.repo(), tag).await?;
            release
                .map(|r| r.tag_name)
                .or_else(|| Some(tag.to_string()))
        }
        BepInExBuild::Thunderstore {
            community,
            namespace,
            name,
        } => {
            let client = ThunderstoreClient::new()?;
            let detail = client.get_package(community, namespace, name).await?;
            detail
                .versions
                .first()
                .map(|v| v.version_number.clone())
        }
    };
    Ok(vec![build_status(install_path, build, latest.as_deref())])
}

pub async fn install_tool(
    paths: &Paths,
    install_path: &Path,
    build: BepInExBuild,
    tool_id: &str,
) -> Result<PluginToolActionResult> {
    if tool_id != TOOL_BEPINEX {
        anyhow::bail!("unknown BepInEx tool: {tool_id}");
    }
    let cache = paths.bepinex_cache_dir(&build.cache_id());
    let version = download_build(&cache, build).await?;
    if !bepinex_cache_ready(&cache) {
        anyhow::bail!("downloaded BepInEx archive did not contain a loader");
    }
    let installed_files = copy_bepinex_tree(&cache, install_path)?;
    save_state(
        install_path,
        &BepInExState {
            version: Some(version.clone()),
            build_id: Some(build.cache_id()),
            installed_files,
        },
    )?;

    let mut message = match build {
        BepInExBuild::Github { label, .. } => format!("Installed {label}."),
        BepInExBuild::Thunderstore {
            namespace, name, ..
        } => format!("Installed BepInEx pack {namespace}-{name} ({version})."),
    };
    if matches!(build, BepInExBuild::Github { il2cpp: true, .. }) {
        message.push_str(" The first launch generates interop assemblies.");
    }
    let tool = build_status(install_path, build, Some(&version));
    Ok(PluginToolActionResult {
        tool,
        message,
        warnings: Vec::new(),
    })
}

pub fn uninstall_tool(
    _paths: &Paths,
    install_path: &Path,
    build: BepInExBuild,
    tool_id: &str,
) -> Result<PluginToolActionResult> {
    if tool_id != TOOL_BEPINEX {
        anyhow::bail!("unknown BepInEx tool: {tool_id}");
    }
    let state = load_state(install_path);
    if !state.installed_files.is_empty() {
        for rel in &state.installed_files {
            if is_user_bepinex_path(rel) {
                continue;
            }
            let target = install_path.join(rel);
            if target.is_file() {
                let _ = fs::remove_file(&target);
            }
        }
    } else {
        // Fallback cleanup for unmanaged BepInEx installs: remove core loader and doorstop files,
        // preserving user plugins and configs.
        for file in [
            "winhttp.dll",
            "version.dll",
            "doorstop_config.ini",
            ".doorstop_version",
            "libdoorstop.so",
            "libdoorstop_x64.so",
            "run_bepinex.sh",
            "start_game_bepinex.sh",
        ] {
            let p = install_path.join(file);
            if p.is_file() {
                let _ = fs::remove_file(&p);
            }
        }
        let core = install_path.join("BepInEx").join("core");
        if core.is_dir() {
            let _ = fs::remove_dir_all(&core);
        }
        for dir in ["doorstop_libs", "dotnet"] {
            let p = install_path.join(dir);
            if p.is_dir() {
                let _ = fs::remove_dir_all(&p);
            }
        }
    }

    // Prune empty BepInEx/core or BepInEx if nothing user-owned remains.
    for dir in [
        install_path.join("BepInEx").join("core"),
        install_path.join("BepInEx").join("interop"),
        install_path.join("doorstop_libs"),
        install_path.join("dotnet"),
    ] {
        if dir.is_dir() && dir_is_empty(&dir) {
            let _ = fs::remove_dir(&dir);
        }
    }
    remove_state(install_path);

    let tool = build_status(install_path, build, None);
    Ok(PluginToolActionResult {
        tool,
        message: "Uninstalled BepInEx (preserved BepInEx/plugins and BepInEx/config).".into(),
        warnings: Vec::new(),
    })
}

pub fn recommended_tools(
    install_path: &Path,
    content_roots: &[&Path],
    build: BepInExBuild,
    allows_empty: impl Fn(&Path) -> bool,
) -> Vec<PluginToolRecommendation> {
    if !should_auto_install_bepinex(install_path, content_roots, Some(build), allows_empty) {
        return Vec::new();
    }
    vec![PluginToolRecommendation {
        tool_id: TOOL_BEPINEX.into(),
        tool_name: build_display_name(build),
        reason: "Enabled Unity mods require BepInEx to load.".into(),
    }]
}

fn is_user_bepinex_path(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.starts_with("bepinex/plugins/") || lower.starts_with("bepinex/config/")
}

fn dir_is_empty(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|mut it| it.next().is_none())
        .unwrap_or(false)
}

async fn download_build(cache: &Path, build: BepInExBuild) -> Result<String> {
    let (bytes, name, version) = match build {
        BepInExBuild::Github { tag, asset, .. } => {
            let release = github_release::fetch_release_by_tag(build.repo(), tag)
                .await?
                .ok_or_else(|| anyhow!("GitHub release {tag} was not found"))?;
            let found = release
                .assets
                .iter()
                .find(|item| item.name == asset)
                .ok_or_else(|| anyhow!("release {tag} has no {asset}"))?;
            let bytes = github_release::download_bytes(&found.browser_download_url).await?;
            (bytes, found.name.clone(), release.tag_name)
        }
        BepInExBuild::Thunderstore {
            community,
            namespace,
            name,
            ..
        } => {
            let client = ThunderstoreClient::new()?;
            let detail = client.get_package(community, namespace, name).await?;
            let version = detail.versions.first().ok_or_else(|| {
                anyhow!("BepInEx pack {namespace}-{name} has no published versions")
            })?;
            let bytes = github_release::download_bytes(&version.download_url).await?;
            (
                bytes,
                format!("{namespace}-{name}-{}.zip", version.version_number),
                version.version_number.clone(),
            )
        }
    };
    if cache.exists() {
        let _ = fs::remove_dir_all(cache);
    }
    tool_install::extract_archive(&bytes, &name, cache)?;
    Ok(version)
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
    fn recommended_tools_and_uninstall_preserve_user_plugins() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = test_paths(tmp.path());
        let install = tmp.path().join("game");
        let mod_root = tmp.path().join("mod");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&mod_root).unwrap();
        fs::write(mod_root.join("Example.dll"), b"dll").unwrap();

        let build = BepInExBuild::Github {
            tag: "v5.4.23.5",
            asset: "BepInEx_win_x64_5.4.23.5.zip",
            cache_id: "bepinex-5-win-x64",
            label: "BepInEx 5.4.23.5",
            il2cpp: false,
        };

        let recs = recommended_tools(&install, &[mod_root.as_path()], build, |_| false);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].tool_id, TOOL_BEPINEX);

        // Simulate installed BepInEx + user plugin.
        fs::create_dir_all(install.join("BepInEx/core")).unwrap();
        fs::create_dir_all(install.join("BepInEx/plugins")).unwrap();
        fs::write(install.join("BepInEx/core/BepInEx.dll"), b"core").unwrap();
        fs::write(install.join("winhttp.dll"), b"doorstop").unwrap();
        fs::write(install.join("BepInEx/plugins/UserMod.dll"), b"user").unwrap();
        save_state(
            &install,
            &BepInExState {
                version: Some("v5.4.23.5".into()),
                build_id: Some(build.cache_id()),
                installed_files: vec![
                    "BepInEx/core/BepInEx.dll".into(),
                    "winhttp.dll".into(),
                    "BepInEx/plugins/UserMod.dll".into(),
                ],
            },
        )
        .unwrap();

        assert!(recommended_tools(&install, &[mod_root.as_path()], build, |_| false).is_empty());

        let res = uninstall_tool(&paths, &install, build, TOOL_BEPINEX).unwrap();
        assert!(!res.tool.installed);
        assert!(!install.join("winhttp.dll").exists());
        assert!(install.join("BepInEx/plugins/UserMod.dll").is_file());
    }
}
