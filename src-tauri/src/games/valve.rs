//! Valve GoldSrc, Source 1, and Source 2 game plugins (plus Garry's Mod).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

const BACKUP_SUFFIX: &str = ".emperor-bak";

const VAC_WARNING: &str = "This game uses VAC on official servers. Only use mods in offline/local play or on -insecure / community servers that allow custom content.";

/// Top-level folder names preserved during archive root peeling.
const VALVE_ROOT_DIRS: &[&str] = &[
    // Source / GoldSrc asset directories
    "materials",
    "models",
    "sound",
    "scripts",
    "cfg",
    "maps",
    "resource",
    "particles",
    "scenes",
    "expressions",
    "media",
    "sprites",
    "gfx",
    "cl_dlls",
    "dlls",
    "overviews",
    "lua",
    "panorama",
    // Mod / addon containers
    "custom",
    "addons",
    "game",
    // Game directories
    "valve",
    "valve_addon",
    "gearbox",
    "gearbox_addon",
    "bshift",
    "bshift_addon",
    "cstrike",
    "cstrike_addon",
    "czero",
    "czero_addon",
    "dod",
    "dod_addon",
    "tfc",
    "tfc_addon",
    "dmc",
    "dmc_addon",
    "ricochet",
    "ricochet_addon",
    "hl2",
    "episodic",
    "ep2",
    "lostcoast",
    "hl2mp",
    "hl1",
    "hl1mp",
    "portal",
    "portal2",
    "tf",
    "left4dead",
    "left4dead2",
    "swarm",
    "garrysmod",
    "hlvr",
    "csgo",
    "dota",
    "citadel",
    "deskjob",
];

#[derive(Clone, Copy)]
enum Layout {
    GoldSrc {
        game_dir: &'static str,
    },
    Source1Custom {
        game_dir: &'static str,
        extra_game_dirs: &'static [&'static str],
    },
    Source1Addons {
        game_dir: &'static str,
        uses_addonlist: bool,
    },
    Source2 {
        game_dir: &'static str,
        sub_dir: &'static str,
        patch_gameinfo: bool,
        uses_addonlist: bool,
    },
}

#[derive(Clone, Copy)]
struct ValveGame {
    id: &'static str,
    display_name: &'static str,
    nexus_domain: &'static str,
    match_names: &'static [&'static str],
    layout: Layout,
    vac_warning: bool,
}

impl ValveGame {
    const fn info(self) -> GamePluginInfo {
        GamePluginInfo {
            id: self.id,
            display_name: self.display_name,
            nexus_domain: self.nexus_domain,
            match_names: self.match_names,
        }
    }

    fn should_wrap(&self, content_root: &Path) -> bool {
        match self.layout {
            Layout::GoldSrc { .. } => false,
            Layout::Source1Custom {
                game_dir,
                extra_game_dirs,
            } => should_wrap_source1_custom(content_root, game_dir, extra_game_dirs),
            Layout::Source1Addons { game_dir, .. } => {
                should_wrap_source1_addons(content_root, game_dir)
            }
            Layout::Source2 { sub_dir, .. } => should_wrap_source2(content_root, sub_dir),
        }
    }

    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> PathBuf {
        let normalized = normalize_relative(relative);
        let mut components = normalized.components().filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        });
        let Some(first) = components.next() else {
            return install_path.to_path_buf();
        };

        match self.layout {
            Layout::GoldSrc { game_dir } => {
                let addon_dir = format!("{game_dir}_addon");
                if first.eq_ignore_ascii_case(game_dir) || first.eq_ignore_ascii_case(&addon_dir) {
                    install_path.join(&normalized)
                } else {
                    install_path.join(game_dir).join(&normalized)
                }
            }
            Layout::Source1Custom {
                game_dir,
                extra_game_dirs,
            } => {
                if first.eq_ignore_ascii_case(game_dir)
                    || extra_game_dirs
                        .iter()
                        .any(|d| first.eq_ignore_ascii_case(d))
                {
                    install_path.join(&normalized)
                } else if first.eq_ignore_ascii_case("custom")
                    || first.eq_ignore_ascii_case("addons")
                {
                    install_path.join(game_dir).join(&normalized)
                } else {
                    install_path.join(game_dir).join("custom").join(&normalized)
                }
            }
            Layout::Source1Addons { game_dir, .. } => {
                if first.eq_ignore_ascii_case(game_dir) {
                    install_path.join(&normalized)
                } else if first.eq_ignore_ascii_case("addons")
                    || first.eq_ignore_ascii_case("maps")
                {
                    install_path.join(game_dir).join(&normalized)
                } else if is_single_bsp_file(&normalized) {
                    install_path.join(game_dir).join("maps").join(&normalized)
                } else {
                    install_path.join(game_dir).join("addons").join(&normalized)
                }
            }
            Layout::Source2 {
                game_dir, sub_dir, ..
            } => {
                if first.eq_ignore_ascii_case("game") {
                    install_path.join(&normalized)
                } else if first.eq_ignore_ascii_case(sub_dir) {
                    install_path.join("game").join(&normalized)
                } else if first.eq_ignore_ascii_case("addons") {
                    install_path.join(game_dir).join(&normalized)
                } else {
                    install_path.join(game_dir).join("addons").join(&normalized)
                }
            }
        }
    }

    fn prunable_dirs(&self, install_path: &Path) -> Vec<PathBuf> {
        match self.layout {
            Layout::GoldSrc { game_dir } => vec![
                install_path.join(game_dir).join("addons"),
                install_path.join(format!("{game_dir}_addon")),
            ],
            Layout::Source1Custom { game_dir, .. } => vec![
                install_path.join(game_dir).join("custom"),
                install_path.join(game_dir).join("addons"),
            ],
            Layout::Source1Addons { game_dir, .. } => vec![
                install_path.join(game_dir).join("addons"),
                install_path.join(game_dir).join("maps"),
            ],
            Layout::Source2 { game_dir, .. } => vec![install_path.join(game_dir).join("addons")],
        }
    }

    fn preflight_warnings(&self) -> Vec<String> {
        if self.vac_warning {
            vec![VAC_WARNING.to_string()]
        } else {
            Vec::new()
        }
    }

    fn after_deploy(&self, install_path: &Path) -> Result<Vec<String>> {
        let mut warnings = Vec::new();
        match self.layout {
            Layout::Source1Addons {
                game_dir,
                uses_addonlist: true,
            } => {
                update_addonlist(&install_path.join(game_dir))?;
            }
            Layout::Source2 {
                game_dir,
                sub_dir,
                patch_gameinfo,
                uses_addonlist,
            } => {
                let target_dir = install_path.join(game_dir);
                if patch_gameinfo {
                    if let Some(warn) = patch_source2_gameinfo(&target_dir, sub_dir)? {
                        warnings.push(warn);
                    }
                }
                if uses_addonlist {
                    update_addonlist(&target_dir)?;
                }
            }
            _ => {}
        }
        Ok(warnings)
    }

    fn on_revert(&self, install_path: &Path) -> Result<()> {
        match self.layout {
            Layout::Source1Addons {
                game_dir,
                uses_addonlist: true,
            } => {
                restore_backup(&install_path.join(game_dir).join("addonlist.txt"))?;
            }
            Layout::Source2 {
                game_dir,
                patch_gameinfo,
                uses_addonlist,
                ..
            } => {
                let target_dir = install_path.join(game_dir);
                if patch_gameinfo {
                    restore_backup(&target_dir.join("gameinfo.gi"))?;
                }
                if uses_addonlist {
                    restore_backup(&target_dir.join("addonlist.txt"))?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Game Definitions
// ---------------------------------------------------------------------------

// --- GoldSrc ---

const HALF_LIFE: ValveGame = ValveGame {
    id: "halflife",
    display_name: "Half-Life",
    nexus_domain: "halflife",
    match_names: &["half-life", "halflife"],
    layout: Layout::GoldSrc { game_dir: "valve" },
    vac_warning: false,
};

const HALF_LIFE_OPPOSING_FORCE: ValveGame = ValveGame {
    id: "halflifeopposingforce",
    display_name: "Half-Life: Opposing Force",
    nexus_domain: "halflifeopposingforce",
    match_names: &["opposing force"],
    layout: Layout::GoldSrc {
        game_dir: "gearbox",
    },
    vac_warning: false,
};

const HALF_LIFE_BLUE_SHIFT: ValveGame = ValveGame {
    id: "halflifeblueshift",
    display_name: "Half-Life: Blue Shift",
    nexus_domain: "halflifeblueshift",
    match_names: &["blue shift"],
    layout: Layout::GoldSrc { game_dir: "bshift" },
    vac_warning: false,
};

const COUNTER_STRIKE: ValveGame = ValveGame {
    id: "counterstrike",
    display_name: "Counter-Strike",
    nexus_domain: "counterstrike",
    match_names: &["counter-strike", "counter strike"],
    layout: Layout::GoldSrc {
        game_dir: "cstrike",
    },
    vac_warning: true,
};

const COUNTER_STRIKE_CZ: ValveGame = ValveGame {
    id: "cscz",
    display_name: "Counter-Strike: Condition Zero",
    nexus_domain: "",
    match_names: &["condition zero"],
    layout: Layout::GoldSrc { game_dir: "czero" },
    vac_warning: false,
};

const DAY_OF_DEFEAT: ValveGame = ValveGame {
    id: "dayofdefeat",
    display_name: "Day of Defeat",
    nexus_domain: "",
    match_names: &["day of defeat"],
    layout: Layout::GoldSrc { game_dir: "dod" },
    vac_warning: true,
};

const TEAM_FORTRESS_CLASSIC: ValveGame = ValveGame {
    id: "teamfortressclassic",
    display_name: "Team Fortress Classic",
    nexus_domain: "",
    match_names: &["team fortress classic"],
    layout: Layout::GoldSrc { game_dir: "tfc" },
    vac_warning: true,
};

const DEATHMATCH_CLASSIC: ValveGame = ValveGame {
    id: "deathmatchclassic",
    display_name: "Deathmatch Classic",
    nexus_domain: "",
    match_names: &["deathmatch classic"],
    layout: Layout::GoldSrc { game_dir: "dmc" },
    vac_warning: true,
};

const RICOCHET: ValveGame = ValveGame {
    id: "ricochet",
    display_name: "Ricochet",
    nexus_domain: "",
    match_names: &["ricochet"],
    layout: Layout::GoldSrc {
        game_dir: "ricochet",
    },
    vac_warning: true,
};

// --- Source 1 ---

const HALF_LIFE_2: ValveGame = ValveGame {
    id: "halflife2",
    display_name: "Half-Life 2",
    nexus_domain: "halflife2",
    match_names: &["half-life 2", "half life 2", "halflife2"],
    layout: Layout::Source1Custom {
        game_dir: "hl2",
        extra_game_dirs: &["episodic", "ep2", "lostcoast"],
    },
    vac_warning: false,
};

const HALF_LIFE_2_DM: ValveGame = ValveGame {
    id: "halflife2deathmatch",
    display_name: "Half-Life 2: Deathmatch",
    nexus_domain: "halflife2deathmatch",
    match_names: &["half-life 2 deathmatch", "half-life 2: deathmatch"],
    layout: Layout::Source1Custom {
        game_dir: "hl2mp",
        extra_game_dirs: &[],
    },
    vac_warning: true,
};

const HALF_LIFE_SOURCE: ValveGame = ValveGame {
    id: "halflifesource",
    display_name: "Half-Life: Source",
    nexus_domain: "halflifesource",
    match_names: &["half-life source", "half-life: source"],
    layout: Layout::Source1Custom {
        game_dir: "hl1",
        extra_game_dirs: &[],
    },
    vac_warning: false,
};

const HALF_LIFE_DM_SOURCE: ValveGame = ValveGame {
    id: "hldms",
    display_name: "Half-Life Deathmatch: Source",
    nexus_domain: "",
    match_names: &["half-life deathmatch: source", "half-life deathmatch source"],
    layout: Layout::Source1Custom {
        game_dir: "hl1mp",
        extra_game_dirs: &[],
    },
    vac_warning: true,
};

const PORTAL: ValveGame = ValveGame {
    id: "portal",
    display_name: "Portal",
    nexus_domain: "portal",
    match_names: &["portal"],
    layout: Layout::Source1Custom {
        game_dir: "portal",
        extra_game_dirs: &[],
    },
    vac_warning: false,
};

const PORTAL_2: ValveGame = ValveGame {
    id: "portal2",
    display_name: "Portal 2",
    nexus_domain: "portal2",
    match_names: &["portal 2", "portal2"],
    layout: Layout::Source1Addons {
        game_dir: "portal2",
        uses_addonlist: false,
    },
    vac_warning: false,
};

const TEAM_FORTRESS_2: ValveGame = ValveGame {
    id: "teamfortress2",
    display_name: "Team Fortress 2",
    nexus_domain: "teamfortress2",
    match_names: &["team fortress 2", "teamfortress2"],
    layout: Layout::Source1Custom {
        game_dir: "tf",
        extra_game_dirs: &[],
    },
    vac_warning: true,
};

const LEFT_4_DEAD: ValveGame = ValveGame {
    id: "left4dead",
    display_name: "Left 4 Dead",
    nexus_domain: "left4dead",
    match_names: &["left 4 dead", "left4dead"],
    layout: Layout::Source1Addons {
        game_dir: "left4dead",
        uses_addonlist: true,
    },
    vac_warning: false,
};

const LEFT_4_DEAD_2: ValveGame = ValveGame {
    id: "left4dead2",
    display_name: "Left 4 Dead 2",
    nexus_domain: "left4dead2",
    match_names: &["left 4 dead 2", "left4dead2"],
    layout: Layout::Source1Addons {
        game_dir: "left4dead2",
        uses_addonlist: true,
    },
    vac_warning: false,
};

const COUNTER_STRIKE_SOURCE: ValveGame = ValveGame {
    id: "counterstrikesource",
    display_name: "Counter-Strike: Source",
    nexus_domain: "counterstrikesource",
    match_names: &["counter-strike source", "counter-strike: source"],
    layout: Layout::Source1Custom {
        game_dir: "cstrike",
        extra_game_dirs: &[],
    },
    vac_warning: true,
};

const DAY_OF_DEFEAT_SOURCE: ValveGame = ValveGame {
    id: "dayofdefeatsource",
    display_name: "Day of Defeat: Source",
    nexus_domain: "",
    match_names: &["day of defeat source", "day of defeat: source"],
    layout: Layout::Source1Custom {
        game_dir: "dod",
        extra_game_dirs: &[],
    },
    vac_warning: true,
};

const ALIEN_SWARM: ValveGame = ValveGame {
    id: "alienswarm",
    display_name: "Alien Swarm",
    nexus_domain: "",
    match_names: &["alien swarm"],
    layout: Layout::Source1Addons {
        game_dir: "swarm",
        uses_addonlist: false,
    },
    vac_warning: false,
};

const GARRYS_MOD: ValveGame = ValveGame {
    id: "garrysmod",
    display_name: "Garry's Mod",
    nexus_domain: "garrysmod",
    match_names: &["garry's mod", "garrys mod", "garrysmod"],
    layout: Layout::Source1Addons {
        game_dir: "garrysmod",
        uses_addonlist: false,
    },
    vac_warning: false,
};

// --- Source 2 ---

const HALF_LIFE_ALYX: ValveGame = ValveGame {
    id: "halflifealyx",
    display_name: "Half-Life: Alyx",
    nexus_domain: "halflifealyx",
    match_names: &["half-life alyx", "half-life: alyx", "halflifealyx"],
    layout: Layout::Source2 {
        game_dir: "game/hlvr",
        sub_dir: "hlvr",
        patch_gameinfo: false,
        uses_addonlist: true,
    },
    vac_warning: false,
};

const COUNTER_STRIKE_2: ValveGame = ValveGame {
    id: "counterstrike2",
    display_name: "Counter-Strike 2",
    nexus_domain: "counterstrike2",
    match_names: &["counter-strike 2", "counter strike 2", "counterstrike2"],
    layout: Layout::Source2 {
        game_dir: "game/csgo",
        sub_dir: "csgo",
        patch_gameinfo: false,
        uses_addonlist: false,
    },
    vac_warning: true,
};

const DOTA_2: ValveGame = ValveGame {
    id: "dota2",
    display_name: "Dota 2",
    nexus_domain: "dota2",
    match_names: &["dota 2", "dota2"],
    layout: Layout::Source2 {
        game_dir: "game/dota",
        sub_dir: "dota",
        patch_gameinfo: false,
        uses_addonlist: false,
    },
    vac_warning: true,
};

const DEADLOCK: ValveGame = ValveGame {
    id: "deadlock",
    display_name: "Deadlock",
    nexus_domain: "deadlock",
    match_names: &["deadlock"],
    layout: Layout::Source2 {
        game_dir: "game/citadel",
        sub_dir: "citadel",
        patch_gameinfo: true,
        uses_addonlist: false,
    },
    vac_warning: false,
};

const APERTURE_DESK_JOB: ValveGame = ValveGame {
    id: "aperturedeskjob",
    display_name: "Aperture Desk Job",
    nexus_domain: "",
    match_names: &["aperture desk job"],
    layout: Layout::Source2 {
        game_dir: "game/deskjob",
        sub_dir: "deskjob",
        patch_gameinfo: false,
        uses_addonlist: false,
    },
    vac_warning: false,
};

macro_rules! valve_plugin {
    ($struct:ident, $game:ident) => {
        pub struct $struct;

        impl GamePlugin for $struct {
            fn info(&self) -> GamePluginInfo {
                $game.info()
            }

            fn preserve_staging_root_names(&self) -> &[&str] {
                VALVE_ROOT_DIRS
            }

            fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
                $game.should_wrap(content_root)
            }

            fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
                content_folder_wrap_name(content_root, staged_name)
            }

            fn prunable_dirs(&self, install_path: &Path) -> Vec<PathBuf> {
                $game.prunable_dirs(install_path)
            }

            fn preflight_warnings(&self, _install_path: &Path) -> Vec<String> {
                $game.preflight_warnings()
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                Ok($game.resolve_deploy_root(install_path, relative))
            }

            fn after_deploy(
                &self,
                install_path: &Path,
                _enabled_mod_folders: &[String],
            ) -> Result<Vec<String>> {
                $game.after_deploy(install_path)
            }

            fn on_revert(&self, install_path: &Path) -> Result<()> {
                $game.on_revert(install_path)
            }
        }
    };
}

// GoldSrc
valve_plugin!(HalfLifePlugin, HALF_LIFE);
valve_plugin!(HalfLifeOpposingForcePlugin, HALF_LIFE_OPPOSING_FORCE);
valve_plugin!(HalfLifeBlueShiftPlugin, HALF_LIFE_BLUE_SHIFT);
valve_plugin!(CounterStrikePlugin, COUNTER_STRIKE);
valve_plugin!(CounterStrikeConditionZeroPlugin, COUNTER_STRIKE_CZ);
valve_plugin!(DayOfDefeatPlugin, DAY_OF_DEFEAT);
valve_plugin!(TeamFortressClassicPlugin, TEAM_FORTRESS_CLASSIC);
valve_plugin!(DeathmatchClassicPlugin, DEATHMATCH_CLASSIC);
valve_plugin!(RicochetPlugin, RICOCHET);

// Source 1
valve_plugin!(HalfLife2Plugin, HALF_LIFE_2);
valve_plugin!(HalfLife2DeathmatchPlugin, HALF_LIFE_2_DM);
valve_plugin!(HalfLifeSourcePlugin, HALF_LIFE_SOURCE);
valve_plugin!(HalfLifeDeathmatchSourcePlugin, HALF_LIFE_DM_SOURCE);
valve_plugin!(PortalPlugin, PORTAL);
valve_plugin!(Portal2Plugin, PORTAL_2);
valve_plugin!(TeamFortress2Plugin, TEAM_FORTRESS_2);
valve_plugin!(Left4DeadPlugin, LEFT_4_DEAD);
valve_plugin!(Left4Dead2Plugin, LEFT_4_DEAD_2);
valve_plugin!(CounterStrikeSourcePlugin, COUNTER_STRIKE_SOURCE);
valve_plugin!(DayOfDefeatSourcePlugin, DAY_OF_DEFEAT_SOURCE);
valve_plugin!(AlienSwarmPlugin, ALIEN_SWARM);
valve_plugin!(GarrysModPlugin, GARRYS_MOD);

// Source 2
valve_plugin!(HalfLifeAlyxPlugin, HALF_LIFE_ALYX);
valve_plugin!(CounterStrike2Plugin, COUNTER_STRIKE_2);
valve_plugin!(Dota2Plugin, DOTA_2);
valve_plugin!(DeadlockPlugin, DEADLOCK);
valve_plugin!(ApertureDeskJobPlugin, APERTURE_DESK_JOB);

// ---------------------------------------------------------------------------
// Layout & Wrapping Helpers
// ---------------------------------------------------------------------------

fn top_level_entries(content_root: &Path) -> Vec<(String, bool, bool)> {
    let Ok(rd) = fs::read_dir(content_root) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok())
        .filter_map(|e| {
            let ft = e.file_type().ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            Some((name, ft.is_dir(), ft.is_file()))
        })
        .collect()
}

fn has_ext(name: &str, ext: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case(ext))
}

fn is_single_bsp_file(rel: &Path) -> bool {
    rel.components().count() == 1
        && rel
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("bsp"))
}

fn should_wrap_source1_custom(
    content_root: &Path,
    game_dir: &str,
    extra_game_dirs: &[&str],
) -> bool {
    let entries = top_level_entries(content_root);
    if entries.is_empty() {
        return false;
    }

    for (name, is_dir, _) in &entries {
        if *is_dir
            && (name.eq_ignore_ascii_case("custom")
                || name.eq_ignore_ascii_case("addons")
                || name.eq_ignore_ascii_case(game_dir)
                || extra_game_dirs
                    .iter()
                    .any(|d| name.eq_ignore_ascii_case(d)))
        {
            return false;
        }
    }

    // Loose Source 1 asset directories must sit inside a subfolder under custom/.
    const LOOSE_ASSET_DIRS: &[&str] = &[
        "materials",
        "models",
        "sound",
        "scripts",
        "cfg",
        "maps",
        "resource",
        "particles",
        "scenes",
        "expressions",
        "media",
    ];
    entries.iter().any(|(name, is_dir, _)| {
        *is_dir
            && LOOSE_ASSET_DIRS
                .iter()
                .any(|d| name.eq_ignore_ascii_case(d))
    })
}

fn should_wrap_source1_addons(content_root: &Path, game_dir: &str) -> bool {
    let entries = top_level_entries(content_root);
    if entries.is_empty() {
        return false;
    }

    for (name, is_dir, _) in &entries {
        if *is_dir
            && (name.eq_ignore_ascii_case("addons")
                || name.eq_ignore_ascii_case("maps")
                || name.eq_ignore_ascii_case(game_dir))
        {
            return false;
        }
    }

    const ADDON_MARKERS: &[&str] = &[
        "addoninfo.txt",
        "addon.json",
        "lua",
        "materials",
        "models",
        "sound",
        "scripts",
        "resource",
        "particles",
    ];
    entries.iter().any(|(name, _, _)| {
        ADDON_MARKERS
            .iter()
            .any(|m| name.eq_ignore_ascii_case(m))
    })
}

fn should_wrap_source2(content_root: &Path, sub_dir: &str) -> bool {
    let entries = top_level_entries(content_root);
    if entries.is_empty() {
        return false;
    }

    for (name, is_dir, _) in &entries {
        if *is_dir
            && (name.eq_ignore_ascii_case("game")
                || name.eq_ignore_ascii_case("addons")
                || name.eq_ignore_ascii_case(sub_dir))
        {
            return false;
        }
    }

    const SOURCE2_MARKERS: &[&str] = &[
        "addoninfo.txt",
        "maps",
        "materials",
        "models",
        "sound",
        "particles",
        "panorama",
        "scripts",
    ];
    entries.iter().any(|(name, is_dir, is_file)| {
        (*is_dir || *is_file)
            && SOURCE2_MARKERS
                .iter()
                .any(|m| name.eq_ignore_ascii_case(m))
    })
}

// ---------------------------------------------------------------------------
// Config Patching & Revert Helpers
// ---------------------------------------------------------------------------

fn backup_path_for(target: &Path) -> PathBuf {
    let mut os = target.as_os_str().to_os_string();
    os.push(BACKUP_SUFFIX);
    PathBuf::from(os)
}

fn ensure_backup(target: &Path) -> Result<()> {
    if !target.is_file() {
        return Ok(());
    }
    let backup = backup_path_for(target);
    if !backup.exists() {
        fs::copy(target, &backup).with_context(|| {
            format!(
                "Failed to back up {} to {}",
                target.display(),
                backup.display()
            )
        })?;
    }
    Ok(())
}

fn restore_backup(target: &Path) -> Result<()> {
    let backup = backup_path_for(target);
    if backup.is_file() {
        fs::copy(&backup, target).with_context(|| {
            format!(
                "Failed to restore {} from {}",
                target.display(),
                backup.display()
            )
        })?;
        fs::remove_file(&backup)
            .with_context(|| format!("Failed to remove backup {}", backup.display()))?;
    }
    Ok(())
}

/// Enable deployed `.vpk` files and addon directories in `<game_dir>/addonlist.txt`.
fn update_addonlist(game_dir: &Path) -> Result<()> {
    let addons_dir = game_dir.join("addons");
    if !addons_dir.is_dir() {
        return Ok(());
    }

    let mut deployed_addons = BTreeSet::new();
    for entry in fs::read_dir(&addons_dir)?.filter_map(|e| e.ok()) {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if ft.is_file() && has_ext(&name, "vpk") {
            deployed_addons.insert(name);
        } else if ft.is_dir() && !name.eq_ignore_ascii_case("workshop") {
            deployed_addons.insert(name);
        }
    }

    if deployed_addons.is_empty() {
        return Ok(());
    }

    let addonlist_path = game_dir.join("addonlist.txt");
    ensure_backup(&addonlist_path)?;

    let mut existing_order: Vec<(String, String)> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    if addonlist_path.is_file() {
        let content = fs::read_to_string(&addonlist_path).unwrap_or_default();
        for line in content.lines() {
            if let Some((key, val)) = parse_kv_pair(line) {
                let lower = key.to_lowercase();
                if seen.insert(lower.clone()) {
                    let next_val = if deployed_addons.iter().any(|d| d.eq_ignore_ascii_case(&key)) {
                        "1".to_string()
                    } else {
                        val
                    };
                    existing_order.push((key, next_val));
                }
            }
        }
    }

    for addon in deployed_addons {
        if seen.insert(addon.to_lowercase()) {
            existing_order.push((addon, "1".to_string()));
        }
    }

    let mut out = String::from("\"AddonList\"\n{\n");
    for (key, val) in existing_order {
        out.push_str(&format!("\t\"{key}\"\t\t\"{val}\"\n"));
    }
    out.push_str("}\n");
    fs::write(&addonlist_path, out)
        .with_context(|| format!("Failed to write {}", addonlist_path.display()))?;
    Ok(())
}

fn parse_kv_pair(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('"') {
        return None;
    }
    let mut parts = Vec::new();
    let mut rest = trimmed;
    while let Some(start) = rest.find('"') {
        let after_start = &rest[start + 1..];
        let end = after_start.find('"')?;
        parts.push(after_start[..end].to_string());
        rest = &after_start[end + 1..];
        if parts.len() == 2 {
            break;
        }
    }
    if parts.len() == 2 {
        Some((parts[0].clone(), parts[1].clone()))
    } else {
        None
    }
}

/// Insert `Game <sub_dir>/addons` into Source 2 `gameinfo.gi`'s `SearchPaths` block.
fn patch_source2_gameinfo(target_dir: &Path, sub_dir: &str) -> Result<Option<String>> {
    let gameinfo_path = target_dir.join("gameinfo.gi");
    if !gameinfo_path.is_file() {
        return Ok(Some(format!(
            "Could not find {} to enable addons SearchPaths.",
            gameinfo_path.display()
        )));
    }

    let addons_entry = format!("{sub_dir}/addons");
    let raw = fs::read_to_string(&gameinfo_path)
        .with_context(|| format!("Failed to read {}", gameinfo_path.display()))?;

    if raw
        .to_ascii_lowercase()
        .contains(&addons_entry.to_ascii_lowercase())
    {
        return Ok(None);
    }

    ensure_backup(&gameinfo_path)?;

    let Some(patched) = insert_search_path(&raw, &addons_entry) else {
        return Ok(Some(format!(
            "Could not locate SearchPaths block in {}; add `Game {addons_entry}` manually.",
            gameinfo_path.display()
        )));
    };

    fs::write(&gameinfo_path, patched)
        .with_context(|| format!("Failed to write {}", gameinfo_path.display()))?;
    Ok(None)
}

fn insert_search_path(raw: &str, addons_entry: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len() + 64);
    let mut in_search_paths = false;
    let mut waiting_for_brace = false;
    let mut inserted = false;

    for line in raw.split_inclusive('\n') {
        let trimmed = line.trim();
        if !inserted {
            if !in_search_paths && trimmed.eq_ignore_ascii_case("SearchPaths") {
                waiting_for_brace = true;
            } else if waiting_for_brace && trimmed.starts_with('{') {
                in_search_paths = true;
                waiting_for_brace = false;
                out.push_str(line);
                out.push_str(&format!("\t\t\tGame\t\t\t\t{addons_entry}\n"));
                inserted = true;
                continue;
            }
        }
        out.push_str(line);
    }

    inserted.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn goldsrc_resolves_into_game_dir_and_preserves_addon_dir() {
        let install = TempDir::new().unwrap();
        let dest = HalfLifePlugin
            .resolve_deploy_root(install.path(), Path::new("models/v_crowbar.mdl"))
            .unwrap();
        assert_eq!(dest, install.path().join("valve/models/v_crowbar.mdl"));

        let prefixed = HalfLifePlugin
            .resolve_deploy_root(install.path(), Path::new("valve_addon/sound/foo.wav"))
            .unwrap();
        assert_eq!(
            prefixed,
            install.path().join("valve_addon/sound/foo.wav")
        );
    }

    #[test]
    fn source1_custom_wraps_loose_assets_and_keeps_vpks_unwrapped() {
        let install = TempDir::new().unwrap();
        let loose = TempDir::new().unwrap();
        fs::create_dir_all(loose.path().join("materials")).unwrap();
        assert!(HalfLife2Plugin.should_wrap_as_mod_folder(loose.path()));

        let vpk_only = TempDir::new().unwrap();
        fs::write(vpk_only.path().join("hd_pack.vpk"), b"").unwrap();
        assert!(!HalfLife2Plugin.should_wrap_as_mod_folder(vpk_only.path()));

        let dest_vpk = HalfLife2Plugin
            .resolve_deploy_root(install.path(), Path::new("hd_pack.vpk"))
            .unwrap();
        assert_eq!(dest_vpk, install.path().join("hl2/custom/hd_pack.vpk"));

        let dest_ep2 = HalfLife2Plugin
            .resolve_deploy_root(install.path(), Path::new("ep2/custom/mod/a.vtf"))
            .unwrap();
        assert_eq!(dest_ep2, install.path().join("ep2/custom/mod/a.vtf"));

        let dest_addons = TeamFortress2Plugin
            .resolve_deploy_root(install.path(), Path::new("addons/sourcemod/plugins/a.smx"))
            .unwrap();
        assert_eq!(
            dest_addons,
            install.path().join("tf/addons/sourcemod/plugins/a.smx")
        );
    }

    #[test]
    fn source1_addons_patches_and_reverts_l4d2_addonlist() {
        let install = TempDir::new().unwrap();
        let l4d2 = install.path().join("left4dead2");
        fs::create_dir_all(l4d2.join("addons")).unwrap();
        fs::write(
            l4d2.join("addonlist.txt"),
            "\"AddonList\"\n{\n\t\"existing.vpk\"\t\t\"0\"\n}\n",
        )
        .unwrap();
        fs::write(l4d2.join("addons/custom_campaign.vpk"), b"vpk").unwrap();

        let warnings = Left4Dead2Plugin.after_deploy(install.path(), &[]).unwrap();
        assert!(warnings.is_empty());

        let updated = fs::read_to_string(l4d2.join("addonlist.txt")).unwrap();
        assert!(updated.contains("\"custom_campaign.vpk\"\t\t\"1\""));
        assert!(updated.contains("\"existing.vpk\"\t\t\"0\""));
        assert!(l4d2.join("addonlist.txt.emperor-bak").is_file());

        Left4Dead2Plugin.on_revert(install.path()).unwrap();
        let restored = fs::read_to_string(l4d2.join("addonlist.txt")).unwrap();
        assert!(!restored.contains("custom_campaign.vpk"));
        assert!(!l4d2.join("addonlist.txt.emperor-bak").exists());
    }

    #[test]
    fn deadlock_patches_and_reverts_gameinfo() {
        let install = TempDir::new().unwrap();
        let citadel = install.path().join("game/citadel");
        fs::create_dir_all(&citadel).unwrap();
        let original = "\"GameInfo\"\n{\n\tFileSystem\n\t{\n\t\tSearchPaths\n\t\t{\n\t\t\tGame\t\t\t\tcitadel\n\t\t}\n\t}\n}\n";
        fs::write(citadel.join("gameinfo.gi"), original).unwrap();

        let dest = DeadlockPlugin
            .resolve_deploy_root(install.path(), Path::new("pak01_dir.vpk"))
            .unwrap();
        assert_eq!(dest, citadel.join("addons/pak01_dir.vpk"));

        let warnings = DeadlockPlugin.after_deploy(install.path(), &[]).unwrap();
        assert!(warnings.is_empty());

        let patched = fs::read_to_string(citadel.join("gameinfo.gi")).unwrap();
        assert!(patched.contains("citadel/addons"));
        assert!(citadel.join("gameinfo.gi.emperor-bak").is_file());

        DeadlockPlugin.on_revert(install.path()).unwrap();
        let restored = fs::read_to_string(citadel.join("gameinfo.gi")).unwrap();
        assert_eq!(restored, original);
        assert!(!citadel.join("gameinfo.gi.emperor-bak").exists());
    }

    #[test]
    fn vac_warning_only_on_vac_protected_titles() {
        assert!(!CounterStrike2Plugin
            .preflight_warnings(Path::new("/tmp"))
            .is_empty());
        assert!(!TeamFortress2Plugin
            .preflight_warnings(Path::new("/tmp"))
            .is_empty());
        assert!(HalfLife2Plugin
            .preflight_warnings(Path::new("/tmp"))
            .is_empty());
        assert!(Portal2Plugin
            .preflight_warnings(Path::new("/tmp"))
            .is_empty());
    }
}
