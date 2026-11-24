mod app_log;
mod app_update;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod assist;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod commands;
mod config;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod deep_link;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod detection;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod diagnostics;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod games;
mod github_release;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod launch;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod launcher_install;
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
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod migration;
#[cfg(any(target_os = "android", target_os = "ios", test))]
mod mobile_catalog;
#[cfg(any(target_os = "android", target_os = "ios"))]
mod mobile_commands;
mod modio_api;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod mods;
mod nexus;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod p2p;
mod rate_limit;
mod remote;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod share;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod system_theme;
mod thunderstore;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod tools;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use commands::AppState;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri_plugin_deep_link::DeepLinkExt;

#[cfg(any(target_os = "android", target_os = "ios"))]
#[tauri::mobile_entry_point]
pub fn run() {
    app_log::init(None);

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init());

    #[cfg(target_os = "android")]
    {
        builder = builder
            .plugin(
                tauri::plugin::Builder::<tauri::Wry, ()>::new("app-update")
                    .setup(|app, api| {
                        use tauri::Manager;
                        let handle = api.register_android_plugin(
                            "dev.emperormodmanager.mobile",
                            "AppUpdatePlugin",
                        )?;
                        app.manage(handle);
                        Ok(())
                    })
                    .build(),
            )
            .plugin(
                tauri::plugin::Builder::<tauri::Wry, ()>::new("foldable")
                    .setup(|_app, api| {
                        let _ = api.register_android_plugin(
                            "dev.emperormodmanager.mobile",
                            "FoldablePlugin",
                        )?;
                        Ok(())
                    })
                    .build(),
            )
            .plugin(
                tauri::plugin::Builder::<tauri::Wry, ()>::new("theme")
                    .setup(|app, api| {
                        use tauri::Manager;
                        if let Ok(handle) = api.register_android_plugin(
                            "dev.emperormodmanager.mobile",
                            "ThemePlugin",
                        ) {
                            app.manage(mobile_commands::AndroidThemeHandle(handle));
                        }
                        Ok(())
                    })
                    .build(),
            );
    }

    builder
        .invoke_handler(tauri::generate_handler![
            mobile_commands::get_app_info,
            mobile_commands::get_mobile_system_palette,
            app_update::check_app_update,
            app_update::install_app_update,
            mobile_commands::discover_lan_desktops,
            mobile_commands::pair_with_desktop,
            mobile_commands::unpair_desktop,
            mobile_commands::get_mobile_connection_state,
            mobile_commands::get_mobile_snapshot,
            mobile_commands::sync_mobile_now,
            mobile_commands::mobile_queue_download,
            mobile_commands::mobile_control_download,
            mobile_commands::mobile_set_mod_enabled,
            mobile_commands::mobile_set_load_order,
            mobile_commands::mobile_set_collection_enabled,
            mobile_commands::mobile_set_active_game,
            mobile_commands::mobile_launch_game,
            mobile_commands::mobile_close_game,
            mobile_commands::mobile_get_game_cover,
            mobile_commands::mobile_catalog_rpc,
            mobile_commands::mobile_auto_sort_mods,
            mobile_commands::mobile_sort_bethesda_plugins,
            mobile_commands::mobile_check_mod_updates,
            mobile_commands::mobile_update_mod,
            mobile_commands::mobile_update_all_mods,
            mobile_commands::mobile_get_fomod_installer,
            mobile_commands::mobile_set_fomod_selection,
            mobile_commands::mobile_get_fomod_image,
            mobile_commands::mobile_get_saved_collection,
            mobile_commands::mobile_check_collection_integrity,
            mobile_commands::mobile_sync_friend_collection,
            mobile_commands::mobile_save_collection_changes,
            mobile_commands::mobile_set_collection_auto_save,
            mobile_commands::mobile_rename_installed_collection,
            mobile_commands::mobile_uninstall_collection,
            mobile_commands::mobile_dismiss_banner,
        ])
        .setup(|app| {
            use tauri::Manager;
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            let paths = config::Paths::from_dirs(config_dir, data_dir, cache_dir)?;
            let remote = std::sync::Arc::new(remote::client::RemoteClientService::new(paths));
            {
                let bg_remote = std::sync::Arc::clone(&remote);
                let bg_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    loop {
                        if !bg_remote.connection_state().paired {
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            continue;
                        }
                        if let Ok(snap) = bg_remote.sync_now().await {
                            let _ = bg_app.emit("remote-connection-changed", bg_remote.connection_state());
                            let _ = bg_app.emit("remote-snapshot-synced", &snap);
                        }
                        let evt_remote = std::sync::Arc::clone(&bg_remote);
                        let evt_app = bg_app.clone();
                        let res = bg_remote
                            .subscribe_events(move |evt| {
                                use crate::remote::protocol::RemoteEvent;
                                evt_remote.apply_event_to_cache(&evt);
                                match evt {
                                    RemoteEvent::BannersChanged { banners, .. } => {
                                        let _ = evt_app.emit("remote-banners-changed", &banners);
                                    }
                                    RemoteEvent::DownloadProgress { item } => {
                                        let _ = evt_app.emit("remote-download-progress", &item);
                                    }
                                    RemoteEvent::SnapshotInvalidated { .. } => {
                                        let r = std::sync::Arc::clone(&evt_remote);
                                        let a = evt_app.clone();
                                        tauri::async_runtime::spawn(async move {
                                            if let Ok(snap) = r.sync_now().await {
                                                let _ = a.emit("remote-connection-changed", r.connection_state());
                                                let _ = a.emit("remote-snapshot-synced", &snap);
                                            }
                                        });
                                    }
                                    RemoteEvent::Revoked => {
                                        let _ = evt_app.emit("remote-connection-changed", evt_remote.connection_state());
                                    }
                                    _ => {
                                        if let Some(snap) = evt_remote.cached_snapshot() {
                                            let _ = evt_app.emit("remote-snapshot-synced", &snap);
                                        }
                                    }
                                }
                            })
                            .await;
                        let _ = bg_app.emit("remote-connection-changed", bg_remote.connection_state());
                        let backoff = if res.is_ok() { 2 } else { 4 };
                        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                    }
                });
            }
            let catalog = std::sync::Arc::new(mobile_catalog::MobileCatalogService::new(
                std::sync::Arc::clone(&remote),
            ));
            app.manage(mobile_commands::MobileAppState { remote, catalog });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri mobile application");
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
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
            commands::scan_launcher_libraries,
            commands::get_launcher_install_options,
            commands::request_launcher_install,
            commands::cancel_launcher_install,
            commands::get_steam_sync_status,
            commands::set_steam_sync_config,
            commands::clear_steam_api_key,
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
            commands::set_auto_sort_before_deploy,
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
            commands::auto_sort_staged_mods,
            commands::get_bethesda_plugins,
            commands::set_bethesda_plugin_order,
            commands::sort_bethesda_plugins,
            commands::update_loot_masterlist,
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
            assist::reload_assist_webview,
            assist::clear_assist_session,
            assist::clear_modio_session,
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
            commands::set_friends_list_visibility,
            commands::set_allow_mutual_connections,
            commands::set_allow_cache_relay,
            commands::set_share_active_loadouts_in_feed,
            commands::set_share_playing_status,
            commands::add_friend_by_code,
            commands::add_friend_by_peer,
            commands::cancel_queued_friend_invite,
            commands::respond_friend_request,
            commands::remove_friend,
            commands::refresh_friends,
            commands::mark_friend_feed_read,
            commands::clear_friend_feed,
            commands::decode_friend_share_code,
            commands::join_or_sync_friend_collection,
            commands::finalize_friend_collection_sync,
            commands::set_friend_collection_auto_sync,
            commands::get_friend_collection_sync_states,
            commands::plan_collection_resolution,
            commands::set_collection_enabled,
            commands::start_mobile_pairing,
            commands::get_active_mobile_pairing,
            commands::cancel_mobile_pairing,
            commands::list_paired_devices,
            commands::revoke_paired_device,
            commands::set_paired_device_permissions,
            commands::get_remote_allow_relay,
            commands::set_remote_allow_relay,
            commands::sync_desktop_banners,
        ])
        .setup(|app| {
            {
                use tauri::Manager;
                let st = app.state::<AppState>();
                st.remote_host.attach_backend(std::sync::Arc::new(
                    remote::desktop_backend::TauriDesktopBackend::new(app.handle().clone()),
                ));
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
