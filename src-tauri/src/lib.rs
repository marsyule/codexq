mod commands;
pub mod core;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut context = tauri::generate_context!();

    #[cfg(desktop)]
    {
        #[cfg(debug_assertions)]
        {
            context.config_mut().identifier.push_str(".dev");
        }

        #[cfg(not(debug_assertions))]
        {
            if crate::core::paths::is_portable() {
                context.config_mut().identifier.push_str(".portable");
            }
        }
    }

    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                if let Err(err) = window.show() {
                    log::warn!("Failed to show main window on single-instance activation: {err}");
                }
                if let Err(err) = window.unminimize() {
                    log::warn!("Failed to unminimize main window on single-instance activation: {err}");
                }
                if let Err(err) = window.set_focus() {
                    log::warn!("Failed to focus main window on single-instance activation: {err}");
                }
            }
        }));
    }

    builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::refresh_all,
            commands::switch_account,
            commands::restart_codex,
            commands::set_alias,
            commands::reset_all_aliases,
            commands::get_history,
            commands::remove_account,
            commands::list_trash,
            commands::restore_account,
            commands::purge_trash,
            commands::get_app_settings,
            commands::set_app_setting,
            commands::list_account_alarms,
            commands::save_account_alarm,
            commands::delete_account_alarm,
            commands::trigger_warmup,
            commands::set_locale,
            commands::import_auth_content,
            commands::import_auth_file,
            commands::launch_codex_login,
            commands::run_diagnostics,
            commands::get_account_rollover,
            commands::save_account_rollover,
            commands::list_providers,
            commands::save_provider,
            commands::delete_provider,
            commands::test_provider_connectivity,
            commands::switch_to_provider,
            commands::get_active_runtime_mode,
        ])
        .setup(|app| {
            // Ensure host ~/.codex/config.toml enforces cli_auth_credentials_store = "file"
            let _ = crate::core::auth::ensure_host_codex_config();

            let menu = create_tray_menu(app.handle(), "en-US")?;

            // Start background alarm scheduler ticker
            crate::core::scheduler::start_alarm_scheduler(Some(app.handle().clone()));

            let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("CodexQ - Quota Manager");

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }

            let _tray = tray_builder
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                    "refresh" => {
                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = app_handle.emit("tray-refresh", ());
                        });
                    }
                    "restart" => {
                        tauri::async_runtime::spawn(async move {
                            let _ = commands::restart_codex(Some(true), Some(true)).await;
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Prevent app exit on window close; hide to tray instead
            if let Some(window) = app.get_webview_window("main") {
                let w_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w_clone.hide();
                    }
                });
            }

            Ok(())
        })
        .run(context)
        .expect("error while running tauri application");
}

/// Builds the system tray menu localized according to the specified locale string.
///
/// # Arguments
///
/// * `app` - Tauri application handle.
/// * `locale` - Locale identifier (e.g. "zh-CN", "en-US").
///
/// # Errors
///
/// Returns `Err` if creating menu items fails.
pub fn create_tray_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    locale: &str,
) -> tauri::Result<Menu<R>> {
    let is_zh = locale.starts_with("zh");
    let show_text = if is_zh { "显示 / 隐藏主窗口" } else { "Show / Hide Dashboard" };
    let refresh_text = if is_zh { "刷新全部配额" } else { "Refresh All Quotas" };
    let restart_text = if is_zh { "重启 Codex 客户端" } else { "Restart Codex App" };
    let quit_text = if is_zh { "退出 CodexQ" } else { "Quit CodexQ" };

    let show_i = MenuItem::with_id(app, "show", show_text, true, None::<&str>)?;
    let refresh_i = MenuItem::with_id(app, "refresh", refresh_text, true, None::<&str>)?;
    let restart_i = MenuItem::with_id(app, "restart", restart_text, true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", quit_text, true, None::<&str>)?;

    Menu::with_items(app, &[&show_i, &refresh_i, &restart_i, &quit_i])
}

