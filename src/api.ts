import { invoke } from "@tauri-apps/api/core";
import type { EmperorOpenMod } from "./modDeepLink";
import type {
  AuthorSuggestion,
  BannerPersistence,
  BannerTimeouts,
  BrowseMeta,
  BrowseSearchOpts,
  CatalogSearchPage,
  CatalogSuggestion,
  CatalogSuggestRequest,
  CatalogCoverHit,
  CatalogCoverRequest,
  ShareModMetadataHit,
  ShareModEntry,
  CollectionDetail,
  CollectionModFile,
  CollectionSearchPage,
  CollectionVersionAdvice,
  CollectionVisibility,
  GameLaunchState,
  LaunchCommandSource,
  LaunchEnvVar,
  LaunchPreview,
  LaunchResult,
  LoadDiagnosis,
  DetectedGame,
  DownloadItem,
  GameHealth,
  GameInfo,
  GamePluginInfo,
  GameVersionReport,
  ImportResult,
  InstalledCollection,
  CollectionIntegrityReport,
  RetryCollectionModTarget,
  RetryCollectionModsResult,
  IgnoredRequirement,
  ManagedGame,
  ModDetail,
  ModFileInfo,
  MissingRequirements,
  ModioFileInfo,
  ModioModDetail,
  FomodInstallerView,
  FomodSelection,
  ModOptionSelection,
  ModOptionsView,
  ModSearchPage,
  NexusUser,
  OrphanScan,
  PersistedBanner,
  RecoveryReport,
  ClearDownloadCacheResult,
  Settings,
  StagedMod,
  StagedModUpdate,
  WineOverrideAdvice,
  ThemePreference,
  ThemeStyle,
  LinuxSystemBackend,
  SystemPalette,
  InstallClickBehavior,
  TsPackageDetail,
  UeLayoutInfo,
  ExportShareResult,
  ShareDecodePreview,
  DetectImportResult,
  ImportCodeResult,
  ShareImportResult,
  ThunderstoreProfileImportResult,
  SavedCollectionEntry,
  SavedCollectionDetail,
  ToolsStatusResponse,
  ToolUpdatesResponse,
  LsfgVkConfig,
  AutoHdrVkConfig,
  GameToolOverrides,
  GameToolsResponse,
  AppInfo,
  AppUpdateStatus,
  AppInstallResult,
  FriendsState,
  FriendSyncResult,
  FriendCollectionSyncStatus,
} from "./types";

export const api = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  checkAppUpdate: () => invoke<AppUpdateStatus>("check_app_update"),
  installAppUpdate: () => invoke<AppInstallResult>("install_app_update"),
  getSettings: () => invoke<Settings>("get_settings"),
  completeInitialSetup: () => invoke<void>("complete_initial_setup"),
  setApiKey: (key: string) => invoke<NexusUser>("set_api_key", { key }),
  validateUser: () => invoke<NexusUser>("validate_user"),
  clearApiKey: () => invoke<void>("clear_api_key"),
  setModioApiKey: (key: string) => invoke<void>("set_modio_api_key", { key }),
  clearModioApiKey: () => invoke<void>("clear_modio_api_key"),
  setAdultContent: (enabled: boolean) =>
    invoke<void>("set_adult_content", { enabled }),
  setGameVersionWarnings: (enabled: boolean) =>
    invoke<void>("set_game_version_warnings", { enabled }),
  setAutoclickFreeDownload: (enabled: boolean) =>
    invoke<void>("set_autoclick_free_download", { enabled }),
  setTheme: (theme: ThemePreference) => invoke<void>("set_theme", { theme }),
  setThemeStyle: (style: ThemeStyle) => invoke<void>("set_theme_style", { style }),
  setAccentColor: (color: string) => invoke<void>("set_accent_color", { color }),
  setLinuxSystemBackend: (backend: LinuxSystemBackend) =>
    invoke<void>("set_linux_system_backend", { backend }),
  getSystemPalette: () => invoke<SystemPalette>("get_system_palette"),
  setInstallClickBehavior: (behavior: InstallClickBehavior) =>
    invoke<void>("set_install_click_behavior", { behavior }),
  setRecentDownloadsLimit: (limit: number) =>
    invoke<void>("set_recent_downloads_limit", { limit }),
  setBannerTimeouts: (timeouts: BannerTimeouts) =>
    invoke<void>("set_banner_timeouts", { timeouts }),
  setBannerPersistence: (persistence: BannerPersistence) =>
    invoke<void>("set_banner_persistence", { persistence }),
  setDismissBannerOnRetry: (enabled: boolean) =>
    invoke<void>("set_dismiss_banner_on_retry", { enabled }),
  getPersistedBanners: () => invoke<PersistedBanner[]>("get_persisted_banners"),
  savePersistedBanners: (banners: PersistedBanner[]) =>
    invoke<void>("save_persisted_banners", { banners }),
  setBrowseGridSize: (size: number) =>
    invoke<void>("set_browse_grid_size", { size }),
  scanModOrphans: () => invoke<OrphanScan>("scan_mod_orphans"),
  clearDownloadCache: () =>
    invoke<ClearDownloadCacheResult>("clear_download_cache"),
  recoverLegacyModData: () =>
    invoke<RecoveryReport>("recover_legacy_mod_data"),
  scanGames: () => invoke<DetectedGame[]>("scan_games"),
  resolveCatalogCovers: (requests: CatalogCoverRequest[]) =>
    invoke<CatalogCoverHit[]>("resolve_catalog_covers", { requests }),
  resolveShareModMetadata: (mods: ShareModEntry[]) =>
    invoke<ShareModMetadataHit[]>("resolve_share_mod_metadata", { mods }),
  getGameHealth: () => invoke<GameHealth[]>("get_game_health"),
  relinkManagedGame: (oldId: string, newId: string) =>
    invoke<ManagedGame>("relink_managed_game", { oldId, newId }),
  listManaged: () => invoke<ManagedGame[]>("list_managed"),
  manageGame: (game: {
    id: string;
    title: string;
    nexusDomain: string;
    installPath: string;
    launcher: string;
    pluginId: string;
    coverPath?: string | null;
    coverUrl?: string | null;
    projectName?: string | null;
    thunderstoreCommunity?: string | null;
    modioGameId?: number | null;
  }) => invoke<ManagedGame>("manage_game", game),
  updateManagedGame: (args: {
    id: string;
    nexusDomain?: string | null;
    projectName?: string | null;
    thunderstoreCommunity?: string | null;
    modioGameId?: number | null;
  }) => invoke<ManagedGame>("update_managed_game", args),
  setGameVersion: (id: string, version: string | null) =>
    invoke<ManagedGame>("set_game_version", { id, version }),
  setIgnoredRequirements: (id: string, ignored: IgnoredRequirement[]) =>
    invoke<ManagedGame>("set_ignored_requirements", { id, ignored }),
  setWineOverridesDismissed: (id: string, dismissed: boolean) =>
    invoke<ManagedGame>("set_wine_overrides_dismissed", { id, dismissed }),
  checkModGameVersions: (gameId: string) =>
    invoke<GameVersionReport>("check_mod_game_versions", { gameId }),
  adviseCollectionVersions: (
    gameId: string,
    collections: { id: string; gameVersions?: string[] | null }[],
  ) =>
    invoke<CollectionVersionAdvice[]>("advise_collection_versions", {
      gameId,
      collections: collections.map((c) => ({
        id: c.id,
        gameVersions: c.gameVersions ?? null,
      })),
    }),
  listPlugins: () => invoke<GamePluginInfo[]>("list_plugins"),
  detectUeLayout: (installPath: string, projectName?: string | null) =>
    invoke<UeLayoutInfo | null>("detect_ue_layout", {
      installPath,
      projectName: projectName ?? null,
    }),
  suggestCatalogIds: (title: string) =>
    invoke<CatalogSuggestion>("suggest_catalog_ids", { title }),
  suggestCatalogIdsBatch: (requests: CatalogSuggestRequest[]) =>
    invoke<Record<string, CatalogSuggestion>>("suggest_catalog_ids_batch", {
      requests,
    }),
  unmanageGame: (id: string) => invoke<void>("unmanage_game", { id }),
  getUnmanageCollectionStatus: (gameId: string) =>
    invoke<{
      has_portable_mods: boolean;
      mod_count: number;
      skipped_count: number;
      already_saved: boolean;
      default_name: string;
    }>("get_unmanage_collection_status", { gameId }),
  setActiveGame: (id: string) => invoke<void>("set_active_game", { id }),
  searchMods: (domain: string, query: string, opts: BrowseSearchOpts = {}) =>
    invoke<ModSearchPage>("search_mods", {
      domain,
      query,
      sort: opts.sort ?? null,
      category: opts.category ?? null,
      tagsInclude: opts.tagsInclude ?? null,
      tagsExclude: opts.tagsExclude ?? null,
      gameVersion: opts.gameVersion ?? null,
      author: opts.author ?? null,
      offset: opts.offset ?? null,
      count: opts.count ?? null,
    }),
  searchCatalog: (
    gameId: string,
    query: string,
    opts: BrowseSearchOpts & { sourceFilter?: string | null } = {},
  ) =>
    invoke<CatalogSearchPage>("search_catalog", {
      gameId,
      query,
      sourceFilter: opts.sourceFilter ?? null,
      sort: opts.sort ?? null,
      category: opts.category ?? null,
      tagsInclude: opts.tagsInclude ?? null,
      tagsExclude: opts.tagsExclude ?? null,
      gameVersion: opts.gameVersion ?? null,
      author: opts.author ?? null,
      offset: opts.offset ?? null,
      count: opts.count ?? null,
    }),
  suggestAuthors: (gameId: string, query: string) =>
    invoke<AuthorSuggestion[]>("suggest_authors", { gameId, query }),
  getThunderstorePackage: (
    community: string,
    namespace: string,
    name: string,
  ) =>
    invoke<TsPackageDetail>("get_thunderstore_package", {
      community,
      namespace,
      name,
    }),
  downloadThunderstoreMod: (args: {
    gameId: string;
    community: string;
    namespace: string;
    name: string;
    version?: string | null;
    recordAsModpack?: boolean | null;
  }) => invoke<StagedMod[]>("download_thunderstore_mod", args),
  getModioMod: (gameId: number, modId: number) =>
    invoke<ModioModDetail>("get_modio_mod", { gameId, modId }),
  modioFiles: (gameId: number, modId: number) =>
    invoke<ModioFileInfo[]>("modio_files", { gameId, modId }),
  downloadModioMod: (args: {
    gameId: string;
    modioGameId: number;
    modId: number;
    fileId?: number | null;
    name: string;
    version?: string | null;
    installDeps?: boolean | null;
  }) => invoke<StagedMod[]>("download_modio_mod", args),
  getMod: (domain: string, modId: number) =>
    invoke<ModDetail>("get_mod", { domain, modId }),
  missingNexusRequirements: (gameId: string, stagedId?: string | null) =>
    invoke<MissingRequirements>("missing_nexus_requirements", {
      gameId,
      stagedId: stagedId ?? null,
    }),
  nexusModRequirements: (args: {
    gameId: string;
    domain: string;
    modId: number;
    name: string;
  }) =>
    invoke<MissingRequirements>("nexus_mod_requirements", {
      gameId: args.gameId,
      domain: args.domain,
      modId: args.modId,
      name: args.name,
    }),
  linkStagedRequirements: (
    gameId: string,
    requirementId: string,
    requiredBy: string[],
  ) =>
    invoke<void>("link_staged_requirements", {
      gameId,
      requirementId,
      requiredBy,
    }),
  getGame: (domain: string) => invoke<GameInfo>("get_game", { domain }),
  modFiles: (domain: string, modId: number) =>
    invoke<ModFileInfo[]>("mod_files", { domain, modId }),
  searchCollections: (
    domain: string,
    query: string,
    opts: BrowseSearchOpts = {},
  ) =>
    invoke<CollectionSearchPage>("search_collections", {
      domain,
      query,
      sort: opts.sort ?? null,
      category: opts.category ?? null,
      tagsInclude: opts.tagsInclude ?? null,
      tagsExclude: opts.tagsExclude ?? null,
      gameVersion: opts.gameVersion ?? null,
      author: opts.author ?? null,
      offset: opts.offset ?? null,
      count: opts.count ?? null,
    }),
  searchCollectionCatalog: (
    gameId: string,
    query: string,
    opts: BrowseSearchOpts & { sourceFilter?: string | null } = {},
  ) =>
    invoke<CollectionSearchPage>("search_collection_catalog", {
      gameId,
      query,
      sourceFilter: opts.sourceFilter ?? null,
      sort: opts.sort ?? null,
      category: opts.category ?? null,
      tagsInclude: opts.tagsInclude ?? null,
      tagsExclude: opts.tagsExclude ?? null,
      gameVersion: opts.gameVersion ?? null,
      author: opts.author ?? null,
      offset: opts.offset ?? null,
      count: opts.count ?? null,
    }),
  browseMeta: (domain: string) => invoke<BrowseMeta>("browse_meta", { domain }),
  getCollection: (args: { slug: string; domain?: string | null }) =>
    invoke<CollectionDetail>("get_collection", {
      slug: args.slug,
      domain: args.domain ?? null,
    }),
  downloadMod: (args: {
    gameId: string;
    domain: string;
    modId: number;
    fileId: number;
    name: string;
    version?: string | null;
    replaceStagedId?: string | null;
    fileName?: string | null;
  }) =>
    invoke<StagedMod>("download_mod", {
      gameId: args.gameId,
      domain: args.domain,
      modId: args.modId,
      fileId: args.fileId,
      name: args.name,
      version: args.version ?? null,
      replaceStagedId: args.replaceStagedId ?? null,
      fileName: args.fileName ?? null,
    }),
  openDownloadAssist: (args: {
    gameId: string;
    domain: string;
    modId: number;
    fileId: number;
    name: string;
    batchId?: string | null;
    replaceStagedId?: string | null;
    version?: string | null;
    fileName?: string | null;
  }) =>
    invoke<void>("open_download_assist", {
      gameId: args.gameId,
      domain: args.domain,
      modId: args.modId,
      fileId: args.fileId,
      name: args.name,
      batchId: args.batchId ?? null,
      replaceStagedId: args.replaceStagedId ?? null,
      version: args.version ?? null,
      fileName: args.fileName ?? null,
    }),
  setAssistBounds: (bounds: {
    x: number;
    y: number;
    width: number;
    height: number;
  }) => invoke<void>("set_assist_bounds", bounds),
  setAssistVisible: (visible: boolean) =>
    invoke<void>("set_assist_visible", { visible }),
  closeDownloadAssist: () => invoke<void>("close_download_assist"),
  openNexusWebsiteLogin: () => invoke<void>("open_nexus_website_login"),
  confirmNexusWebsiteLogin: () => invoke<void>("confirm_nexus_website_login"),
  clearAssistSession: () => invoke<void>("clear_assist_session"),
  collectionFiles: (args: { slug: string; revision?: number | null }) =>
    invoke<CollectionModFile[]>("collection_files", {
      slug: args.slug,
      revision: args.revision ?? null,
    }),
  installCollection: (args: {
    gameId: string;
    slug: string;
    revision?: number | null;
    includeOptional: boolean;
    name?: string | null;
  }) => invoke<number>("install_collection", args),
  importThunderstoreProfile: (gameId: string, code: string) =>
    invoke<ThunderstoreProfileImportResult>("import_thunderstore_profile", {
      gameId,
      code,
    }),
  exportShareCode: (gameId: string, name?: string | null) =>
    invoke<ExportShareResult>("export_share_code", {
      gameId,
      name: name ?? null,
    }),
  writeShareFile: (path: string, code: string) =>
    invoke<void>("write_share_file", { path, code }),
  readShareFile: (path: string) => invoke<string>("read_share_file", { path }),
  decodeShareCode: (code: string) =>
    invoke<ShareDecodePreview>("decode_share_code", { code }),
  detectImportCode: (code: string) =>
    invoke<DetectImportResult>("detect_import_code", { code }),
  importCode: (gameId: string, code: string, name?: string | null) =>
    invoke<ImportCodeResult>("import_code", {
      gameId,
      code,
      name: name ?? null,
    }),
  importShareCode: (gameId: string, code: string, name?: string | null) =>
    invoke<ShareImportResult>("import_share_code", {
      gameId,
      code,
      name: name ?? null,
    }),
  recordEmperorShare: (args: {
    gameId: string;
    collectionId: string;
    name: string;
    code?: string | null;
    files: { domain: string; modId: number; fileId: number }[];
    memberIds?: string[] | null;
    existingModIds?: string[] | null;
  }) =>
    invoke<InstalledCollection>("record_emperor_share", {
      gameId: args.gameId,
      collectionId: args.collectionId,
      name: args.name,
      code: args.code ?? null,
      files: args.files,
      memberIds: args.memberIds ?? null,
      existingModIds: args.existingModIds ?? null,
    }),
  emperorShareId: (code: string) => invoke<string>("emperor_share_id", { code }),
  listSavedCollections: () =>
    invoke<SavedCollectionEntry[]>("list_saved_collections"),
  saveCollectionCode: (args: {
    code: string;
    name?: string | null;
    sourceGameId?: string | null;
  }) =>
    invoke<SavedCollectionEntry>("save_collection_code", {
      code: args.code,
      name: args.name ?? null,
      sourceGameId: args.sourceGameId ?? null,
    }),
  getSavedCollection: (id: string) =>
    invoke<SavedCollectionDetail>("get_saved_collection", { id }),
  renameSavedCollection: (id: string, name: string) =>
    invoke<SavedCollectionEntry>("rename_saved_collection", { id, name }),
  renameInstalledCollection: (
    gameId: string,
    collectionId: string,
    name: string,
  ) =>
    invoke<InstalledCollection>("rename_installed_collection", {
      gameId,
      collectionId,
      name,
    }),
  deleteSavedCollection: (id: string) =>
    invoke<void>("delete_saved_collection", { id }),
  listInstalledCollections: (gameId: string) =>
    invoke<InstalledCollection[]>("list_installed_collections", { gameId }),
  saveCollectionChanges: (gameId: string, collectionId: string) =>
    invoke<InstalledCollection>("save_collection_changes", {
      gameId,
      collectionId,
    }),
  setCollectionAutoSave: (
    gameId: string,
    collectionId: string,
    autoSave: boolean,
  ) =>
    invoke<InstalledCollection>("set_collection_auto_save", {
      gameId,
      collectionId,
      autoSave,
    }),
  uninstallCollection: (gameId: string, collectionId: string) =>
    invoke<number>("uninstall_collection", { gameId, collectionId }),
  checkInstalledCollectionsLocal: (gameId: string) =>
    invoke<CollectionIntegrityReport[]>("check_installed_collections_local", {
      gameId,
    }),
  checkCollectionIntegrity: (
    gameId: string,
    collectionId: string,
    checkRemote = true,
  ) =>
    invoke<CollectionIntegrityReport>("check_collection_integrity", {
      gameId,
      collectionId,
      checkRemote,
    }),
  retryCollectionMods: (
    gameId: string,
    collectionId: string,
    targets: RetryCollectionModTarget[],
  ) =>
    invoke<RetryCollectionModsResult>("retry_collection_mods", {
      gameId,
      collectionId,
      targets,
    }),
  setModCollections: (gameId: string, modId: string, collectionIds: string[]) =>
    invoke<StagedMod>("set_mod_collections", { gameId, modId, collectionIds }),
  recordNexusCollection: (args: {
    gameId: string;
    slug: string;
    name: string;
    revision?: number | null;
    files: { modId: number; fileId: number }[];
    existingModIds?: string[] | null;
  }) =>
    invoke<InstalledCollection>("record_nexus_collection", {
      gameId: args.gameId,
      slug: args.slug,
      name: args.name,
      revision: args.revision ?? null,
      files: args.files,
      existingModIds: args.existingModIds ?? null,
    }),
  handleNxm: (url: string) => invoke<DownloadItem>("handle_nxm", { url }),
  takePendingEmperorModLink: () =>
    invoke<EmperorOpenMod | null>("take_pending_emperor_mod_link"),
  importMods: (args: { gameId: string; paths: string[] }) =>
    invoke<ImportResult>("import_mods", args),
  importAssistDownload: (path: string) =>
    invoke<StagedMod | null>("import_assist_download", { path }),
  listMods: (gameId: string) => invoke<StagedMod[]>("list_mods", { gameId }),
  wineOverrideAdvice: (gameId: string) =>
    invoke<WineOverrideAdvice | null>("wine_override_advice", { gameId }),
  applyWineOverrides: (gameId: string) =>
    invoke<WineOverrideAdvice | null>("apply_wine_overrides", { gameId }),
  fillStagedPictures: (gameId: string) =>
    invoke<StagedMod[]>("fill_staged_pictures", { gameId }),
  checkStagedModUpdates: (gameId: string) =>
    invoke<StagedModUpdate[]>("check_staged_mod_updates", { gameId }),
  updateStagedMod: (gameId: string, stagedId: string) =>
    invoke<StagedMod>("update_staged_mod", { gameId, stagedId }),
  setModEnabled: (gameId: string, modId: string, enabled: boolean) =>
    invoke<void>("set_mod_enabled", { gameId, modId, enabled }),
  setLoadOrder: (gameId: string, orderedIds: string[]) =>
    invoke<void>("set_load_order", { gameId, orderedIds }),
  getModOptions: (gameId: string, modId: string) =>
    invoke<ModOptionsView | null>("get_mod_options", { gameId, modId }),
  setModOptions: (gameId: string, modId: string, selection: ModOptionSelection) =>
    invoke<ModOptionSelection>("set_mod_options", { gameId, modId, selection }),
  listModsWithOptions: (gameId: string) =>
    invoke<string[]>("list_mods_with_options", { gameId }),
  getFomodInstaller: (
    gameId: string,
    modId: string,
    selection?: FomodSelection | null,
  ) =>
    invoke<FomodInstallerView | null>(
      "get_fomod_installer",
      selection != null ? { gameId, modId, selection } : { gameId, modId },
    ),
  setFomodSelection: (gameId: string, modId: string, selection: FomodSelection) =>
    invoke<FomodSelection>("set_fomod_selection", { gameId, modId, selection }),
  listModsNeedingFomod: (gameId: string) =>
    invoke<string[]>("list_mods_needing_fomod", { gameId }),
  removeMod: (gameId: string, modId: string) =>
    invoke<void>("remove_mod", { gameId, modId }),
  removeAllMods: (gameId: string) =>
    invoke<void>("remove_all_mods", { gameId }),
  launchModded: (gameId: string) => invoke<LaunchResult>("launch_modded", { gameId }),
  launchUnmodded: (gameId: string) =>
    invoke<GameLaunchState>("launch_unmodded", { gameId }),
  revertDeployment: (gameId: string) =>
    invoke<void>("revert_deployment", { gameId }),
  getLaunchState: (gameId: string) =>
    invoke<GameLaunchState>("get_launch_state", { gameId }),
  hasActiveDeployment: (gameId: string) =>
    invoke<boolean>("has_active_deployment", { gameId }),
  closeGame: (gameId: string) => invoke<void>("close_game", { gameId }),
  setLaunchCommandSource: (gameId: string, source: LaunchCommandSource) =>
    invoke<void>("set_launch_command_source", { gameId, source }),
  getLaunchPreview: (gameId: string) =>
    invoke<LaunchPreview>("get_launch_preview", { gameId }),
  setGameLaunchOptions: (
    gameId: string,
    source: LaunchCommandSource,
    customEnvVars: LaunchEnvVar[],
    customWrapper: string | null,
    customLaunchArgs: string | null,
  ) =>
    invoke<LaunchPreview>("set_game_launch_options", {
      gameId,
      source,
      customEnvVars,
      customWrapper,
      customLaunchArgs,
    }),
  diagnoseLoad: (gameId: string) =>
    invoke<LoadDiagnosis>("diagnose_load", { gameId }),
  listDownloads: () => invoke<DownloadItem[]>("list_downloads"),
  clearRecentDownloads: (ids: string[]) =>
    invoke<void>("clear_recent_downloads", { ids }),
  removeDownload: (id: string) => invoke<void>("remove_download", { id }),
  cancelDownload: (id: string) => invoke<void>("cancel_download", { id }),
  cancelDownloadBatch: (batchId: string) =>
    invoke<void>("cancel_download_batch", { batchId }),
  pauseDownload: (id: string) => invoke<void>("pause_download", { id }),
  resumeDownload: (id: string) => invoke<void>("resume_download", { id }),
  restartDownload: (id: string) => invoke<void>("restart_download", { id }),
  forceResetDownload: (id: string) => invoke<void>("force_reset_download", { id }),
  getToolsStatus: () => invoke<ToolsStatusResponse>("get_tools_status"),
  checkToolUpdates: () => invoke<ToolUpdatesResponse>("check_tool_updates"),
  installTool: (tool: "lsfg_vk" | "autohdr_vk") =>
    invoke<ToolsStatusResponse>("install_tool", { tool }),
  getLsfgVkConfig: () => invoke<LsfgVkConfig>("get_lsfg_vk_config"),
  setLsfgVkConfig: (config: LsfgVkConfig) =>
    invoke<void>("set_lsfg_vk_config", { config }),
  getAutohdrVkConfig: () => invoke<AutoHdrVkConfig>("get_autohdr_vk_config"),
  setAutohdrVkConfig: (config: AutoHdrVkConfig) =>
    invoke<void>("set_autohdr_vk_config", { config }),
  detectGameExecutables: (gameId: string) =>
    invoke<string[]>("detect_game_executables", { gameId }),
  getGameTools: (gameId: string) => invoke<GameToolsResponse>("get_game_tools", { gameId }),
  setGameTools: (gameId: string, overrides: GameToolOverrides) =>
    invoke<GameToolsResponse>("set_game_tools", {
      request: { game_id: gameId, overrides },
    }),
  syncGameTools: (gameId: string) => invoke<GameToolsResponse>("sync_game_tools", { gameId }),
  getFriendsState: () => invoke<FriendsState>("get_friends_state"),
  setP2pDisplayName: (name: string) =>
    invoke<FriendsState>("set_p2p_display_name", { name }),
  setP2pProfilePicture: (dataUrl: string | null) =>
    invoke<FriendsState>("set_p2p_profile_picture", { dataUrl }),
  setP2pAppearOffline: (appearOffline: boolean) =>
    invoke<FriendsState>("set_p2p_appear_offline", { appearOffline }),
  setCollectionVisibility: (collectionId: string, hidden: boolean) =>
    invoke<FriendsState>("set_collection_visibility", { collectionId, hidden }),
  setCollectionSharing: (collectionId: string, visibility: CollectionVisibility) =>
    invoke<FriendsState>("set_collection_sharing", { collectionId, visibility }),
  addFriendByCode: (code: string) =>
    invoke<FriendsState>("add_friend_by_code", { code }),
  respondFriendRequest: (peerId: string, accept: boolean) =>
    invoke<FriendsState>("respond_friend_request", { peerId, accept }),
  removeFriend: (peerId: string) =>
    invoke<FriendsState>("remove_friend", { peerId }),
  refreshFriends: () => invoke<FriendsState>("refresh_friends"),
  decodeFriendShareCode: (code: string) =>
    invoke<ShareDecodePreview>("decode_friend_share_code", { code }),
  joinOrSyncFriendCollection: (args: {
    gameId: string;
    peerId: string;
    friendCollectionId: string;
  }) =>
    invoke<FriendSyncResult>("join_or_sync_friend_collection", {
      gameId: args.gameId,
      peerId: args.peerId,
      friendCollectionId: args.friendCollectionId,
    }),
  finalizeFriendCollectionSync: (args: {
    gameId: string;
    peerId: string;
    friendCollectionId: string;
    shareCode: string;
  }) =>
    invoke<InstalledCollection>("finalize_friend_collection_sync", {
      gameId: args.gameId,
      peerId: args.peerId,
      friendCollectionId: args.friendCollectionId,
      shareCode: args.shareCode,
    }),
  setFriendCollectionAutoSync: (
    gameId: string,
    collectionId: string,
    autoSync: boolean,
  ) =>
    invoke<void>("set_friend_collection_auto_sync", {
      gameId,
      collectionId,
      autoSync,
    }),
  getFriendCollectionSyncStates: (gameId: string) =>
    invoke<FriendCollectionSyncStatus[]>("get_friend_collection_sync_states", {
      gameId,
    }),
};
