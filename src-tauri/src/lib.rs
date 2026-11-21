mod app_log;
mod app_update;
mod assist;
mod commands;
mod config;
mod deep_link;
mod detection;
mod diagnostics;
mod games;
mod github_release;
mod launch;
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
mod linux_embed;
#[cfg(target_os = "linux")]
mod linux_startup;
mod migration;
mod modio_api;
mod mods;
mod nexus;
mod p2p;
mod rate_limit;
mod share;
mod system_theme;
mod thunderstore;
mod tools;

use commands::AppState;
use tauri_plugin_deep_link::DeepLinkExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Must run before GTK/Tauri: a full /tmp (quota) makes icon load assert-abort.
    #[cfg(target_os = "linux")]
    {
        let _ = linux_startup::ensure_writable_tmpdir();
    }

    let log_path = config::Paths::resolve().ok().map(|paths| paths.log_file());
    app_log::init(log_path);

    let state = match AppState::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to init app state: {e:#}");
            std::process::exit(1);
        }
    };

    // Single-instance must be first: on Linux/Windows a deep link starts a second
    // process; with the deep-link feature that process forwards the URL into the
    // primary instance's on_open_url instead of opening another window.
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .register_uri_scheme_protocol("nxm", |ctx, request| {
            let uri = request.uri().to_string();
            assist::handle_nxm_uri_scheme(ctx.app_handle(), &uri);
            tauri::http::Response::builder()
                .status(204)
                .body(Vec::new())
                .unwrap()
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            app_update::check_app_update,
            app_update::install_app_update,
            commands::scan_games,
            commands::resolve_catalog_covers,
            commands::resolve_share_mod_metadata,
            commands::list_plugins,
            commands::list_managed,
            commands::manage_game,
            commands::update_managed_game,
            commands::set_game_version,
            commands::set_ignored_requirements,
            commands::set_wine_overrides_dismissed,
            commands::detect_ue_layout,
            commands::suggest_catalog_ids,
            commands::suggest_catalog_ids_batch,
            commands::unmanage_game,
            commands::get_unmanage_collection_status,
            commands::get_game_health,
            commands::relink_managed_game,
            commands::get_settings,
            commands::complete_initial_setup,
            commands::set_adult_content,
            commands::set_game_version_warnings,
            commands::set_theme,
            commands::set_theme_style,
            commands::set_accent_color,
            commands::set_linux_system_backend,
            commands::get_system_palette,
            commands::set_autoclick_free_download,
            commands::set_install_click_behavior,
            commands::set_recent_downloads_limit,
            commands::set_banner_timeouts,
            commands::set_banner_persistence,
            commands::set_dismiss_banner_on_retry,
            commands::get_persisted_banners,
            commands::save_persisted_banners,
            commands::set_browse_grid_size,
            commands::set_active_game,
            commands::set_api_key,
            commands::validate_user,
            commands::clear_api_key,
            commands::set_modio_api_key,
            commands::clear_modio_api_key,
            commands::search_mods,
            commands::search_catalog,
            commands::suggest_authors,
            commands::get_thunderstore_package,
            commands::download_thunderstore_mod,
            commands::get_modio_mod,
            commands::modio_files,
            commands::download_modio_mod,
            commands::get_game,
            commands::get_mod,
            commands::missing_nexus_requirements,
            commands::check_mod_game_versions,
            commands::advise_collection_versions,
            commands::nexus_mod_requirements,
            commands::link_staged_requirements,
            commands::mod_files,
            commands::search_collections,
            commands::search_collection_catalog,
            commands::browse_meta,
            commands::get_collection,
            commands::collection_files,
            commands::download_mod,
            commands::install_collection,
            commands::import_thunderstore_profile,
            commands::export_share_code,
            commands::write_share_file,
            commands::read_share_file,
            commands::decode_share_code,
            commands::detect_import_code,
            commands::import_code,
            commands::import_share_code,
            commands::record_emperor_share,
            commands::emperor_share_id,
            commands::list_saved_collections,
            commands::save_collection_code,
            commands::get_saved_collection,
            commands::rename_saved_collection,
            commands::rename_installed_collection,
            commands::delete_saved_collection,
            commands::handle_nxm,
            deep_link::take_pending_emperor_mod_link,
            deep_link::take_pending_emperor_friend_link,
            commands::import_mods,
            commands::import_assist_download,
            commands::scan_mod_orphans,
            commands::clear_download_cache,
            commands::recover_legacy_mod_data,
            commands::list_mods,
            commands::wine_override_advice,
            commands::apply_wine_overrides,
            commands::fill_staged_pictures,
            commands::check_staged_mod_updates,
            commands::update_staged_mod,
            commands::list_installed_collections,
            commands::uninstall_collection,
            commands::set_mod_collections,
            commands::save_collection_changes,
            commands::set_collection_auto_save,
            commands::record_nexus_collection,
            commands::check_installed_collections_local,
            commands::check_collection_integrity,
            commands::retry_collection_mods,
            commands::set_mod_enabled,
            commands::set_load_order,
            commands::get_mod_options,
            commands::set_mod_options,
            commands::list_mods_with_options,
            commands::get_fomod_installer,
            commands::set_fomod_selection,
            commands::list_mods_needing_fomod,
            commands::remove_mod,
            commands::remove_all_mods,
            commands::launch_modded,
            commands::launch_unmodded,
            commands::revert_deployment,
            commands::get_launch_state,
            commands::has_active_deployment,
            commands::close_game,
            commands::set_launch_command_source,
            commands::get_launch_preview,
            commands::set_game_launch_options,
            commands::diagnose_load,
            commands::list_downloads,
            commands::clear_recent_downloads,
            commands::remove_download,
            commands::cancel_download,
            commands::cancel_download_batch,
            commands::pause_download,
            commands::resume_download,
            commands::restart_download,
            commands::force_reset_download,
            assist::open_download_assist,
            assist::open_nexus_website_login,
            assist::open_modio_website_login,
            assist::confirm_nexus_website_login,
            assist::close_download_assist,
            assist::clear_assist_session,
            assist::set_assist_bounds,
            assist::set_assist_visible,
            #[cfg(target_os = "linux")]
            tools::get_tools_status,
            #[cfg(target_os = "linux")]
            tools::check_tool_updates,
            #[cfg(target_os = "linux")]
            tools::install_tool,
            tools::get_lsfg_vk_config,
            tools::set_lsfg_vk_config,
            tools::get_autohdr_vk_config,
            tools::set_autohdr_vk_config,
            tools::detect_game_executables,
            tools::get_game_tools,
            tools::set_game_tools,
            tools::sync_game_tools,
            commands::get_friends_state,
            commands::set_p2p_display_name,
            commands::set_p2p_profile_picture,
            commands::set_p2p_appear_offline,
            commands::set_collection_visibility,
            commands::set_collection_sharing,
            commands::add_friend_by_code,
            commands::respond_friend_request,
            commands::remove_friend,
            commands::refresh_friends,
            commands::decode_friend_share_code,
            commands::join_or_sync_friend_collection,
            commands::finalize_friend_collection_sync,
            commands::set_friend_collection_auto_sync,
            commands::get_friend_collection_sync_states,
        ])
        .setup(|app| {
            {
                use tauri::Manager;
                let st = app.state::<AppState>();
                st.p2p.start(app.handle().clone());
                let games = st
                    .config
                    .lock()
                    .map(|c| c.managed_games.clone())
                    .unwrap_or_default();
                launch::reconcile_orphaned_deployments(
                    app.handle(),
                    &st.paths,
                    &games,
                    &st.launch_states,
                );
            }
            // Register nxm:// and emperor:// with the OS when supported (Windows installer / Linux desktop).
            if let Err(e) = app.deep_link().register_all() {
                log::warn!("deep-link register_all failed: {e}");
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let s = url.to_string();
                    if s.starts_with("nxm://") {
                        assist::emit_nxm_url(&handle, &s);
                    } else if s.starts_with("emperor://") {
                        deep_link::emit_emperor_url(&handle, &s);
                    }
                }
            });
            for arg in std::env::args().skip(1) {
                if arg.starts_with("nxm://") {
                    assist::emit_nxm_url(app.handle(), &arg);
                } else if arg.starts_with("emperor://") {
                    deep_link::emit_emperor_url(app.handle(), &arg);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
