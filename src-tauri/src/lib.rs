mod actions;
mod analyzer;
mod commands;
mod db;
mod detect;
mod scan;
mod search;
mod settings;
mod signals;

use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// （重新）注册全局快捷键：先注销旧的，再挂上唤起快搜浮层的回调。
pub fn register_shortcut(app: &AppHandle, sc: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let shortcut: Shortcut = sc
        .parse()
        .map_err(|e| format!("快捷键格式错误: {e}"))?;
    gs.on_shortcut(shortcut, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            commands::toggle_quick_window(app);
        }
    })
    .map_err(|e| format!("注册快捷键失败（可能被其他程序占用）: {e}"))
}

fn build_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show_i = MenuItem::with_id(app, "show", "显示项目库", true, None::<&str>)?;
    let quick_i = MenuItem::with_id(app, "quick", "快速搜索", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_i, &quick_i, &quit_i])?;

    let mut tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => commands::show_main_window(app),
            "quick" => commands::toggle_quick_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                commands::show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = settings::data_dir();
            std::fs::create_dir_all(&data_dir).ok();
            let database = db::Db::open(&settings::db_path(&data_dir))
                .map_err(|e| format!("打开数据库失败: {e}"))?;
            app.manage(commands::AppState {
                db: Mutex::new(database),
                data_dir,
                scanning: AtomicBool::new(false),
                analyzing: AtomicBool::new(false),
            });

            let cfg = settings::load(app.state::<commands::AppState>().data_dir.as_path());
            if let Err(e) = register_shortcut(app.handle(), &cfg.shortcut) {
                eprintln!("[project-finder] {e}");
            }

            build_tray(app)?;

            // 启动后若 AI 开关已开启，自动把上次没跑完的 pending 补跑
            if cfg.ai.enabled && settings::ai_verified(&cfg.ai) && settings::ai_configured(&cfg.ai) {
                let pending = app
                    .state::<commands::AppState>()
                    .db
                    .lock()
                    .ok()
                    .and_then(|db| db.pending_ids(1).ok())
                    .map(|v| !v.is_empty())
                    .unwrap_or(false);
                if pending {
                    commands::spawn_analysis(app.handle(), "pending");
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    // 点关闭 = 缩到托盘，不退出
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::scan_now,
            commands::list_projects,
            commands::project_facets,
            commands::classify_untyped,
            commands::search_projects,
            commands::update_notes,
            commands::update_tags,
            commands::open_project,
            commands::copy_path,
            commands::add_manual_project,
            commands::confirm_candidate,
            commands::delete_project,
            commands::analyze_project,
            commands::analyze_pending,
            commands::test_ai,
            commands::get_status,
            commands::data_info,
            commands::set_autostart,
            commands::hide_quick,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
