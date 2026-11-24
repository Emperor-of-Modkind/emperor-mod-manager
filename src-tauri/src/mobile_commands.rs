//! Android/mobile Tauri IPC command surface backed by `RemoteClientService`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use crate::config::APP_NAME;
use crate::mobile_catalog::MobileCatalogService;
use crate::remote::client::RemoteClientService;
use crate::remote::lan_discovery;
use crate::remote::protocol::{
    DesktopSnapshot, DiscoveredDesktop, MobileConnectionState, RemoteDownloadControl,
    RemoteDownloadRequest, RemoteRequest, RemoteResponse,
};

pub struct MobileAppState {
    pub remote: Arc<RemoteClientService>,
    pub catalog: Arc<MobileCatalogService>,
}

#[cfg(target_os = "android")]
pub struct AndroidThemeHandle(pub tauri::plugin::PluginHandle<tauri::Wry>);

#[tauri::command]
pub fn get_mobile_system_palette(app: AppHandle) -> Option<serde_json::Value> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        let handle = app.try_state::<AndroidThemeHandle>()?;
        let val: serde_json::Value = handle
            .0
            .run_mobile_plugin("getSystemPalette", ())
            .ok()?;
        if val.get("available").and_then(|v| v.as_bool()) == Some(true) {
            return Some(val);
        }
        None
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        None
    }
}

#[tauri::command]
pub fn get_app_info() -> serde_json::Value {
    serde_json::json!({
        "name": APP_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "is_mobile": true,
        "linux_only": false,
    })
}

#[tauri::command]
pub async fn discover_lan_desktops() -> Result<Vec<DiscoveredDesktop>, String> {
    lan_discovery::discover_lan_desktops(Duration::from_millis(1800))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pair_with_desktop(
    app: AppHandle,
    state: State<'_, MobileAppState>,
    desktop_endpoint_id: String,
    lan_addrs: Vec<String>,
    code: String,
    device_name: String,
    device_model: String,
) -> Result<MobileConnectionState, String> {
    let parsed_addrs: Vec<SocketAddr> = lan_addrs
        .into_iter()
        .filter_map(|s| s.parse::<SocketAddr>().ok())
        .collect();
    let conn_state = state
        .remote
        .pair_with_desktop(
            &desktop_endpoint_id,
            parsed_addrs,
            &code,
            device_name,
            device_model,
        )
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit("remote-connection-changed", &conn_state);
    if let Some(snap) = state.remote.cached_snapshot() {
        let _ = app.emit("remote-snapshot-synced", &snap);
    }
    Ok(conn_state)
}

#[tauri::command]
pub async fn unpair_desktop(
    app: AppHandle,
    state: State<'_, MobileAppState>,
) -> Result<(), String> {
    state.remote.unpair().await.map_err(|e| e.to_string())?;
    let _ = app.emit("remote-connection-changed", state.remote.connection_state());
    Ok(())
}

#[tauri::command]
pub fn get_mobile_connection_state(state: State<'_, MobileAppState>) -> MobileConnectionState {
    state.remote.connection_state()
}

#[tauri::command]
pub fn get_mobile_snapshot(state: State<'_, MobileAppState>) -> Option<DesktopSnapshot> {
    state.remote.cached_snapshot()
}

#[tauri::command]
pub async fn sync_mobile_now(
    app: AppHandle,
    state: State<'_, MobileAppState>,
) -> Result<DesktopSnapshot, String> {
    let snap = state.remote.sync_now().await.map_err(|e| e.to_string())?;
    let _ = app.emit("remote-connection-changed", state.remote.connection_state());
    let _ = app.emit("remote-snapshot-synced", &snap);
    Ok(snap)
}

#[tauri::command]
pub async fn mobile_queue_download(
    state: State<'_, MobileAppState>,
    request: RemoteDownloadRequest,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::QueueDownload(request)).await
}

#[tauri::command]
pub async fn mobile_control_download(
    state: State<'_, MobileAppState>,
    control: RemoteDownloadControl,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::ControlDownload(control)).await
}

#[tauri::command]
pub async fn mobile_set_mod_enabled(
    state: State<'_, MobileAppState>,
    game_id: String,
    mod_id: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::SetModEnabled {
            game_id,
            mod_id,
            enabled,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_set_load_order(
    state: State<'_, MobileAppState>,
    game_id: String,
    ordered_ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::SetLoadOrder {
            game_id,
            ordered_ids,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_set_collection_enabled(
    state: State<'_, MobileAppState>,
    game_id: String,
    collection_id: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::SetCollectionEnabled {
            game_id,
            collection_id,
            enabled,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_set_active_game(
    state: State<'_, MobileAppState>,
    game_id: Option<String>,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::SetActiveGame { game_id }).await
}

#[tauri::command]
pub async fn mobile_launch_game(
    state: State<'_, MobileAppState>,
    game_id: String,
    modded: bool,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::LaunchGame { game_id, modded }).await
}

#[tauri::command]
pub async fn mobile_close_game(
    state: State<'_, MobileAppState>,
    game_id: String,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::CloseGame { game_id }).await
}

#[tauri::command]
pub async fn mobile_get_game_cover(
    state: State<'_, MobileAppState>,
    game_id: String,
) -> Result<Option<String>, String> {
    state
        .remote
        .get_cover_data_url(&game_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mobile_catalog_rpc(
    state: State<'_, MobileAppState>,
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if let Some(local) = state
        .catalog
        .handle_rpc(&method, &params)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(local);
    }
    send_ok(&state, &RemoteRequest::CatalogRpc { method, params }).await
}

#[tauri::command]
pub async fn mobile_auto_sort_mods(
    state: State<'_, MobileAppState>,
    game_id: String,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::AutoSortStagedMods { game_id }).await
}

#[tauri::command]
pub async fn mobile_sort_bethesda_plugins(
    state: State<'_, MobileAppState>,
    game_id: String,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::SortBethesdaPlugins { game_id }).await
}

#[tauri::command]
pub async fn mobile_check_mod_updates(
    state: State<'_, MobileAppState>,
    game_id: String,
) -> Result<serde_json::Value, String> {
    let updates = state
        .catalog
        .check_mod_updates(&game_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::Value::Array(updates))
}

#[tauri::command]
pub async fn mobile_update_mod(
    state: State<'_, MobileAppState>,
    game_id: String,
    mod_id: String,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::UpdateStagedMod { game_id, mod_id }).await
}

#[tauri::command]
pub async fn mobile_update_all_mods(
    state: State<'_, MobileAppState>,
    game_id: String,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::UpdateAllStagedMods { game_id }).await
}

#[tauri::command]
pub async fn mobile_get_fomod_installer(
    state: State<'_, MobileAppState>,
    game_id: String,
    mod_id: String,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::GetFomodInstaller { game_id, mod_id }).await
}

#[tauri::command]
pub async fn mobile_set_fomod_selection(
    state: State<'_, MobileAppState>,
    game_id: String,
    mod_id: String,
    selection: serde_json::Value,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::SetFomodSelection {
            game_id,
            mod_id,
            selection,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_get_fomod_image(
    state: State<'_, MobileAppState>,
    path: String,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::GetFomodImage { path }).await
}

#[tauri::command]
pub async fn mobile_get_saved_collection(
    state: State<'_, MobileAppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::GetSavedCollectionDetail { id }).await
}

#[tauri::command]
pub async fn mobile_check_collection_integrity(
    state: State<'_, MobileAppState>,
    game_id: String,
    collection_id: String,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::CheckCollectionIntegrity {
            game_id,
            collection_id,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_sync_friend_collection(
    state: State<'_, MobileAppState>,
    game_id: String,
    peer_id: String,
    friend_collection_id: String,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::JoinOrSyncFriendCollection {
            game_id,
            peer_id,
            friend_collection_id,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_save_collection_changes(
    state: State<'_, MobileAppState>,
    game_id: String,
    collection_id: String,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::SaveCollectionChanges {
            game_id,
            collection_id,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_set_collection_auto_save(
    state: State<'_, MobileAppState>,
    game_id: String,
    collection_id: String,
    auto_save: bool,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::SetCollectionAutoSave {
            game_id,
            collection_id,
            auto_save,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_rename_installed_collection(
    state: State<'_, MobileAppState>,
    game_id: String,
    collection_id: String,
    name: String,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::RenameInstalledCollection {
            game_id,
            collection_id,
            name,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_uninstall_collection(
    state: State<'_, MobileAppState>,
    game_id: String,
    collection_id: String,
    remove_mods: bool,
) -> Result<serde_json::Value, String> {
    send_ok(
        &state,
        &RemoteRequest::UninstallCollection {
            game_id,
            collection_id,
            remove_mods,
        },
    )
    .await
}

#[tauri::command]
pub async fn mobile_dismiss_banner(
    state: State<'_, MobileAppState>,
    id: u64,
) -> Result<serde_json::Value, String> {
    send_ok(&state, &RemoteRequest::DismissDesktopBanner { id }).await
}

async fn send_ok(
    state: &State<'_, MobileAppState>,
    req: &RemoteRequest,
) -> Result<serde_json::Value, String> {
    match state.remote.send_request(req).await.map_err(|e| e.to_string())? {
        RemoteResponse::Ok { data, rev } => Ok(data.unwrap_or_else(|| serde_json::json!({ "rev": rev }))),
        RemoteResponse::Error { message, .. } => Err(message),
        other => Err(format!("Unexpected desktop response: {other:?}")),
    }
}
