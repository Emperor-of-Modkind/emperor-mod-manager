//! id Tech titles: the Doom reboot series and the Wolfenstein reboot series.
//!
//! Doom mods are loose folders under the loader's mods directory. Wolfenstein
//! reboot mods are files in the install (`base/` and proxy DLLs at the root).
//! This does not download or run DOOMModLoader, the Eternal injector, or Atlan.
//! Those loaders disagree with each other, and DOOMModLoader starts the game.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

/// Staging roots that are part of the game layout and must not be peeled.
const ID_TECH_ROOT_DIRS: &[&str] = &["base", "Mods", "mods"];

/// Proxy DLLs the game loads from the install root. Wolfenstein packs often
/// nest these one folder deep.
const PROXY_DLLS: &[&str] = &[
    "xinput1_3.dll",
    "dinput8.dll",
    "dxgi.dll",
    "winmm.dll",
    "version.dll",
];

const DOOM_2016_LOADER_WARNING: &str = "DOOMModLoader not found in the game folder. Mods will not load until DOOMModLoader is installed.";

const ETERNAL_LOADER_WARNING: &str = "No DOOM Eternal mod loader found in the game folder (DEternal_loadMods.exe or AtlanModLoader.exe). Mods will not load until one of them is installed. Do not install both; they are incompatible.";

const DARK_AGES_LOADER_WARNING: &str = "Atlan Mod Loader not found in the game folder (AtlanModLoader.exe). Mods will not load until it is installed.";

/// The legacy Eternal injector only honors `loadPriority` inside zips, and it
/// loads loose folders after those zips.
const ETERNAL_DEPLOY_WARNING: &str = "DOOM Eternal mods were deployed as loose folders under Mods/. The legacy injector ignores EternalMod.json on those folders and loads them after any zipped mods, so Mods-tab order is not applied. Run the injector after deploy.";

#[derive(Clone, Copy)]
enum Layout {
    /// One folder per mod under `mods_dir`.
    Injector {
        mods_dir: &'static str,
        /// Create the priority file when the mod folder does not have one.
        /// Eternal only updates a file the mod already shipped.
        create_priority: bool,
        priority_files: &'static [&'static str],
        loader_names: &'static [&'static str],
        missing_loader_warning: &'static str,
        deploy_warning: Option<&'static str>,
    },
    /// Relative paths land on the install. Proxy DLLs are hoisted to the root.
    InstallRoot,
}

#[derive(Clone, Copy)]
struct IdTechGame {
    id: &'static str,
    display_name: &'static str,
    nexus_domain: &'static str,
    match_names: &'static [&'static str],
    layout: Layout,
}

impl IdTechGame {
    fn info(self) -> GamePluginInfo {
        GamePluginInfo {
            id: self.id,
            display_name: self.display_name,
            nexus_domain: self.nexus_domain,
            match_names: self.match_names,
        }
    }

    fn should_wrap(self, content_root: &Path) -> bool {
        let Layout::Injector { .. } = self.layout else {
            return false;
        };
        !self.deploys_to_install_root(content_root) && !has_child_dir(content_root, "mods")
    }

    fn deploys_to_install_root(self, content_root: &Path) -> bool {
        let Layout::Injector { loader_names, .. } = self.layout else {
            return false;
        };
        contains_named_file(content_root, loader_names)
    }

    fn prunable_dirs(self, install_path: &Path) -> Vec<PathBuf> {
        let Layout::Injector { mods_dir, .. } = self.layout else {
            return Vec::new();
        };
        let dir = install_path.join(mods_dir);
        if dir.is_dir() {
            vec![dir]
        } else {
            Vec::new()
        }
    }

    fn preflight_warnings(self, install_path: &Path) -> Vec<String> {
        let Layout::Injector {
            loader_names,
            missing_loader_warning,
            ..
        } = self.layout
        else {
            return Vec::new();
        };
        if loader_names
            .iter()
            .any(|name| install_has_file(install_path, name))
        {
            Vec::new()
        } else {
            vec![missing_loader_warning.to_string()]
        }
    }

    fn load_order_entries(self, content_root: &Path, staged_name: &str) -> Vec<String> {
        if !matches!(self.layout, Layout::Injector { .. }) {
            return Vec::new();
        }
        if self.deploys_to_install_root(content_root) {
            return Vec::new();
        }
        if self.should_wrap(content_root) {
            let name = content_folder_wrap_name(content_root, staged_name);
            return if name.is_empty() {
                Vec::new()
            } else {
                vec![name]
            };
        }
        child_mod_folders(content_root)
    }

    fn resolve_deploy_root(self, install_path: &Path, relative: &Path) -> PathBuf {
        match self.layout {
            Layout::Injector { mods_dir, .. } => resolve_injector(install_path, mods_dir, relative),
            Layout::InstallRoot => resolve_install_root(install_path, relative),
        }
    }

    fn after_deploy(
        self,
        install_path: &Path,
        enabled_mod_folders: &[String],
    ) -> Result<Vec<String>> {
        let Layout::Injector {
            mods_dir,
            create_priority,
            priority_files,
            deploy_warning,
            ..
        } = self.layout
        else {
            return Ok(Vec::new());
        };

        let mut warnings = Vec::new();
        let count = enabled_mod_folders.len();
        for (index, name) in enabled_mod_folders.iter().enumerate() {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            let priority = i32::try_from(count - 1 - index).unwrap_or(i32::MAX);
            let folder = install_path.join(mods_dir).join(trimmed);
            if !folder.is_dir() {
                continue;
            }
            for file_name in priority_files {
                if let Some(warning) =
                    write_priority(&folder, file_name, priority, create_priority)?
                {
                    warnings.push(warning);
                }
            }
        }
        if let Some(warning) = deploy_warning {
            if !enabled_mod_folders.is_empty() {
                warnings.push(warning.to_string());
            }
        }
        Ok(warnings)
    }
}

const DARK_AGES: IdTechGame = IdTechGame {
    id: "doomthedarkages",
    display_name: "DOOM: The Dark Ages",
    nexus_domain: "doomthedarkages",
    match_names: &["doom: the dark ages", "doom the dark ages"],
    layout: Layout::Injector {
        mods_dir: "mods",
        create_priority: true,
        priority_files: &["darkagesmod.txt"],
        loader_names: &["AtlanModLoader.exe", "DarkAgesModManager.exe"],
        missing_loader_warning: DARK_AGES_LOADER_WARNING,
        deploy_warning: None,
    },
};

const ETERNAL: IdTechGame = IdTechGame {
    id: "doometernal",
    display_name: "DOOM Eternal",
    nexus_domain: "doometernal",
    match_names: &["doom eternal", "doometernal"],
    layout: Layout::Injector {
        mods_dir: "Mods",
        create_priority: false,
        priority_files: &["EternalMod.json", "atlanmod.txt"],
        loader_names: &["DEternal_loadMods.exe", "AtlanModLoader.exe"],
        missing_loader_warning: ETERNAL_LOADER_WARNING,
        deploy_warning: Some(ETERNAL_DEPLOY_WARNING),
    },
};

const DOOM_2016: IdTechGame = IdTechGame {
    id: "doom2016",
    display_name: "DOOM (2016)",
    nexus_domain: "doom",
    match_names: &["doom (2016)", "doom 2016"],
    layout: Layout::Injector {
        mods_dir: "Mods",
        create_priority: false,
        priority_files: &[],
        loader_names: &["DOOMModLoader.exe", "DOOMModLoader"],
        missing_loader_warning: DOOM_2016_LOADER_WARNING,
        deploy_warning: None,
    },
};

const WOLFENSTEIN_2: IdTechGame = IdTechGame {
    id: "wolfenstein2thenewcolossus",
    display_name: "Wolfenstein II: The New Colossus",
    nexus_domain: "wolfenstein2thenewcolossus",
    match_names: &["wolfenstein ii", "wolfenstein 2"],
    layout: Layout::InstallRoot,
};

const WOLFENSTEIN_YOUNGBLOOD: IdTechGame = IdTechGame {
    id: "wolfensteinyoungblood",
    display_name: "Wolfenstein: Youngblood",
    nexus_domain: "wolfensteinyoungblood",
    match_names: &[
        "wolfenstein: youngblood",
        "wolfenstein youngblood",
        "wolfensteinyoungblood",
    ],
    layout: Layout::InstallRoot,
};

const WOLFENSTEIN_OLD_BLOOD: IdTechGame = IdTechGame {
    id: "wolfensteintheoldblood",
    display_name: "Wolfenstein: The Old Blood",
    nexus_domain: "wolfensteintheoldblood",
    match_names: &["wolfenstein: the old blood", "wolfenstein the old blood"],
    layout: Layout::InstallRoot,
};

const WOLFENSTEIN_NEW_ORDER: IdTechGame = IdTechGame {
    id: "wolfensteintheneworder",
    display_name: "Wolfenstein: The New Order",
    nexus_domain: "wolfensteintheneworder",
    match_names: &["wolfenstein: the new order", "wolfenstein the new order"],
    layout: Layout::InstallRoot,
};

/// Steam reports DOOM (2016) as "DOOM". That title matches this install only
/// when the folder has a 2016 executable and not an Eternal or Dark Ages one.
pub fn is_doom_2016_install(install_path: &Path) -> bool {
    if !install_path.is_dir() {
        return false;
    }
    if install_has_file(install_path, "DOOMEternalx64vk.exe")
        || install_has_file(install_path, "DOOMEternal.exe")
        || install_has_file(install_path, "DOOMTheDarkAges.exe")
    {
        return false;
    }
    install_has_file(install_path, "DOOMx64.exe") || install_has_file(install_path, "DOOMx64vk.exe")
}

macro_rules! id_tech_plugin {
    ($struct:ident, $game:ident) => {
        pub struct $struct;

        impl GamePlugin for $struct {
            fn info(&self) -> GamePluginInfo {
                $game.info()
            }

            fn preserve_staging_root_names(&self) -> &[&str] {
                ID_TECH_ROOT_DIRS
            }

            fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
                $game.should_wrap(content_root)
            }

            fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
                content_folder_wrap_name(content_root, staged_name)
            }

            fn deploys_to_install_root(&self, content_root: &Path) -> bool {
                $game.deploys_to_install_root(content_root)
            }

            fn prunable_dirs(&self, install_path: &Path) -> Vec<PathBuf> {
                $game.prunable_dirs(install_path)
            }

            fn preflight_warnings(&self, install_path: &Path) -> Vec<String> {
                $game.preflight_warnings(install_path)
            }

            fn load_order_entries(&self, content_root: &Path, staged_name: &str) -> Vec<String> {
                $game.load_order_entries(content_root, staged_name)
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                Ok($game.resolve_deploy_root(install_path, relative))
            }

            fn after_deploy(
                &self,
                install_path: &Path,
                enabled_mod_folders: &[String],
            ) -> Result<Vec<String>> {
                $game.after_deploy(install_path, enabled_mod_folders)
            }
        }
    };
}

id_tech_plugin!(DoomTheDarkAgesPlugin, DARK_AGES);
id_tech_plugin!(DoomEternalPlugin, ETERNAL);
id_tech_plugin!(Doom2016Plugin, DOOM_2016);
id_tech_plugin!(Wolfenstein2Plugin, WOLFENSTEIN_2);
id_tech_plugin!(WolfensteinYoungbloodPlugin, WOLFENSTEIN_YOUNGBLOOD);
id_tech_plugin!(WolfensteinOldBloodPlugin, WOLFENSTEIN_OLD_BLOOD);
id_tech_plugin!(WolfensteinNewOrderPlugin, WOLFENSTEIN_NEW_ORDER);

fn resolve_injector(install_path: &Path, mods_dir: &str, relative: &Path) -> PathBuf {
    let normalized = normalize_relative(relative);
    let mut components = normalized.components().filter_map(|c| match c {
        Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
        _ => None,
    });
    let Some(first) = components.next() else {
        return install_path.join(mods_dir);
    };
    let mut out = install_path.join(mods_dir);
    if !first.eq_ignore_ascii_case("mods") {
        out.push(first);
    }
    for component in components {
        out.push(component);
    }
    out
}

fn resolve_install_root(install_path: &Path, relative: &Path) -> PathBuf {
    let normalized = normalize_relative(relative);
    if let Some(name) = normalized.file_name().and_then(|n| n.to_str()) {
        if PROXY_DLLS.iter().any(|dll| name.eq_ignore_ascii_case(dll)) {
            return install_path.join(name);
        }
    }
    install_path.join(normalized)
}

fn write_priority(
    folder: &Path,
    file_name: &str,
    priority: i32,
    create: bool,
) -> Result<Option<String>> {
    if let Some(existing) = find_child_file(folder, file_name) {
        if file_name.eq_ignore_ascii_case("EternalMod.json") {
            return update_json_priority(&existing, priority);
        }
        update_text_priority(&existing, priority)?;
        return Ok(None);
    }
    if !create {
        return Ok(None);
    }
    let path = folder.join(file_name);
    std::fs::write(&path, format!("loadPriority = {priority}\n"))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(None)
}

fn update_json_priority(path: &Path, priority: i32) -> Result<Option<String>> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(err) => {
            return Ok(Some(format!(
                "Could not read {} as JSON ({err}); load priority was left unchanged.",
                path.display()
            )));
        }
    };
    let Some(obj) = value.as_object_mut() else {
        return Ok(Some(format!(
            "{} is not a JSON object; load priority was left unchanged.",
            path.display()
        )));
    };
    obj.insert("loadPriority".to_string(), serde_json::json!(priority));
    let rendered = serde_json::to_string_pretty(&value)
        .with_context(|| format!("serialize {}", path.display()))?;
    std::fs::write(path, rendered + "\n").with_context(|| format!("write {}", path.display()))?;
    Ok(None)
}

fn update_text_priority(path: &Path, priority: i32) -> Result<()> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let updated = upsert_load_priority(&raw, priority);
    if updated != raw {
        std::fs::write(path, updated).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

fn upsert_load_priority(body: &str, priority: i32) -> String {
    let mut found = false;
    let mut out = String::with_capacity(body.len() + 24);
    for line in body.split_inclusive('\n') {
        let logical = line.trim_end_matches(['\r', '\n']);
        let key = logical
            .trim_start()
            .split_once('=')
            .map(|(key, _)| key.trim());
        if key.is_some_and(|key| key.eq_ignore_ascii_case("loadPriority")) {
            found = true;
            let newline = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            out.push_str("loadPriority = ");
            out.push_str(&priority.to_string());
            out.push_str(newline);
        } else {
            out.push_str(line);
        }
    }
    if !found {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("loadPriority = ");
        out.push_str(&priority.to_string());
        out.push('\n');
    }
    out
}

fn child_mod_folders(content_root: &Path) -> Vec<String> {
    let Some(mods) = find_child_dir(content_root, "mods") else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(mods) else {
        return names;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        names.push(name.to_string());
    }
    names.sort();
    names
}

fn has_child_dir(dir: &Path, name: &str) -> bool {
    find_child_dir(dir, name).is_some()
}

fn find_child_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    find_child(dir, name, true)
}

fn find_child_file(dir: &Path, name: &str) -> Option<PathBuf> {
    find_child(dir, name, false)
}

fn find_child(dir: &Path, name: &str, want_dir: bool) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            continue;
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if is_dir == want_dir {
            return Some(entry.path());
        }
    }
    None
}

fn install_has_file(install_path: &Path, name: &str) -> bool {
    find_child_file(install_path, name).is_some()
}

fn contains_named_file(root: &Path, names: &[&str]) -> bool {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry.file_type().is_file()
                && names.iter().any(|name| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(name)
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_a_decl_under_the_loader_mods_folder() {
        let staged = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(staged.path().join("generated").join("decls")).unwrap();
        assert!(DoomEternalPlugin.should_wrap_as_mod_folder(staged.path()));

        let dest = DoomEternalPlugin
            .resolve_deploy_root(
                Path::new("/game"),
                Path::new("CoolMod/generated/decls/foo.decl"),
            )
            .unwrap();
        assert_eq!(
            dest,
            PathBuf::from("/game/Mods/CoolMod/generated/decls/foo.decl")
        );

        let dark_ages = DoomTheDarkAgesPlugin
            .resolve_deploy_root(Path::new("/game"), Path::new("CoolMod/decls/a.decl"))
            .unwrap();
        assert_eq!(dark_ages, PathBuf::from("/game/mods/CoolMod/decls/a.decl"));
    }

    #[test]
    fn keeps_an_archive_that_already_starts_with_mods() {
        let staged = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(staged.path().join("Mods").join("CoolMod")).unwrap();
        assert!(!Doom2016Plugin.should_wrap_as_mod_folder(staged.path()));

        let dest = Doom2016Plugin
            .resolve_deploy_root(
                Path::new("/game"),
                Path::new("Mods/CoolMod/generated/decls/foo.decl"),
            )
            .unwrap();
        assert_eq!(
            dest,
            PathBuf::from("/game/Mods/CoolMod/generated/decls/foo.decl")
        );
    }

    #[test]
    fn loader_pack_deploys_at_the_install_root() {
        let staged = tempfile::tempdir().unwrap();
        std::fs::write(staged.path().join("DOOMModLoader.exe"), b"x").unwrap();
        std::fs::create_dir_all(staged.path().join("Mods")).unwrap();

        assert!(Doom2016Plugin.deploys_to_install_root(staged.path()));
        assert!(!Doom2016Plugin.should_wrap_as_mod_folder(staged.path()));
        assert!(Doom2016Plugin
            .load_order_entries(staged.path(), "DOOMModLoader")
            .is_empty());
    }

    #[test]
    fn wolfenstein_keeps_base_and_hoists_proxy_dlls() {
        let resources = Wolfenstein2Plugin
            .resolve_deploy_root(Path::new("/game"), Path::new("base/foo.resources"))
            .unwrap();
        assert_eq!(resources, PathBuf::from("/game/base/foo.resources"));

        let dll = WolfensteinNewOrderPlugin
            .resolve_deploy_root(Path::new("/game"), Path::new("nested/XINPUT1_3.dll"))
            .unwrap();
        assert_eq!(dll, PathBuf::from("/game/XINPUT1_3.dll"));

        let lower = WolfensteinOldBloodPlugin
            .resolve_deploy_root(Path::new("/game"), Path::new("pack/dinput8.DLL"))
            .unwrap();
        assert_eq!(lower, PathBuf::from("/game/dinput8.DLL"));

        let config = WolfensteinYoungbloodPlugin
            .resolve_deploy_root(Path::new("/game"), Path::new("KaibzMod/config.ini"))
            .unwrap();
        assert_eq!(config, PathBuf::from("/game/KaibzMod/config.ini"));

        let staging = tempfile::tempdir().unwrap();
        let base = staging.path().join("base");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("foo.resources"), b"x").unwrap();
        let kept = super::super::normalize_staging_root(staging.path(), &WolfensteinNewOrderPlugin)
            .unwrap();
        assert_eq!(kept, staging.path());
    }

    #[test]
    fn dark_ages_load_priority_lets_the_last_mod_win() {
        let install = tempfile::tempdir().unwrap();
        let mods = install.path().join("mods");
        std::fs::create_dir_all(mods.join("First")).unwrap();
        std::fs::create_dir_all(mods.join("Second")).unwrap();
        std::fs::write(
            mods.join("First").join("darkagesmod.txt"),
            "name = \"First\"\nloadPriority = 50\n",
        )
        .unwrap();

        let warnings = DoomTheDarkAgesPlugin
            .after_deploy(install.path(), &["First".into(), "Second".into()])
            .unwrap();
        assert!(warnings.is_empty());

        let first = std::fs::read_to_string(mods.join("First").join("darkagesmod.txt")).unwrap();
        assert!(first.contains("name = \"First\""));
        assert!(first.contains("loadPriority = 1"));
        assert!(!first.contains("50"));
        let second = std::fs::read_to_string(mods.join("Second").join("darkagesmod.txt")).unwrap();
        assert_eq!(second, "loadPriority = 0\n");
    }

    #[test]
    fn eternal_updates_existing_priority_files_and_warns() {
        let install = tempfile::tempdir().unwrap();
        let mods = install.path().join("Mods");
        std::fs::create_dir_all(mods.join("Alpha")).unwrap();
        std::fs::create_dir_all(mods.join("Beta")).unwrap();
        std::fs::write(
            mods.join("Alpha").join("EternalMod.json"),
            r#"{"name":"Alpha","author":"A","loadPriority":9}"#,
        )
        .unwrap();
        std::fs::write(
            mods.join("Alpha").join("atlanmod.txt"),
            "name = \"Alpha\"\r\nloadPriority = 4\r\n",
        )
        .unwrap();

        let warnings = DoomEternalPlugin
            .after_deploy(install.path(), &["Alpha".into(), "Beta".into()])
            .unwrap();
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("loose folders")));

        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(mods.join("Alpha").join("EternalMod.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(json["loadPriority"], 1);
        assert_eq!(json["name"], "Alpha");
        assert_eq!(json["author"], "A");
        let atlan = std::fs::read_to_string(mods.join("Alpha").join("atlanmod.txt")).unwrap();
        assert_eq!(atlan, "name = \"Alpha\"\r\nloadPriority = 1\r\n");
        assert!(!mods.join("Beta").join("EternalMod.json").exists());
        assert!(!mods.join("Beta").join("atlanmod.txt").exists());
    }

    #[test]
    fn preflight_warns_until_a_loader_is_present() {
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(Doom2016Plugin.preflight_warnings(empty.path()).len(), 1);
        assert_eq!(DoomEternalPlugin.preflight_warnings(empty.path()).len(), 1);
        assert_eq!(
            DoomTheDarkAgesPlugin.preflight_warnings(empty.path()).len(),
            1
        );
        assert!(Wolfenstein2Plugin
            .preflight_warnings(empty.path())
            .is_empty());

        std::fs::write(empty.path().join("DOOMModLoader"), b"x").unwrap();
        assert!(Doom2016Plugin.preflight_warnings(empty.path()).is_empty());

        let eternal = tempfile::tempdir().unwrap();
        std::fs::write(eternal.path().join("AtlanModLoader.exe"), b"x").unwrap();
        assert!(DoomEternalPlugin
            .preflight_warnings(eternal.path())
            .is_empty());
    }

    #[test]
    fn bare_doom_install_requires_a_2016_executable() {
        let doom = tempfile::tempdir().unwrap();
        assert!(!is_doom_2016_install(doom.path()));
        std::fs::write(doom.path().join("DOOMx64vk.exe"), b"x").unwrap();
        assert!(is_doom_2016_install(doom.path()));

        let eternal = tempfile::tempdir().unwrap();
        std::fs::write(eternal.path().join("DOOMx64.exe"), b"x").unwrap();
        std::fs::write(eternal.path().join("DOOMEternal.exe"), b"x").unwrap();
        assert!(!is_doom_2016_install(eternal.path()));
    }
}
