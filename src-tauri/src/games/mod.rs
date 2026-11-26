//! Per-game deploy plugins.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Paths;
use crate::mods::options::ModOptionSet;
use crate::nexus::NexusClient;

mod assettocorsa;
mod baldursgate3;
mod beamng;
mod bepinex_install;
mod bladeandsorcery;
mod civilization;
mod controlresonant;
pub(crate) mod creation_engine;
mod cyberpunk2077;
mod daysgone;
mod decima;
mod diablo2r;
mod dragonageorigins;
mod dressmaker;
mod exe_version;
mod factorio;
mod farmingsimulator;
mod firstlight;
mod folder_mods;
mod fromsoft_loaders;
mod fromsoftware;
mod ghostoftsushima;
mod hades;
mod helldivers2;
mod id_tech;
mod kenshi;
mod kingdomcome;
mod kotor;
mod ksp;
mod log_sources;
pub mod loot;
mod melonloader;
mod monsterhunterworld;
mod mountandblade;
mod mysummercar;
mod nomanssky;
mod paradox;
mod projectzomboid;
mod projectzomboid_pz3d;
mod rage_engine;
mod re_engine;
mod rimworld;
mod scstrucksim;
mod sevendaystodie;
mod sims2;
mod sims3;
mod sims4;
mod sims_framework;
mod smapi_install;
mod snowrunner;
mod spaceengineers;
mod stardewvalley;
mod starsector;
mod theforest;
mod totalwar;
mod umm;
mod unity_bepinex;
mod unreal_engine;
mod user_data;
mod valve;
mod warhammer40kdarktide;
mod wine_overrides;
mod witcher3;
mod x4foundations;
mod xray_engine;

pub use assettocorsa::AssettoCorsaPlugin;
pub use baldursgate3::{BaldursGate3Plugin, DivinityOriginalSin2Plugin};
pub use beamng::BeamNgDrivePlugin;
pub use bladeandsorcery::BladeAndSorceryPlugin;
pub use civilization::{Civilization6Plugin, Civilization7Plugin};
pub use controlresonant::ControlResonantPlugin;
pub use creation_engine::{
    Fallout3Plugin, Fallout4Plugin, FalloutNewVegasPlugin, MorrowindPlugin, OblivionPlugin,
    OblivionRemasteredPlugin, SkyrimPlugin, SkyrimSpecialEditionPlugin, StarfieldPlugin,
};
pub use cyberpunk2077::Cyberpunk2077Plugin;
pub use daysgone::DaysGonePlugin;
pub use decima::{DeathStrandingPlugin, HorizonForbiddenWestPlugin, HorizonZeroDawnPlugin};
pub use diablo2r::Diablo2ResurrectedPlugin;
pub use dragonageorigins::DragonAgeOriginsPlugin;
pub(crate) use dressmaker::plugin_id_for_install;
pub use dressmaker::DressmakerPlugin;
pub use exe_version::detect_install_version;
pub use factorio::FactorioPlugin;
pub use farmingsimulator::{FarmingSimulator22Plugin, FarmingSimulator25Plugin};
pub use firstlight::FirstLightPlugin;
pub use folder_mods::{
    BarotraumaPlugin, DarkestDungeonPlugin, DwarfFortressPlugin, VintageStoryPlugin,
};
pub use fromsoft_loaders::{
    install_legacy_from_archive, me3_game_name, resolve_me3_installation,
};
pub use fromsoftware::{
    resolve_game_root as fromsoft_resolve_game_root, ArmoredCore6Plugin, DarkSouls2Plugin,
    DarkSouls3Plugin, DarkSoulsPlugin, DarkSoulsRemasteredPlugin, EldenRingNightreignPlugin,
    EldenRingPlugin, SekiroPlugin,
};
pub use ghostoftsushima::GhostOfTsushimaPlugin;
pub use hades::{Hades2Plugin, HadesPlugin};
pub use helldivers2::Helldivers2Plugin;
pub use id_tech::{
    Doom2016Plugin, DoomEternalPlugin, DoomTheDarkAgesPlugin, Wolfenstein2Plugin,
    WolfensteinNewOrderPlugin, WolfensteinOldBloodPlugin, WolfensteinYoungbloodPlugin,
};
pub use kenshi::KenshiPlugin;
pub use kingdomcome::{KingdomComeDeliverance2Plugin, KingdomComeDeliverancePlugin};
pub use kotor::{Kotor2Plugin, KotorPlugin};
pub use ksp::KerbalSpaceProgramPlugin;
pub use melonloader::{BloonsTd6Plugin, BonelabPlugin, BoneworksPlugin, TheLongDarkPlugin};
pub use monsterhunterworld::{DragonsDogmaDarkArisenPlugin, MonsterHunterWorldPlugin};
pub use mountandblade::{MountAndBlade2BannerlordPlugin, MountAndBladeWarbandPlugin};
pub use mysummercar::MySummerCarPlugin;
pub use nomanssky::NoMansSkyPlugin;
pub use paradox::{
    CrusaderKings3Plugin, EuropaUniversalis4Plugin, HeartsOfIron4Plugin, StellarisPlugin,
    Victoria3Plugin,
};
pub use projectzomboid::ProjectZomboidPlugin;
pub use rage_engine::{GrandTheftAuto5Plugin, RedDeadRedemption2Plugin};
pub use re_engine::{
    DevilMayCry5Plugin, DragonsDogma2Plugin, MonsterHunterRisePlugin, MonsterHunterWildsPlugin,
    ResidentEvil22019Plugin, ResidentEvil32020Plugin, ResidentEvil42023Plugin, ResidentEvil7Plugin,
    ResidentEvilRequiemPlugin, ResidentEvilVillagePlugin, StreetFighter6Plugin,
};
pub use rimworld::RimWorldPlugin;
pub use scstrucksim::{AmericanTruckSimulatorPlugin, EuroTruckSimulator2Plugin};
pub use sevendaystodie::SevenDaysToDiePlugin;
pub use sims2::Sims2Plugin;
pub use sims3::Sims3Plugin;
pub use sims4::Sims4Plugin;
pub use snowrunner::SnowRunnerPlugin;
pub use spaceengineers::SpaceEngineersPlugin;
pub use stardewvalley::StardewValleyPlugin;
pub use starsector::StarsectorPlugin;
pub use theforest::TheForestPlugin;
pub use totalwar::{
    TotalWarRome2Plugin, TotalWarShogun2Plugin, TotalWarThreeKingdomsPlugin,
    TotalWarWarhammer3Plugin,
};
pub use umm::{PathfinderWotrPlugin, Warhammer40kRogueTraderPlugin};
pub use unity_bepinex::{
    bepinex_pack_deploy_root, looks_like_unity_install, rewrite_ror2_bepinexpack_dest,
    thunderstore_community_for_plugin, AgainstTheStormPlugin, AmongUsPlugin, AtlyssPlugin,
    BepInExBuild, BepInExPlugin, ChefRpgPlugin, ContentWarningPlugin, CultOfTheLambPlugin,
    DysonSphereProgramPlugin, GtfoPlugin, H3vrPlugin, HollowKnightSilksongPlugin,
    InscryptionPlugin, LethalCompanyPlugin, PeakPlugin, RepoPlugin, RiskOfRain2Plugin,
    RoundsPlugin, Schedule1Plugin, SonsOfTheForestPlugin, SubnauticaBelowZeroPlugin,
    SubnauticaPlugin, TimberbornPlugin, UltrakillPlugin, ValheimPlugin,
};
pub use unreal_engine::{
    detect_ue_layout, layout_info, DeepRockGalacticPlugin, DeployContext, HogwartsLegacyPlugin,
    MarvelRivalsPlugin, PalworldPlugin, PavlovPlugin, ReadyOrNotPlugin,
    Stalker2HeartOfChornobylPlugin, Subnautica2Plugin, UeLayoutInfo, UnrealEnginePlugin,
};
pub use valve::{
    AlienSwarmPlugin, ApertureDeskJobPlugin, CounterStrike2Plugin,
    CounterStrikeConditionZeroPlugin, CounterStrikePlugin, CounterStrikeSourcePlugin,
    DayOfDefeatPlugin, DayOfDefeatSourcePlugin, DeadlockPlugin, DeathmatchClassicPlugin,
    Dota2Plugin, GarrysModPlugin, HalfLife2DeathmatchPlugin, HalfLife2Plugin, HalfLifeAlyxPlugin,
    HalfLifeBlueShiftPlugin, HalfLifeDeathmatchSourcePlugin, HalfLifeOpposingForcePlugin,
    HalfLifePlugin, HalfLifeSourcePlugin, Left4Dead2Plugin, Left4DeadPlugin, Portal2Plugin,
    PortalPlugin, RicochetPlugin, TeamFortress2Plugin, TeamFortressClassicPlugin,
};
pub use warhammer40kdarktide::Warhammer40kDarktidePlugin;
pub use wine_overrides::WineOverrideAdvice;
#[cfg(target_os = "linux")]
pub use wine_overrides::{advise_wine_overrides, StagedProxySource};
pub use witcher3::Witcher3Plugin;
pub use x4foundations::X4FoundationsPlugin;
pub use xray_engine::{
    StalkerAnomalyPlugin, StalkerCallOfPripyatPlugin, StalkerShadowOfChernobylPlugin,
};

/// What `deploy_mods` knows about the install while a plugin prepares it.
pub struct RuntimePrep<'a> {
    pub paths: &'a Paths,
    pub install_path: &'a Path,
    pub nexus: Option<&'a NexusClient>,
    pub premium: bool,
    /// Normalized content roots of enabled staged mods.
    pub content_roots: &'a [&'a Path],
}

/// Nexus download the command should open in Download Assist, then poll.
#[derive(Debug, Clone)]
pub struct LoaderAssistRequest {
    pub domain: String,
    pub mod_id: u64,
    pub file_id: u64,
    pub plugin_id: String,
    pub install_path: PathBuf,
    pub label: String,
}

/// Result of [`GamePlugin::prepare_runtime`].
#[derive(Debug)]
pub enum RuntimePrepOutcome {
    Done(Vec<String>),
    NeedsAssist {
        warnings: Vec<String>,
        request: LoaderAssistRequest,
    },
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct GamePluginInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub nexus_domain: &'static str,
    pub match_names: &'static [&'static str],
}

/// Plugin metadata returned by `list_plugins`, including whether the plugin
/// exposes optional game-specific tools in the Tools modal.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct GamePluginSummary {
    #[serde(flatten)]
    pub info: GamePluginInfo,
    pub has_tools: bool,
}

/// Status of an optional game-specific tool provided by a `GamePlugin`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginToolStatus {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub homepage_url: Option<String>,
    pub installed: bool,
    #[serde(default)]
    pub installed_version: Option<String>,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub update_available: bool,
    #[serde(default)]
    pub status_detail: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub can_uninstall: bool,
}

/// Result of installing or uninstalling a plugin-provided tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginToolActionResult {
    pub tool: PluginToolStatus,
    pub message: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}

pub trait GamePlugin: Send + Sync {
    fn info(&self) -> GamePluginInfo;

    /// Resolve where staged mod files should be linked inside the game install.
    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf>;

    /// Resolve with optional deploy context (UE project override, LogicMod markers).
    fn resolve_deploy_root_ctx(
        &self,
        install_path: &Path,
        relative: &Path,
        ctx: &DeployContext<'_>,
    ) -> Result<PathBuf> {
        let _ = ctx;
        self.resolve_deploy_root(install_path, relative)
    }

    /// Prefer a subdirectory name under Mods/ for folder-style games.
    fn prefers_mod_folder(&self) -> bool {
        false
    }

    /// Top-level staging folder names that must not be peeled as archive wrappers.
    fn preserve_staging_root_names(&self) -> &[&str] {
        &[]
    }

    /// When true, a single top-level archive folder with this name is kept.
    fn preserves_staging_root(&self, name: &str) -> bool {
        self.preserve_staging_root_names()
            .iter()
            .any(|root| name.eq_ignore_ascii_case(root))
    }

    /// When true, deploy under a sanitized mod-name folder (e.g. REDmod info.json packs).
    fn should_wrap_as_mod_folder(&self, _content_root: &Path) -> bool {
        false
    }

    /// Name of the directory used when wrapping a staged mod.
    fn wrap_mod_folder_name(&self, _content_root: &Path, staged_name: &str) -> String {
        sanitize_filename::sanitize(staged_name)
    }

    /// When true, link files at the game install root instead of via `resolve_deploy_root`
    /// (e.g. SMAPI installer packages).
    fn deploys_to_install_root(&self, _content_root: &Path) -> bool {
        false
    }

    /// When false, a staged file is not linked into the game (e.g. readmes and
    /// preview images that the game never loads). `relative` is the path inside
    /// the staged mod, before any mod-folder wrapping.
    fn should_deploy_file(&self, _relative: &Path) -> bool {
        true
    }

    /// Same as `should_deploy_file`, with the staging layout available.
    fn should_deploy_file_ctx(&self, relative: &Path, ctx: &DeployContext<'_>) -> bool {
        let _ = ctx;
        self.should_deploy_file(relative)
    }

    /// Directories owned by the mod loader whose empty leftovers should be pruned
    /// on purge (e.g. `BepInEx/plugins` folders from mods that are gone).
    fn prunable_dirs(&self, _install_path: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Install-level warnings shown before/during deploy (e.g. missing mod loader).
    fn preflight_warnings(&self, _install_path: &Path) -> Vec<String> {
        Vec::new()
    }

    fn preflight_warnings_ctx(&self, install_path: &Path, ctx: &DeployContext<'_>) -> Vec<String> {
        let _ = ctx;
        self.preflight_warnings(install_path)
    }

    /// Per-staging-pack warnings (e.g. SMAPI installer deployed as a mod).
    fn staging_deploy_warnings(&self, _content_root: &Path, _mod_name: &str) -> Vec<String> {
        Vec::new()
    }

    /// When true, deploying zero files from this pack is expected (a save overwrite
    /// that is applied beside the install, not linked into it).
    fn allows_empty_deploy(&self, _content_root: &Path) -> bool {
        false
    }

    /// Options the mod declares for itself (Arsenal / HD2MM `manifest.json`).
    /// `None` means the mod is deployed whole, which is the norm.
    fn mod_options(&self, _content_root: &Path) -> Option<ModOptionSet> {
        None
    }

    /// Optional install prep before purge/link (rename paks, etc.). Returns warnings.
    fn prepare_deploy(&self, _install_path: &Path) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Plugin id to persist when the install is a more specific game than the
    /// saved type. The default is this plugin's own id.
    fn preferred_plugin_id(&self, _install_path: &Path) -> &'static str {
        self.info().id
    }

    /// Loader archive this plugin wants installed when the game does not have
    /// one yet. `None` means this plugin does not auto-install BepInEx.
    fn bepinex_loader(&self, _install_path: &Path) -> Option<BepInExBuild> {
        None
    }

    /// Download or cache anything the game needs before files are linked.
    /// The default installs BepInEx when [`Self::bepinex_loader`] returns a build.
    fn prepare_runtime<'a>(
        &'a self,
        ctx: &'a RuntimePrep<'_>,
    ) -> Pin<Box<dyn Future<Output = Result<RuntimePrepOutcome>> + Send + 'a>> {
        let build = self.bepinex_loader(ctx.install_path);
        let install = unity_bepinex::should_auto_install_bepinex(
            ctx.install_path,
            ctx.content_roots,
            build,
            |root| self.allows_empty_deploy(root),
        );
        Box::pin(async move {
            let Some(build) = build.filter(|_| install) else {
                return Ok(RuntimePrepOutcome::Done(Vec::new()));
            };
            let warnings =
                bepinex_install::ensure_bepinex(ctx.paths, ctx.install_path, build).await;
            Ok(RuntimePrepOutcome::Done(warnings))
        })
    }

    /// Copy a loader into the game after Download Assist finishes.
    fn finish_runtime_prep(&self, _ctx: &RuntimePrep<'_>) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Whether the Assist download requested by `prepare_runtime` is on disk.
    fn loader_assist_ready(&self, _ctx: &RuntimePrep<'_>) -> bool {
        false
    }

    /// Place staged files that must not be linked into the game (save overwrites).
    fn deploy_staged_extra(
        &self,
        _install_path: &Path,
        _content_root: &Path,
        _mod_name: &str,
    ) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Names this staged mod contributes to a native load-order file.
    ///
    /// The default is the wrap folder name when the mod is deployed under one
    /// folder, which is what `after_deploy` has always received.
    fn load_order_entries(&self, content_root: &Path, staged_name: &str) -> Vec<String> {
        let wrap = !self.deploys_to_install_root(content_root)
            && (self.prefers_mod_folder() || self.should_wrap_as_mod_folder(content_root));
        if !wrap {
            return Vec::new();
        }
        let name = self.wrap_mod_folder_name(content_root, staged_name);
        if name.is_empty() {
            Vec::new()
        } else {
            vec![name]
        }
    }

    /// Called after files are linked. `enabled_mod_folders` are load-order entries
    /// from `load_order_entries` (wrap folder names, unless a plugin overrides that).
    /// Returns optional warning strings (deploy succeeds even if warnings are returned).
    fn after_deploy(
        &self,
        _install_path: &Path,
        _enabled_mod_folders: &[String],
    ) -> Result<Vec<String>> {
        Ok(vec![])
    }

    /// Restore any native game config files patched during `after_deploy`
    /// (e.g. `gameinfo.gi` or `addonlist.txt` backups).
    fn on_revert(&self, _install_path: &Path) -> Result<()> {
        Ok(())
    }

    /// Plugins left out of the load order because a master file is missing.
    fn plugin_skips(&self, _install_path: &Path) -> Vec<PluginSkip> {
        Vec::new()
    }

    /// Loader and crash logs this game writes. Missing files are included so a
    /// diagnosis can say the loader never ran.
    fn load_log_sources(&self, _install_path: &Path) -> Vec<LogSource> {
        Vec::new()
    }

    /// Whether this plugin provides optional game-specific tools for the Tools modal.
    fn has_plugin_tools(&self) -> bool {
        false
    }

    /// Fast, offline status of this plugin's optional tools for `install_path`.
    fn plugin_tools_status(&self, _paths: &Paths, _install_path: &Path) -> Vec<PluginToolStatus> {
        Vec::new()
    }

    /// Check remote sources for newer versions of this plugin's optional tools.
    fn check_plugin_tool_updates<'a>(
        &'a self,
        _paths: &'a Paths,
        _install_path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PluginToolStatus>>> + Send + 'a>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    /// Install or update one of this plugin's optional tools.
    fn install_plugin_tool<'a>(
        &'a self,
        _paths: &'a Paths,
        _install_path: &'a Path,
        tool_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<PluginToolActionResult>> + Send + 'a>> {
        let id = tool_id.to_string();
        Box::pin(async move { Err(anyhow::anyhow!("unknown plugin tool: {id}")) })
    }

    /// Uninstall one of this plugin's optional tools.
    fn uninstall_plugin_tool<'a>(
        &'a self,
        _paths: &'a Paths,
        _install_path: &'a Path,
        tool_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<PluginToolActionResult>> + Send + 'a>> {
        let id = tool_id.to_string();
        Box::pin(async move { Err(anyhow::anyhow!("unknown plugin tool: {id}")) })
    }
}

/// A loader or engine log the diagnosis pass knows how to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    BepInEx,
    MelonLoader,
    Smapi,
    ReFramework,
    Ue4ss,
    ScriptExtender,
    CrashLog,
    /// A plugin's own text log next to the script extender log.
    PluginLog,
    /// Papyrus script log. Only useful when it is from the same launch.
    Papyrus,
    /// Binary minidump. Mentioned, never parsed.
    CrashDump,
    Ds2Lighting,
    Me3,
    CrLoader,
    Player,
    Console,
}

#[derive(Debug, Clone)]
pub struct LogSource {
    pub label: String,
    pub kind: LogKind,
    pub path: PathBuf,
}

/// A plugin omitted at deploy because one of its masters is not loadable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSkip {
    pub plugin: String,
    pub missing_master: String,
}

pub fn all_plugins() -> Vec<&'static dyn GamePlugin> {
    // More specific match_names first (FromSoft + RE Engine remakes + UE + BepInEx titles).
    // Generic UnrealEnginePlugin / BepInExPlugin have empty match_names (detection via engine_hint).
    vec![
        &DoomTheDarkAgesPlugin,
        &DoomEternalPlugin,
        &Doom2016Plugin,
        &Wolfenstein2Plugin,
        &WolfensteinYoungbloodPlugin,
        &WolfensteinOldBloodPlugin,
        &WolfensteinNewOrderPlugin,
        &StardewValleyPlugin,
        &BaldursGate3Plugin,
        &DivinityOriginalSin2Plugin,
        &Cyberpunk2077Plugin,
        &Witcher3Plugin,
        &FirstLightPlugin,
        &ControlResonantPlugin,
        &DaysGonePlugin,
        &Warhammer40kDarktidePlugin,
        &Warhammer40kRogueTraderPlugin,
        &PathfinderWotrPlugin,
        &Helldivers2Plugin,
        &Stalker2HeartOfChornobylPlugin,
        &StalkerAnomalyPlugin,
        &StalkerShadowOfChernobylPlugin,
        &StalkerCallOfPripyatPlugin,
        &PalworldPlugin,
        &HogwartsLegacyPlugin,
        &ReadyOrNotPlugin,
        &Subnautica2Plugin,
        &MarvelRivalsPlugin,
        &DeepRockGalacticPlugin,
        &PavlovPlugin,
        &OblivionRemasteredPlugin,
        &OblivionPlugin,
        &UnrealEnginePlugin,
        &HollowKnightSilksongPlugin,
        &LethalCompanyPlugin,
        &ValheimPlugin,
        &RiskOfRain2Plugin,
        &AmongUsPlugin,
        &ContentWarningPlugin,
        &GtfoPlugin,
        &TimberbornPlugin,
        &CultOfTheLambPlugin,
        &AgainstTheStormPlugin,
        &RoundsPlugin,
        &SonsOfTheForestPlugin,
        &SubnauticaBelowZeroPlugin,
        &SubnauticaPlugin,
        &Schedule1Plugin,
        &RepoPlugin,
        &PeakPlugin,
        &H3vrPlugin,
        &UltrakillPlugin,
        &AtlyssPlugin,
        &DysonSphereProgramPlugin,
        &InscryptionPlugin,
        &BonelabPlugin,
        &BoneworksPlugin,
        &TheLongDarkPlugin,
        &BloonsTd6Plugin,
        &BladeAndSorceryPlugin,
        &TheForestPlugin,
        &MySummerCarPlugin,
        &SevenDaysToDiePlugin,
        &ProjectZomboidPlugin,
        &RimWorldPlugin,
        &NoMansSkyPlugin,
        &Sims4Plugin,
        &Sims3Plugin,
        &Sims2Plugin,
        &SnowRunnerPlugin,
        &EuroTruckSimulator2Plugin,
        &AmericanTruckSimulatorPlugin,
        &SpaceEngineersPlugin,
        &SkyrimSpecialEditionPlugin,
        &SkyrimPlugin,
        &Fallout4Plugin,
        &FalloutNewVegasPlugin,
        &Fallout3Plugin,
        &MorrowindPlugin,
        &StarfieldPlugin,
        &MountAndBlade2BannerlordPlugin,
        &MountAndBladeWarbandPlugin,
        &KingdomComeDeliverance2Plugin,
        &KingdomComeDeliverancePlugin,
        &DressmakerPlugin,
        &ChefRpgPlugin,
        &BepInExPlugin,
        &EldenRingNightreignPlugin,
        &EldenRingPlugin,
        &ArmoredCore6Plugin,
        &SekiroPlugin,
        &DarkSouls3Plugin,
        &DarkSouls2Plugin,
        &DarkSoulsRemasteredPlugin,
        &DarkSoulsPlugin,
        &MonsterHunterWildsPlugin,
        &MonsterHunterRisePlugin,
        &MonsterHunterWorldPlugin,
        &DragonsDogma2Plugin,
        &DragonsDogmaDarkArisenPlugin,
        &DevilMayCry5Plugin,
        &StreetFighter6Plugin,
        &ResidentEvilRequiemPlugin,
        &ResidentEvilVillagePlugin,
        &ResidentEvil42023Plugin,
        &ResidentEvil32020Plugin,
        &ResidentEvil22019Plugin,
        &ResidentEvil7Plugin,
        // Paradox Clausewitz
        &HeartsOfIron4Plugin,
        &StellarisPlugin,
        &CrusaderKings3Plugin,
        &EuropaUniversalis4Plugin,
        &Victoria3Plugin,
        // Total War
        &TotalWarWarhammer3Plugin,
        &TotalWarThreeKingdomsPlugin,
        &TotalWarRome2Plugin,
        &TotalWarShogun2Plugin,
        // Rockstar RAGE
        &RedDeadRedemption2Plugin,
        &GrandTheftAuto5Plugin,
        // Decima
        &HorizonZeroDawnPlugin,
        &HorizonForbiddenWestPlugin,
        &DeathStrandingPlugin,
        // Standalone & Paired
        &FactorioPlugin,
        &FarmingSimulator25Plugin,
        &FarmingSimulator22Plugin,
        &AssettoCorsaPlugin,
        &BeamNgDrivePlugin,
        &KerbalSpaceProgramPlugin,
        &KenshiPlugin,
        &X4FoundationsPlugin,
        &Civilization7Plugin,
        &Civilization6Plugin,
        &GhostOfTsushimaPlugin,
        &DragonAgeOriginsPlugin,
        &Diablo2ResurrectedPlugin,
        &StarsectorPlugin,
        &Hades2Plugin,
        &HadesPlugin,
        &Kotor2Plugin,
        &KotorPlugin,
        &BarotraumaPlugin,
        &DwarfFortressPlugin,
        &DarkestDungeonPlugin,
        &VintageStoryPlugin,
        // Valve (more specific titles first)
        &HalfLife2DeathmatchPlugin,
        &HalfLife2Plugin,
        &HalfLifeAlyxPlugin,
        &HalfLifeDeathmatchSourcePlugin,
        &HalfLifeSourcePlugin,
        &HalfLifeOpposingForcePlugin,
        &HalfLifeBlueShiftPlugin,
        &HalfLifePlugin,
        &CounterStrike2Plugin,
        &CounterStrikeSourcePlugin,
        &CounterStrikeConditionZeroPlugin,
        &CounterStrikePlugin,
        &Left4Dead2Plugin,
        &Left4DeadPlugin,
        &Portal2Plugin,
        &PortalPlugin,
        &TeamFortress2Plugin,
        &TeamFortressClassicPlugin,
        &DayOfDefeatSourcePlugin,
        &DayOfDefeatPlugin,
        &DeathmatchClassicPlugin,
        &RicochetPlugin,
        &AlienSwarmPlugin,
        &GarrysModPlugin,
        &Dota2Plugin,
        &DeadlockPlugin,
        &ApertureDeskJobPlugin,
    ]
}

pub fn plugin_by_id(id: &str) -> Option<&'static dyn GamePlugin> {
    all_plugins().into_iter().find(|p| p.info().id == id)
}

/// Serializes tests that deploy Helldivers 2 mods, which share a process-global
/// patch index.
#[cfg(test)]
pub(crate) fn hd2_test_gate() -> std::sync::MutexGuard<'static, ()> {
    helldivers2::TEST_GATE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Lowercase a store title and drop trademark marks and spaced separators.
///
/// Steam and `lib_game_detector` report names like `The Sims™ 4`, which would
/// otherwise never match a `sims 4` entry in `match_names`. A shortcut named
/// `Doom - The Dark Ages` is the same phrase as `DOOM: The Dark Ages` once the
/// spaced hyphen and the colon are word breaks. A hyphen inside a word
/// (`spider-man`) stays put.
pub fn normalize_title(title: &str) -> String {
    let stripped: String = title
        .chars()
        .map(|c| {
            if matches!(c, '™' | '®' | '©') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut out = String::with_capacity(stripped.len());
    for word in stripped.split_whitespace() {
        let word = word.trim_matches(is_title_separator);
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word.to_lowercase());
    }
    out
}

/// Spaced punctuation that splits a title, not a hyphen inside one word.
fn is_title_separator(c: char) -> bool {
    matches!(c, '-' | '–' | '—' | ':')
}

pub fn match_plugin(title: &str, install_path: Option<&str>) -> Option<GamePluginInfo> {
    let lower = normalize_title(title);
    for plugin in all_plugins() {
        let info = plugin.info();
        if info.match_names.is_empty() {
            continue;
        }
        for name in info.match_names {
            if lower.contains(&name.to_lowercase()) {
                return Some(info);
            }
        }
    }

    // Steam lists DOOM (2016) as "DOOM". Only an id Tech 6 install is that game.
    if lower == "doom" {
        if let Some(path) = install_path {
            if id_tech::is_doom_2016_install(Path::new(path)) {
                return Some(Doom2016Plugin.info());
            }
        }
    }

    // Bare "Resident Evil 4" matches the 2023 remake only when the install looks like RE Engine.
    if re_engine::is_ambiguous_re4_title(&lower) {
        if let Some(path) = install_path {
            if re_engine::is_re_engine_remake_install(Path::new(path)) {
                return Some(ResidentEvil42023Plugin.info());
            }
        }
    }

    None
}

pub fn list_plugins() -> Vec<GamePluginSummary> {
    all_plugins()
        .into_iter()
        .map(|p| GamePluginSummary {
            info: p.info(),
            has_tools: p.has_plugin_tools(),
        })
        .collect()
}

/// Turn archive-originated `\` separators into normal path components.
pub fn normalize_relative(relative: &Path) -> PathBuf {
    let raw = relative.to_string_lossy().replace('\\', "/");
    let trimmed = raw.trim_matches('/');
    if trimmed.is_empty() {
        return PathBuf::new();
    }
    PathBuf::from(trimmed)
}

/// Prefer a peeled content-folder name over the Nexus title when wrapping.
///
/// Staging directories are `{safe}_{mod_id}_{file_id}`; those names are never used
/// as the on-disk wrap folder.
pub fn content_folder_wrap_name(content_root: &Path, staged_name: &str) -> String {
    if let Some(name) = content_root.file_name().and_then(|n| n.to_str()) {
        let sanitized = sanitize_filename::sanitize(name);
        if !sanitized.is_empty() && !looks_like_staging_dir_name(&sanitized) {
            return sanitized;
        }
    }
    sanitize_filename::sanitize(staged_name)
}

fn looks_like_staging_dir_name(name: &str) -> bool {
    let mut parts = name.rsplitn(3, '_');
    let file_id = parts.next();
    let mod_id = parts.next();
    let rest = parts.next();
    matches!(
        (file_id, mod_id, rest),
        (Some(f), Some(m), Some(r))
            if !r.is_empty() && f.parse::<u64>().is_ok() && m.parse::<u64>().is_ok()
    )
}

/// Strip a single top-level archive folder if the archive contained one root dir,
/// unless that folder is a meaningful game-root name for the plugin.
pub fn normalize_staging_root(staging_dir: &Path, plugin: &dyn GamePlugin) -> Result<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(staging_dir)
        .with_context(|| format!("read staging {}", staging_dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s != "__MACOSX" && !s.starts_with('.')
        })
        .collect();
    if entries.len() == 1 {
        let only = entries.remove(0);
        if only.file_type()?.is_dir() {
            let name = only.file_name();
            let name = name.to_string_lossy();
            let preserve = plugin.preserves_staging_root(&name);
            if !preserve {
                return Ok(only.path());
            }
        }
    }
    Ok(staging_dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyberpunk2077::Cyberpunk2077Plugin;

    #[test]
    fn peels_wrapper_but_keeps_cyberpunk_roots() {
        let staging = tempfile::tempdir().unwrap();
        let wrapper = staging.path().join("Some Mod Pack");
        std::fs::create_dir_all(wrapper.join("bin").join("x64")).unwrap();
        std::fs::write(wrapper.join("bin").join("x64").join("a.dll"), b"x").unwrap();

        let root = normalize_staging_root(staging.path(), &Cyberpunk2077Plugin).unwrap();
        assert_eq!(root, wrapper);

        let bin_only = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(bin_only.path().join("bin").join("x64")).unwrap();
        std::fs::write(bin_only.path().join("bin").join("x64").join("a.dll"), b"x").unwrap();
        let kept = normalize_staging_root(bin_only.path(), &Cyberpunk2077Plugin).unwrap();
        assert_eq!(kept, bin_only.path());

        let red4 = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(red4.path().join("red4ext").join("plugins")).unwrap();
        let kept_red = normalize_staging_root(red4.path(), &Cyberpunk2077Plugin).unwrap();
        assert_eq!(kept_red, red4.path());
    }

    #[test]
    fn content_folder_wrap_name_skips_staging_suffix() {
        let staging = tempfile::tempdir().unwrap();
        let named = staging.path().join("CoolMod_12_34");
        std::fs::create_dir_all(&named).unwrap();
        assert_eq!(content_folder_wrap_name(&named, "Cool Mod"), "Cool Mod");

        let peeled = staging.path().join("CoolMod");
        std::fs::create_dir_all(&peeled).unwrap();
        assert_eq!(content_folder_wrap_name(&peeled, "Cool Mod"), "CoolMod");
    }

    #[test]
    fn peels_for_folder_style_plugins() {
        let staging = tempfile::tempdir().unwrap();
        let wrapper = staging.path().join("MyCoolMod");
        std::fs::create_dir_all(&wrapper).unwrap();
        std::fs::write(wrapper.join("manifest.json"), "{}").unwrap();
        let root = normalize_staging_root(staging.path(), &StardewValleyPlugin).unwrap();
        assert_eq!(root, wrapper);
    }

    #[test]
    fn match_ignores_trademark_symbols_in_store_titles() {
        // Steam and lib_game_detector report titles with ™ / ®.
        assert_eq!(
            match_plugin("The Witcher® 3: Wild Hunt", None).unwrap().id,
            "witcher3"
        );
        assert_eq!(normalize_title("The Sims™ 4"), "the sims 4");
        assert_eq!(normalize_title("HELLDIVERS™ 2"), "helldivers 2");
        assert_eq!(match_plugin("The Sims™ 4", None).unwrap().id, "sims4");
        assert_eq!(match_plugin("The Sims™ 3", None).unwrap().id, "sims3");
        assert_eq!(match_plugin("R.E.P.O.", None).unwrap().id, "repo");
    }

    #[test]
    fn match_treats_spaced_hyphens_as_word_breaks() {
        assert_eq!(
            normalize_title("Doom - The Dark Ages"),
            "doom the dark ages"
        );
        assert_eq!(normalize_title("spider-man"), "spider-man");
        assert_eq!(
            match_plugin("Doom - The Dark Ages", None).unwrap().id,
            "doomthedarkages"
        );
        assert_eq!(
            match_plugin("DOOM – The Dark Ages", None).unwrap().id,
            "doomthedarkages"
        );
        assert_eq!(
            match_plugin("DOOM: The Dark Ages", None).unwrap().id,
            "doomthedarkages"
        );
        assert_eq!(
            match_plugin("Wolfenstein - The New Order", None)
                .unwrap()
                .id,
            "wolfensteintheneworder"
        );
        assert_eq!(
            match_plugin("Subnautica - Below Zero", None).unwrap().id,
            "subnauticabelowzero"
        );
        assert!(match_plugin("Return to Castle Wolfenstein", None).is_none());
        assert!(match_plugin("Doom 3", None).is_none());
        assert!(match_plugin("Control", None).is_none());
    }

    #[test]
    fn match_re_engine_titles() {
        assert_eq!(
            match_plugin("Resident Evil 7 Biohazard", None).unwrap().id,
            "residentevil7"
        );
        assert_eq!(
            match_plugin("Resident Evil Village", None).unwrap().id,
            "residentevilvillage"
        );
        assert_eq!(
            match_plugin("Resident Evil Requiem", None).unwrap().id,
            "residentevilrequiem"
        );
        assert_eq!(
            match_plugin("Resident Evil 2", None).unwrap().id,
            "residentevil22019"
        );
        assert_eq!(
            match_plugin("Resident Evil 3", None).unwrap().id,
            "residentevil32020"
        );
        assert_eq!(
            match_plugin("RESIDENT EVIL 4 BIOHAZARD RE4", None)
                .unwrap()
                .id,
            "residentevil42023"
        );

        // Bare RE4 without remake install markers stays unsupported.
        assert!(match_plugin("Resident Evil 4", None).is_none());
        let remake = tempfile::tempdir().unwrap();
        std::fs::write(remake.path().join("re4.exe"), b"x").unwrap();
        assert_eq!(
            match_plugin("Resident Evil 4", Some(remake.path().to_str().unwrap()))
                .unwrap()
                .id,
            "residentevil42023"
        );
    }

    #[test]
    fn preserves_stardew_mods_root() {
        let staging = tempfile::tempdir().unwrap();
        let mods = staging.path().join("Mods");
        std::fs::create_dir_all(mods.join("CoolMod")).unwrap();
        std::fs::write(mods.join("CoolMod").join("manifest.json"), "{}").unwrap();
        let root = normalize_staging_root(staging.path(), &StardewValleyPlugin).unwrap();
        assert_eq!(root, staging.path());
    }

    #[test]
    fn darktide_preserves_loader_and_dmf_roots() {
        let loader = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(loader.path().join("tools")).unwrap();
        std::fs::create_dir_all(loader.path().join("mods")).unwrap();
        let kept = normalize_staging_root(loader.path(), &Warhammer40kDarktidePlugin).unwrap();
        assert_eq!(kept, loader.path());

        let dmf_only = tempfile::tempdir().unwrap();
        let dmf = dmf_only.path().join("dmf");
        std::fs::create_dir_all(&dmf).unwrap();
        std::fs::write(dmf.join("dmf.lua"), b"x").unwrap();
        let kept_dmf =
            normalize_staging_root(dmf_only.path(), &Warhammer40kDarktidePlugin).unwrap();
        assert_eq!(kept_dmf, dmf_only.path());

        let normal = tempfile::tempdir().unwrap();
        let wrapper = normal.path().join("scoreboard");
        std::fs::create_dir_all(&wrapper).unwrap();
        std::fs::write(wrapper.join("scoreboard.mod"), b"x").unwrap();
        let peeled = normalize_staging_root(normal.path(), &Warhammer40kDarktidePlugin).unwrap();
        assert_eq!(peeled, wrapper);
    }

    #[test]
    fn match_subnautica_family_and_forest_order() {
        assert_eq!(
            match_plugin("Subnautica 2", None).unwrap().id,
            "subnautica2"
        );
        assert_eq!(
            match_plugin("Subnautica: Below Zero", None).unwrap().id,
            "subnauticabelowzero"
        );
        assert_eq!(match_plugin("Subnautica", None).unwrap().id, "subnautica");
        assert_eq!(
            match_plugin("Sons of the Forest", None).unwrap().id,
            "sonsoftheforest"
        );
        assert_eq!(match_plugin("Ready or Not", None).unwrap().id, "readyornot");
        assert_eq!(match_plugin("Schedule I", None).unwrap().id, "schedule1");
        assert_eq!(
            match_plugin("Monster Hunter Wilds", None).unwrap().id,
            "monsterhunterwilds"
        );
        assert_eq!(
            match_plugin("Monster Hunter Rise", None).unwrap().id,
            "monsterhunterrise"
        );
        assert_eq!(match_plugin("The Forest", None).unwrap().id, "theforest");
        assert_eq!(
            match_plugin("The Witcher 3: Wild Hunt", None).unwrap().id,
            "witcher3"
        );
        assert_eq!(
            match_plugin("HELLDIVERS 2", None).unwrap().id,
            "helldivers2"
        );
        assert_eq!(
            match_plugin("Blade & Sorcery", None).unwrap().id,
            "bladeandsorcery"
        );
        assert_eq!(
            match_plugin("My Summer Car", None).unwrap().id,
            "mysummercar"
        );
        assert_eq!(match_plugin("Dressmaker", None).unwrap().id, "dressmaker");
        assert_eq!(
            match_plugin("CONTROL Resonant", None).unwrap().id,
            "controlresonant"
        );
        assert!(match_plugin("Control", None).is_none());
        assert!(match_plugin("Control Ultimate Edition", None).is_none());
        assert_eq!(
            match_plugin("007 First Light", None).unwrap().id,
            "007firstlight"
        );
        assert_eq!(
            match_plugin("007FirstLight", None).unwrap().id,
            "007firstlight"
        );
        assert_eq!(
            match_plugin("Monster Hunter: World", None).unwrap().id,
            "monsterhunterworld"
        );
        assert_eq!(match_plugin("R.E.P.O.", None).unwrap().id, "repo");
        assert_eq!(match_plugin("PEAK", None).unwrap().id, "peak");
        assert_eq!(
            match_plugin("Hot Dogs, Horseshoes & Hand Grenades", None)
                .unwrap()
                .id,
            "h3vr"
        );
        assert_eq!(match_plugin("ULTRAKILL", None).unwrap().id, "ultrakill");
        assert_eq!(match_plugin("ATLYSS", None).unwrap().id, "atlyss");
        assert_eq!(
            match_plugin("Dyson Sphere Program", None).unwrap().id,
            "dysonsphereprogram"
        );
        assert_eq!(match_plugin("Inscryption", None).unwrap().id, "inscryption");
        assert_eq!(
            match_plugin("Hollow Knight: Silksong", None).unwrap().id,
            "hollowknightsilksong"
        );
        assert_eq!(
            match_plugin("7 Days to Die", None).unwrap().id,
            "7daystodie"
        );
        assert_eq!(match_plugin("RimWorld", None).unwrap().id, "rimworld");
        assert_eq!(match_plugin("No Man's Sky", None).unwrap().id, "nomanssky");
        assert_eq!(
            match_plugin("Marvel Rivals", None).unwrap().id,
            "marvelrivals"
        );
        assert_eq!(match_plugin("BONELAB", None).unwrap().id, "bonelab");
        assert_eq!(match_plugin("BONEWORKS", None).unwrap().id, "boneworks");
        assert_eq!(
            match_plugin("Deep Rock Galactic", None).unwrap().id,
            "deeprockgalactic"
        );
        assert_eq!(match_plugin("Pavlov VR", None).unwrap().id, "pavlov");
        assert_eq!(match_plugin("SnowRunner", None).unwrap().id, "snowrunner");
        assert_eq!(
            match_plugin("Euro Truck Simulator 2", None).unwrap().id,
            "eurotrucksimulator2"
        );
        assert_eq!(
            match_plugin("American Truck Simulator", None).unwrap().id,
            "americantrucksimulator"
        );
        assert!(match_plugin("Euro Truck Simulator", None).is_none());
        assert_eq!(
            match_plugin("Space Engineers", None).unwrap().id,
            "spaceengineers"
        );
        assert_eq!(
            match_plugin("The Elder Scrolls V: Skyrim Special Edition", None)
                .unwrap()
                .id,
            "skyrimspecialedition"
        );
        assert_eq!(match_plugin("Fallout 4", None).unwrap().id, "fallout4");
        assert_eq!(match_plugin("Starfield", None).unwrap().id, "starfield");
        assert_eq!(
            match_plugin("The Elder Scrolls IV: Oblivion Remastered", None)
                .unwrap()
                .id,
            "oblivionremastered"
        );
    }

    #[test]
    fn match_id_tech_titles() {
        assert_eq!(
            match_plugin("DOOM: The Dark Ages", None).unwrap().id,
            "doomthedarkages"
        );
        assert_eq!(
            match_plugin("DOOM The Dark Ages", None).unwrap().id,
            "doomthedarkages"
        );
        assert_eq!(
            match_plugin("DOOM Eternal", None).unwrap().id,
            "doometernal"
        );
        assert_eq!(match_plugin("DOOM (2016)", None).unwrap().id, "doom2016");
        assert_eq!(match_plugin("DOOM 2016", None).unwrap().id, "doom2016");
        assert!(match_plugin("DOOM", None).is_none());
        assert!(match_plugin("Doom 3", None).is_none());
        assert!(match_plugin("DOOM 64", None).is_none());
        assert!(match_plugin("Ultimate Doom", None).is_none());
        assert!(match_plugin("Return to Castle Wolfenstein", None).is_none());
        assert!(match_plugin("Wolfenstein", None).is_none());

        let doom = tempfile::tempdir().unwrap();
        std::fs::write(doom.path().join("DOOMx64.exe"), b"x").unwrap();
        assert_eq!(
            match_plugin("DOOM", Some(doom.path().to_str().unwrap()))
                .unwrap()
                .id,
            "doom2016"
        );
        let vk = tempfile::tempdir().unwrap();
        std::fs::write(vk.path().join("DOOMx64vk.exe"), b"x").unwrap();
        assert_eq!(
            match_plugin("DOOM®", Some(vk.path().to_str().unwrap()))
                .unwrap()
                .id,
            "doom2016"
        );
        let eternal = tempfile::tempdir().unwrap();
        std::fs::write(eternal.path().join("DOOMx64.exe"), b"x").unwrap();
        std::fs::write(eternal.path().join("DOOMEternal.exe"), b"x").unwrap();
        assert!(match_plugin("DOOM", Some(eternal.path().to_str().unwrap())).is_none());

        assert_eq!(
            match_plugin("Wolfenstein II: The New Colossus", None)
                .unwrap()
                .id,
            "wolfenstein2thenewcolossus"
        );
        assert_eq!(
            match_plugin("Wolfenstein 2: The New Colossus", None)
                .unwrap()
                .id,
            "wolfenstein2thenewcolossus"
        );
        assert_eq!(
            match_plugin("Wolfenstein: Youngblood", None).unwrap().id,
            "wolfensteinyoungblood"
        );
        assert_eq!(
            match_plugin("Wolfenstein Youngblood", None).unwrap().id,
            "wolfensteinyoungblood"
        );
        assert_eq!(
            match_plugin("Wolfenstein: The Old Blood", None).unwrap().id,
            "wolfensteintheoldblood"
        );
        assert_eq!(
            match_plugin("Wolfenstein: The New Order", None).unwrap().id,
            "wolfensteintheneworder"
        );
        assert_eq!(
            match_plugin("Wolfenstein The New Order", None).unwrap().id,
            "wolfensteintheneworder"
        );
    }

    #[test]
    fn match_valve_titles() {
        let cases = [
            ("Half-Life 2: Deathmatch", "halflife2deathmatch"),
            ("Half-Life 2", "halflife2"),
            ("Half-Life: Alyx", "halflifealyx"),
            ("Half-Life Deathmatch: Source", "hldms"),
            ("Half-Life: Source", "halflifesource"),
            ("Half-Life: Opposing Force", "halflifeopposingforce"),
            ("Half-Life: Blue Shift", "halflifeblueshift"),
            ("Half-Life", "halflife"),
            ("Counter-Strike 2", "counterstrike2"),
            ("Counter-Strike: Source", "counterstrikesource"),
            ("Counter-Strike: Condition Zero", "cscz"),
            ("Counter-Strike", "counterstrike"),
            ("Left 4 Dead 2", "left4dead2"),
            ("Left 4 Dead", "left4dead"),
            ("Portal 2", "portal2"),
            ("Portal", "portal"),
            ("Team Fortress 2", "teamfortress2"),
            ("Team Fortress Classic", "teamfortressclassic"),
            ("Day of Defeat: Source", "dayofdefeatsource"),
            ("Day of Defeat", "dayofdefeat"),
            ("Deathmatch Classic", "deathmatchclassic"),
            ("Ricochet", "ricochet"),
            ("Alien Swarm", "alienswarm"),
            ("Garry's Mod", "garrysmod"),
            ("Dota 2", "dota2"),
            ("Deadlock", "deadlock"),
            ("Aperture Desk Job", "aperturedeskjob"),
        ];
        for (title, expected_id) in cases {
            assert_eq!(
                match_plugin(title, None)
                    .unwrap_or_else(|| panic!("expected match for {title}"))
                    .id,
                expected_id,
                "title: {title}"
            );
        }
    }

    #[test]
    fn match_expanded_game_titles_and_disambiguation() {
        let cases = [
            // Creation Engine disambiguation
            ("The Elder Scrolls V: Skyrim Special Edition", "skyrimspecialedition"),
            ("The Elder Scrolls V: Skyrim", "skyrim"),
            ("The Elder Scrolls IV: Oblivion Remastered", "oblivionremastered"),
            ("The Elder Scrolls IV: Oblivion", "oblivion"),
            ("The Elder Scrolls III: Morrowind", "morrowind"),
            ("Fallout: New Vegas", "newvegas"),
            ("Fallout 3 - Game of the Year Edition", "fallout3"),
            // FromSoftware disambiguation
            ("ELDEN RING NIGHTREIGN", "eldenringnightreign"),
            ("ELDEN RING", "eldenring"),
            ("ARMORED CORE VI FIRES OF RUBICON", "armoredcore6firesofrubicon"),
            ("Sekiro: Shadows Die Twice", "sekiro"),
            // Capcom RE Engine & MT Framework disambiguation
            ("Dragon's Dogma 2", "dragonsdogma2"),
            ("Dragon's Dogma: Dark Arisen", "dragonsdogma"),
            ("Devil May Cry 5", "devilmaycry5"),
            ("Street Fighter 6", "streetfighter6"),
            // Larian & Sims & MelonLoader
            ("Divinity: Original Sin 2 - Definitive Edition", "divinityoriginalsin2definitiveedition"),
            ("The Sims 2 Legacy", "sims2"),
            ("The Long Dark", "thelongdark"),
            ("Bloons TD 6", "bloonstd6"),
            // Paradox Clausewitz
            ("Hearts of Iron IV", "heartsofironiv"),
            ("Stellaris", "stellaris"),
            ("Crusader Kings III", "crusaderkings3"),
            ("Europa Universalis IV", "europauniversalisiv"),
            ("Victoria 3", "victoria3"),
            // Total War
            ("Total War: WARHAMMER III", "totalwarwarhammer3"),
            ("Total War: THREE KINGDOMS", "totalwarthreekingdoms"),
            ("Total War: ROME II - Emperor Edition", "totalwarrome2"),
            ("Total War: SHOGUN 2", "totalwarshogun2"),
            // Rockstar RAGE
            ("Red Dead Redemption 2", "reddeadredemption2"),
            ("Grand Theft Auto V", "gta5"),
            // Decima
            ("Horizon Zero Dawn Complete Edition", "horizonzerodawn"),
            ("Horizon Forbidden West Complete Edition", "horizonforbiddenwest"),
            ("DEATH STRANDING DIRECTOR'S CUT", "deathstranding"),
            // S.T.A.L.K.E.R. X-Ray vs S.T.A.L.K.E.R. 2
            ("S.T.A.L.K.E.R. 2: Heart of Chornobyl", "stalker2heartofchornobyl"),
            ("S.T.A.L.K.E.R.: Anomaly", "stalkeranomaly"),
            ("S.T.A.L.K.E.R.: Shadow of Chernobyl", "stalkershadowofchernobyl"),
            ("S.T.A.L.K.E.R.: Call of Pripyat", "stalkercallofpripyat"),
            // Standalone & Paired
            ("Factorio", "factorio"),
            ("Farming Simulator 25", "farmingsimulator25"),
            ("Farming Simulator 22", "farmingsimulator22"),
            ("Assetto Corsa", "assettocorsa"),
            ("BeamNG.drive", "beamngdrive"),
            ("Kerbal Space Program", "kerbalspaceprogram"),
            ("Kenshi", "kenshi"),
            ("X4: Foundations", "x4foundations"),
            ("Sid Meier's Civilization VII", "civilizationvii"),
            ("Sid Meier's Civilization VI", "civilizationvi"),
            ("Ghost of Tsushima DIRECTOR'S CUT", "ghostoftsushima"),
            ("Dragon Age: Origins - Ultimate Edition", "dragonage"),
            ("Diablo II: Resurrected", "diablo2resurrected"),
            ("Starsector", "starsector"),
            ("Hades II", "hades2"),
            ("Hades", "hades"),
            ("STAR WARS Knights of the Old Republic II - The Sith Lords", "kotor2"),
            ("STAR WARS - Knights of the Old Republic", "kotor"),
            ("Barotrauma", "barotrauma"),
            ("Dwarf Fortress", "dwarffortress"),
            ("Darkest Dungeon", "darkestdungeon"),
            ("Vintage Story", "vintagestory"),
            // Owlcat / UMM
            ("Pathfinder: Wrath of the Righteous - Enhanced Edition", "pathfinderwrathoftherighteous"),
            ("Warhammer 40,000: Rogue Trader", "warhammer40kroguetrader"),
        ];
        for (title, expected_id) in cases {
            assert_eq!(
                match_plugin(title, None)
                    .unwrap_or_else(|| panic!("expected match for {title}"))
                    .id,
                expected_id,
                "title: {title}"
            );
        }
    }
}
