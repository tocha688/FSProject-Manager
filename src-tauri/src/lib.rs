mod actions;
mod auth;
mod analyzer;
mod commands;
mod db;
mod detect;
mod editors;
mod portable_update;
mod scan;
mod search;
mod security;
mod settings;
mod signals;

use std::sync::atomic::{AtomicBool, AtomicI64};
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

/// 启动阶段遇到无法恢复的错误时，用系统原生弹窗告知用户。
/// 此时 WebView 可能还没起来，不能依赖前端 UI。
fn show_fatal_error(message: &str) {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: *mut u8, text: *const u16, caption: *const u16, utype: u32) -> i32;
        }
        let to_wide = |s: &str| -> Vec<u16> {
            OsStr::new(s)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };
        let text = to_wide(message);
        let caption = to_wide("项目快搜 — 启动失败");
        // SAFETY: 两个指针都指向以 NUL 结尾的有效 UTF-16 缓冲区
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                0x0000_0010, // MB_ICONERROR
            );
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("[project-finder] 启动失败: {message}");
    }
}

pub fn run() {
    // 记录启动信息：首次启动（尤其便携版刚解压）若被杀软扫描或 WebView2 初始化慢，
    // 用户反馈"闪退"时可据此判断是没起来还是起来后退出。
    eprintln!(
        "[project-finder] 启动 v{}，数据目录: {}",
        env!("CARGO_PKG_VERSION"),
        settings::data_dir().display()
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let data_dir = settings::data_dir();
            if let Err(e) = std::fs::create_dir_all(&data_dir) {
                eprintln!("[project-finder] 创建数据目录失败 {data_dir:?}: {e}");
            }
            // 数据库打不开时不要退出：用内存库兜底让界面能起来，
            // 用户可以在设置里改数据目录，或看到明确的错误提示。
            let database = match db::Db::open(&settings::db_path(&data_dir)) {
                Ok(d) => d,
                Err(e) => {
                    let msg = format!(
                        "无法打开数据库：{e}\n\n数据目录：{}\n\n\
                         本次将以临时模式运行（数据不会保存）。\
                         请检查该目录是否可写，或设置环境变量 PF_DATA_DIR 指向可写目录。",
                        data_dir.display()
                    );
                    eprintln!("[project-finder] {msg}");
                    show_fatal_error(&msg);
                    // 内存库兜底：界面可用，只是重启后数据丢失
                    db::Db::open_in_memory().unwrap_or_else(|e2| {
                        panic!("连内存数据库都无法创建: {e2}")
                    })
                }
            };
            let startup_cfg = settings::load(&data_dir);
            let using_password = startup_cfg.security.locked_enabled();
            app.manage(commands::AppState {
                db: Mutex::new(database),
                data_dir,
                scanning: AtomicBool::new(false),
                analyzing: AtomicBool::new(false),
                // 设了密码则启动即锁定，必须解锁后才能看项目
                unlocked: AtomicBool::new(!using_password),
                last_activity: AtomicI64::new(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                ),
            });

            let cfg = settings::load(app.state::<commands::AppState>().data_dir.as_path());
            if let Err(e) = register_shortcut(app.handle(), &cfg.shortcut) {
                eprintln!("[project-finder] 快捷键注册失败（不影响主功能）: {e}");
            }

            // 托盘创建失败绝不能导致应用起不来
            if let Err(e) = build_tray(app) {
                eprintln!("[project-finder] 托盘图标创建失败（不影响主窗口）: {e}");
            }

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
                    // 点关闭 = 缩到托盘，不退出。
                    // 但如果窗口从未成功显示过（首次启动 WebView 初始化失败），
                    // 隐藏会导致"进程还在但看不到窗口"，用户感知为闪退。
                    // 此时直接允许关闭，让用户能重新打开。
                    let shown = window
                        .is_visible()
                        .ok()
                        .filter(|v| *v)
                        .is_some();
                    if shown {
                        api.prevent_close();
                        let _ = window.hide();
                    } else {
                        eprintln!(
                            "[project-finder] 主窗口未显示即收到关闭请求，允许退出以便重试"
                        );
                    }
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
            commands::app_version,
            commands::portable_info,
            commands::download_portable_update,
            commands::apply_portable_update,
            commands::set_autostart,
            commands::hide_quick,
            commands::get_lock_state,
            commands::check_auto_lock,
            commands::touch_activity,
            commands::lock_now,
            commands::unlock,
            commands::set_password,
            commands::purge_project,
            commands::list_editors,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
