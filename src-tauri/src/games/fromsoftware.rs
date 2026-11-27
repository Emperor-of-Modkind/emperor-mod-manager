//! Shared deploy logic for FromSoftware titles (Dark Souls trilogy + Elden Ring).
//!
//! Legacy Mod Engine titles (DS1 / Remastered / DS2) merge into `<Game>/mod/` and
//! update `modengine.ini`. Modern titles (Elden Ring, Nightreign, AC6, DS3) keep
//! `<Game>/mods/<ModName>/` and autoconfigure `emperor.me3` for Mod Engine 3 (`me3`).

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use super::{
    content_folder_wrap_name, normalize_relative, DeployContext, GamePlugin, GamePluginInfo,
};

/// Top-level FromSoft asset / overlay folders that must not be peeled as archive wrappers.
pub const FROMSOFT_ROOT_DIRS: &[&str] = &[
    "action",
    "asset",
    "chr",
    "cutscene",
    "event",
    "expression",
    "facegen",
    "font",
    "lang",
    "layout",
    "map",
    "material",
    "menu",
    "mod",
    "mods",
    "movie",
    "msg",
    "mtd",
    "other",
    "param",
    "parts",
    "script",
    "sd",
    "sfx",
    "shader",
    "sound",
    // Engine / co-op packs (must not be peeled as archive wrappers).
    "SeamlessCoop",
    "tex_override",
    "ds2le_atmosphere_presets",
];

/// Known game executables under a Steam `Game/` subdirectory.
const GAME_SUBDIR_EXES: &[&str] = &[
    "eldenring.exe",
    "nightreign.exe",
    "armoredcore6.exe",
    "sekiro.exe",
    "darksoulsiii.exe",
    "darksoulsii.exe",
    "darksouls.exe",
    "darksoulsremastered.exe",
];

/// Injector / proxy files that must sit next to the game executable.
const ROOT_INJECTOR_FILES: &[&str] = &[
    "dinput8.dll",
    "modengine.ini",
    "winmm.dll",
    "version.dll",
    "dsound.dll",
    "dxgi.dll",
    "d3d11.dll",
    "d3d12.dll",
];

/// Graphics proxy DLLs that may still sit next to the game exe on modern ME3 titles (e.g. ReShade).
const GRAPHICS_PROXY_FILES: &[&str] = &["dxgi.dll", "d3d11.dll", "d3d12.dll"];

/// FromSoft game asset override directories recognized as ME3 `[[packages]]` content.
const FROMSOFT_ASSET_DIRS: &[&str] = &[
    "action",
    "asset",
    "chr",
    "cutscene",
    "event",
    "expression",
    "facegen",
    "font",
    "lang",
    "layout",
    "map",
    "material",
    "menu",
    "movie",
    "msg",
    "mtd",
    "other",
    "param",
    "parts",
    "script",
    "sd",
    "sfx",
    "shader",
    "sound",
    "tex_override",
];

const FROMSOFT_ASSET_EXTENSIONS: &[&str] = &[
    "dcx", "bnd", "tpf", "flver", "msb", "emevd", "luabnd", "bdt", "bhd",
];

/// Docs / previews that Lighting Engine and similar packs ship but the game never loads.
const ROOT_INJECTOR_SKIP_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "webp", "gif", "bmp", "md", "txt"];

const ROOT_INJECTOR_MARKER_FILES: &[&str] = &[
    "dxgi.dll",
    "d3d11.dll",
    "d3d12.dll",
    "ds2sc_launcher.exe",
    "ds2sc.dll",
];

/// Prefer `install_path/Game` when that subdirectory holds the real game root.
pub fn resolve_game_root(install_path: &Path) -> PathBuf {
    let Some(game_subdir) = find_child_dir_ci(install_path, "Game") else {
        return install_path.to_path_buf();
    };
    if has_known_exe(&game_subdir) || !has_known_exe(install_path) {
        return game_subdir;
    }
    install_path.to_path_buf()
}

fn find_child_dir_ci(parent: &Path, name: &str) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return None;
    };
    entries.flatten().map(|entry| entry.path()).find(|path| {
        path.is_dir()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|found| found.eq_ignore_ascii_case(name))
    })
}

pub(crate) fn find_file_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    entries.flatten().map(|entry| entry.path()).find(|path| {
        path.is_file()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|found| found.eq_ignore_ascii_case(name))
    })
}

/// True when legacy Mod Engine's injector is already next to the game exe.
pub fn legacy_modengine_present(install_path: &Path) -> bool {
    find_file_ci(&resolve_game_root(install_path), "dinput8.dll").is_some()
}

fn has_known_exe(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let lower = name.to_string_lossy().to_lowercase();
        if GAME_SUBDIR_EXES.iter().any(|exe| *exe == lower.as_str()) {
            return true;
        }
    }
    false
}

fn path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn is_mod_segment(name: &str) -> bool {
    name.eq_ignore_ascii_case("mod") || name.eq_ignore_ascii_case("mods")
}

fn is_root_injector(name: &str) -> bool {
    ROOT_INJECTOR_FILES
        .iter()
        .any(|file| name.eq_ignore_ascii_case(file))
}

fn is_graphics_proxy(name: &str) -> bool {
    GRAPHICS_PROXY_FILES
        .iter()
        .any(|file| name.eq_ignore_ascii_case(file))
}

/// True when a staged mod is a graphics proxy pack (e.g. ERSS-FG with `D3D12.dll` + `ERSS-FG.dll`
/// + `ERSS2/`, OptiScaler, ReShade) that must deploy intact to `Game/` rather than `Game/mods/<Mod>/`.
pub fn looks_like_modern_graphics_proxy_pack(content_root: &Path) -> bool {
    if dir_has_graphics_proxy(content_root) {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(content_root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && dir_has_graphics_proxy(&path) {
            return true;
        }
    }
    false
}

fn dir_has_graphics_proxy(dir: &Path) -> bool {
    GRAPHICS_PROXY_FILES
        .iter()
        .any(|name| find_file_ci(dir, name).is_some())
}

/// Subdirectory names inside a mod folder that hold helper/SDK dependency DLLs, not mod plugins.
const NATIVE_DEPENDENCY_DIRS: &[&str] = &[
    "bin", "lib", "libs", "deps", "redist", "x64", "x86", "runtime", "runtimes",
];

fn is_non_mod_native_dll(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if is_root_injector(&lower) {
        return true;
    }
    matches!(
        lower.as_str(),
        "me3_mod_host.dll"
            | "modengine2.dll"
            | "modengine.dll"
            | "steam_api64.dll"
            | "steam_api.dll"
            | "_winhttp.dll"
            | "winhttp.dll"
            | "dxcompiler.dll"
            | "dxil.dll"
    ) || lower.starts_with("oo2core_")
        || lower.starts_with("vcruntime")
        || lower.starts_with("msvcp")
        || lower.starts_with("concrt")
        || lower.starts_with("ucrtbase")
        || lower.starts_with("api-ms-win-")
        || lower.starts_with("amd_fidelityfx_")
        || lower.starts_with("ffx_")
        || lower.starts_with("libxess")
        || lower.starts_with("xess")
        || lower.starts_with("nvngx_")
        || lower.starts_with("sl.")
        || lower.starts_with("d3dcompiler_")
        || lower.starts_with("vulkan-")
        || lower.starts_with("openvr_")
        || lower.starts_with("openxr_")
        || lower.starts_with("xinput")
        || lower.starts_with("d3dx")
        || lower.starts_with("physx")
        || lower.starts_with("bink2")
        || lower.starts_with("fmod")
        || lower.starts_with("wwise")
        || lower.starts_with("sdl2")
        || lower.starts_with("sdl3")
}

/// True when a staged pack is Elden Mod Loader (`dinput8.dll` + `mod_loader_config.ini`),
/// which is superseded by Mod Engine 3 (`me3`) on modern FromSoft titles.
pub fn looks_like_elden_mod_loader(content_root: &Path) -> bool {
    if dir_has_elden_mod_loader_markers(content_root) {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(content_root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && dir_has_elden_mod_loader_markers(&path) {
            return true;
        }
    }
    false
}

fn dir_has_elden_mod_loader_markers(dir: &Path) -> bool {
    let has_loader_file =
        find_file_ci(dir, "mod_loader_config.ini").is_some() || find_file_ci(dir, "dinput8.dll").is_some();
    if !has_loader_file {
        return false;
    }
    if mod_folder_has_assets(dir) {
        return false;
    }
    // Only treat the pack as standalone Elden Mod Loader when it has NO real mod DLLs.
    // Bundles (like "Elden Ring Ultrawide Fix") that ship dinput8.dll + mods/*.dll should
    // skip only the loader files while deploying their real DLL mods.
    let has_other_mod_dll = WalkDir::new(dir)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .any(|e| {
            e.file_type().is_file()
                && e.file_name().to_str().is_some_and(|n| {
                    n.to_ascii_lowercase().ends_with(".dll") && !is_non_mod_native_dll(n)
                })
        });
    !has_other_mod_dll
}

/// Collect top-level mod DLL filenames from an Elden-Mod-Loader-style flat DLL mod pack
/// (either `<root>/*.dll` or `<root>/mods/*.dll`), when the pack has no FromSoft assets,
/// is not a graphics proxy pack, and is not a Seamless Co-op pack.
pub fn flat_modern_dll_mod_files(content_root: &Path) -> Vec<String> {
    if looks_like_elden_mod_loader(content_root)
        || looks_like_modern_graphics_proxy_pack(content_root)
        || mod_folder_has_assets(content_root)
        || find_child_dir_ci(content_root, "SeamlessCoop").is_some()
    {
        return Vec::new();
    }
    let search_dir = find_child_dir_ci(content_root, "mods")
        .or_else(|| find_child_dir_ci(content_root, "mod"))
        .unwrap_or_else(|| content_root.to_path_buf());
    if mod_folder_has_assets(&search_dir) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&search_dir) else {
        return Vec::new();
    };
    let mut dlls = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.to_ascii_lowercase().ends_with(".dll") && !is_non_mod_native_dll(name) {
            dlls.push(name.to_string());
        }
    }
    dlls.sort();
    dlls
}

/// True when a staged pack is a flat Elden-Mod-Loader-style DLL mod (`mods/<Mod>.dll` +
/// `mods/<Mod>/config.ini`) that must deploy directly under `Game/mods/` so its hardcoded
/// `mods/<Mod>/config.ini` path resolves.
pub fn looks_like_modern_flat_dll_mod(content_root: &Path) -> bool {
    !flat_modern_dll_mod_files(content_root).is_empty()
}

/// Drop a leading `mod`/`mods` segment, and a nested one immediately after a wrap folder.
pub fn strip_mod_overlay_segments(relative: &Path) -> PathBuf {
    let originals = path_components(relative);
    if originals.is_empty() {
        return PathBuf::new();
    }
    let mut i = 0;
    if is_mod_segment(&originals[0]) {
        i = 1;
    }
    let mut out: Vec<&str> = Vec::new();
    while i < originals.len() {
        if out.len() == 1 && is_mod_segment(&originals[i]) {
            i += 1;
            continue;
        }
        out.push(originals[i].as_str());
        i += 1;
    }
    out.iter().collect()
}

fn resolve_overlay_deploy(install_path: &Path, relative: &Path, overlay: &str) -> PathBuf {
    let root = resolve_game_root(install_path);
    let normalized = normalize_relative(relative);
    let components = path_components(&normalized);
    if components.is_empty() {
        return root.join(overlay);
    }

    let leaf = components[components.len() - 1].as_str();
    if is_root_injector(leaf) {
        return root.join(
            normalized
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(leaf)),
        );
    }

    let stripped = strip_mod_overlay_segments(&normalized);
    if stripped.as_os_str().is_empty() {
        return root.join(overlay);
    }
    root.join(overlay).join(stripped)
}

fn resolve_legacy_deploy(install_path: &Path, relative: &Path) -> PathBuf {
    resolve_overlay_deploy(install_path, relative, "mod")
}

fn resolve_modern_deploy(install_path: &Path, relative: &Path) -> PathBuf {
    let root = resolve_game_root(install_path);
    let normalized = normalize_relative(relative);
    let components = path_components(&normalized);
    if components.is_empty() {
        return root.join("mods");
    }

    let leaf = components[components.len() - 1].as_str();
    if is_graphics_proxy(leaf) {
        return root.join(
            normalized
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(leaf)),
        );
    }

    let stripped = strip_mod_overlay_segments(&normalized);
    if stripped.as_os_str().is_empty() {
        return root.join("mods");
    }
    root.join("mods").join(stripped)
}

fn resolve_modern_deploy_ctx(
    install_path: &Path,
    relative: &Path,
    ctx: &DeployContext<'_>,
) -> PathBuf {
    if let Some(content) = ctx.content_root {
        if looks_like_modern_graphics_proxy_pack(content) {
            let root = resolve_game_root(install_path);
            let normalized = normalize_relative(relative);
            let comps = path_components(&normalized);
            // `deploy_tree` prepends the wrap folder name because `prefers_mod_folder()` is true;
            // drop that synthetic leading segment so the pack's files land directly under `Game/`.
            let unwrapped: PathBuf = if comps.len() > 1 {
                comps.into_iter().skip(1).collect()
            } else {
                normalized
            };
            let stripped = strip_leading_game_segment(&unwrapped);
            if stripped.as_os_str().is_empty() {
                return root;
            }
            return root.join(stripped);
        }
        if looks_like_modern_flat_dll_mod(content) {
            let root = resolve_game_root(install_path);
            let normalized = normalize_relative(relative);
            let comps = path_components(&normalized);
            // Drop the synthetic wrap folder name and any leading `mods/`/`mod/` segment
            // so `mods/CameraFix.dll` and `mods/CameraFix/config.ini` land directly under `Game/mods/`.
            let unwrapped: PathBuf = if comps.len() > 1 {
                comps.into_iter().skip(1).collect()
            } else {
                normalized
            };
            let stripped = strip_mod_overlay_segments(&unwrapped);
            if stripped.as_os_str().is_empty() {
                return root.join("mods");
            }
            return root.join("mods").join(stripped);
        }
    }
    resolve_modern_deploy(install_path, relative)
}

fn should_deploy_modern_file(relative: &Path, ctx: &DeployContext<'_>) -> bool {
    if let Some(content) = ctx.content_root {
        if looks_like_elden_mod_loader(content) {
            return false;
        }
    }
    if let Some(leaf) = relative.file_name().and_then(|n| n.to_str()) {
        if leaf.eq_ignore_ascii_case("dinput8.dll")
            || leaf.eq_ignore_ascii_case("modengine.ini")
            || leaf.eq_ignore_ascii_case("mod_loader_config.ini")
            || leaf.eq_ignore_ascii_case("anti_cheat_toggler_config.ini")
            || leaf.eq_ignore_ascii_case("_winhttp.dll")
            || leaf.eq_ignore_ascii_case("toggle_anti_cheat.exe")
        {
            return false;
        }
    }
    true
}

fn modern_staging_deploy_warnings(content_root: &Path, mod_name: &str) -> Vec<String> {
    if looks_like_elden_mod_loader(content_root) {
        return vec![format!(
            "{mod_name} (Elden Mod Loader) is superseded by Mod Engine 3 (me3); skipped deploying dinput8.dll. All DLL mods are loaded natively via me3 [[natives]]."
        )];
    }
    Vec::new()
}

/// Lighting Engine / Seamless Co-op style packs must land next to the exe, not in `mod/`.
pub fn looks_like_fromsoft_root_injector_pack(content_root: &Path) -> bool {
    if pack_has_root_injector_markers(content_root) {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(content_root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && pack_has_root_injector_markers(&path) {
            return true;
        }
    }
    false
}

fn pack_has_root_injector_markers(dir: &Path) -> bool {
    for name in ROOT_INJECTOR_MARKER_FILES {
        if find_file_ci(dir, name).is_some() {
            return true;
        }
    }
    find_child_dir_ci(dir, "SeamlessCoop").is_some()
}

fn strip_leading_game_segment(relative: &Path) -> PathBuf {
    let comps = path_components(relative);
    if comps
        .first()
        .is_some_and(|c| c.eq_ignore_ascii_case("Game"))
    {
        return comps.into_iter().skip(1).collect();
    }
    comps.into_iter().collect()
}

fn resolve_legacy_deploy_ctx(
    install_path: &Path,
    relative: &Path,
    ctx: &DeployContext<'_>,
) -> PathBuf {
    if let Some(content) = ctx.content_root {
        if looks_like_fromsoft_root_injector_pack(content) {
            let root = resolve_game_root(install_path);
            let normalized = normalize_relative(relative);
            let stripped = strip_leading_game_segment(&normalized);
            if stripped.as_os_str().is_empty() {
                return root;
            }
            return root.join(stripped);
        }
    }
    resolve_legacy_deploy(install_path, relative)
}

fn should_deploy_legacy_file(relative: &Path, ctx: &DeployContext<'_>) -> bool {
    let Some(content) = ctx.content_root else {
        return true;
    };
    if !looks_like_fromsoft_root_injector_pack(content) {
        return true;
    }
    let Some(ext) = relative.extension().and_then(|e| e.to_str()) else {
        return true;
    };
    !ROOT_INJECTOR_SKIP_EXTENSIONS
        .iter()
        .any(|skip| ext.eq_ignore_ascii_case(skip))
}

fn legacy_preflight_warnings(plugin_id: &str, install_path: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let root = resolve_game_root(install_path);
    let has_injectors = plugin_id == "darksouls2" && game_has_root_injector_pack(&root);
    let needs_me_assets = plugin_id == "darksouls2" && game_has_me_overlay_assets(&root);

    if plugin_id == "darksouls2" && has_injectors && !needs_me_assets {
        // Injector-only layout: Mod Engine is optional / often harmful — do not nag.
        warnings.extend(dinput8_override_warnings(install_path));
    } else if !legacy_modengine_present(install_path) {
        warnings.push(missing_modengine_message(plugin_id));
    } else {
        warnings.extend(dinput8_override_warnings(install_path));
    }
    if plugin_id == "darksouls2" {
        if find_file_ci(&root, "DarkSoulsII.exe").is_none() {
            warnings.push(
                "DarkSoulsII.exe was not found under the resolved Game folder. Check that the managed install path points at the Steam game root (Scholar of the First Sin)."
                    .into(),
            );
        }
        if has_injectors {
            warnings.extend(ds2_preflight_injector_hints(&root));
        }
    }
    warnings
}

fn ds2_preflight_injector_hints(game_root: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    if find_file_ci(game_root, "dxgi.dll").is_some() {
        warnings.push(
            "Lighting Engine loads via dxgi.dll — open its menu with F1 in-game, or check \
             DS2LE.log next to the exe if visuals look wrong or the game crashes on load."
                .into(),
        );
    }
    if find_file_ci(game_root, "ds2sc_launcher.exe").is_some() {
        warnings.push(
            "Seamless Co-op requires Steam Launch Options that rewrite DarkSoulsII.exe to \
             ds2sc_launcher.exe (Emperor sets this on deploy)."
                .into(),
        );
        if empty_seamless_cooppassword(game_root) {
            warnings.push(
                "Seamless Co-op cooppassword is empty in SeamlessCoop/ds2sc_settings.ini.".into(),
            );
        }
    }
    warnings
}

fn ds2_log_sources(install_path: &Path) -> Vec<super::LogSource> {
    let root = resolve_game_root(install_path);
    let path = find_file_ci(&root, "DS2LE.log").unwrap_or_else(|| root.join("DS2LE.log"));
    vec![super::LogSource {
        label: "DS2LE".into(),
        kind: super::LogKind::Ds2Lighting,
        path,
    }]
}

fn missing_modengine_message(plugin_id: &str) -> String {
    if plugin_id == "darksouls2" {
        "Legacy Mod Engine (dinput8.dll) was not found next to the game executable. Deploy again to auto-install it from GitHub when you enable loose asset mods under Game/mod/, or copy dinput8.dll into the Game folder manually."
            .into()
    } else {
        "Legacy Mod Engine (dinput8.dll) was not found next to the game executable. Deploy again with a Nexus API key (Premium one-click, or free Download Assist) to auto-install it, or copy Mod Engine into the Game folder manually."
            .into()
    }
}

fn modern_preflight_warnings(install_path: &Path) -> Vec<String> {
    let root = resolve_game_root(install_path);
    let mut warnings = Vec::new();
    if find_file_ci(&root, "dinput8.dll").is_some() {
        warnings.push(
            "Found dinput8.dll (Elden Mod Loader / legacy Mod Engine) in the Game folder. Remove it if the game crashes or DLL mods fail to load under Mod Engine 3 (me3)."
                .into(),
        );
    }
    if let Some(ini_path) = empty_er_seamless_cooppassword(&root) {
        warnings.push(format!(
            "Seamless Co-op cooppassword is empty in {} — set a password before launching or Seamless Co-op will refuse to start.",
            ini_path.display()
        ));
    }
    if !root.join("emperor.me3").is_file() {
        warnings.push(
            "Mod Engine 3 profile (emperor.me3) was not found in the Game folder. Deploy again before launching."
                .into(),
        );
    }
    warnings
}

/// Ensure Mod Engine loads the `mod/` overlay (`useModOverrideDirectory` + path).
pub fn write_modengine_ini(game_root: &Path) -> Result<()> {
    std::fs::create_dir_all(game_root)
        .with_context(|| format!("create {}", game_root.display()))?;
    let path =
        find_file_ci(game_root, "modengine.ini").unwrap_or_else(|| game_root.join("modengine.ini"));
    let existing = if path.is_file() {
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };
    let updated = upsert_legacy_modengine_ini(&existing);
    std::fs::write(&path, updated).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Stub / fallback ini body when no upstream `modengine.ini` is available.
pub fn legacy_modengine_stub_ini() -> &'static str {
    "; Generated by Emperor Mod Manager.\nuseModOverrideDirectory=1\nmodOverrideDirectory=\"\\mod\"\n"
}

/// Guarantee `useModOverrideDirectory=1` and `modOverrideDirectory="\mod"`.
pub fn upsert_legacy_modengine_ini(contents: &str) -> String {
    const USE_LINE: &str = "useModOverrideDirectory=1";
    const DIR_LINE: &str = "modOverrideDirectory=\"\\mod\"";
    let mut found_use = false;
    let mut found_dir = false;
    let mut out = String::new();
    for line in contents.lines() {
        let lower = line.trim().to_ascii_lowercase();
        if lower.starts_with("usemodoverridedirectory") {
            out.push_str(USE_LINE);
            out.push('\n');
            found_use = true;
        } else if lower.starts_with("modoverridedirectory") {
            out.push_str(DIR_LINE);
            out.push('\n');
            found_dir = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if contents.is_empty() {
        out.push_str("; Generated by Emperor Mod Manager.\n");
    } else if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !found_use {
        out.push_str(USE_LINE);
        out.push('\n');
    }
    if !found_dir {
        out.push_str(DIR_LINE);
        out.push('\n');
    }
    out
}

fn escape_toml_basic(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// True when a deployed mod folder contains FromSoft asset overrides for `[[packages]]`.
pub fn mod_folder_has_assets(mod_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(mod_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if path.is_dir() {
            if FROMSOFT_ASSET_DIRS
                .iter()
                .any(|dir| name.eq_ignore_ascii_case(dir))
            {
                return true;
            }
        } else if path.is_file() {
            if name.eq_ignore_ascii_case("regulation.bin") {
                return true;
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if FROMSOFT_ASSET_EXTENSIONS
                    .iter()
                    .any(|allowed| ext.eq_ignore_ascii_case(allowed))
                {
                    return true;
                }
            }
        }
    }
    false
}

fn path_has_dependency_dir(rel_within_mod: &Path) -> bool {
    let Some(parent) = rel_within_mod.parent() else {
        return false;
    };
    parent.components().any(|c| match c {
        Component::Normal(seg) => seg.to_str().is_some_and(|s| {
            NATIVE_DEPENDENCY_DIRS
                .iter()
                .any(|dep| s.eq_ignore_ascii_case(dep))
        }),
        _ => false,
    })
}

fn collect_native_dlls(game_root: &Path, enabled_mod_folders: &[String]) -> Vec<String> {
    let mut natives: Vec<String> = Vec::new();
    for folder in enabled_mod_folders {
        let trimmed = folder.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Flat DLL mods deploy directly under `Game/mods/<Mod>.dll` and pass their DLL filename.
        if trimmed.to_ascii_lowercase().ends_with(".dll") {
            let direct = game_root.join("mods").join(trimmed);
            if direct.is_file() && !is_non_mod_native_dll(trimmed) {
                let rel_str = format!("mods/{}", trimmed.replace('\\', "/"));
                if !natives.iter().any(|existing| existing.eq_ignore_ascii_case(&rel_str)) {
                    natives.push(rel_str);
                }
            }
            continue;
        }
        let mod_dir = game_root.join("mods").join(trimmed);
        if !mod_dir.is_dir() {
            continue;
        }
        let mut candidates: Vec<(usize, String)> = Vec::new();
        for entry in WalkDir::new(&mod_dir)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".dll") {
                continue;
            }
            if is_non_mod_native_dll(name) {
                continue;
            }
            let Ok(rel_in_mod) = path.strip_prefix(&mod_dir) else {
                continue;
            };
            if path_has_dependency_dir(rel_in_mod) {
                continue;
            }
            let Ok(rel) = path.strip_prefix(game_root) else {
                continue;
            };
            let rel_str = normalize_relative(rel).to_string_lossy().replace('\\', "/");
            if !rel_str.is_empty() {
                candidates.push((entry.depth(), rel_str));
            }
        }
        // Only register entry-point DLLs at the shallowest depth inside this mod folder
        // (e.g. ERSS-FG.dll at depth 1 ignores nested helper/SDK DLLs, while SeamlessCoop/ersc.dll
        // at depth 2 is picked when no depth-1 DLL exists).
        let Some(min_depth) = candidates.iter().map(|(d, _)| *d).min() else {
            continue;
        };
        let mut mod_dlls: Vec<String> = candidates
            .into_iter()
            .filter(|(d, _)| *d == min_depth)
            .map(|(_, path)| path)
            .collect();
        // Sort deterministically within each mod folder while preserving user load order across mods.
        mod_dlls.sort();
        for dll in mod_dlls {
            if !natives.iter().any(|existing| existing == &dll) {
                natives.push(dll);
            }
        }
    }
    natives
}

/// Write a me3 profile listing enabled asset packages and native DLLs in load order.
pub fn write_me3_profile(
    game_root: &Path,
    me3_game: &str,
    enabled_mod_folders: &[String],
) -> Result<()> {
    let mut body = String::from(
        "# Auto-generated by Emperor Mod Manager.\nprofileVersion = \"v1\"\n\n[[supports]]\ngame = \"",
    );
    body.push_str(&escape_toml_basic(me3_game));
    body.push_str("\"\n");

    let mut seen_packages: Vec<String> = Vec::new();
    for folder in enabled_mod_folders {
        let trimmed = folder.trim();
        if trimmed.is_empty() || seen_packages.iter().any(|p| p == trimmed) {
            continue;
        }
        let mod_dir = game_root.join("mods").join(trimmed);
        if !mod_folder_has_assets(&mod_dir) {
            continue;
        }
        seen_packages.push(trimmed.to_string());
        body.push_str("\n[[packages]]\nid = \"");
        body.push_str(&escape_toml_basic(trimmed));
        body.push_str("\"\npath = \"mods/");
        body.push_str(&escape_toml_basic(trimmed));
        body.push_str("\"\n");
    }

    for native in collect_native_dlls(game_root, enabled_mod_folders) {
        body.push_str("\n[[natives]]\npath = \"");
        body.push_str(&escape_toml_basic(&native));
        body.push_str("\"\n");
    }

    let path = game_root.join("emperor.me3");
    std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn legacy_after_deploy(plugin_id: &str, install_path: &Path) -> Result<Vec<String>> {
    let root = resolve_game_root(install_path);
    let mut warnings = Vec::new();

    if plugin_id == "darksouls2" {
        warnings.extend(reconcile_ds2_modengine(plugin_id, install_path)?);
        // DLL overrides first, then Seamless rewrite (so sed is written last / preserved).
        warnings.extend(ensure_fromsoft_proton_overrides(install_path));
        warnings.extend(ensure_seamless_launch(install_path));
        warnings.extend(ds2_combo_warnings(install_path, &root));
    } else {
        if legacy_modengine_present(install_path) {
            write_modengine_ini(&root)?;
        }
        warnings.extend(ensure_fromsoft_proton_overrides(install_path));
    }

    Ok(warnings)
}

/// True when `Game/mod/` has deployed loose assets (not just an empty overlay).
pub fn game_has_me_overlay_assets(game_root: &Path) -> bool {
    let mod_dir = game_root.join("mod");
    if !mod_dir.is_dir() {
        return false;
    }
    WalkDir::new(&mod_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .any(|e| e.file_type().is_file())
}

/// Lighting Engine / Seamless markers at the game root.
pub fn game_has_root_injector_pack(game_root: &Path) -> bool {
    find_file_ci(game_root, "dxgi.dll").is_some()
        || find_file_ci(game_root, "ds2sc_launcher.exe").is_some()
        || find_child_dir_ci(game_root, "SeamlessCoop").is_some()
}

fn reconcile_ds2_modengine(plugin_id: &str, install_path: &Path) -> Result<Vec<String>> {
    let root = resolve_game_root(install_path);
    let needs_me = game_has_me_overlay_assets(&root);
    let has_injectors = game_has_root_injector_pack(&root);
    let mut warnings = Vec::new();

    if needs_me {
        if legacy_modengine_present(install_path) {
            write_modengine_ini(&root)?;
        }
    } else if has_injectors {
        warnings.extend(remove_emperor_modengine_if_safe(plugin_id, &root));
    } else {
        // Vanilla-ish deploy: leave any existing ME alone; do not auto-install.
    }
    Ok(warnings)
}

fn remove_emperor_modengine_if_safe(plugin_id: &str, game_root: &Path) -> Vec<String> {
    let Ok(paths) = crate::config::Paths::resolve() else {
        return Vec::new();
    };
    let cache = paths.fromsoft_legacy_modengine_dir(plugin_id);
    remove_emperor_modengine_files(game_root, &cache)
}

/// Remove Vestiges/Emperor Mod Engine injectors when they match the tools cache or stub.
pub fn remove_emperor_modengine_files(game_root: &Path, cache: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let dll = game_root.join("dinput8.dll");
    let ini =
        find_file_ci(game_root, "modengine.ini").unwrap_or_else(|| game_root.join("modengine.ini"));

    let dll_matches_cache = dll.is_file()
        && find_file_ci(cache, "dinput8.dll")
            .map(|c| files_same_len(&dll, &c))
            .unwrap_or(false);
    let ini_text = if ini.is_file() {
        std::fs::read_to_string(&ini).unwrap_or_default()
    } else {
        String::new()
    };
    let ini_lower = ini_text.to_ascii_lowercase();
    let ini_is_ours = ini.is_file()
        && (ini_lower.contains("generated by emperor mod manager")
            || ini_lower.contains("cached by emperor mod manager")
            || (find_file_ci(cache, "modengine.ini")
                .map(|c| files_same_len(&ini, &c))
                .unwrap_or(false)
                && (ini_lower.contains("blocknetworkaccess")
                    || ini_lower.contains("usemodoverridedirectory"))));

    // Safe to remove ME dll when it matches our cache, or when paired with our stub ini.
    let dll_is_ours = dll_matches_cache || (dll.is_file() && ini_is_ours);

    if dll_is_ours {
        match std::fs::remove_file(&dll) {
            Ok(()) => warnings.push(
                "Removed auto-installed Mod Engine dinput8.dll so it does not conflict with \
                 Lighting Engine (dxgi.dll) / Seamless Co-op. Loose asset mods in Game/mod/ \
                 will reinstall it on the next deploy."
                    .into(),
            ),
            Err(e) => warnings.push(format!("Could not remove conflicting dinput8.dll: {e}")),
        }
    }
    if ini_is_ours {
        let _ = std::fs::remove_file(&ini);
    }
    warnings
}

fn files_same_len(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => ma.len() == mb.len(),
        _ => false,
    }
}

const SEAMLESS_SED: &str =
    r#"echo "%command%" | sed 's/DarkSoulsII\.exe/ds2sc_launcher.exe/' | sh"#;

/// True when launch options rewrite Steam's exe to the Seamless launcher.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn launch_options_have_seamless_rewrite(opts: &str) -> bool {
    let lower = opts.to_lowercase();
    lower.contains("ds2sc_launcher.exe")
        && (lower.contains("sed ") || lower.contains("launch_emperor_seamless"))
}

pub fn empty_seamless_cooppassword(game_root: &Path) -> bool {
    let ini = game_root.join("SeamlessCoop").join("ds2sc_settings.ini");
    let Ok(text) = std::fs::read_to_string(&ini) else {
        return false;
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.to_ascii_lowercase().starts_with("cooppassword") {
            if let Some((_, val)) = trimmed.split_once('=') {
                return val.trim().is_empty();
            }
            return true;
        }
    }
    false
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn ensure_seamless_launch_options(existing: &str) -> String {
    if launch_options_have_seamless_rewrite(existing) {
        return existing.to_string();
    }
    let trimmed = existing.trim();
    // Drop a trailing %command% — the sed pipeline consumes Steam's command.
    let base = trimmed
        .strip_suffix("%command%")
        .map(str::trim)
        .unwrap_or(trimmed);
    if base.is_empty() {
        SEAMLESS_SED.to_string()
    } else {
        format!("{base} {SEAMLESS_SED}")
    }
}

fn ensure_seamless_launch(install_path: &Path) -> Vec<String> {
    let root = resolve_game_root(install_path);
    if find_file_ci(&root, "ds2sc_launcher.exe").is_none() {
        return Vec::new();
    }

    let helper = root.join("launch_emperor_seamless.sh");
    let helper_body = format!(
        "#!/bin/sh\n# Seamless Co-op must start ds2sc_launcher.exe, not DarkSoulsII.exe.\n\
         # Steam Launch Options (recommended):\n# {SEAMLESS_SED}\n\
         echo \"Use Steam Launch Options to rewrite DarkSoulsII.exe -> ds2sc_launcher.exe\"\n"
    );
    let _ = std::fs::write(&helper, helper_body);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&helper) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&helper, perms);
        }
    }

    let mut warnings = Vec::new();
    if empty_seamless_cooppassword(&root) {
        warnings.push(
            "Seamless Co-op cooppassword is empty in SeamlessCoop/ds2sc_settings.ini — \
             set the same password as your co-op partners before connecting."
                .into(),
        );
    }

    #[cfg(target_os = "linux")]
    {
        use crate::tools::steam_launch::{read_launch_options, write_launch_options};
        let Some(app_id) =
            crate::tools::executables::steam_app_id_from_install(&install_path.to_string_lossy())
        else {
            warnings.push(
                "Seamless Co-op is installed. Set Steam Launch Options to: \
                 echo \"%command%\" | sed 's/DarkSoulsII\\.exe/ds2sc_launcher.exe/' | sh"
                    .into(),
            );
            return warnings;
        };
        let existing = read_launch_options(&app_id)
            .ok()
            .flatten()
            .unwrap_or_default();
        if launch_options_have_seamless_rewrite(&existing) {
            return warnings;
        }
        let repaired = ensure_seamless_launch_options(&existing);
        match write_launch_options(&app_id, &repaired) {
            Ok(()) => warnings.push(
                "Updated Steam Launch Options to start ds2sc_launcher.exe (Seamless Co-op). \
                 Restart Steam if the game was already open."
                    .into(),
            ),
            Err(e) => warnings.push(format!(
                "Seamless Co-op needs Launch Options that rewrite DarkSoulsII.exe to \
                 ds2sc_launcher.exe. Could not write them automatically ({e:#})."
            )),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        warnings.push(
            "Seamless Co-op is installed — launch ds2sc_launcher.exe (not the vanilla game shortcut)."
                .into(),
        );
    }
    warnings
}

fn ds2_combo_warnings(install_path: &Path, game_root: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let has_le = find_file_ci(game_root, "dxgi.dll").is_some();
    let has_seamless = find_file_ci(game_root, "ds2sc_launcher.exe").is_some()
        || find_child_dir_ci(game_root, "SeamlessCoop").is_some();
    if has_le {
        warnings.push(
            "Lighting Engine loads via dxgi.dll — open its menu with F1 in-game, or check \
             DS2LE.log next to the exe if visuals look wrong or the game crashes on load."
                .into(),
        );
    }
    if has_le && has_seamless {
        warnings.push(
            "Lighting Engine + Seamless Co-op together can be unstable on Proton. If the game \
             crashes on the loading screen, try deploying with only one of them enabled."
                .into(),
        );
    }
    #[cfg(target_os = "linux")]
    if has_le {
        use crate::tools::steam_launch::read_launch_options;
        if let Some(app_id) =
            crate::tools::executables::steam_app_id_from_install(&install_path.to_string_lossy())
        {
            let opts = read_launch_options(&app_id)
                .ok()
                .flatten()
                .unwrap_or_default();
            let upper = opts.to_ascii_uppercase();
            if upper.contains("ENABLE_LSFG") {
                warnings.push(
                    "ENABLE_LSFG with Lighting Engine at high resolutions is heavy — if load \
                     crashes continue, try disabling LSFG for this game."
                        .into(),
                );
            }
            if upper.contains("PROTON_ENABLE_HDR") {
                warnings.push(
                    "PROTON_ENABLE_HDR with Lighting Engine can be unstable — if load crashes \
                     continue, try turning HDR off for this game."
                        .into(),
                );
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = install_path;
    }
    warnings
}

/// Native DLL names Proton must load from the Game folder.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn needed_fromsoft_native_dlls(game_root: &Path) -> Vec<&'static str> {
    let mut dlls = Vec::new();
    if find_file_ci(game_root, "dinput8.dll").is_some() {
        dlls.push("dinput8");
    }
    if find_file_ci(game_root, "dxgi.dll").is_some() {
        dlls.push("dxgi");
        if find_file_ci(game_root, "d3d11.dll").is_some() {
            dlls.push("d3d11");
        }
    }
    dlls
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn winedll_has_native(opts: &str, dll: &str) -> bool {
    let lower = opts.to_lowercase();
    let dll = dll.to_ascii_lowercase();
    let needle = format!("{dll}=");
    if lower.contains(&needle) {
        if let Some(rest) = lower.split(&needle).nth(1) {
            let token = rest.split_whitespace().next().unwrap_or("");
            return token.starts_with('n') || token.contains(",n") || token.starts_with("n,");
        }
    }
    if let Some(idx) = lower.find("winedlloverrides") {
        let slice = &lower[idx..];
        if let Some(eq) = slice.find('=') {
            let value = &slice[eq + 1..];
            let value = value.trim_start_matches(|c| c == '"' || c == '\\');
            for part in value.split_whitespace() {
                let part = part.trim_matches('"');
                if let Some((names, order)) = part.split_once('=') {
                    let names = names.trim_matches('\\').trim_matches('"');
                    if names.split(',').any(|n| n.trim() == dll)
                        && order.split(',').any(|o| o.trim() == "n")
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// True when launch options contain a valid native `dinput8` Wine override.
#[cfg(test)]
pub fn launch_options_have_dinput8_native(opts: &str) -> bool {
    winedll_has_native(opts, "dinput8")
}

/// Fix `dinput8,n,b` typo and/or ensure a proper dinput8 native override.
#[cfg(test)]
pub fn repair_dinput8_override(existing: &str) -> String {
    merge_fromsoft_winedll_overrides(existing, &["dinput8"])
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn strip_seamless_rewrite(opts: &str) -> (String, bool) {
    if !launch_options_have_seamless_rewrite(opts) {
        return (opts.to_string(), false);
    }
    // Drop from `echo "%command%"` (or single-quoted) through end — the sed pipeline.
    let lower = opts.to_lowercase();
    if let Some(idx) = lower.find("echo \"%command%\"") {
        return (opts[..idx].trim().to_string(), true);
    }
    if let Some(idx) = lower.find("echo '%command%'") {
        return (opts[..idx].trim().to_string(), true);
    }
    if let Some(idx) = lower.find("launch_emperor_seamless") {
        return (opts[..idx].trim().to_string(), true);
    }
    (opts.to_string(), true)
}

/// Strip `WINEDLLOVERRIDES=…` tokens (quoted or bare) and bare `%command%` placeholders.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn strip_winedll_tokens(opts: &str) -> (String, bool) {
    let mut out = String::new();
    let mut had_command = false;
    let bytes = opts.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            i += 1;
            continue;
        }
        let rest = &opts[i..];
        let rest_upper = rest.to_ascii_uppercase();
        if rest_upper.starts_with("WINEDLLOVERRIDES=") {
            i += "WINEDLLOVERRIDES=".len();
            if i < bytes.len() && bytes[i] == b'"' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(bytes.len());
                        continue;
                    }
                    if bytes[i] == b'"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            } else {
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            continue;
        }
        if rest.starts_with("%command%") {
            i += "%command%".len();
            had_command = true;
            continue;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        out.push_str(&opts[start..i]);
    }
    (out.trim().to_string(), had_command)
}

/// Merge required native DLL overrides into existing Steam launch options.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn merge_fromsoft_winedll_overrides(existing: &str, dlls: &[&str]) -> String {
    if dlls.is_empty() {
        return existing.to_string();
    }
    let mut opts = existing.to_string();
    for dll in dlls {
        let typo = format!("{dll},n,b");
        opts = opts.replace(&typo, &format!("{dll}=n,b"));
        opts = opts.replace(&typo.to_ascii_uppercase(), &format!("{dll}=n,b"));
    }
    if dlls.iter().all(|d| winedll_has_native(&opts, d)) {
        return opts;
    }

    let (without_seamless, had_seamless) = strip_seamless_rewrite(&opts);
    let (env_rest, had_command) = strip_winedll_tokens(&without_seamless);

    let mut union: Vec<String> = dlls.iter().map(|d| (*d).to_string()).collect();
    for dll in ["winmm", "version"] {
        if winedll_has_native(existing, dll) && !union.iter().any(|u| u == dll) {
            union.push(dll.to_string());
        }
    }
    // Preserve order: dinput8, dxgi, d3d11, then extras.
    let preferred = ["dinput8", "dxgi", "d3d11", "winmm", "version"];
    union.sort_by_key(|d| {
        preferred
            .iter()
            .position(|p| p == d)
            .unwrap_or(preferred.len())
    });
    union.dedup();

    let override_token = format!("WINEDLLOVERRIDES={}=n,b", union.join(","));
    let mut parts = Vec::new();
    if !env_rest.is_empty() {
        parts.push(env_rest);
    }
    parts.push(override_token);
    if had_seamless {
        parts.push(SEAMLESS_SED.to_string());
    } else if had_command || existing.contains("%command%") || existing.trim().is_empty() {
        parts.push("%command%".into());
    } else {
        parts.push("%command%".into());
    }
    parts.join(" ")
}

/// Repair / write Steam launch options for whatever injectors are on disk.
fn ensure_fromsoft_proton_overrides(install_path: &Path) -> Vec<String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = install_path;
        Vec::new()
    }
    #[cfg(target_os = "linux")]
    {
        use crate::tools::steam_launch::{read_launch_options, write_launch_options};

        let root = resolve_game_root(install_path);
        let needed = needed_fromsoft_native_dlls(&root);
        if needed.is_empty() {
            return Vec::new();
        }
        let Some(app_id) =
            crate::tools::executables::steam_app_id_from_install(&install_path.to_string_lossy())
        else {
            return vec![format!(
                "Could not resolve Steam app id. Add WINEDLLOVERRIDES={}=n,b to launch options.",
                needed.join(",")
            )];
        };
        let existing = match read_launch_options(&app_id) {
            Ok(v) => v.unwrap_or_default(),
            Err(e) => {
                return vec![format!(
                    "Could not read Steam launch options ({e:#}). Set \
                     WINEDLLOVERRIDES={}=n,b manually.",
                    needed.join(",")
                )];
            }
        };
        if needed.iter().all(|d| winedll_has_native(&existing, d)) {
            return Vec::new();
        }
        let repaired = merge_fromsoft_winedll_overrides(&existing, &needed);
        // Preserve seamless rewrite if merge dropped it.
        let repaired = if launch_options_have_seamless_rewrite(&existing)
            && !launch_options_have_seamless_rewrite(&repaired)
        {
            ensure_seamless_launch_options(&repaired)
        } else {
            repaired
        };
        match write_launch_options(&app_id, &repaired) {
            Ok(()) => vec![format!(
                "Updated Steam Launch Options WINEDLLOVERRIDES={}=n,b for Proton. \
                 Restart Steam if the game was already open.",
                needed.join(",")
            )],
            Err(e) => vec![format!(
                "Steam Launch Options need WINEDLLOVERRIDES={}=n,b. \
                 Could not write them automatically ({e:#}).",
                needed.join(",")
            )],
        }
    }
}

fn dinput8_override_warnings(install_path: &Path) -> Vec<String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = install_path;
        Vec::new()
    }
    #[cfg(target_os = "linux")]
    {
        use crate::tools::steam_launch::read_launch_options;

        let root = resolve_game_root(install_path);
        let needed = needed_fromsoft_native_dlls(&root);
        if needed.is_empty() {
            return Vec::new();
        }
        let Some(app_id) =
            crate::tools::executables::steam_app_id_from_install(&install_path.to_string_lossy())
        else {
            return vec![format!(
                "Proton needs WINEDLLOVERRIDES={}=n,b in Steam launch options.",
                needed.join(",")
            )];
        };
        let existing = read_launch_options(&app_id)
            .ok()
            .flatten()
            .unwrap_or_default();
        if needed.iter().all(|d| winedll_has_native(&existing, d)) {
            return Vec::new();
        }
        vec![format!(
            "Proton needs WINEDLLOVERRIDES={}=n,b in Steam launch options or injectors never load.",
            needed.join(",")
        )]
    }
}

fn ini_has_empty_cooppassword(ini: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(ini) else {
        return false;
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.to_ascii_lowercase().starts_with("cooppassword") {
            if let Some((_, val)) = trimmed.split_once('=') {
                return val.trim().is_empty();
            }
            return true;
        }
    }
    false
}

/// Find an Elden Ring Seamless Co-op `ersc_settings.ini` whose `cooppassword` is empty.
pub fn empty_er_seamless_cooppassword(game_root: &Path) -> Option<PathBuf> {
    if let Some(sc_dir) = find_child_dir_ci(game_root, "SeamlessCoop") {
        if let Some(ini) = find_file_ci(&sc_dir, "ersc_settings.ini") {
            if ini_has_empty_cooppassword(&ini) {
                return Some(ini);
            }
        }
    }
    let mods_dir = game_root.join("mods");
    let Ok(entries) = std::fs::read_dir(&mods_dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let mod_dir = entry.path();
        if !mod_dir.is_dir() {
            continue;
        }
        if let Some(sc_dir) = find_child_dir_ci(&mod_dir, "SeamlessCoop") {
            if let Some(ini) = find_file_ci(&sc_dir, "ersc_settings.ini") {
                if ini_has_empty_cooppassword(&ini) {
                    return Some(ini);
                }
            }
        }
        if let Some(ini) = find_file_ci(&mod_dir, "ersc_settings.ini") {
            if ini_has_empty_cooppassword(&ini) {
                return Some(ini);
            }
        }
    }
    None
}

fn modern_after_deploy(
    install_path: &Path,
    me3_game: &str,
    enabled_mod_folders: &[String],
) -> Result<Vec<String>> {
    let root = resolve_game_root(install_path);
    write_me3_profile(&root, me3_game, enabled_mod_folders)?;
    let mut warnings = Vec::new();
    if let Some(ini_path) = empty_er_seamless_cooppassword(&root) {
        warnings.push(format!(
            "Seamless Co-op cooppassword is empty in {} — set the same password as your co-op partners before launching.",
            ini_path.display()
        ));
    }
    Ok(warnings)
}

fn modern_on_revert(install_path: &Path) -> Result<()> {
    let root = resolve_game_root(install_path);
    for name in [
        "emperor.me3",
        "config_emperor.toml",
        "launch_emperor_me3.sh",
        "launch_emperor_me3.bat",
        "launch_emperor_me2.bat",
    ] {
        let path = root.join(name);
        if path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

fn modern_log_sources(install_path: &Path) -> Vec<super::LogSource> {
    let root = resolve_game_root(install_path);
    let mut sources = vec![super::LogSource {
        label: "Mod Engine 3 (me3)".into(),
        kind: super::LogKind::Me3,
        path: root.join("emperor_me3.log"),
    }];

    // Also include the newest me3 host log if present in me3's standard data directory.
    let me3_logs_dir = me3_standard_logs_dir();
    if let Some(dir) = me3_logs_dir.filter(|d| d.is_dir()) {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut logs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("log"))
                })
                .collect();
            logs.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
            if let Some(newest) = logs.pop() {
                sources.push(super::LogSource {
                    label: "me3 host log".into(),
                    kind: super::LogKind::Me3,
                    path: newest,
                });
            }
        }
    }

    sources
}

fn me3_standard_logs_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|d| d.join("garyttierney").join("me3").join("data").join("logs"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|h| h.join(".local").join("share"))
            })?;
        let direct = base.join("me3").join("logs");
        if direct.is_dir() {
            return Some(direct);
        }
        Some(base.join("garyttierney").join("me3").join("logs"))
    }
}

macro_rules! fromsoft_legacy_plugin {
    ($struct:ident, $id:expr, $display:expr, $domain:expr, $matches:expr) => {
        pub struct $struct;

        impl GamePlugin for $struct {
            fn info(&self) -> GamePluginInfo {
                GamePluginInfo {
                    id: $id,
                    display_name: $display,
                    nexus_domain: $domain,
                    match_names: $matches,
                }
            }

            fn prefers_mod_folder(&self) -> bool {
                false
            }

            fn preserve_staging_root_names(&self) -> &[&str] {
                FROMSOFT_ROOT_DIRS
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                Ok(resolve_legacy_deploy(install_path, relative))
            }

            fn resolve_deploy_root_ctx(
                &self,
                install_path: &Path,
                relative: &Path,
                ctx: &DeployContext<'_>,
            ) -> Result<PathBuf> {
                Ok(resolve_legacy_deploy_ctx(install_path, relative, ctx))
            }

            fn should_deploy_file_ctx(&self, relative: &Path, ctx: &DeployContext<'_>) -> bool {
                should_deploy_legacy_file(relative, ctx)
            }

            fn prunable_dirs(&self, install_path: &Path) -> Vec<PathBuf> {
                let root = resolve_game_root(install_path);
                vec![root.join("mod"), root.join("mods")]
            }

            fn preflight_warnings(&self, install_path: &Path) -> Vec<String> {
                legacy_preflight_warnings($id, install_path)
            }

            fn load_log_sources(&self, install_path: &Path) -> Vec<super::LogSource> {
                if $id == "darksouls2" {
                    ds2_log_sources(install_path)
                } else {
                    Vec::new()
                }
            }

            fn after_deploy(
                &self,
                install_path: &Path,
                _enabled_mod_folders: &[String],
            ) -> Result<Vec<String>> {
                legacy_after_deploy($id, install_path)
            }

            fn has_plugin_tools(&self) -> bool {
                crate::games::fromsoft_loaders::has_plugin_tools($id)
            }

            fn plugin_tools_status(
                &self,
                paths: &crate::config::Paths,
                _game_id: &str,
                install_path: &Path,
            ) -> Vec<super::PluginToolStatus> {
                crate::games::fromsoft_loaders::tools_status(paths, $id, install_path)
            }

            fn check_plugin_tool_updates<'a>(
                &'a self,
                paths: &'a crate::config::Paths,
                _game_id: &'a str,
                install_path: &'a Path,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Vec<super::PluginToolStatus>>> + Send + 'a>,
            > {
                Box::pin(crate::games::fromsoft_loaders::check_tool_updates(
                    paths,
                    $id,
                    install_path,
                ))
            }

            fn install_plugin_tool_ctx<'a>(
                &'a self,
                ctx: &'a super::PluginToolInstallContext<'_>,
                tool_id: &'a str,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<super::PluginToolInstallOutcome>>
                        + Send
                        + 'a,
                >,
            > {
                Box::pin(crate::games::fromsoft_loaders::install_tool_ctx(
                    ctx,
                    $id,
                    tool_id,
                ))
            }

            fn install_plugin_tool_from_archive(
                &self,
                paths: &crate::config::Paths,
                install_path: &Path,
                tool_id: &str,
                archive: &Path,
            ) -> Result<super::PluginToolActionResult> {
                crate::games::fromsoft_loaders::install_tool_from_archive(
                    paths,
                    $id,
                    install_path,
                    tool_id,
                    archive,
                )
            }

            fn tool_assist_ready(
                &self,
                _paths: &crate::config::Paths,
                install_path: &Path,
                tool_id: &str,
            ) -> bool {
                crate::games::fromsoft_loaders::tool_assist_ready($id, install_path, tool_id)
            }

            fn uninstall_plugin_tool<'a>(
                &'a self,
                paths: &'a crate::config::Paths,
                _game_id: &'a str,
                install_path: &'a Path,
                tool_id: &'a str,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<super::PluginToolActionResult>>
                        + Send
                        + 'a,
                >,
            > {
                let id = tool_id.to_string();
                Box::pin(async move {
                    crate::games::fromsoft_loaders::uninstall_tool(paths, $id, install_path, &id)
                })
            }

            fn recommended_plugin_tools(
                &self,
                paths: &crate::config::Paths,
                install_path: &Path,
                content_roots: &[&Path],
            ) -> Vec<super::PluginToolRecommendation> {
                let needs = legacy_content_needs_modengine($id, content_roots);
                crate::games::fromsoft_loaders::recommended_tools(paths, $id, install_path, needs)
            }
        }
    };
}

fn legacy_content_needs_modengine(plugin_id: &str, content_roots: &[&Path]) -> bool {
    if content_roots.is_empty() {
        return false;
    }
    for root in content_roots {
        if find_file_ci(root, "dinput8.dll").is_some() {
            return false;
        }
    }
    if plugin_id == "darksouls2" {
        return content_roots
            .iter()
            .any(|root| !game_has_root_injector_pack(root));
    }
    true
}

macro_rules! fromsoft_modern_plugin {
    ($struct:ident, $id:expr, $display:expr, $domain:expr, $me3_game:expr, $matches:expr) => {
        pub struct $struct;

        impl GamePlugin for $struct {
            fn info(&self) -> GamePluginInfo {
                GamePluginInfo {
                    id: $id,
                    display_name: $display,
                    nexus_domain: $domain,
                    match_names: $matches,
                }
            }

            fn prefers_mod_folder(&self) -> bool {
                true
            }

            fn preserve_staging_root_names(&self) -> &[&str] {
                FROMSOFT_ROOT_DIRS
            }

            fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
                content_folder_wrap_name(content_root, staged_name)
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                Ok(resolve_modern_deploy(install_path, relative))
            }

            fn resolve_deploy_root_ctx(
                &self,
                install_path: &Path,
                relative: &Path,
                ctx: &DeployContext<'_>,
            ) -> Result<PathBuf> {
                Ok(resolve_modern_deploy_ctx(install_path, relative, ctx))
            }

            fn should_deploy_file_ctx(&self, relative: &Path, ctx: &DeployContext<'_>) -> bool {
                should_deploy_modern_file(relative, ctx)
            }

            fn allows_empty_deploy(&self, content_root: &Path) -> bool {
                looks_like_elden_mod_loader(content_root)
            }

            fn staging_deploy_warnings(&self, content_root: &Path, mod_name: &str) -> Vec<String> {
                modern_staging_deploy_warnings(content_root, mod_name)
            }

            fn load_order_entries(&self, content_root: &Path, staged_name: &str) -> Vec<String> {
                if looks_like_elden_mod_loader(content_root)
                    || looks_like_modern_graphics_proxy_pack(content_root)
                {
                    return Vec::new();
                }
                let flat_dlls = flat_modern_dll_mod_files(content_root);
                if !flat_dlls.is_empty() {
                    return flat_dlls;
                }
                let name = self.wrap_mod_folder_name(content_root, staged_name);
                if name.is_empty() {
                    Vec::new()
                } else {
                    vec![name]
                }
            }

            fn prunable_dirs(&self, install_path: &Path) -> Vec<PathBuf> {
                vec![resolve_game_root(install_path).join("mods")]
            }

            fn preflight_warnings(&self, install_path: &Path) -> Vec<String> {
                modern_preflight_warnings(install_path)
            }

            fn load_log_sources(&self, install_path: &Path) -> Vec<super::LogSource> {
                modern_log_sources(install_path)
            }

            fn after_deploy(
                &self,
                install_path: &Path,
                enabled_mod_folders: &[String],
            ) -> Result<Vec<String>> {
                modern_after_deploy(install_path, $me3_game, enabled_mod_folders)
            }

            fn on_revert(&self, install_path: &Path) -> Result<()> {
                modern_on_revert(install_path)
            }

            fn has_plugin_tools(&self) -> bool {
                crate::games::fromsoft_loaders::has_plugin_tools($id)
            }

            fn plugin_tools_status(
                &self,
                paths: &crate::config::Paths,
                _game_id: &str,
                install_path: &Path,
            ) -> Vec<super::PluginToolStatus> {
                crate::games::fromsoft_loaders::tools_status(paths, $id, install_path)
            }

            fn check_plugin_tool_updates<'a>(
                &'a self,
                paths: &'a crate::config::Paths,
                _game_id: &'a str,
                install_path: &'a Path,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Vec<super::PluginToolStatus>>> + Send + 'a>,
            > {
                Box::pin(crate::games::fromsoft_loaders::check_tool_updates(
                    paths,
                    $id,
                    install_path,
                ))
            }

            fn install_plugin_tool_ctx<'a>(
                &'a self,
                ctx: &'a super::PluginToolInstallContext<'_>,
                tool_id: &'a str,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<super::PluginToolInstallOutcome>>
                        + Send
                        + 'a,
                >,
            > {
                Box::pin(crate::games::fromsoft_loaders::install_tool_ctx(
                    ctx,
                    $id,
                    tool_id,
                ))
            }

            fn uninstall_plugin_tool<'a>(
                &'a self,
                paths: &'a crate::config::Paths,
                _game_id: &'a str,
                install_path: &'a Path,
                tool_id: &'a str,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<super::PluginToolActionResult>>
                        + Send
                        + 'a,
                >,
            > {
                let id = tool_id.to_string();
                Box::pin(async move {
                    crate::games::fromsoft_loaders::uninstall_tool(paths, $id, install_path, &id)
                })
            }

            fn recommended_plugin_tools(
                &self,
                paths: &crate::config::Paths,
                install_path: &Path,
                content_roots: &[&Path],
            ) -> Vec<super::PluginToolRecommendation> {
                let needs = content_roots
                    .iter()
                    .any(|root| !self.load_order_entries(root, "mod").is_empty());
                crate::games::fromsoft_loaders::recommended_tools(paths, $id, install_path, needs)
            }
        }
    };
}

fromsoft_modern_plugin!(
    EldenRingNightreignPlugin,
    "eldenringnightreign",
    "Elden Ring Nightreign",
    "eldenringnightreign",
    "nightreign",
    &["elden ring nightreign", "eldenringnightreign", "nightreign"]
);

fromsoft_modern_plugin!(
    ArmoredCore6Plugin,
    "armoredcore6firesofrubicon",
    "Armored Core VI: Fires of Rubicon",
    "armoredcore6firesofrubicon",
    "armoredcore6",
    &[
        "armored core vi",
        "armored core 6",
        "armoredcore6",
        "fires of rubicon",
    ]
);

fromsoft_modern_plugin!(
    EldenRingPlugin,
    "eldenring",
    "Elden Ring",
    "eldenring",
    "eldenring",
    &["elden ring", "eldenring"]
);

fromsoft_modern_plugin!(
    DarkSouls3Plugin,
    "darksouls3",
    "Dark Souls 3",
    "darksouls3",
    "darksouls3",
    &[
        "dark souls iii",
        "dark souls 3",
        "darksouls3",
        "darksoulsiii"
    ]
);

fromsoft_legacy_plugin!(
    SekiroPlugin,
    "sekiro",
    "Sekiro: Shadows Die Twice",
    "sekiro",
    &["sekiro", "shadows die twice"]
);

fromsoft_legacy_plugin!(
    DarkSouls2Plugin,
    "darksouls2",
    "Dark Souls 2",
    "darksouls2",
    &[
        "scholar of the first sin",
        "dark souls ii",
        "dark souls 2",
        "darksouls2",
        "darksoulsii",
    ]
);

fromsoft_legacy_plugin!(
    DarkSoulsRemasteredPlugin,
    "darksoulsremastered",
    "Dark Souls Remastered",
    "darksoulsremastered",
    &["dark souls remastered", "darksoulsremastered"]
);

fromsoft_legacy_plugin!(
    DarkSoulsPlugin,
    "darksouls",
    "Dark Souls",
    "darksouls",
    &[
        "prepare to die",
        "dark souls: prepare",
        "dark souls prepare",
        "darksouls",
        "dark souls",
    ]
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::{match_plugin, normalize_staging_root};

    #[test]
    fn prefers_game_subdir_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("ELDEN RING");
        let game = install.join("Game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("eldenring.exe"), b"x").unwrap();
        assert_eq!(resolve_game_root(&install), game);
    }

    #[test]
    fn prefers_lowercase_game_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("DARK SOULS II");
        let game = install.join("game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("DarkSoulsII.exe"), b"x").unwrap();
        assert_eq!(resolve_game_root(&install), game);
    }

    #[test]
    fn keeps_install_when_no_game_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("DARK SOULS REMASTERED");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("DarkSoulsRemastered.exe"), b"x").unwrap();
        assert_eq!(resolve_game_root(&install), install);
    }

    #[test]
    fn prefers_game_subdir_without_exe_heuristic() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("DARK SOULS III");
        let game = install.join("Game");
        std::fs::create_dir_all(&game).unwrap();
        // No exe written — still prefer Game/ when install root has no known exe.
        assert_eq!(resolve_game_root(&install), game);
    }

    #[test]
    fn modern_deploy_lands_under_mods() {
        let install = Path::new("/games/ELDEN RING");
        let p = EldenRingPlugin;
        let dest = p
            .resolve_deploy_root(install, Path::new("CoolMod/parts/a.partsbnd.dcx"))
            .unwrap();
        assert_eq!(
            dest,
            PathBuf::from("/games/ELDEN RING/mods/CoolMod/parts/a.partsbnd.dcx")
        );
    }

    #[test]
    fn legacy_deploy_lands_under_mod_without_wrap() {
        let install = Path::new("/games/DS2");
        let p = DarkSouls2Plugin;
        let dest = p
            .resolve_deploy_root(install, Path::new("parts/a.partsbnd.dcx"))
            .unwrap();
        assert_eq!(dest, PathBuf::from("/games/DS2/mod/parts/a.partsbnd.dcx"));
        assert!(!p.prefers_mod_folder());
    }

    #[test]
    fn strips_leading_and_nested_mods_prefix() {
        assert_eq!(
            strip_mod_overlay_segments(Path::new("mods/parts/a.dcx")),
            PathBuf::from("parts/a.dcx")
        );
        assert_eq!(
            strip_mod_overlay_segments(Path::new("CoolMod/mods/parts/a.dcx")),
            PathBuf::from("CoolMod/parts/a.dcx")
        );
        assert_eq!(
            strip_mod_overlay_segments(Path::new("CoolMod/mod/chr/c.dcx")),
            PathBuf::from("CoolMod/chr/c.dcx")
        );

        let p = EldenRingPlugin;
        let dest = p
            .resolve_deploy_root(
                Path::new("/games/ER"),
                Path::new("CoolMod/mods/parts/a.dcx"),
            )
            .unwrap();
        assert_eq!(dest, PathBuf::from("/games/ER/mods/CoolMod/parts/a.dcx"));

        let legacy = DarkSouls2Plugin;
        let dest = legacy
            .resolve_deploy_root(Path::new("/games/DS2"), Path::new("mods/parts/a.dcx"))
            .unwrap();
        assert_eq!(dest, PathBuf::from("/games/DS2/mod/parts/a.dcx"));
    }

    #[test]
    fn injector_files_land_at_game_root() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("DS2");
        let game = install.join("Game");
        std::fs::create_dir_all(&game).unwrap();

        let dest = DarkSouls2Plugin
            .resolve_deploy_root(&install, Path::new("dinput8.dll"))
            .unwrap();
        assert_eq!(dest, game.join("dinput8.dll"));

        let dest = DarkSouls2Plugin
            .resolve_deploy_root(&install, Path::new("dxgi.dll"))
            .unwrap();
        assert_eq!(dest, game.join("dxgi.dll"));

        let dest = EldenRingPlugin
            .resolve_deploy_root(&install, Path::new("SomeMod/dxgi.dll"))
            .unwrap();
        assert_eq!(dest, game.join("dxgi.dll"));
    }

    #[test]
    fn root_injector_pack_deploys_to_game_root_not_mod() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("DS2");
        let game = install.join("Game");
        std::fs::create_dir_all(&game).unwrap();

        let staging = tmp.path().join("le");
        std::fs::create_dir_all(staging.join("SeamlessCoop")).unwrap();
        std::fs::write(staging.join("dxgi.dll"), b"x").unwrap();
        std::fs::write(staging.join("SeamlessCoop").join("ds2sc.dll"), b"x").unwrap();
        std::fs::write(staging.join("ReadMe.txt"), b"docs").unwrap();
        std::fs::write(staging.join("preview.png"), b"img").unwrap();
        assert!(looks_like_fromsoft_root_injector_pack(&staging));

        let ctx = DeployContext {
            project_name: None,
            content_root: Some(&staging),
            source_dir: None,
        };
        let dxgi = DarkSouls2Plugin
            .resolve_deploy_root_ctx(&install, Path::new("dxgi.dll"), &ctx)
            .unwrap();
        assert_eq!(dxgi, game.join("dxgi.dll"));
        let coop = DarkSouls2Plugin
            .resolve_deploy_root_ctx(&install, Path::new("SeamlessCoop/ds2sc.dll"), &ctx)
            .unwrap();
        assert_eq!(coop, game.join("SeamlessCoop/ds2sc.dll"));
        assert!(!coop.components().any(|c| {
            c.as_os_str()
                .to_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("mod"))
        }));

        assert!(!DarkSouls2Plugin.should_deploy_file_ctx(Path::new("ReadMe.txt"), &ctx));
        assert!(!DarkSouls2Plugin.should_deploy_file_ctx(Path::new("preview.png"), &ctx));
        assert!(DarkSouls2Plugin.should_deploy_file_ctx(Path::new("dxgi.dll"), &ctx));
    }

    #[test]
    fn me_asset_pack_still_merges_into_mod() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("DS2");
        let game = install.join("Game");
        std::fs::create_dir_all(&game).unwrap();

        let staging = tmp.path().join("armor");
        std::fs::create_dir_all(staging.join("parts")).unwrap();
        std::fs::write(staging.join("parts").join("a.dcx"), b"x").unwrap();
        assert!(!looks_like_fromsoft_root_injector_pack(&staging));

        let ctx = DeployContext {
            project_name: None,
            content_root: Some(&staging),
            source_dir: None,
        };
        let dest = DarkSouls2Plugin
            .resolve_deploy_root_ctx(&install, Path::new("parts/a.dcx"), &ctx)
            .unwrap();
        assert_eq!(dest, game.join("mod/parts/a.dcx"));
    }

    #[test]
    fn deploy_uses_game_subdir_when_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("ELDEN RING");
        let game = install.join("Game");
        std::fs::create_dir_all(&game).unwrap();
        let p = EldenRingPlugin;
        let dest = p
            .resolve_deploy_root(&install, Path::new("MyMod/regulation.bin"))
            .unwrap();
        assert_eq!(dest, game.join("mods").join("MyMod").join("regulation.bin"));
    }

    #[test]
    fn peels_wrapper_but_keeps_parts_and_cutscene() {
        let staging = tempfile::tempdir().unwrap();
        let wrapper = staging.path().join("Cool Armor Pack");
        std::fs::create_dir_all(wrapper.join("parts")).unwrap();
        std::fs::write(wrapper.join("parts").join("a.partsbnd.dcx"), b"x").unwrap();
        let root = normalize_staging_root(staging.path(), &EldenRingPlugin).unwrap();
        assert_eq!(root, wrapper);

        let parts_only = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(parts_only.path().join("parts")).unwrap();
        let kept = normalize_staging_root(parts_only.path(), &EldenRingPlugin).unwrap();
        assert_eq!(kept, parts_only.path());

        let cutscene_only = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(cutscene_only.path().join("cutscene")).unwrap();
        let kept = normalize_staging_root(cutscene_only.path(), &DarkSouls2Plugin).unwrap();
        assert_eq!(kept, cutscene_only.path());
    }

    #[test]
    fn writes_modengine_ini_override() {
        let tmp = tempfile::tempdir().unwrap();
        write_modengine_ini(tmp.path()).unwrap();
        let raw = std::fs::read_to_string(tmp.path().join("modengine.ini")).unwrap();
        assert!(raw.contains("useModOverrideDirectory=1"));
        assert!(raw.contains("modOverrideDirectory=\"\\mod\""));

        std::fs::write(
            tmp.path().join("modengine.ini"),
            "useAlternateSave=1\nmodOverrideDirectory=\"\\old\"\n",
        )
        .unwrap();
        write_modengine_ini(tmp.path()).unwrap();
        let raw = std::fs::read_to_string(tmp.path().join("modengine.ini")).unwrap();
        assert!(raw.contains("useAlternateSave=1"));
        assert!(raw.contains("useModOverrideDirectory=1"));
        assert!(raw.contains("modOverrideDirectory=\"\\mod\""));
        assert!(!raw.contains("\\old"));
    }

    #[test]
    fn upsert_legacy_modengine_ini_rewrites_vestiges_and_stubs() {
        let vestiges = "useModOverrideDirectory=1\nmodOverrideDirectory=\"\\Vestiges\"\nblockNetworkAccess=1\n";
        let out = upsert_legacy_modengine_ini(vestiges);
        assert!(out.contains("useModOverrideDirectory=1"));
        assert!(out.contains("modOverrideDirectory=\"\\mod\""));
        assert!(out.contains("blockNetworkAccess=1"));
        assert!(!out.contains("\\Vestiges"));

        let stub_missing_flag = "modOverrideDirectory=\"\\mod\"\n";
        let out = upsert_legacy_modengine_ini(stub_missing_flag);
        assert!(out.contains("useModOverrideDirectory=1"));
        assert!(out.contains("modOverrideDirectory=\"\\mod\""));

        let empty = upsert_legacy_modengine_ini("");
        assert!(empty.contains("useModOverrideDirectory=1"));
        assert!(empty.contains("modOverrideDirectory=\"\\mod\""));
    }

    #[test]
    fn repair_dinput8_launch_options() {
        assert!(launch_options_have_dinput8_native(
            r#"WINEDLLOVERRIDES="dinput8=n,b" %command%"#
        ));
        assert!(launch_options_have_dinput8_native(
            r#"WINEDLLOVERRIDES="dinput8,version=n,b" %command%"#
        ));
        assert!(!launch_options_have_dinput8_native(
            r#"WINEDLLOVERRIDES="dinput8,n,b" %command%"#
        ));
        assert!(!launch_options_have_dinput8_native(""));

        let fixed = repair_dinput8_override("");
        assert_eq!(fixed, "WINEDLLOVERRIDES=dinput8=n,b %command%");

        let fixed = repair_dinput8_override(r#"WINEDLLOVERRIDES="dinput8,n,b" %command%"#);
        assert!(launch_options_have_dinput8_native(&fixed));

        let fixed = repair_dinput8_override("PROTON_ENABLE_WAYLAND=1 %command%");
        assert!(fixed.contains("PROTON_ENABLE_WAYLAND=1"));
        assert!(launch_options_have_dinput8_native(&fixed));
        assert!(fixed.contains("%command%"));
        assert!(!fixed.contains(r#"WINEDLLOVERRIDES=""#));
    }

    #[test]
    fn merges_winedll_overrides_for_le_and_me() {
        let le_only = merge_fromsoft_winedll_overrides(
            "PROTON_ENABLE_WAYLAND=1 ENABLE_LSFG=1 %command%",
            &["dxgi"],
        );
        assert!(le_only.contains("PROTON_ENABLE_WAYLAND=1"));
        assert!(le_only.contains("ENABLE_LSFG=1"));
        assert!(winedll_has_native(&le_only, "dxgi"));
        assert!(!winedll_has_native(&le_only, "dinput8"));
        assert!(le_only.contains("%command%"));

        let both = merge_fromsoft_winedll_overrides(
            r#"WINEDLLOVERRIDES=dinput8=n,b %command%"#,
            &["dinput8", "dxgi"],
        );
        assert!(winedll_has_native(&both, "dinput8"));
        assert!(winedll_has_native(&both, "dxgi"));
        assert!(
            both.contains("dinput8,dxgi=n,b") || both.contains("WINEDLLOVERRIDES=dinput8,dxgi=n,b")
        );
    }

    #[test]
    fn seamless_launch_rewrite_is_idempotent() {
        let once = ensure_seamless_launch_options("PROTON_ENABLE_WAYLAND=1 %command%");
        assert!(launch_options_have_seamless_rewrite(&once));
        assert!(once.contains("ds2sc_launcher.exe"));
        assert!(!once.trim_end().ends_with("%command%"));

        let twice = ensure_seamless_launch_options(&once);
        assert_eq!(once, twice);

        let with_dll = merge_fromsoft_winedll_overrides(&once, &["dxgi"]);
        assert!(launch_options_have_seamless_rewrite(&with_dll));
        assert!(winedll_has_native(&with_dll, "dxgi"));
        assert!(with_dll.contains("PROTON_ENABLE_WAYLAND=1"));
    }

    #[test]
    fn empty_cooppassword_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("SeamlessCoop");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ds2sc_settings.ini"), "cooppassword = \n").unwrap();
        assert!(empty_seamless_cooppassword(tmp.path()));
        std::fs::write(dir.join("ds2sc_settings.ini"), "cooppassword = secret\n").unwrap();
        assert!(!empty_seamless_cooppassword(tmp.path()));
    }

    #[test]
    fn removes_emperor_modengine_when_matching_cache() {
        let game = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        std::fs::write(cache.path().join("dinput8.dll"), b"vestiges-dll").unwrap();
        std::fs::write(game.path().join("dinput8.dll"), b"vestiges-dll").unwrap();
        std::fs::write(
            game.path().join("modengine.ini"),
            legacy_modengine_stub_ini(),
        )
        .unwrap();
        std::fs::write(game.path().join("dxgi.dll"), b"le").unwrap();

        let warnings = remove_emperor_modengine_files(game.path(), cache.path());
        assert!(warnings
            .iter()
            .any(|w| w.contains("Removed auto-installed")));
        assert!(!game.path().join("dinput8.dll").is_file());
        assert!(!game.path().join("modengine.ini").is_file());
        assert!(game.path().join("dxgi.dll").is_file());
    }

    #[test]
    fn keeps_foreign_dinput8_when_removing_me() {
        let game = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        std::fs::write(cache.path().join("dinput8.dll"), b"vestiges").unwrap();
        std::fs::write(game.path().join("dinput8.dll"), b"user-owned-longer").unwrap();
        let warnings = remove_emperor_modengine_files(game.path(), cache.path());
        assert!(warnings.is_empty());
        assert!(game.path().join("dinput8.dll").is_file());
    }

    #[test]
    fn needed_dlls_reflect_disk() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(needed_fromsoft_native_dlls(tmp.path()).is_empty());
        std::fs::write(tmp.path().join("dxgi.dll"), b"x").unwrap();
        assert_eq!(needed_fromsoft_native_dlls(tmp.path()), vec!["dxgi"]);
        std::fs::write(tmp.path().join("dinput8.dll"), b"x").unwrap();
        assert_eq!(
            needed_fromsoft_native_dlls(tmp.path()),
            vec!["dinput8", "dxgi"]
        );
    }

    #[test]
    fn injector_only_preflight_skips_missing_me_nag() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("DarkSoulsII.exe"), b"x").unwrap();
        std::fs::write(tmp.path().join("dxgi.dll"), b"x").unwrap();
        let warnings = DarkSouls2Plugin.preflight_warnings(tmp.path());
        assert!(!warnings.iter().any(|w| w.contains("was not found next to")));
        assert!(warnings.iter().any(|w| w.contains("Lighting Engine")));
        assert!(warnings.iter().any(|w| w.contains("DS2LE.log")));
    }

    #[test]
    fn game_has_me_overlay_assets_detects_files() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!game_has_me_overlay_assets(tmp.path()));
        let mod_dir = tmp.path().join("mod").join("parts");
        std::fs::create_dir_all(&mod_dir).unwrap();
        assert!(!game_has_me_overlay_assets(tmp.path()));
        std::fs::write(mod_dir.join("a.partsbnd"), b"x").unwrap();
        assert!(game_has_me_overlay_assets(tmp.path()));
    }

    #[test]
    fn writes_me3_profile_with_assets_and_natives() {
        let tmp = tempfile::tempdir().unwrap();
        let mod_a = tmp.path().join("mods").join("CoolMod");
        std::fs::create_dir_all(mod_a.join("parts")).unwrap();
        std::fs::write(mod_a.join("helper.dll"), b"x").unwrap();
        std::fs::write(mod_a.join("dinput8.dll"), b"x").unwrap();

        write_me3_profile(tmp.path(), "eldenring", &["CoolMod".into()]).unwrap();

        let me3 = std::fs::read_to_string(tmp.path().join("emperor.me3")).unwrap();
        assert!(me3.contains("game = \"eldenring\""));
        assert!(me3.contains("path = \"mods/CoolMod\""));
        assert!(me3.contains("path = \"mods/CoolMod/helper.dll\""));
        assert!(!me3.contains("dinput8.dll"));
    }

    #[test]
    fn me3_profile_separates_packages_and_natives_in_load_order() {
        let tmp = tempfile::tempdir().unwrap();
        // 1. Pure DLL mod (Zeta) — listed FIRST in user load order
        let zeta = tmp.path().join("mods").join("ZetaFps");
        std::fs::create_dir_all(&zeta).unwrap();
        std::fs::write(zeta.join("UnlockTheFps.dll"), b"x").unwrap();
        std::fs::write(zeta.join("vcruntime140.dll"), b"rt").unwrap();
        std::fs::write(zeta.join("steam_api64.dll"), b"steam").unwrap();

        // 2. Seamless Co-op — pure native DLL inside SeamlessCoop/
        let sc = tmp.path().join("mods").join("Seamless Co-op");
        std::fs::create_dir_all(sc.join("SeamlessCoop")).unwrap();
        std::fs::write(sc.join("SeamlessCoop").join("ersc.dll"), b"x").unwrap();
        std::fs::write(sc.join("ersc_launcher.exe"), b"x").unwrap();

        // 3. Asset + DLL hybrid mod with apostrophe in name
        let hybrid = tmp.path().join("mods").join("Malenia's Overhaul");
        std::fs::create_dir_all(hybrid.join("asset")).unwrap();
        std::fs::write(hybrid.join("regulation.bin"), b"x").unwrap();
        std::fs::write(hybrid.join("AlphaHook.dll"), b"x").unwrap();
        std::fs::write(hybrid.join("me3_mod_host.dll"), b"x").unwrap();

        let order = vec![
            "ZetaFps".to_string(),
            "Seamless Co-op".to_string(),
            "Malenia's Overhaul".to_string(),
        ];
        write_me3_profile(tmp.path(), "eldenring", &order).unwrap();
        let me3 = std::fs::read_to_string(tmp.path().join("emperor.me3")).unwrap();

        // Only the hybrid mod has FromSoft assets, so only it gets [[packages]].
        assert!(!me3.contains("id = \"ZetaFps\""));
        assert!(!me3.contains("id = \"Seamless Co-op\""));
        assert!(me3.contains("id = \"Malenia's Overhaul\""));
        assert!(me3.contains("path = \"mods/Malenia's Overhaul\""));

        // Non-mod runtime/host DLLs are excluded.
        assert!(!me3.contains("vcruntime140.dll"));
        assert!(!me3.contains("steam_api64.dll"));
        assert!(!me3.contains("me3_mod_host.dll"));

        // Native DLLs preserve user load order (Zeta -> Seamless -> Alpha), not alphabetical.
        let pos_zeta = me3.find("mods/ZetaFps/UnlockTheFps.dll").unwrap();
        let pos_sc = me3.find("mods/Seamless Co-op/SeamlessCoop/ersc.dll").unwrap();
        let pos_alpha = me3.find("mods/Malenia's Overhaul/AlphaHook.dll").unwrap();
        assert!(pos_zeta < pos_sc && pos_sc < pos_alpha);
    }

    #[test]
    fn me3_profile_ignores_nested_sdk_and_dependency_dlls() {
        let tmp = tempfile::tempdir().unwrap();
        let erss = tmp.path().join("mods").join("ERSS-FG-v4.13.0-Release");
        std::fs::create_dir_all(erss.join("ERSS2").join("bin")).unwrap();
        std::fs::write(erss.join("ERSS-FG.dll"), b"mod").unwrap();
        std::fs::write(
            erss.join("ERSS2").join("bin").join("nvngx_dlss.dll"),
            b"sdk",
        )
        .unwrap();
        std::fs::write(
            erss.join("ERSS2")
                .join("bin")
                .join("amd_fidelityfx_framegeneration_dx12.dll"),
            b"sdk",
        )
        .unwrap();
        std::fs::write(
            erss.join("ERSS2").join("bin").join("sl.interposer.dll"),
            b"sdk",
        )
        .unwrap();
        // Even a non-SDK helper DLL in a deeper folder is skipped when depth-1 entry DLL exists.
        std::fs::create_dir_all(erss.join("helper")).unwrap();
        std::fs::write(erss.join("helper").join("internal_dep.dll"), b"dep").unwrap();

        let sc = tmp.path().join("mods").join("Seamless Co-op (Elden Ring)");
        std::fs::create_dir_all(sc.join("SeamlessCoop")).unwrap();
        std::fs::write(sc.join("SeamlessCoop").join("ersc.dll"), b"sc").unwrap();

        write_me3_profile(
            tmp.path(),
            "eldenring",
            &[
                "Seamless Co-op (Elden Ring)".into(),
                "ERSS-FG-v4.13.0-Release".into(),
            ],
        )
        .unwrap();

        let me3 = std::fs::read_to_string(tmp.path().join("emperor.me3")).unwrap();
        assert!(me3.contains("mods/Seamless Co-op (Elden Ring)/SeamlessCoop/ersc.dll"));
        assert!(me3.contains("mods/ERSS-FG-v4.13.0-Release/ERSS-FG.dll"));
        assert!(!me3.contains("nvngx_dlss.dll"));
        assert!(!me3.contains("amd_fidelityfx"));
        assert!(!me3.contains("sl.interposer.dll"));
        assert!(!me3.contains("internal_dep.dll"));
    }

    #[test]
    fn graphics_proxy_pack_deploys_to_game_root_and_skips_me3_natives() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("ELDEN RING");
        let game = install.join("Game");
        std::fs::create_dir_all(&game).unwrap();

        let erss = tmp.path().join("ERSS-FG-v4.13.0-Release");
        std::fs::create_dir_all(erss.join("ERSS2").join("bin")).unwrap();
        std::fs::write(erss.join("D3D12.dll"), b"proxy").unwrap();
        std::fs::write(erss.join("ERSS-FG.dll"), b"mod").unwrap();
        std::fs::write(erss.join("ERSS2").join("ERSS-FG.toml"), b"cfg").unwrap();
        std::fs::write(erss.join("ERSS2").join("bin").join("libxess.dll"), b"sdk").unwrap();

        assert!(looks_like_modern_graphics_proxy_pack(&erss));
        assert!(EldenRingPlugin
            .load_order_entries(&erss, "ERSS-FG-v4.13.0-Release")
            .is_empty());

        let ctx = DeployContext {
            project_name: None,
            content_root: Some(&erss),
            source_dir: None,
        };
        assert_eq!(
            EldenRingPlugin
                .resolve_deploy_root_ctx(
                    &install,
                    Path::new("ERSS-FG-v4.13.0-Release/D3D12.dll"),
                    &ctx
                )
                .unwrap(),
            game.join("D3D12.dll")
        );
        assert_eq!(
            EldenRingPlugin
                .resolve_deploy_root_ctx(
                    &install,
                    Path::new("ERSS-FG-v4.13.0-Release/ERSS-FG.dll"),
                    &ctx
                )
                .unwrap(),
            game.join("ERSS-FG.dll")
        );
        assert_eq!(
            EldenRingPlugin
                .resolve_deploy_root_ctx(
                    &install,
                    Path::new("ERSS-FG-v4.13.0-Release/ERSS2/bin/libxess.dll"),
                    &ctx
                )
                .unwrap(),
            game.join("ERSS2").join("bin").join("libxess.dll")
        );
    }

    #[test]
    fn flat_dll_mods_and_eml_bundles_deploy_directly_under_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("ELDEN RING");
        let game = install.join("Game");
        std::fs::create_dir_all(&game).unwrap();

        // 1. Individual techiew mod: Fix the camera -> mods/CameraFix.dll + mods/CameraFix/config.ini
        let cam = tmp.path().join("Fix the camera");
        std::fs::create_dir_all(cam.join("mods").join("CameraFix")).unwrap();
        std::fs::write(cam.join("mods").join("CameraFix.dll"), b"dll").unwrap();
        std::fs::write(cam.join("mods").join("CameraFix").join("config.ini"), b"ini").unwrap();

        assert!(looks_like_modern_flat_dll_mod(&cam));
        assert_eq!(
            EldenRingPlugin.load_order_entries(&cam, "Fix the camera"),
            vec!["CameraFix.dll".to_string()]
        );
        let cam_ctx = DeployContext {
            project_name: None,
            content_root: Some(&cam),
            source_dir: None,
        };
        assert_eq!(
            EldenRingPlugin
                .resolve_deploy_root_ctx(
                    &install,
                    Path::new("Fix the camera/mods/CameraFix.dll"),
                    &cam_ctx
                )
                .unwrap(),
            game.join("mods").join("CameraFix.dll")
        );
        assert_eq!(
            EldenRingPlugin
                .resolve_deploy_root_ctx(
                    &install,
                    Path::new("Fix the camera/mods/CameraFix/config.ini"),
                    &cam_ctx
                )
                .unwrap(),
            game.join("mods").join("CameraFix").join("config.ini")
        );

        // 2. Bundle pack (like Elden Ring Ultrawide Fix) that includes dinput8.dll + mod_loader_config.ini
        // alongside real DLL mods under mods/.
        let uw = tmp.path().join("Elden Ring Ultrawide Fix");
        std::fs::create_dir_all(uw.join("mods").join("UltrawideFix")).unwrap();
        std::fs::write(uw.join("dinput8.dll"), b"eml").unwrap();
        std::fs::write(uw.join("mod_loader_config.ini"), b"cfg").unwrap();
        std::fs::write(uw.join("anti_cheat_toggler_config.ini"), b"eac").unwrap();
        std::fs::write(uw.join("_winhttp.dll"), b"eac").unwrap();
        std::fs::write(uw.join("mods").join("UltrawideFix.dll"), b"uw").unwrap();
        std::fs::write(uw.join("mods").join("CameraFix.dll"), b"cam").unwrap();

        // Must NOT be skipped as standalone Elden Mod Loader!
        assert!(!looks_like_elden_mod_loader(&uw));
        assert!(looks_like_modern_flat_dll_mod(&uw));
        let uw_ctx = DeployContext {
            project_name: None,
            content_root: Some(&uw),
            source_dir: None,
        };
        // Loader files are skipped, real DLLs are deployed.
        assert!(!EldenRingPlugin.should_deploy_file_ctx(Path::new("dinput8.dll"), &uw_ctx));
        assert!(!EldenRingPlugin.should_deploy_file_ctx(Path::new("mod_loader_config.ini"), &uw_ctx));
        assert!(!EldenRingPlugin.should_deploy_file_ctx(Path::new("anti_cheat_toggler_config.ini"), &uw_ctx));
        assert!(!EldenRingPlugin.should_deploy_file_ctx(Path::new("_winhttp.dll"), &uw_ctx));
        assert!(EldenRingPlugin.should_deploy_file_ctx(Path::new("mods/UltrawideFix.dll"), &uw_ctx));
        assert_eq!(
            EldenRingPlugin.load_order_entries(&uw, "Elden Ring Ultrawide Fix"),
            vec!["CameraFix.dll".to_string(), "UltrawideFix.dll".to_string()]
        );

        // Deployed flat DLLs are registered in emperor.me3 and deduplicated across mods.
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::write(game.join("mods").join("CameraFix.dll"), b"dll").unwrap();
        std::fs::write(game.join("mods").join("UltrawideFix.dll"), b"uw").unwrap();
        write_me3_profile(
            &game,
            "eldenring",
            &[
                "CameraFix.dll".into(),
                "CameraFix.dll".into(),
                "UltrawideFix.dll".into(),
            ],
        )
        .unwrap();
        let me3 = std::fs::read_to_string(game.join("emperor.me3")).unwrap();
        assert_eq!(me3.matches("mods/CameraFix.dll").count(), 1);
        assert_eq!(me3.matches("mods/UltrawideFix.dll").count(), 1);
        assert!(!me3.contains("[[packages]]"));
    }

    #[test]
    fn elden_mod_loader_is_skipped_with_warning_for_modern_fromsoft() {
        let tmp = tempfile::tempdir().unwrap();
        let eml = tmp.path().join("EldenModLoader");
        std::fs::create_dir_all(&eml).unwrap();
        std::fs::write(eml.join("dinput8.dll"), b"x").unwrap();
        std::fs::write(eml.join("mod_loader_config.ini"), b"[modloader]").unwrap();

        assert!(looks_like_elden_mod_loader(&eml));
        let ctx = DeployContext {
            project_name: None,
            content_root: Some(&eml),
            source_dir: None,
        };
        assert!(!EldenRingPlugin.should_deploy_file_ctx(Path::new("dinput8.dll"), &ctx));
        assert!(!EldenRingPlugin.should_deploy_file_ctx(Path::new("mod_loader_config.ini"), &ctx));
        assert!(EldenRingPlugin.allows_empty_deploy(&eml));
        assert!(EldenRingPlugin
            .load_order_entries(&eml, "Elden Mod Loader")
            .is_empty());
        let warnings = EldenRingPlugin.staging_deploy_warnings(&eml, "Elden Mod Loader");
        assert!(warnings.iter().any(|w| w.contains("superseded by Mod Engine 3")));
    }

    #[test]
    fn er_seamless_coop_empty_password_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let sc = tmp
            .path()
            .join("mods")
            .join("SeamlessCoop")
            .join("SeamlessCoop");
        std::fs::create_dir_all(&sc).unwrap();
        std::fs::write(sc.join("ersc_settings.ini"), "[PASSWORD]\ncooppassword = \n").unwrap();

        assert!(empty_er_seamless_cooppassword(tmp.path()).is_some());
        let warnings = EldenRingPlugin
            .after_deploy(tmp.path(), &["SeamlessCoop".into()])
            .unwrap();
        assert!(warnings.iter().any(|w| w.contains("cooppassword is empty")));

        std::fs::write(
            sc.join("ersc_settings.ini"),
            "[PASSWORD]\ncooppassword = tarnished\n",
        )
        .unwrap();
        assert!(empty_er_seamless_cooppassword(tmp.path()).is_none());
    }

    #[test]
    fn modern_on_revert_removes_generated_me3_files() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("emperor.me3"), b"profile").unwrap();
        std::fs::write(game.join("launch_emperor_me3.sh"), b"sh").unwrap();
        std::fs::write(game.join("launch_emperor_me3.bat"), b"bat").unwrap();

        EldenRingPlugin.on_revert(tmp.path()).unwrap();
        assert!(!game.join("emperor.me3").exists());
        assert!(!game.join("launch_emperor_me3.sh").exists());
        assert!(!game.join("launch_emperor_me3.bat").exists());
    }

    #[test]
    fn legacy_preflight_warns_without_dinput8() {
        let tmp = tempfile::tempdir().unwrap();
        let warnings = DarkSouls2Plugin.preflight_warnings(tmp.path());
        assert!(warnings.iter().any(|w| w.contains("GitHub")));
        assert!(warnings.iter().any(|w| w.contains("DarkSoulsII.exe")));
        assert!(!warnings.iter().any(|w| w.contains("Nexus")));

        std::fs::write(tmp.path().join("dinput8.dll"), b"x").unwrap();
        std::fs::write(tmp.path().join("DarkSoulsII.exe"), b"x").unwrap();
        let after = DarkSouls2Plugin.preflight_warnings(tmp.path());
        assert!(!after.iter().any(|w| w.contains("was not found next to")));
        assert!(!after
            .iter()
            .any(|w| w.contains("DarkSoulsII.exe was not found")));
    }

    #[test]
    fn legacy_preflight_nexus_copy_for_ds1() {
        let tmp = tempfile::tempdir().unwrap();
        let warnings = DarkSoulsPlugin.preflight_warnings(tmp.path());
        assert!(warnings.iter().any(|w| w.contains("Nexus")));
        assert!(!warnings.iter().any(|w| w.contains("GitHub")));
    }

    #[test]
    fn modern_preflight_checks_profile_and_dinput8() {
        let tmp = tempfile::tempdir().unwrap();
        let warnings = EldenRingPlugin.preflight_warnings(tmp.path());
        assert!(warnings.iter().any(|w| w.contains("emperor.me3")));

        std::fs::write(tmp.path().join("emperor.me3"), b"profileVersion = \"v1\"\n").unwrap();
        assert!(EldenRingPlugin.preflight_warnings(tmp.path()).is_empty());

        std::fs::write(tmp.path().join("dinput8.dll"), b"x").unwrap();
        let conflict = EldenRingPlugin.preflight_warnings(tmp.path());
        assert!(conflict.iter().any(|w| w.contains("dinput8.dll")));
    }

    #[test]
    fn title_matching_does_not_collide() {
        assert_eq!(match_plugin("ELDEN RING", None).unwrap().id, "eldenring");
        assert_eq!(
            match_plugin("DARK SOULS III", None).unwrap().id,
            "darksouls3"
        );
        assert_eq!(
            match_plugin("Dark Souls II Scholar of the First Sin", None)
                .unwrap()
                .id,
            "darksouls2"
        );
        assert_eq!(
            match_plugin("DARK SOULS REMASTERED", None).unwrap().id,
            "darksoulsremastered"
        );
        assert_eq!(
            match_plugin("Dark Souls: Prepare to Die Edition", None)
                .unwrap()
                .id,
            "darksouls"
        );
    }
}
