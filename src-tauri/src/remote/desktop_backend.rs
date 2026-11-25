//! Desktop `RemoteBackend` implementation backed by Tauri `AppHandle` and `AppState`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use base64::Engine;
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;
use crate::config::{save_config, APP_VERSION};
use crate::mods;
use crate::mods::fomod::FomodSelection;
use crate::share;

use super::host::{BoxFuture, RemoteBackend};
use super::lan_discovery::local_host_name;
use super::protocol::{
    DesktopSnapshot, PairedDevice, RemoteAccountSecrets, RemoteAccountSummary,
    RemoteDownloadControl, RemoteDownloadRequest, RemoteRequest,
};

pub struct TauriDesktopBackend {
    app: AppHandle,
    /// Caches whether a mod's immutable `staging_path` contains a configurable
    /// FOMOD installer so `build_snapshot` doesn't walk disk / parse XML for
    /// every staged mod on every sync.
    fomod_needed_cache: std::sync::Mutex<HashMap<String, bool>>,
}

impl TauriDesktopBackend {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            fomod_needed_cache: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn staging_has_configurable_fomod(&self, staging_path: &str) -> bool {
        if let Ok(cache) = self.fomod_needed_cache.lock() {
            if let Some(&cached) = cache.get(staging_path) {
                return cached;
            }
        }
        let root = Path::new(staging_path);
        let needed = root.exists()
            && matches!(
                mods::fomod::load_module(root),
                Ok(Some(module)) if !module.unsupported_scripted
            );
        if let Ok(mut cache) = self.fomod_needed_cache.lock() {
            if cache.len() > 4096 {
                cache.clear();
            }
            cache.insert(staging_path.to_string(), needed);
        }
        needed
    }
}

impl RemoteBackend for TauriDesktopBackend {
    fn build_snapshot(&self, allow_remote_launch: bool, rev: u64) -> DesktopSnapshot {
        let state = self.app.state::<AppState>();
        let cfg = state.config.lock().map(|c| c.clone()).unwrap_or_default();
        let nexus_connected = state
            .api_key
            .lock()
            .ok()
            .and_then(|k| k.as_ref().map(|s| !s.trim().is_empty()))
            .unwrap_or(false);
        let modio_connected = state
            .modio_api_key
            .lock()
            .ok()
            .and_then(|k| k.as_ref().map(|s| !s.trim().is_empty()))
            .unwrap_or(false);
        let nexus_user = state
            .user
            .lock()
            .ok()
            .and_then(|u| u.as_ref().and_then(|v| serde_json::to_value(v).ok()));
        let friends_state = state.p2p.get_state().ok();

        let account = RemoteAccountSummary {
            nexus_connected,
            nexus_user,
            modio_connected,
            p2p_display_name: friends_state
                .as_ref()
                .map(|f| f.display_name.clone())
                .unwrap_or_default(),
            p2p_profile_picture: friends_state
                .as_ref()
                .and_then(|f| f.profile_picture.clone()),
            theme: serde_json::to_value(&cfg.theme)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| "system".into()),
            theme_style: serde_json::to_value(&cfg.theme_style)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| "app".into()),
            accent_color: cfg.accent_color.clone(),
            adult_content: cfg.adult_content,
            game_version_warnings: cfg.game_version_warnings,
            auto_sort_before_deploy: cfg.auto_sort_before_deploy,
        };

        let mut mods_by_game = HashMap::new();
        let mut installed_collections_by_game = HashMap::new();
        let mut bethesda_plugins_by_game = HashMap::new();
        let mut mods_needing_fomod_by_game = HashMap::new();

        for game in &cfg.managed_games {
            if let Ok(order) = mods::load_loadorder(&state.paths, &game.id) {
                let mut staged = order.mods;
                staged.sort_by_key(|m| m.order);

                let needing: Vec<String> = staged
                    .iter()
                    .filter(|m| {
                        m.fomod_selection.is_none()
                            && self.staging_has_configurable_fomod(&m.staging_path)
                    })
                    .map(|m| m.id.clone())
                    .collect();
                if !needing.is_empty() {
                    mods_needing_fomod_by_game.insert(game.id.clone(), needing);
                }

                let vals = staged
                    .into_iter()
                    .filter_map(|m| serde_json::to_value(m).ok())
                    .collect();
                mods_by_game.insert(game.id.clone(), vals);
            }
            if let Ok(cols) = mods::list_installed_collections(&state.paths, &game.id) {
                let vals = cols
                    .into_iter()
                    .filter_map(|c| serde_json::to_value(c).ok())
                    .collect();
                installed_collections_by_game.insert(game.id.clone(), vals);
            }
            if let Ok(plugins_state) =
                crate::commands::get_bethesda_plugins(state.clone(), game.id.clone())
            {
                if let Ok(val) = serde_json::to_value(plugins_state) {
                    bethesda_plugins_by_game.insert(game.id.clone(), val);
                }
            }
        }

        let saved_collections = share::load_saved_collections(&state.paths)
            .map(|store| store.collections)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| serde_json::to_value(s).ok())
            .collect();

        let downloads = state
            .downloads
            .lock()
            .map(|d| {
                d.iter()
                    .filter_map(|item| serde_json::to_value(item).ok())
                    .collect()
            })
            .unwrap_or_default();

        let launch_states = state
            .launch_states
            .lock()
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| serde_json::to_value(v).ok().map(|val| (k.clone(), val)))
                    .collect()
            })
            .unwrap_or_default();

        let banners = state.remote_host.desktop_banners();

        DesktopSnapshot {
            rev,
            synced_at: Utc::now().to_rfc3339(),
            desktop_name: local_host_name(),
            app_version: APP_VERSION.to_string(),
            allow_remote_launch,
            account,
            active_game_id: cfg.last_active_game_id.clone(),
            managed_games: cfg
                .managed_games
                .iter()
                .filter_map(|g| serde_json::to_value(g).ok())
                .collect(),
            mods_by_game,
            installed_collections_by_game,
            saved_collections,
            bethesda_plugins_by_game,
            downloads,
            friends: friends_state.and_then(|f| serde_json::to_value(f).ok()),
            launch_states,
            banners,
            mods_needing_fomod_by_game,
        }
    }

    fn account_secrets(&self) -> RemoteAccountSecrets {
        let state = self.app.state::<AppState>();
        let nexus_api_key = state.api_key.lock().ok().and_then(|g| g.clone());
        let modio_api_key = state.modio_api_key.lock().ok().and_then(|g| g.clone());
        RemoteAccountSecrets {
            nexus_api_key,
            modio_api_key,
        }
    }

    fn allow_relay(&self) -> bool {
        let state = self.app.state::<AppState>();
        state
            .config
            .lock()
            .map(|c| c.remote_allow_relay)
            .unwrap_or(true)
    }

    fn cover_thumb(&self, game_id: &str) -> Result<(String, Vec<u8>)> {
        let state = self.app.state::<AppState>();
        let cover_path = state
            .config
            .lock()
            .ok()
            .and_then(|c| {
                c.managed_games
                    .iter()
                    .find(|g| g.id == game_id)
                    .and_then(|g| g.cover_path.clone())
            })
            .ok_or_else(|| anyhow!("No local cover for game {game_id}"))?;

        let path = Path::new(&cover_path);
        let bytes = fs::read(path)?;
        let mime = match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("png") => "image/png",
            Some("webp") => "image/webp",
            _ => "image/jpeg",
        };
        Ok((mime.to_string(), bytes))
    }

    fn execute_request(
        &self,
        req: RemoteRequest,
        allow_remote_launch: bool,
    ) -> BoxFuture<'_, Result<Option<serde_json::Value>>> {
        let app = self.app.clone();
        Box::pin(async move {
            let state = app.state::<AppState>();
            match req {
                RemoteRequest::SetActiveGame { game_id } => {
                    {
                        let mut cfg = state.config.lock().map_err(|e| anyhow!("{e}"))?;
                        cfg.last_active_game_id = game_id;
                        save_config(&state.paths, &cfg)?;
                    }
                    let _ = app.emit("remote-state-changed", ());
                    Ok(None)
                }
                RemoteRequest::SetModEnabled {
                    game_id,
                    mod_id,
                    enabled,
                } => {
                    mods::set_enabled(&state.paths, &game_id, &mod_id, enabled)?;
                    state.p2p.trigger_local_activity_sync();
                    let _ = app.emit("remote-state-changed", ());
                    Ok(None)
                }
                RemoteRequest::SetLoadOrder {
                    game_id,
                    ordered_ids,
                } => {
                    mods::set_load_order(&state.paths, &game_id, &ordered_ids)?;
                    state.p2p.trigger_local_activity_sync();
                    let _ = app.emit("remote-state-changed", ());
                    Ok(None)
                }
                RemoteRequest::AutoSortStagedMods { game_id } => {
                    let res = crate::commands::auto_sort_staged_mods(state, game_id)
                        .await
                        .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(res)?))
                }
                RemoteRequest::SortBethesdaPlugins { game_id } => {
                    let res = crate::commands::sort_bethesda_plugins(state, game_id)
                        .await
                        .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(res)?))
                }
                RemoteRequest::CheckStagedModUpdates { game_id } => {
                    let p2p = state.p2p.clone();
                    let _ = p2p.refresh_all_friends().await;
                    let updates = crate::commands::check_staged_mod_updates(state, game_id)
                        .await
                        .map_err(|e| anyhow!(e))?;
                    Ok(Some(serde_json::to_value(updates)?))
                }
                RemoteRequest::UpdateStagedMod { game_id, mod_id } => {
                    if !state.ensure_premium_status().await {
                        let updates = crate::commands::check_staged_mod_updates(
                            state.clone(),
                            game_id.clone(),
                        )
                        .await
                        .unwrap_or_default();
                        let order = mods::load_loadorder(&state.paths, &game_id).ok();
                        let staged = order
                            .as_ref()
                            .and_then(|o| o.mods.iter().find(|m| m.id == mod_id));
                        let upd = updates.iter().find(|u| u.staged_id == mod_id);
                        if let (Some(m), Some(u)) = (staged, upd) {
                            if u.source == mods::ModSource::Nexus && u.nexus_file_id.is_some() {
                                let domain = if !m.domain.is_empty() {
                                    m.domain.clone()
                                } else {
                                    state
                                        .config
                                        .lock()
                                        .ok()
                                        .and_then(|c| {
                                            c.managed_games
                                                .iter()
                                                .find(|g| g.id == game_id)
                                                .map(|g| g.nexus_domain.clone())
                                        })
                                        .unwrap_or_default()
                                };
                                let _ = app.emit(
                                    "remote-enqueue-assist",
                                    serde_json::json!({
                                        "batch_label": format!("Update {}", m.name),
                                        "entries": [{
                                            "game_id": game_id,
                                            "domain": domain,
                                            "mod_id": m.nexus_mod_id,
                                            "file_id": u.nexus_file_id.unwrap(),
                                            "name": m.name,
                                            "version": u.available_version,
                                            "replace_staged_id": m.id,
                                            "file_name": u.file_name,
                                        }],
                                    }),
                                );
                                return Ok(Some(serde_json::json!({ "queued_assist": 1 })));
                            }
                        }
                    }
                    let updated =
                        crate::commands::update_staged_mod(app.clone(), state, game_id, mod_id)
                            .await
                            .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(updated)?))
                }
                RemoteRequest::UpdateAllStagedMods { game_id } => {
                    let updates =
                        crate::commands::check_staged_mod_updates(state.clone(), game_id.clone())
                            .await
                            .map_err(|e| anyhow!(e))?;
                    let is_premium = state.ensure_premium_status().await;
                    let order = mods::load_loadorder(&state.paths, &game_id).ok();
                    let fallback_domain = state
                        .config
                        .lock()
                        .ok()
                        .and_then(|c| {
                            c.managed_games
                                .iter()
                                .find(|g| g.id == game_id)
                                .map(|g| g.nexus_domain.clone())
                        })
                        .unwrap_or_default();

                    let mut assist_entries = Vec::new();
                    let mut api_updates = Vec::new();
                    for u in updates {
                        if !is_premium
                            && u.source == mods::ModSource::Nexus
                            && u.nexus_file_id.is_some()
                        {
                            if let Some(m) = order
                                .as_ref()
                                .and_then(|o| o.mods.iter().find(|m| m.id == u.staged_id))
                            {
                                let domain = if !m.domain.is_empty() {
                                    m.domain.clone()
                                } else {
                                    fallback_domain.clone()
                                };
                                assist_entries.push(serde_json::json!({
                                    "game_id": game_id,
                                    "domain": domain,
                                    "mod_id": m.nexus_mod_id,
                                    "file_id": u.nexus_file_id.unwrap(),
                                    "name": m.name,
                                    "version": u.available_version,
                                    "replace_staged_id": m.id,
                                    "file_name": u.file_name,
                                }));
                                continue;
                            }
                        }
                        api_updates.push(u);
                    }

                    let queued_assist = assist_entries.len();
                    if queued_assist > 0 {
                        let _ = app.emit(
                            "remote-enqueue-assist",
                            serde_json::json!({
                                "batch_label": format!(
                                    "Update {queued_assist} mod{}",
                                    if queued_assist == 1 { "" } else { "s" }
                                ),
                                "entries": assist_entries,
                            }),
                        );
                    }

                    let mut updated_count = 0usize;
                    let mut errors = Vec::new();
                    for u in api_updates {
                        match crate::commands::update_staged_mod(
                            app.clone(),
                            state.clone(),
                            game_id.clone(),
                            u.staged_id.clone(),
                        )
                        .await
                        {
                            Ok(_) => updated_count += 1,
                            Err(e) => errors.push(format!("{}: {e}", u.staged_id)),
                        }
                    }
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::json!({
                        "updated": updated_count,
                        "queued_assist": queued_assist,
                        "errors": errors,
                    })))
                }
                RemoteRequest::GetFomodInstaller { game_id, mod_id } => {
                    let view =
                        crate::commands::get_fomod_installer(state, game_id, mod_id, None)
                            .map_err(|e| anyhow!(e))?;
                    Ok(Some(serde_json::to_value(view)?))
                }
                RemoteRequest::SetFomodSelection {
                    game_id,
                    mod_id,
                    selection,
                } => {
                    let parsed: FomodSelection = serde_json::from_value(selection)?;
                    let saved =
                        crate::commands::set_fomod_selection(state, game_id, mod_id, parsed)
                            .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(saved)?))
                }
                RemoteRequest::GetFomodImage { path } => {
                    let p = Path::new(&path);
                    let canonical = fs::canonicalize(p)?;
                    let data_root = fs::canonicalize(&state.paths.data_dir)?;
                    if !canonical.starts_with(&data_root) {
                        bail!("Image path outside Emperor data directory");
                    }
                    let bytes = fs::read(&canonical)?;
                    if bytes.len() > 1024 * 1024 {
                        bail!("FOMOD image exceeds 1 MiB limit");
                    }
                    let mime = match canonical
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_ascii_lowercase())
                        .as_deref()
                    {
                        Some("png") => "image/png",
                        Some("webp") => "image/webp",
                        Some("gif") => "image/gif",
                        _ => "image/jpeg",
                    };
                    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                    Ok(Some(serde_json::json!({
                        "data_url": format!("data:{mime};base64,{b64}")
                    })))
                }
                RemoteRequest::GetSavedCollectionDetail { id } => {
                    let detail = crate::commands::get_saved_collection(state, id)
                        .await
                        .map_err(|e| anyhow!(e))?;
                    Ok(Some(serde_json::to_value(detail)?))
                }
                RemoteRequest::CheckCollectionIntegrity {
                    game_id,
                    collection_id,
                } => {
                    let report = crate::commands::check_collection_integrity(
                        state,
                        game_id,
                        collection_id,
                        Some(false),
                    )
                    .await
                    .map_err(|e| anyhow!(e))?;
                    Ok(Some(serde_json::to_value(report)?))
                }
                RemoteRequest::JoinOrSyncFriendCollection {
                    game_id,
                    peer_id,
                    friend_collection_id,
                } => {
                    let res = crate::commands::join_or_sync_friend_collection(
                        app.clone(),
                        state,
                        game_id,
                        peer_id,
                        friend_collection_id,
                        None,
                    )
                    .await
                    .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(res)?))
                }
                RemoteRequest::DismissDesktopBanner { id } => {
                    state.remote_host.dismiss_desktop_banner(id);
                    let _ = app.emit("remote-dismiss-banner", id);
                    Ok(None)
                }
                RemoteRequest::SetCollectionEnabled {
                    game_id,
                    collection_id,
                    enabled,
                } => {
                    let updated = crate::commands::set_collection_enabled(
                        state,
                        game_id,
                        collection_id,
                        enabled,
                        None,
                    )
                    .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(updated)?))
                }
                RemoteRequest::SetModCollections {
                    game_id,
                    mod_id,
                    collection_ids,
                } => {
                    let updated = crate::commands::set_mod_collections(
                        state,
                        game_id,
                        mod_id,
                        collection_ids,
                    )
                    .await
                    .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(updated)?))
                }
                RemoteRequest::SaveCollectionChanges {
                    game_id,
                    collection_id,
                } => {
                    let res =
                        crate::commands::save_collection_changes(state, game_id, collection_id)
                            .await
                            .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(res)?))
                }
                RemoteRequest::SetCollectionAutoSave {
                    game_id,
                    collection_id,
                    auto_save,
                } => {
                    let res = mods::set_collection_auto_save(
                        &state.paths,
                        &game_id,
                        &collection_id,
                        auto_save,
                    )?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(res)?))
                }
                RemoteRequest::RenameInstalledCollection {
                    game_id,
                    collection_id,
                    name,
                } => {
                    let _ = mods::rename_installed_collection(
                        &state.paths,
                        &game_id,
                        &collection_id,
                        &name,
                    )?;
                    state.p2p.trigger_local_activity_sync();
                    let _ = app.emit("remote-state-changed", ());
                    Ok(None)
                }
                RemoteRequest::RenameSavedCollection { id, name } => {
                    let res = crate::commands::rename_saved_collection(state, id, name)
                        .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(res)?))
                }
                RemoteRequest::DeleteSavedCollection { id } => {
                    crate::commands::delete_saved_collection(state, id).map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(None)
                }
                RemoteRequest::UninstallCollection {
                    game_id,
                    collection_id,
                    remove_mods: _,
                } => {
                    let res =
                        crate::commands::uninstall_collection(state, game_id, collection_id)
                            .map_err(|e| anyhow!(e))?;
                    let _ = app.emit("remote-state-changed", ());
                    Ok(Some(serde_json::to_value(res)?))
                }
                RemoteRequest::RemoveMod { game_id, mod_id } => {
                    mods::remove_mod(&state.paths, &game_id, &mod_id)?;
                    state.p2p.trigger_local_activity_sync();
                    let _ = app.emit("remote-state-changed", ());
                    Ok(None)
                }
                RemoteRequest::QueueDownload(dl_req) => match dl_req {
                    RemoteDownloadRequest::NexusMod {
                        game_id,
                        domain,
                        mod_id,
                        file_id,
                        mod_name,
                        file_name,
                        version,
                    } => {
                        if !state.ensure_premium_status().await {
                            let _ = app.emit(
                                "remote-enqueue-assist",
                                serde_json::json!({
                                    "batch_label": null,
                                    "entries": [{
                                        "game_id": game_id,
                                        "domain": domain,
                                        "mod_id": mod_id,
                                        "file_id": file_id,
                                        "name": mod_name,
                                        "version": version,
                                        "replace_staged_id": null,
                                        "file_name": file_name,
                                    }],
                                }),
                            );
                            return Ok(Some(serde_json::json!({ "queued_assist": 1 })));
                        }
                        let res = crate::commands::download_mod(
                            app.clone(),
                            state,
                            game_id,
                            domain,
                            mod_id,
                            file_id,
                            mod_name,
                            version,
                            None,
                            file_name,
                        )
                        .await
                        .map_err(|e| anyhow!(e))?;
                        let _ = app.emit("remote-state-changed", ());
                        Ok(Some(serde_json::to_value(res)?))
                    }
                    RemoteDownloadRequest::ThunderstoreMod {
                        game_id,
                        community,
                        namespace,
                        name,
                    } => {
                        let res = crate::commands::download_thunderstore_mod(
                            app.clone(),
                            state,
                            game_id,
                            community,
                            namespace,
                            name,
                            None,
                            None,
                            None,
                        )
                        .await
                        .map_err(|e| anyhow!(e))?;
                        let _ = app.emit("remote-state-changed", ());
                        Ok(Some(serde_json::to_value(res)?))
                    }
                    RemoteDownloadRequest::ModioMod {
                        game_id,
                        modio_game_id,
                        mod_id,
                        file_id,
                    } => {
                        let res = crate::commands::download_modio_mod(
                            app.clone(),
                            state,
                            game_id,
                            modio_game_id,
                            mod_id,
                            file_id,
                            format!("mod.io #{mod_id}"),
                            None,
                            Some(true),
                        )
                        .await
                        .map_err(|e| anyhow!(e))?;
                        let _ = app.emit("remote-state-changed", ());
                        Ok(Some(serde_json::to_value(res)?))
                    }
                    RemoteDownloadRequest::NexusCollection {
                        game_id,
                        slug,
                        revision,
                    } => {
                        let res = crate::commands::install_collection(
                            app.clone(),
                            state,
                            game_id,
                            slug,
                            revision.map(|r| r as i64),
                            false,
                            None,
                            None,
                        )
                        .await
                        .map_err(|e| anyhow!(e))?;
                        let _ = app.emit("remote-state-changed", ());
                        Ok(Some(serde_json::to_value(res)?))
                    }
                    RemoteDownloadRequest::ImportShareCode { game_id, code } => {
                        let res = crate::commands::import_share_code(
                            app.clone(),
                            state,
                            game_id.clone(),
                            code,
                            None,
                            None,
                        )
                        .await
                        .map_err(|e| anyhow!(e))?;
                        if !res.needs_assist.is_empty() {
                            let entries: Vec<serde_json::Value> = res
                                .needs_assist
                                .iter()
                                .map(|a| {
                                    serde_json::json!({
                                        "game_id": game_id,
                                        "domain": a.domain,
                                        "mod_id": a.mod_id,
                                        "file_id": a.file_id,
                                        "name": a.name,
                                        "version": a.version,
                                        "replace_staged_id": null,
                                        "file_name": null,
                                    })
                                })
                                .collect();
                            let _ = app.emit(
                                "remote-enqueue-assist",
                                serde_json::json!({
                                    "batch_label": res.name,
                                    "entries": entries,
                                }),
                            );
                        }
                        let _ = app.emit("remote-state-changed", ());
                        Ok(Some(serde_json::to_value(res)?))
                    }
                },
                RemoteRequest::ControlDownload(ctrl) => {
                    match ctrl {
                        RemoteDownloadControl::Pause { id } => {
                            crate::commands::pause_download(state, id).map_err(|e| anyhow!(e))?;
                        }
                        RemoteDownloadControl::Resume { id } => {
                            crate::commands::resume_download(app.clone(), state, id)
                                .map_err(|e| anyhow!(e))?;
                        }
                        RemoteDownloadControl::Cancel { id } => {
                            crate::commands::cancel_download(app.clone(), state, id)
                                .map_err(|e| anyhow!(e))?;
                        }
                        RemoteDownloadControl::CancelBatch { batch_id } => {
                            crate::commands::cancel_download_batch(app.clone(), state, batch_id)
                                .map_err(|e| anyhow!(e))?;
                        }
                        RemoteDownloadControl::Restart { id } => {
                            crate::commands::restart_download(app.clone(), state, id)
                                .map_err(|e| anyhow!(e))?;
                        }
                        RemoteDownloadControl::Remove { id } => {
                            crate::commands::remove_download(state, id).map_err(|e| anyhow!(e))?;
                        }
                        RemoteDownloadControl::ClearRecent => {
                            let ids: Vec<String> = state
                                .downloads
                                .lock()
                                .map(|d| d.iter().map(|i| i.id.clone()).collect())
                                .unwrap_or_default();
                            crate::commands::clear_recent_downloads(state, ids)
                                .map_err(|e| anyhow!(e))?;
                        }
                    }
                    let _ = app.emit("remote-state-changed", ());
                    Ok(None)
                }
                RemoteRequest::LaunchGame { game_id, modded } => {
                    if !allow_remote_launch {
                        bail!("Remote game launch is disabled for this device in Desktop Settings");
                    }
                    if modded {
                        let res = crate::commands::launch_modded(app.clone(), state, game_id)
                            .await
                            .map_err(|e| anyhow!(e))?;
                        Ok(Some(serde_json::to_value(res)?))
                    } else {
                        let res = crate::commands::launch_unmodded(app.clone(), state, game_id)
                            .await
                            .map_err(|e| anyhow!(e))?;
                        Ok(Some(serde_json::to_value(res)?))
                    }
                }
                RemoteRequest::CloseGame { game_id } => {
                    if !allow_remote_launch {
                        bail!("Remote game launch is disabled for this device in Desktop Settings");
                    }
                    crate::commands::close_game(state, game_id)
                        .await
                        .map_err(|e| anyhow!(e))?;
                    Ok(None)
                }
                RemoteRequest::CatalogRpc { method, params } => {
                    let result = dispatch_catalog_rpc(state, &method, params).await?;
                    Ok(Some(result))
                }
                other => {
                    bail!("Unsupported remote RPC action: {other:?}");
                }
            }
        })
    }

    fn on_paired(&self, device: &PairedDevice) {
        let _ = self.app.emit("mobile-paired", device);
    }

    fn on_pairing_failed(&self, remaining_attempts: u32, message: &str) {
        let _ = self.app.emit(
            "mobile-pairing-failed",
            serde_json::json!({
                "remaining_attempts": remaining_attempts,
                "message": message,
            }),
        );
    }
}

async fn dispatch_catalog_rpc(
    state: tauri::State<'_, AppState>,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let str_param = |k: &str| -> Option<String> {
        params.get(k).and_then(|v| v.as_str()).map(String::from)
    };
    let u64_param = |k: &str| -> Option<u64> { params.get(k).and_then(|v| v.as_u64()) };
    let u32_param = |k: &str| -> Option<u32> {
        params.get(k).and_then(|v| v.as_u64()).map(|n| n as u32)
    };

    match method {
        "search_catalog" => {
            let game_id = str_param("game_id").ok_or_else(|| anyhow!("missing game_id"))?;
            let query = str_param("query").unwrap_or_default();
            let res = crate::commands::search_catalog(
                state,
                game_id,
                query,
                str_param("source_filter"),
                str_param("sort"),
                str_param("category"),
                None,
                None,
                str_param("game_version"),
                str_param("author"),
                u32_param("offset"),
                u32_param("count"),
            )
            .await
            .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "search_collection_catalog" => {
            let game_id = str_param("game_id").ok_or_else(|| anyhow!("missing game_id"))?;
            let query = str_param("query").unwrap_or_default();
            let res = crate::commands::search_collection_catalog(
                state,
                game_id,
                query,
                str_param("source_filter"),
                str_param("sort"),
                str_param("category"),
                None,
                None,
                str_param("game_version"),
                str_param("author"),
                u32_param("offset"),
                u32_param("count"),
            )
            .await
            .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "get_mod" => {
            let domain = str_param("domain").ok_or_else(|| anyhow!("missing domain"))?;
            let mod_id = u64_param("mod_id").ok_or_else(|| anyhow!("missing mod_id"))?;
            let res = crate::commands::get_mod(state, domain, mod_id)
                .await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "mod_files" => {
            let domain = str_param("domain").ok_or_else(|| anyhow!("missing domain"))?;
            let mod_id = u64_param("mod_id").ok_or_else(|| anyhow!("missing mod_id"))?;
            let res = crate::commands::mod_files(state, domain, mod_id)
                .await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "get_collection" => {
            let slug = str_param("slug").ok_or_else(|| anyhow!("missing slug"))?;
            let res = crate::commands::get_collection(state, slug, str_param("domain"))
                .await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "collection_files" => {
            let slug = str_param("slug").ok_or_else(|| anyhow!("missing slug"))?;
            let revision = params.get("revision").and_then(|v| v.as_i64());
            let res = crate::commands::collection_files(state, slug, revision)
                .await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "get_thunderstore_package" => {
            let community = str_param("community").ok_or_else(|| anyhow!("missing community"))?;
            let namespace = str_param("namespace").ok_or_else(|| anyhow!("missing namespace"))?;
            let name = str_param("name").ok_or_else(|| anyhow!("missing name"))?;
            let res = crate::commands::get_thunderstore_package(state, community, namespace, name)
                .await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "get_modio_mod" => {
            let game_id = u32_param("game_id").ok_or_else(|| anyhow!("missing game_id"))?;
            let mod_id = u64_param("mod_id").ok_or_else(|| anyhow!("missing mod_id"))?;
            let res = crate::commands::get_modio_mod(state, game_id, mod_id)
                .await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "modio_files" => {
            let game_id = u32_param("game_id").ok_or_else(|| anyhow!("missing game_id"))?;
            let mod_id = u64_param("mod_id").ok_or_else(|| anyhow!("missing mod_id"))?;
            let res = crate::commands::modio_files(state, game_id, mod_id)
                .await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "browse_meta" => {
            let domain = str_param("domain").ok_or_else(|| anyhow!("missing domain"))?;
            let res = crate::commands::browse_meta(state, domain)
                .await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "decode_share_code" => {
            let code = str_param("code").ok_or_else(|| anyhow!("missing code"))?;
            let res = crate::commands::decode_share_code(code).map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        "resolve_share_mod_metadata" => {
            let mods_val = params
                .get("mods")
                .cloned()
                .ok_or_else(|| anyhow!("missing mods"))?;
            let mods: Vec<share::ShareModEntry> = serde_json::from_value(mods_val)?;
            let res = crate::commands::resolve_share_mod_metadata(state, mods)
                .await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_value(res)?)
        }
        other => bail!("Unsupported catalog RPC method: {other}"),
    }
}
