//! Desktop `RemoteBackend` implementation backed by Tauri `AppHandle` and `AppState`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;
use crate::config::{save_config, APP_VERSION};
use crate::mods;
use crate::share;

use super::host::{BoxFuture, RemoteBackend};
use super::lan_discovery::local_host_name;
use super::protocol::{
    DesktopSnapshot, PairedDevice, RemoteAccountSecrets, RemoteAccountSummary,
    RemoteDownloadControl, RemoteDownloadRequest, RemoteRequest,
};

pub struct TauriDesktopBackend {
    app: AppHandle,
}

impl TauriDesktopBackend {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
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
        let bethesda_plugins_by_game = HashMap::new();

        for game in &cfg.managed_games {
            if let Ok(order) = mods::load_loadorder(&state.paths, &game.id) {
                let mut staged = order.mods;
                staged.sort_by_key(|m| m.order);
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
                        Ok(Some(serde_json::to_value(res)?))
                    }
                    RemoteDownloadRequest::ImportShareCode { game_id, code } => {
                        let res = crate::commands::import_share_code(
                            app.clone(),
                            state,
                            game_id,
                            code,
                            None,
                            None,
                        )
                        .await
                        .map_err(|e| anyhow!(e))?;
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
