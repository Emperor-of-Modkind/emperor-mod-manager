export type ThemePreference = "light" | "dark" | "system";
export type ThemeStyle = "brand" | "app" | "system";
export type LinuxSystemBackend = "gtk" | "qt";
export type InstallClickBehavior = "stay" | "downloads";

export type SystemPalette = {
  source: string;
  dark: boolean;
  bg: string;
  bg_2: string;
  panel: string;
  ink: string;
  muted: string;
  line: string;
  accent: string;
  /** Toolkit selection foreground. Used only when it contrasts with `accent`. */
  on_accent?: string | null;
  input_bg: string;
};

export type AppInfo = {
  name: string;
  version: string;
  platform: string;
  linux_only: boolean;
};

export type AppInstallVia = "pacman" | "deb" | "appimage" | "portable" | "nsis";

export type AppUpdateStatus = {
  current_version: string;
  latest_version: string | null;
  update_available: boolean;
  asset_name: string | null;
  release_url: string | null;
  install_via: AppInstallVia | null;
};

export type AppInstallResult = {
  message: string;
  path: string | null;
  will_exit: boolean;
};

export type DetectedGame = {
  id: string;
  title: string;
  install_path: string | null;
  launcher: string;
  supported: boolean;
  plugin_id: string | null;
  nexus_domain: string | null;
  cover_path: string | null;
  /** Remote poster when `cover_path` is empty. */
  cover_url?: string | null;
  /** Launcher icon used only when no poster is available. */
  icon_path?: string | null;
  engine_hint: string | null;
};

export type ManagedGame = {
  id: string;
  title: string;
  nexus_domain: string;
  install_path: string;
  launcher: string;
  plugin_id: string;
  cover_path?: string | null;
  /** Remote poster when `cover_path` is empty. */
  cover_url?: string | null;
  project_name?: string | null;
  thunderstore_community?: string | null;
  modio_game_id?: number | null;
  tool_overrides?: GameToolOverrides | null;
  /** User-set game version. Empty means use the detected executable version. */
  game_version?: string | null;
  /** Requirements hidden from the review until the user unignores them. */
  ignored_requirements?: IgnoredRequirement[];
  /** Optional relative or absolute executable override for launching non-Steam games. */
  launch_executable?: string | null;
  /** Whether Emperor applies its own launch commands (WINEDLLOVERRIDES, tool env) or defers to the launcher. */
  launch_command_source?: LaunchCommandSource;
  custom_env_vars?: LaunchEnvVar[];
  custom_wrapper?: string | null;
  custom_launch_args?: string | null;
  wine_overrides_dismissed?: boolean;
};

export type IgnoredRequirement = {
  key: string;
  name: string;
};

export type ToolGameState = {
  enabled: boolean;
  use_global_settings: boolean;
};

export type GameToolOverrides = {
  lsfg_vk: ToolGameState;
  autohdr_vk: ToolGameState;
  executables: string[];
};

export type ToolStatus = {
  installed: boolean;
  version: string | null;
  path: string | null;
  latest_version: string | null;
  update_available: boolean;
  config_path: string;
};

export type ToolsStatusResponse = {
  platform_linux: boolean;
  lsfg_vk: ToolStatus;
  autohdr_vk: ToolStatus;
  lsfg_vk_repo: string;
  autohdr_vk_repo: string;
};

export type ToolUpdatesResponse = {
  lsfg_vk: ToolStatus;
  autohdr_vk: ToolStatus;
};

export type LsfgVkGlobal = {
  dll: string | null;
  allow_fp16: boolean;
};

export type LsfgVkProfile = {
  name: string;
  active_in: string[];
  multiplier: number;
  target_fps: number;
  flow_scale: number;
  performance_mode: boolean;
  pacing: string;
  gpu: string | null;
};

export type LsfgVkConfig = {
  global: LsfgVkGlobal;
  profiles: LsfgVkProfile[];
};

export type AutoHdrVkGlobal = {
  intensity: number;
  color_intensity: number;
  expansion_shape: number;
  black_floor: number;
  highlight_stretch: number;
  encoding: string;
  set_hdr_metadata: boolean;
  prefer_hdr_swapchain: boolean;
  enabled: boolean;
};

export type AutoHdrVkProfile = {
  exe: string;
  enabled: boolean;
  intensity?: number | null;
  color_intensity?: number | null;
  expansion_shape?: number | null;
  black_floor?: number | null;
  highlight_stretch?: number | null;
};

export type AutoHdrVkConfig = {
  global: AutoHdrVkGlobal;
  profiles: AutoHdrVkProfile[];
};

export type GameToolsResponse = {
  overrides: GameToolOverrides;
  executables: string[];
  steam_app_id: string | null;
  launch_options: string | null;
};

export type GameHealthStatus = "ok" | "missing" | "relocate_candidate";

export type GameHealth = {
  game_id: string;
  status: GameHealthStatus;
  new_install_path: string | null;
  relocate_target_id: string | null;
};

export type UeLayoutInfo = {
  project_name: string;
  binaries_platform: string;
  paks_dir: string;
  binaries_dir: string;
};

export type CatalogCandidate = {
  id: string;
  name: string;
  score: number;
};

export type CatalogSuggestion = {
  nexus_domain: string | null;
  nexus_name: string | null;
  thunderstore_community: string | null;
  thunderstore_name: string | null;
  modio_game_id: number | null;
  modio_name: string | null;
  nexus_candidates?: CatalogCandidate[];
  thunderstore_candidates?: CatalogCandidate[];
  modio_candidates?: CatalogCandidate[];
  ambiguous?: boolean;
};

export type CatalogSuggestRequest = {
  id: string;
  title: string;
};

export type CatalogCoverRequest = {
  id: string;
  nexus_domain?: string | null;
  modio_game_id?: number | null;
  icon_path?: string | null;
};

export type CatalogCoverHit = {
  id: string;
  cover_url: string;
  cover_path?: string | null;
  title?: string | null;
};

export type ShareModMetadataHit = {
  key: string;
  picture_url?: string | null;
  author?: string | null;
  summary?: string | null;
  downloads?: number | null;
  endorsements?: number | null;
};

export type CatalogSource = "nexus" | "thunderstore" | "modio";

export type CatalogHit = {
  source: CatalogSource;
  id: string;
  name: string;
  summary: string | null;
  picture_url: string | null;
  author: string | null;
  downloads: number | null;
  endorsements: number | null;
  category: string | null;
  tags: string[];
  mod_id: number | null;
  domain_name: string | null;
  community: string | null;
  namespace: string | null;
  package_name: string | null;
  full_name: string | null;
  package_url: string | null;
  rating_score: number | null;
  latest_version: string | null;
  modio_game_id: number | null;
  modio_mod_id: number | null;
  profile_url: string | null;
};

export type CatalogSearchPage = {
  items: CatalogHit[];
  total_count: number;
  next_offset: number;
  has_more: boolean;
  nexus_available: boolean;
  thunderstore_available: boolean;
  modio_available: boolean;
};

export type TsPackageVersion = {
  name: string;
  full_name: string;
  description: string;
  icon: string;
  version_number: string;
  dependencies: string[];
  download_url: string;
  downloads: number;
  date_created: string;
  website_url: string;
  is_active: boolean;
  uuid4: string;
  file_size: number;
};

export type TsPackageDetail = {
  community: string;
  namespace: string;
  name: string;
  full_name: string;
  package_url: string;
  uuid4: string;
  rating_score: number;
  is_deprecated: boolean;
  has_nsfw_content: boolean;
  categories: string[];
  description: string | null;
  icon_url: string | null;
  downloads: number;
  versions: TsPackageVersion[];
  latest_version: string | null;
};

export type GameCategory = {
  category_id: number;
  name: string;
};

export type GameInfo = {
  id: number;
  name: string;
  domain_name: string;
  genre: string | null;
  forum_url: string | null;
  nexusmods_url: string | null;
  mods: number | null;
  file_count: number | null;
  downloads: number | null;
  categories: GameCategory[];
};

export type NexusUser = {
  user_id: number;
  key: string;
  name: string;
  is_premium: boolean;
  is_supporter: boolean;
};

export type ModSearchHit = {
  mod_id: number;
  name: string;
  summary: string | null;
  picture_url: string | null;
  downloads: number | null;
  endorsements: number | null;
  author: string | null;
  domain_name: string;
  category?: string | null;
  tags?: string[];
};

export type ModGalleryImage = {
  url: string;
  caption?: string | null;
};

export type ModDetail = {
  mod_id: number;
  name: string;
  summary: string | null;
  description: string | null;
  picture_url: string | null;
  /** Nexus author gallery, in carousel order. */
  image_urls?: ModGalleryImage[];
  author: string | null;
  version: string | null;
  downloads: number | null;
  endorsements: number | null;
  created_timestamp: number | null;
  updated_timestamp: number | null;
  domain_name: string;
  category_id?: number | null;
  category?: string | null;
  uploaded_by?: string | null;
  status?: string | null;
  contains_adult_content?: boolean;
  uploaded_users_profile_url?: string | null;
  /** False when Nexus has hidden, removed, or otherwise unpublished the mod. */
  available?: boolean | null;
  /** Hidden or removed notice from the public mod page, when Nexus says why. */
  availability_note?: string | null;
};

export type ModFileInfo = {
  file_id: number;
  name: string;
  file_name: string | null;
  version: string | null;
  category_name: string | null;
  size_kb: number | null;
  uploaded_timestamp: number | null;
  is_primary: boolean;
  description: string | null;
};

export type CollectionHit = {
  slug: string;
  name: string;
  summary: string | null;
  endorsements: number | null;
  total_downloads: number | null;
  domain_name: string | null;
  revision_number: number | null;
  tile_image_url?: string | null;
  author: string | null;
  category: string | null;
  overall_rating: number | null;
  overall_rating_count: number | null;
  created_at: string | null;
  updated_at: string | null;
  mod_count: number | null;
  file_size: number | null;
  source?: "nexus" | "thunderstore";
  id?: string;
  community?: string | null;
  namespace?: string | null;
  package_name?: string | null;
  latest_version?: string | null;
};

export type CollectionDetail = {
  slug: string;
  name: string;
  summary: string | null;
  description: string | null;
  endorsements: number | null;
  total_downloads: number | null;
  domain_name: string | null;
  revision_number: number | null;
  tile_image_url?: string | null;
};

export type CollectionModFile = {
  file_id: number;
  optional: boolean;
  mod_id: number;
  mod_name: string;
  file_name: string;
  version: string | null;
  domain_name: string;
};

export type BrowseMeta = {
  categories: string[];
  mod_tags: string[];
  collection_tags: string[];
  game_versions: string[];
};

export type TagFilterState = "include" | "exclude";

export type BrowseSearchOpts = {
  sort?: string;
  category?: string | null;
  tagsInclude?: string[];
  tagsExclude?: string[];
  gameVersion?: string | null;
  /** Nexus uploader / Thunderstore owner account name (exact match). */
  author?: string | null;
  offset?: number;
  count?: number;
};

/** Creator autocomplete hit from `suggest_authors`. */
export type AuthorSuggestion = {
  name: string;
  source: "nexus" | "thunderstore";
  member_id: number | null;
  mod_count: number | null;
  avatar: string | null;
};

export type ModSearchPage = {
  items: ModSearchHit[];
  nodes_count: number;
  total_count: number;
};

export type CollectionSearchPage = {
  items: CollectionHit[];
  nodes_count?: number;
  total_count: number;
  next_offset?: number;
  has_more?: boolean;
  nexus_available?: boolean;
  thunderstore_available?: boolean;
};

export type FriendSyncMetadata = {
  friend_peer_id: string;
  friend_collection_id: string;
  last_synced_hash: string;
  last_synced_at: string;
  auto_sync: boolean;
};

export type InstalledCollection = {
  id: string;
  source: "nexus" | "thunderstore" | "emperor";
  kind: "collection" | "modpack" | "profile" | "share";
  name: string;
  slug?: string | null;
  namespace?: string | null;
  package_name?: string | null;
  community?: string | null;
  revision?: number | null;
  version?: string | null;
  profile_code?: string | null;
  mod_ids: string[];
  installed_at: string;
  /** Game versions this collection revision was built for. */
  game_versions?: string[] | null;
  friend_sync?: FriendSyncMetadata | null;
  auto_save?: boolean;
  has_unsaved_changes?: boolean;
};

export type CollectionModIssueKind =
  | { kind: "missing_from_load_order" }
  | { kind: "missing_staging_dir" }
  | {
      kind: "corrupted_staging";
      actual_files: number;
      expected_files?: number | null;
    }
  | { kind: "missing_from_manifest" };

export type CollectionModIssue = {
  mod_id: string;
  name: string;
  source: "nexus" | "thunderstore" | "modio";
  issue: CollectionModIssueKind;
  can_retry: boolean;
  domain: string;
  nexus_mod_id: number;
  nexus_file_id: number;
  ts_namespace?: string | null;
  ts_name?: string | null;
  modio_game_id?: number | null;
  modio_mod_id?: number | null;
  modio_file_id?: number | null;
  version?: string | null;
  file_name?: string | null;
};

export type CollectionIntegrityReport = {
  collection_id: string;
  collection_name: string;
  checked_count: number;
  issues: CollectionModIssue[];
};

export type RetryCollectionModTarget = {
  mod_id: string;
  name: string;
  source: "nexus" | "thunderstore" | "modio";
  domain: string;
  nexus_mod_id: number;
  nexus_file_id: number;
  ts_namespace?: string | null;
  ts_name?: string | null;
  modio_game_id?: number | null;
  modio_mod_id?: number | null;
  modio_file_id?: number | null;
  version?: string | null;
  file_name?: string | null;
};

export type RetryCollectionModFailure = {
  mod_id: string;
  name: string;
  error: string;
};

export type RetryCollectionModsResult = {
  retried: number;
  failed: RetryCollectionModFailure[];
};

export type ShareGameHint = {
  id?: string | null;
  plugin_id?: string | null;
  nexus_domain?: string | null;
  thunderstore_community?: string | null;
  modio_game_id?: number | null;
  /** Display name of the game this share was exported from. */
  title?: string | null;
  /** Unreal project folder when the game uses the generic Unreal plugin. */
  project_name?: string | null;
};

export type ShareModEntry = {
  s: "nexus" | "thunderstore" | "modio";
  domain?: string | null;
  mod_id?: number | null;
  file_id?: number | null;
  community?: string | null;
  namespace?: string | null;
  name?: string | null;
  game_id?: number | null;
  display_name?: string | null;
  version?: string | null;
  fomod?: FomodSelection | null;
  options?: ModOptionSelection | null;
};

export type SavedCollectionEntry = {
  id: string;
  name: string;
  code: string;
  created_at: string;
  updated_at: string;
  source_game_id?: string | null;
  game: ShareGameHint;
  /** Game versions this share was created against. */
  game_versions?: string[] | null;
  mod_count: number;
  nexus_count: number;
  thunderstore_count: number;
  modio_count: number;
  /** emperor-share:{hash}. Filled by the list command. */
  share_id?: string | null;
};

export type SavedCollectionDetail = {
  entry: SavedCollectionEntry;
  mods: ShareModEntry[];
};

export type ShareDecodePreview = {
  name?: string | null;
  game: ShareGameHint;
  mods: ShareModEntry[];
  nexus_count: number;
  thunderstore_count: number;
  modio_count: number;
  code_len: number;
  size_warning: boolean;
};

export type ExportShareResult = {
  code: string;
  warnings: string[];
  skipped: string[];
  mod_count: number;
  saved: SavedCollectionEntry;
};

export type ShareAssistFile = {
  domain: string;
  mod_id: number;
  file_id: number;
  name: string;
  version?: string | null;
  replace_staged_id?: string | null;
};

export type ModSource = "nexus" | "thunderstore" | "modio";

export type ConflictAction =
  | "disable_other_collections"
  | "disable_old_and_install_new"
  | "replace_existing"
  | "keep_existing"
  | "keep_both_enabled";

export type ConflictingCollectionRef = {
  id: string;
  name: string;
  isSynced: boolean;
  enabledModCount: number;
};

export type ConflictingCollectionSummary = {
  id: string;
  name: string;
  isSynced: boolean;
  enabledModCount: number;
  conflictCount: number;
  conflictKeys: string[];
};

export type ModVersionConflict = {
  conflictKey: string;
  source: ModSource;
  incomingName: string;
  incomingVersion?: string | null;
  incomingFileId?: number | null;
  incomingFileName?: string | null;
  existingModId: string;
  existingName: string;
  existingVersion?: string | null;
  existingFileId?: number | null;
  existingFileName?: string | null;
  existingEnabled: boolean;
  existingIndependent: boolean;
  conflictingCollections: ConflictingCollectionRef[];
  recommendedAction: ConflictAction;
};

export type CollectionConflictPlan = {
  targetCollectionId?: string | null;
  targetCollectionName: string;
  isSyncedTarget: boolean;
  exactMatchCount: number;
  repairCount: number;
  installCount: number;
  autoUpdateCount: number;
  conflicts: ModVersionConflict[];
  conflictingCollections: ConflictingCollectionSummary[];
};

export type CollectionResolutionChoices = {
  disableCollectionIds: string[];
  modActions: Record<string, ConflictAction>;
};

export type CollectionPlanSource =
  | { kind: "friend_sync"; peerId: string; friendCollectionId: string }
  | { kind: "share_code"; code: string; name?: string | null }
  | {
      kind: "nexus_collection";
      slug: string;
      revision?: number | null;
      includeOptional?: boolean;
      name?: string | null;
    }
  | { kind: "thunderstore_profile"; code: string }
  | {
      kind: "thunderstore_modpack";
      community: string;
      namespace: string;
      name: string;
      version?: string | null;
    };

export type ShareImportResult = {
  collection_id: string;
  name: string;
  member_ids: string[];
  needs_assist: ShareAssistFile[];
  warnings: string[];
  collection?: InstalledCollection | null;
};

export type DetectImportResult = {
  kind: "emperor" | "thunderstoreprofile" | "unknown";
  preview?: ShareDecodePreview | null;
  message: string;
};

export type ThunderstoreProfileImportResult = {
  collection: InstalledCollection;
  warnings: string[];
};

export type ImportCodeResult =
  | { kind: "emperor"; result: ShareImportResult }
  | {
      kind: "thunderstore_profile";
      collection: InstalledCollection;
      warnings?: string[];
    };

export type ModVersionStatus = "ok" | "mismatch" | "unknown";

export type ModVersionAdvice = {
  staged_id: string;
  status: ModVersionStatus;
  supported: string[];
  message: string;
};

export type GameVersionReport = {
  installed: string | null;
  override_version: string | null;
  current: string | null;
  source: "override" | "detected" | "unknown";
  mods: ModVersionAdvice[];
};

export type CollectionVersionAdvice = {
  id: string;
  status: ModVersionStatus;
  supported: string[];
  message: string;
};

export type GamePluginInfo = {
  id: string;
  display_name: string;
  nexus_domain: string;
};

export type WineDllNeed = {
  dll: string;
  reason: string;
};

export type WineOverrideAdvice = {
  needs: WineDllNeed[];
  launch_options: string;
  typo: boolean;
  can_apply: boolean;
  summary: string;
};

export type StagedMod = {
  id: string;
  name: string;
  source?: "nexus" | "thunderstore" | "modio";
  nexus_mod_id: number;
  nexus_file_id: number;
  version: string | null;
  domain: string;
  staging_path: string;
  enabled: boolean;
  order: number;
  ts_namespace?: string | null;
  ts_name?: string | null;
  ts_package_uuid?: string | null;
  modio_game_id?: number | null;
  modio_mod_id?: number | null;
  modio_file_id?: number | null;
  collection_ids?: string[];
  independent?: boolean;
  depends_on?: string[];
  option_selection?: ModOptionSelection | null;
  fomod_selection?: FomodSelection | null;
  /** File chooser title or archive name, when it differs from the mod title. */
  file_name?: string | null;
  /** Catalog poster. Null means not looked up yet; empty means there is no image. */
  picture_url?: string | null;
  /** Version-line Nexus tags. Null means not looked up yet. */
  supported_game_versions?: string[] | null;
  /** 0 means the dotted-only tag cache still needs a refresh. */
  version_support_generation?: number;
};

export type ImportResult = {
  mods: StagedMod[];
  errors: string[];
};

export type ModSubOption = {
  id: string;
  name: string;
  description: string | null;
  /** Absolute path; render through convertFileSrc. */
  image: string | null;
  include: string[];
};

export type ModOption = {
  id: string;
  name: string;
  description: string | null;
  image: string | null;
  include: string[];
  sub_options: ModSubOption[];
};

export type ModOptionSet = {
  description: string | null;
  icon: string | null;
  options: ModOption[];
};

export type ModOptionSelection = {
  enabled_options: string[];
  /** Option id -> chosen sub-option id. */
  sub_choice: Record<string, string>;
};

export type ModOptionsView = {
  set: ModOptionSet;
  selection: ModOptionSelection;
};

export type FomodGroupType =
  | "SelectAny"
  | "SelectAll"
  | "SelectExactlyOne"
  | "SelectAtMostOne"
  | "SelectAtLeastOne";

export type FomodPluginType =
  | "Optional"
  | "Required"
  | "Recommended"
  | "CouldBeUsable"
  | "NotUsable";

export type FomodPluginView = {
  id: string;
  name: string;
  description: string | null;
  image: string | null;
  plugin_type: FomodPluginType;
};

export type FomodGroupView = {
  id: string;
  name: string;
  type: FomodGroupType;
  plugins: FomodPluginView[];
};

export type FomodStepView = {
  id: string;
  name: string;
  visible: boolean;
  groups: FomodGroupView[];
};

export type FomodSelection = {
  selected: Record<string, string[]>;
};

export type FomodInstallerView = {
  module_name: string;
  steps: FomodStepView[];
  selection: FomodSelection;
  unsupported_scripted: boolean;
};

export type StagedModUpdate = {
  staged_id: string;
  available_version: string | null;
  source: "nexus" | "thunderstore" | "modio";
  nexus_file_id?: number | null;
  ts_version?: string | null;
  modio_file_id?: number | null;
  file_name?: string | null;
};

export type FriendCollectionKind = "active_loadout" | "saved_collection";

export type FriendShareRef = {
  id: string;
  kind: FriendCollectionKind;
  name: string;
  share_code: string;
  share_hash: string;
  updated_at: string;
};

export type SharedFriendRef = {
  peer_id: string;
  display_name: string;
};

export type PeerProfileSummary = {
  peer_id: string;
  display_name: string;
  profile_picture?: string | null;
  updated_at: string;
  collections: FriendShareRef[];
  friends?: SharedFriendRef[] | null;
  allow_mutual_connections?: boolean;
};

export type FriendStatus = "connected" | "pending_outgoing";

export type FriendRecord = {
  peer_id: string;
  display_name: string;
  profile_picture?: string | null;
  status: FriendStatus;
  online: boolean;
  last_seen_at?: string | null;
  cached_profile?: PeerProfileSummary | null;
};

export type PendingFriendRequest = {
  peer_id: string;
  display_name: string;
  profile_picture?: string | null;
  received_at: string;
};

export type CollectionVisibility =
  | { mode: "all_friends" }
  | { mode: "specific_friends"; peer_ids: string[] }
  | { mode: "private" };

export type LocalShareableCollection = {
  id: string;
  kind: FriendCollectionKind;
  name: string;
  game_title: string;
  mod_count: number;
  hidden: boolean;
  visibility?: CollectionVisibility;
};

export type FriendFeedEventKind =
  | "collection_added"
  | "collection_updated"
  | "collection_renamed";

export type FriendFeedDiff = {
  mods_added: string[];
  mods_added_count: number;
  mods_removed: string[];
  mods_removed_count: number;
  mods_updated: string[];
  mods_updated_count: number;
};

export type FriendFeedEvent = {
  id: string;
  peer_id: string;
  friend_display_name: string;
  kind: FriendFeedEventKind;
  collection_id: string;
  collection_kind: FriendCollectionKind;
  collection_name: string;
  previous_name?: string | null;
  game_title?: string | null;
  mod_count?: number | null;
  share_hash: string;
  diff?: FriendFeedDiff | null;
  occurred_at: string;
};

export type FriendsState = {
  endpoint_ready: boolean;
  appear_offline?: boolean;
  peer_id?: string | null;
  join_code?: string | null;
  display_name: string;
  display_name_set: boolean;
  profile_picture?: string | null;
  friends_list_visibility?: CollectionVisibility;
  allow_mutual_connections?: boolean;
  friends: FriendRecord[];
  pending_requests: PendingFriendRequest[];
  local_collections: LocalShareableCollection[];
  feed_events?: FriendFeedEvent[];
  feed_last_read_at?: string | null;
};

export type FriendSyncResult = {
  collection_id: string;
  name: string;
  share_code: string;
  share_hash: string;
  member_ids: string[];
  needs_assist: ShareAssistFile[];
  warnings: string[];
  collection?: InstalledCollection | null;
};

export type FriendCollectionSyncStatus = {
  collection_id: string;
  peer_id: string;
  friend_collection_id: string;
  latest_hash: string;
  has_update: boolean;
  auto_sync: boolean;
  friend_online: boolean;
};

export type ModioModDetail = {
  game_id: number;
  mod_id: number;
  name: string;
  name_id: string;
  summary: string;
  description: string | null;
  picture_url: string | null;
  author: string | null;
  downloads: number;
  profile_url: string;
  tags: string[];
  has_dependencies: boolean;
  primary_file_id: number | null;
};

export type ModioFileInfo = {
  file_id: number;
  filename: string;
  version: string | null;
  filesize: number;
  changelog: string | null;
  is_primary: boolean;
  date_added: number;
};

export type NexusRequirementDownload = {
  domain: string;
  mod_id: number;
  file_id: number;
  name: string;
  version: string | null;
  file_name?: string | null;
  notes: string | null;
  url: string;
  required_by: string[];
  required_by_names: string[];
};

export type ExternalRequirementLink = {
  name: string;
  url: string;
  notes: string | null;
  required_by: string[];
  required_by_names: string[];
};

export type DlcRequirementNote = {
  name: string;
  notes: string | null;
  required_by: string[];
  required_by_names: string[];
};

/** A staged mod that matches a Nexus requirement. Not offered as a download. */
export type StagedRequirementNote = {
  staged_id: string;
  name: string;
  enabled: boolean;
  required_by: string[];
  required_by_names: string[];
};

export type MissingRequirements = {
  nexus: NexusRequirementDownload[];
  external: ExternalRequirementLink[];
  dlc: DlcRequirementNote[];
  staged?: StagedRequirementNote[];
  error?: string | null;
};

export type PluginSkip = {
  plugin: string;
  missing_master: string;
};

export type DeployResult = {
  file_count: number;
  enabled_mods: number;
  warnings: string[];
  plugin_skips?: PluginSkip[];
  missing_requirements?: MissingRequirements;
};

export type LaunchMode = "modded" | "unmodded";

export type LaunchCommandSource = "emperor" | "launcher";

export type LaunchEnvVar = {
  name: string;
  value: string;
};

export type LaunchPreview = {
  source: LaunchCommandSource;
  launcher_name: string;
  auto_env_vars: LaunchEnvVar[];
  custom_env_vars: LaunchEnvVar[];
  custom_wrapper: string;
  custom_launch_args: string;
  launcher_options?: string | null;
  effective_command: string;
};

export type LaunchPhase =
  | "idle"
  | "deploying"
  | "starting"
  | "running"
  | "reverting";

export type GameLaunchState = {
  game_id: string;
  mode: LaunchMode;
  phase: LaunchPhase;
  pids: number[];
  started_at: number;
  error?: string | null;
};

export type LaunchResult = {
  state: GameLaunchState;
  deploy_result?: DeployResult | null;
};

export type LoadFinding = {
  severity: string;
  title: string;
  detail: string;
  related_mod?: string | null;
};

export type LoadDiagnosis = {
  summary: string;
  findings: LoadFinding[];
  report: string;
  excerpt_label: string;
  excerpt_path: string;
  excerpt: string;
};

export type RecoveryReport = {
  games_copied: string[];
  mods_rewritten: number;
  warnings: string[];
};

export type OrphanScan = {
  legacy_game_ids: string[];
  missing_staging: string[];
  untracked_staging: string[];
  deploy_without_loadorder: string[];
};

export type ClearDownloadCacheResult = {
  files_removed: number;
  bytes_freed: number;
};

export type DownloadPageTarget =
  | {
      source: "nexus";
      game_id: string;
      domain: string;
      mod_id: number;
    }
  | {
      source: "thunderstore";
      game_id: string;
      community: string;
      namespace: string;
      package_name: string;
    }
  | {
      source: "modio";
      game_id: string;
      modio_game_id: number;
      modio_mod_id: number;
    };

export type DownloadItem = {
  id: string;
  label: string;
  status: string;
  error: string | null;
  bytes_downloaded: number;
  bytes_total: number | null;
  speed_bps: number;
  batch_id: string | null;
  can_restart?: boolean;
  file_name?: string | null;
  page?: DownloadPageTarget | null;
};

export type BannerTimeouts = {
  info: number | null;
  warn: number | null;
  error: number | null;
};

export type BannerPersistence = "off" | "errors" | "errors_and_warnings";

export type PersistedBanner = {
  id: number;
  kind: "error" | "warn";
  message: string;
  details?: string[];
  download_id?: string | null;
  created_at: number;
};

export type Settings = {
  adult_content: boolean;
  game_version_warnings: boolean;
  autoclick_free_download: boolean;
  last_active_game_id: string | null;
  theme?: ThemePreference;
  theme_style?: ThemeStyle;
  accent_color?: string;
  linux_system_backend?: LinuxSystemBackend;
  install_click_behavior?: InstallClickBehavior;
  recent_downloads_limit?: number;
  browse_grid_size?: number;
  banner_timeouts?: BannerTimeouts;
  banner_persistence?: BannerPersistence;
  dismiss_banner_on_retry?: boolean;
  has_api_key: boolean;
  has_modio_api_key?: boolean;
  initial_setup_complete: boolean;
  nexus_website_signed_in: boolean;
  modio_website_signed_in?: boolean;
  user: NexusUser | null;
  config_dir: string;
  data_dir: string;
  cache_dir: string;
  platform_linux?: boolean;
  platform_windows?: boolean;
};
