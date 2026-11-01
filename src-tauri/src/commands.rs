use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use crate::analyzer::Provider;
use crate::{actions, analyzer, db, detect, scan, search, settings, signals};
use tauri_plugin_autostart::ManagerExt as _;

pub struct AppState {
    pub db: Mutex<db::Db>,
    pub data_dir: PathBuf,
    pub scanning: AtomicBool,
    pub analyzing: AtomicBool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub found: usize,
    pub pruned: usize,
    pub total: usize,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub scanning: bool,
    pub analyzing: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataInfo {
    pub db_path: String,
    pub config_path: String,
}

// ---------- 窗口控制 ----------

pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn toggle_quick_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("quick") {
        let vis = w.is_visible().unwrap_or(false);
        eprintln!("[toggle] hotkey fired, is_visible={vis}");
        if vis {
            let _ = w.hide();
        } else {
            let _ = w.center();
            let _ = w.show();
            let _ = w.set_focus();
        }
    } else {
        eprintln!("[toggle] quick window not found");
    }
}

#[tauri::command]
pub fn hide_quick(app: AppHandle) {
    if let Some(w) = app.get_webview_window("quick") {
        let _ = w.hide();
    }
}

// ---------- 设置 ----------

#[tauri::command]
pub fn get_settings(app: AppHandle) -> settings::SettingsView {
    let st = app.state::<AppState>();
    settings::SettingsView::of(settings::load(&st.data_dir))
}

#[tauri::command]
pub fn save_settings(app: AppHandle, s: settings::Settings) -> Result<(), String> {
    let mut s = s;
    settings::normalize(&mut s);
    {
        let st = app.state::<AppState>();
        settings::save(&st.data_dir, &s)?;
    }
    crate::register_shortcut(&app, &s.shortcut)?;
    let al = app.autolaunch();
    if s.autostart {
        al.enable().map_err(|e| e.to_string())?;
    } else {
        al.disable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn data_info(app: AppHandle) -> DataInfo {
    let st = app.state::<AppState>();
    DataInfo {
        db_path: settings::db_path(&st.data_dir).to_string_lossy().into_owned(),
        config_path: settings::config_path(&st.data_dir)
            .to_string_lossy()
            .into_owned(),
    }
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enable: bool) -> Result<(), String> {
    let al = app.autolaunch();
    if enable {
        al.enable().map_err(|e| e.to_string())
    } else {
        al.disable().map_err(|e| e.to_string())
    }
}

// ---------- 扫描与分析 ----------

#[tauri::command]
pub async fn scan_now(app: AppHandle) -> Result<ScanResult, String> {
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<ScanResult, String> {
        let st = handle.state::<AppState>();
        if st.scanning.swap(true, Ordering::SeqCst) {
            return Err("扫描正在进行中".into());
        }
        let out = do_scan(&st);
        st.scanning.store(false, Ordering::SeqCst);
        out
    })
    .await
    .map_err(|e| e.to_string())?;
    spawn_analysis(&app, "pending");
    result
}

fn do_scan(st: &AppState) -> Result<ScanResult, String> {
    let cfg = settings::load(&st.data_dir);
    let mut found = 0usize;
    {
        let db = st.db.lock().map_err(|_| "数据库被占用")?;
        for root in &cfg.roots {
            let root_path = PathBuf::from(&root.path);
            if !root_path.is_dir() {
                continue;
            }
            for fp in scan::scan_root(&root_path, root.depth) {
                let path_str = fp.path.to_string_lossy().to_string();
                let name = fp
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path_str.clone());
                let lang = fp
                    .detection
                    .language
                    .clone()
                    .unwrap_or_else(|| detect::UNKNOWN_LANG.to_string());
                let sig = signals::extract(&fp.path, &lang);
                db.upsert(&db::NewProject {
                    path: path_str,
                    name,
                    language: lang,
                    status: fp.detection.status().to_string(),
                    deps: sig.deps,
                    commits: sig.commits,
                    readme: sig.readme,
                    git_remote: sig.git_remote,
                    structure: sig.structure,
                    last_commit_at: sig.last_commit_ts,
                })?;
                found += 1;
            }
        }
    }
    let (pruned, total) = {
        let db = st.db.lock().map_err(|_| "数据库被占用")?;
        let pruned = db.prune_missing()?;
        let total = db.list()?.len();
        (pruned, total)
    };
    Ok(ScanResult { found, pruned, total })
}

/// 后台逐个分析 pending 项目；AI 未配置时静默跳过（项目靠 FTS 兜底可用）。
/// 后台并发分析项目。
/// mode: "pending" 只分析未分析的；"all" 全部重新分析（覆盖已有结论）。
pub fn spawn_analysis(app: &AppHandle, mode: &str) {
    let handle = app.clone();
    let mode = mode.to_string();
    std::thread::spawn(move || {
        let st = handle.state::<AppState>();
        let cfg = settings::load(&st.data_dir);
        if !cfg.ai.enabled {
            eprintln!("[analysis] AI 分析开关已关闭，跳过");
            return;
        }
        if !settings::ai_configured(&cfg.ai) {
            eprintln!("[analysis] AI 未配置，跳过");
            return;
        }
        // compare_exchange 防止两个线程同时开跑
        if st
            .analyzing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            eprintln!("[analysis] 已有分析线程在跑，跳过");
            return;
        }

        let ids: Vec<i64> = {
            match st.db.lock() {
                Ok(db) => {
                    if mode == "all" {
                        // 全部重新分析：先把已完成的清成 pending，再统一取
                        let _ = db.reset_all_for_reanalysis();
                    }
                    match db.pending_ids(10000) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("[analysis] 查询待分析失败: {e}");
                            Vec::new()
                        }
                    }
                }
                Err(_) => Vec::new(),
            }
        };

        if ids.is_empty() {
            eprintln!("[analysis] 没有待分析项目");
            st.analyzing.store(false, Ordering::SeqCst);
            return;
        }
        eprintln!(
            "[analysis] 模式={mode} 共 {} 个项目，并发={}",
            ids.len(),
            cfg.ai.concurrency
        );

        let concurrency = cfg.ai.concurrency.clamp(1, settings::MAX_CONCURRENCY) as usize;
        let queue = std::sync::Arc::new(std::sync::Mutex::new(ids));
        let mut workers = Vec::new();
        for _ in 0..concurrency {
            let queue = queue.clone();
            let handle = handle.clone();
            let cfg = cfg.clone();
            workers.push(std::thread::spawn(move || loop {
                let id = match queue.lock() {
                    Ok(mut q) => match q.pop() {
                        Some(id) => id,
                        None => break,
                    },
                    Err(_) => break,
                };
                match analyze_one(&handle, id, &cfg) {
                    Ok(()) => eprintln!("[analysis] #{id} 完成"),
                    Err(e) => eprintln!("[analysis] #{id} 失败: {e}"),
                }
            }));
        }
        for w in workers {
            let _ = w.join();
        }
        st.analyzing.store(false, Ordering::SeqCst);
        eprintln!("[analysis] 全部结束");
    });
}

fn analyze_one(app: &AppHandle, id: i64, cfg: &settings::Settings) -> Result<(), String> {
    let st = app.state::<AppState>();
    let row = {
        let db = st.db.lock().map_err(|_| "数据库被占用")?;
        db.get(id)?.ok_or_else(|| "项目不存在".to_string())?
    };
    if row.status != "pending" {
        return Ok(());
    }
    st.db
        .lock()
        .map_err(|_| "数据库被占用")?
        .set_status(id, "analyzing")?;

    let provider = analyzer::from_config(&cfg.ai);
    let sig = signals::Signals {
        deps: row.deps.clone(),
        readme: row.readme.clone(),
        git_remote: row.git_remote.clone(),
        commits: row.commits.clone(),
        last_commit_ts: row.last_commit_at,
        structure: row.structure.clone(),
    };
    match provider.analyze(&row.name, &row.language, &sig) {
        Ok(a) => {
            st.db
                .lock()
                .map_err(|_| "数据库被占用")?
                .set_analysis(id, &a.description, &a.tags, provider.name())
        }
        Err(e) => {
            let _ = st
                .db
                .lock()
                .map_err(|_| "数据库被占用")?
                .set_status(id, "failed");
            Err(e)
        }
    }
}

#[tauri::command]
pub fn analyze_pending(app: AppHandle, mode: Option<String>) -> Result<(), String> {
    let cfg = settings::load(&app.state::<AppState>().data_dir);
    if !cfg.ai.enabled {
        return Err("AI 分析已关闭，请到设置里开启".into());
    }
    spawn_analysis(&app, mode.as_deref().unwrap_or("pending"));
    Ok(())
}

#[tauri::command]
pub async fn analyze_project(app: AppHandle, id: i64) -> Result<(), String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let st = handle.state::<AppState>();
        let cfg = settings::load(&st.data_dir);
        if !settings::ai_configured(&cfg.ai) {
            return Err("请先在设置中配置 AI".into());
        }
        analyze_one(&handle, id, &cfg)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn test_ai(app: AppHandle) -> Result<String, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let st = handle.state::<AppState>();
        let mut cfg = settings::load(&st.data_dir);
        if !settings::ai_configured(&cfg.ai) {
            return Err("请先填写 API 地址和 Key".into());
        }
        let provider = analyzer::from_config(&cfg.ai);
        let a = provider.analyze("ping", "未知", &signals::Signals::empty())?;
        // 测试通过：记录本次配置指纹，允许开启开关
        cfg.ai.verified_fingerprint = settings::ai_fingerprint(&cfg.ai);
        settings::save(&st.data_dir, &cfg)?;
        Ok(format!("连接成功，模型返回：{}", a.description))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_status(app: AppHandle) -> AppStatus {
    let st = app.state::<AppState>();
    AppStatus {
        scanning: st.scanning.load(Ordering::SeqCst),
        analyzing: st.analyzing.load(Ordering::SeqCst),
    }
}

// ---------- 项目查询与编辑 ----------

#[tauri::command]
pub fn list_projects(app: AppHandle) -> Result<Vec<db::ProjectRow>, String> {
    let st = app.state::<AppState>();
    let db = st.db.lock().map_err(|_| "数据库被占用")?;
    db.list()
}

#[tauri::command]
pub fn search_projects(app: AppHandle, query: String) -> Result<Vec<db::ProjectRow>, String> {
    let st = app.state::<AppState>();
    let db = st.db.lock().map_err(|_| "数据库被占用")?;
    let rows = db.list()?;
    if query.trim().is_empty() {
        return Ok(rows);
    }
    let docs: Vec<search::SearchDoc> = rows.iter().map(search::SearchDoc::from_row).collect();
    let by_id: std::collections::HashMap<i64, db::ProjectRow> =
        rows.into_iter().map(|r| (r.id, r)).collect();
    Ok(search::rank(&docs, &query)
        .into_iter()
        .filter_map(|(id, _)| by_id.get(&id).cloned())
        .collect())
}

#[tauri::command]
pub fn update_notes(app: AppHandle, id: i64, notes: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    let db = st.db.lock().map_err(|_| "数据库被占用")?;
    db.update_notes(id, Some(&notes))
}

#[tauri::command]
pub fn update_tags(app: AppHandle, id: i64, tags: Vec<String>) -> Result<(), String> {
    let st = app.state::<AppState>();
    let db = st.db.lock().map_err(|_| "数据库被占用")?;
    db.update_tags(id, &tags)
}

#[tauri::command]
pub fn delete_project(app: AppHandle, id: i64) -> Result<(), String> {
    let st = app.state::<AppState>();
    let db = st.db.lock().map_err(|_| "数据库被占用")?;
    db.delete(id)
}

#[tauri::command]
pub fn confirm_candidate(app: AppHandle, id: i64) -> Result<(), String> {
    {
        let st = app.state::<AppState>();
        st.db
            .lock()
            .map_err(|_| "数据库被占用")?
            .set_status(id, "pending")?;
    }
    spawn_analysis(&app, "pending");
    Ok(())
}

/// 手动添加扫描漏掉的项目（如无任何标记文件的老目录）。
#[tauri::command]
pub fn add_manual_project(app: AppHandle, path: String) -> Result<db::ProjectRow, String> {
    let p = PathBuf::from(path.trim());
    if !p.is_dir() {
        return Err("目录不存在".into());
    }
    let entries = scan::dir_entries(&p).ok_or("无法读取目录")?;
    let snaps: Vec<(&str, bool)> = entries.iter().map(|(n, d)| (n.as_str(), *d)).collect();
    let detection = detect::detect(&snaps);
    if !detection.is_project() && !detection.is_candidate() {
        return Err("该目录没有任何项目标记文件（package.json、.git 等），无法识别为项目".into());
    }
    let path_str = p.to_string_lossy().to_string();
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path_str.clone());
    let lang = detection
        .language
        .clone()
        .unwrap_or_else(|| detect::UNKNOWN_LANG.to_string());
    let sig = signals::extract(&p, &lang);

    let st = app.state::<AppState>();
    let row = {
        let db = st.db.lock().map_err(|_| "数据库被占用")?;
        db.upsert(&db::NewProject {
            path: path_str.clone(),
            name,
            language: lang,
            status: detection.status().to_string(),
            deps: sig.deps,
            commits: sig.commits,
            readme: sig.readme,
            git_remote: sig.git_remote,
            structure: sig.structure,
            last_commit_at: sig.last_commit_ts,
        })?;
        let id = db
            .get_id_by_path(&path_str)?
            .ok_or_else(|| "写入失败".to_string())?;
        db.get(id)?.ok_or_else(|| "读取失败".to_string())?
    };
    if row.status == "pending" {
        spawn_analysis(&app, "pending");
    }
    Ok(row)
}

// ---------- 打开动作 ----------

#[tauri::command]
pub fn open_project(app: AppHandle, id: i64, target: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    let row = {
        let db = st.db.lock().map_err(|_| "数据库被占用")?;
        db.get(id)?.ok_or_else(|| "项目不存在".to_string())?
    };
    match target.as_str() {
        "editor" => {
            let cfg = settings::load(&st.data_dir);
            let program = cfg.editor_cmd.split_whitespace().next().unwrap_or("code");
            if !actions::command_available(program) {
                return Err(format!(
                    "编辑器命令 \"{program}\" 不可用：请在设置里改成完整路径（如 ...\\code.cmd）"
                ));
            }
            actions::editor_command(&row.path, &cfg.editor_cmd)
                .spawn()
                .map_err(|e| format!("启动编辑器失败（检查设置中的编辑器命令）: {e}"))?;
        }
        "folder" => {
            actions::folder_command(&row.path)
                .spawn()
                .map_err(|e| format!("打开资源管理器失败: {e}"))?;
        }
        "terminal" => {
            let kind = actions::detect_terminal(actions::wt_available());
            actions::terminal_command_for(kind, &row.path)
                .spawn()
                .map_err(|e| format!("打开终端失败: {e}"))?;
        }
        _ => return Err("未知动作".into()),
    }
    Ok(())
}

#[tauri::command]
pub fn copy_path(app: AppHandle, id: i64) -> Result<(), String> {
    let st = app.state::<AppState>();
    let row = {
        let db = st.db.lock().map_err(|_| "数据库被占用")?;
        db.get(id)?.ok_or_else(|| "项目不存在".to_string())?
    };
    let mut child = actions::clipboard_command()
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("复制失败: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("无法写入剪贴板")?;
    stdin
        .write_all(row.path.as_bytes())
        .map_err(|e| format!("复制失败: {e}"))?;
    drop(stdin);
    child.wait().map_err(|e| format!("复制失败: {e}"))?;
    Ok(())
}
