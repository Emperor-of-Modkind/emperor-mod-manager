//! Android/mobile Tauri IPC command surface backed by `RemoteClientService`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use crate::config::APP_NAME;
use crate::remote::client::RemoteClientService;
use crate::remote::lan_discovery;
use crate::remote::protocol::{
    DesktopSnapshot, DiscoveredDesktop, MobileConnectionState, RemoteDownloadControl,
    RemoteDownloadRequest, RemoteRequest, RemoteResponse,
};

pub struct MobileAppState {
    pub remote: Arc<RemoteClientService>,
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
