use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use crate::analyzer::Provider;
use crate::{
    actions, analyzer, auth, db, detect, editors, portable_update, scan, search, security,
    settings, signals,
};
use tauri_plugin_autostart::ManagerExt as _;

pub struct AppState {
    pub db: Mutex<db::Db>,
    pub data_dir: PathBuf,
    pub scanning: AtomicBool,
    pub analyzing: AtomicBool,
    /// 已解锁？应用启动即为未解锁（若设了密码）。
    pub unlocked: AtomicBool,
    /// 最近一次用户活动时间（unix 秒）。用于空闲自动上锁。
    pub last_activity: AtomicI64,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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

// ---------- 锁屏 ----------

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockState {
    /// 是否设置了密码（决定是否需要锁）
    pub has_password: bool,
    /// 当前是否处于锁定状态
    pub locked: bool,
    /// 空闲多少分钟后自动上锁（0 = 不自动锁）
    pub idle_lock_minutes: u32,
    /// 已空闲秒数，供前端显示
    pub idle_seconds: i64,
}

fn lock_state_of(app: &AppHandle) -> LockState {
    let st = app.state::<AppState>();
    let cfg = settings::load(&st.data_dir);
    let has_password = cfg.security.locked_enabled();
    let idle = now_secs() - st.last_activity.load(Ordering::SeqCst);
    LockState {
        has_password,
        locked: has_password && !st.unlocked.load(Ordering::SeqCst),
        idle_lock_minutes: cfg.security.idle_lock_minutes,
        idle_seconds: idle.max(0),
    }
}

#[tauri::command]
pub fn get_lock_state(app: AppHandle) -> LockState {
    lock_state_of(&app)
}

/// 记录一次用户活动（前端在键鼠输入时调用）。
#[tauri::command]
pub fn touch_activity(app: AppHandle) {
    let st = app.state::<AppState>();
    st.last_activity.store(now_secs(), Ordering::SeqCst);
}

/// 立即上锁（托盘菜单/快捷键可用）。
#[tauri::command]
pub fn lock_now(app: AppHandle) {
    let st = app.state::<AppState>();
    let cfg = settings::load(&st.data_dir);
    if cfg.security.locked_enabled() {
        st.unlocked.store(false, Ordering::SeqCst);
    }
}

/// 周期性检查是否该因空闲/后台而上锁；前端也会主动查询状态。
#[tauri::command]
pub fn check_auto_lock(app: AppHandle) -> LockState {
    let st = app.state::<AppState>();
    let cfg = settings::load(&st.data_dir);
    let has_password = cfg.security.locked_enabled();
    let idle = now_secs() - st.last_activity.load(Ordering::SeqCst);

    if security::should_lock_on_idle(has_password, cfg.security.idle_lock_minutes, idle) {
        st.unlocked.store(false, Ordering::SeqCst);
    }
    // 窗口不可见超过阈值也上锁（"置于后台"）
    let backgrounded = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .map(|v| !v)
        .unwrap_or(false);
    if backgrounded
        && security::should_lock_on_background(
            has_password,
            cfg.security.idle_lock_minutes,
            idle,
        )
    {
        st.unlocked.store(false, Ordering::SeqCst);
    }
    lock_state_of(&app)
}

/// 用密码解锁。
#[tauri::command]
pub fn unlock(app: AppHandle, password: String) -> Result<bool, String> {
    let st = app.state::<AppState>();
    let cfg = settings::load(&st.data_dir);
    if !cfg.security.locked_enabled() {
        st.unlocked.store(true, Ordering::SeqCst);
        return Ok(true);
    }
    if auth::verify_password(&password, &cfg.security.password_hash) {
        st.unlocked.store(true, Ordering::SeqCst);
        st.last_activity.store(now_secs(), Ordering::SeqCst);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 设置/修改密码。`old_password` 在已设过密码时必填。
#[tauri::command]
pub fn set_password(
    app: AppHandle,
    old_password: Option<String>,
    new_password: Option<String>,
    idle_lock_minutes: Option<u32>,
) -> Result<(), String> {
    let st = app.state::<AppState>();
    let mut cfg = settings::load(&st.data_dir);
    let has_existing = cfg.security.locked_enabled();

    if has_existing {
        let old = old_password.unwrap_or_default();
        if !auth::verify_password(&old, &cfg.security.password_hash) {
            return Err("原密码不正确".into());
        }
    }

    match new_password {
        // 传空串 = 清除密码（关闭锁屏）
        Some(p) if p.trim().is_empty() => {
            if !has_existing {
                return Err("当前没有设置密码".into());
            }
            cfg.security.password_hash.clear();
            st.unlocked.store(true, Ordering::SeqCst);
        }
        Some(p) => {
            if p.chars().count() < 4 {
                return Err("密码至少 4 个字符".into());
            }
            cfg.security.password_hash = auth::hash_password(&p);
            st.unlocked.store(true, Ordering::SeqCst);
        }
        None => {}
    }

    if let Some(m) = idle_lock_minutes {
        cfg.security.idle_lock_minutes = m.min(120);
    }
    settings::save(&st.data_dir, &cfg)?;
    st.last_activity.store(now_secs(), Ordering::SeqCst);
    Ok(())
}

// ---------- 设置 ----------

#[tauri::command]
pub fn get_settings(app: AppHandle) -> settings::SettingsView {
    let st = app.state::<AppState>();
    settings::SettingsView::sanitized(settings::load(&st.data_dir))
}

#[tauri::command]
pub fn save_settings(app: AppHandle, s: settings::Settings) -> Result<(), String> {
    let mut s = s;
    {
        let st = app.state::<AppState>();
        let existing = settings::load(&st.data_dir);
        // 关键：前端提交的指纹可能是空的（例如只在界面上改了个数字），
        // 但只要配置内容本身没变，就沿用磁盘上已验证的指纹，
        // 否则 enabled 会被误判为"未验证"而强制关闭。
        settings::normalize_preserving_verification(&mut s, &existing);
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

// ---------- 便携版更新 ----------

/// 当前是否便携版运行，以及所在目录。
#[tauri::command]
pub fn portable_info() -> portable_update::PortableInfo {
    let exe = std::env::current_exe().unwrap_or_default();
    portable_update::detect(&exe)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableUpdateResult {
    pub version: String,
    /// 已解压的新版本目录，前端提示用户后调用 apply_portable_update 完成替换
    pub staging_dir: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub version: String,
    pub notes: String,
    /// 当前平台的便携包下载地址（无则为空）
    pub portable_url: String,
}

/// 拉取 latest.json 更新清单。
///
/// 必须由后端请求：WebView 里前端 fetch 外部 HTTPS 会被跨域/CSP 拦住
/// （表现为 "Failed to fetch"，用户看到的就是"检查更新失败"）。
#[tauri::command]
pub async fn fetch_update_manifest(url: String) -> Result<UpdateManifest, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<UpdateManifest, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!("ProjectFinder/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .get(&url)
            .send()
            .map_err(|e| format!("请求更新清单失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("获取更新清单失败: HTTP {}", resp.status()));
        }
        let body: serde_json::Value = resp.json().map_err(|e| format!("清单解析失败: {e}"))?;

        let version = body
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or("更新清单缺少 version 字段")?
            .to_string();
        let notes = body
            .get("notes")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 平台键按编译目标推断，与 latest.json 的键对齐
        let key = portable_update::platform_key();
        let portable_url = body
            .get("portable")
            .and_then(|p| p.get(&key))
            .and_then(|e| e.get("url"))
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();

        Ok(UpdateManifest {
            version,
            notes,
            portable_url,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 下载并解压便携版更新包（不立即替换，等用户确认后重启）。
#[tauri::command]
pub async fn download_portable_update(url: String, version: String) -> Result<PortableUpdateResult, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<PortableUpdateResult, String> {
        let exe = std::env::current_exe().map_err(|e| format!("取可执行文件路径失败: {e}"))?;
        let info = portable_update::detect(&exe);
        if !info.is_portable {
            return Err("当前不是便携版运行，请使用内置更新".into());
        }
        let exe_name = exe
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or("无法获取可执行文件名")?;

        // 下载 zip 到临时目录
        let staging = std::env::temp_dir().join(format!("pf-update-{version}"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        let zip_path = staging.join("update.zip");

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| e.to_string())?;
        let mut resp = client.get(&url).send().map_err(|e| format!("下载失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("下载失败: HTTP {}", resp.status()));
        }
        let mut file = std::fs::File::create(&zip_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut resp, &mut file).map_err(|e| format!("写入失败: {e}"))?;
        drop(file);

        // 解压
        let extract_dir = staging.join("extracted");
        std::fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;
        extract_zip(&zip_path, &extract_dir)?;

        // 校验：解压结果里必须有主程序，否则拒绝继续（避免把用户目录搞坏）
        if portable_update::find_executable_in_dir(&extract_dir, &exe_name).is_none() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err("更新包内容异常：未找到主程序，已中止".into());
        }

        let _ = std::fs::remove_file(&zip_path);
        Ok(PortableUpdateResult {
            version,
            staging_dir: extract_dir.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 用已解压的新版本替换当前程序，然后退出让脚本接管。
#[tauri::command]
pub fn apply_portable_update(app: AppHandle, staging_dir: String) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("取路径失败: {e}"))?;
    let info = portable_update::detect(&exe);
    if !info.is_portable {
        return Err("当前不是便携版".into());
    }
    let exe_name = exe
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or("无法获取可执行文件名")?;

    let staging = PathBuf::from(&staging_dir);
    if !staging.is_dir() {
        return Err("更新目录不存在，请重新下载".into());
    }

    // 写脚本到临时目录，然后启动它，再退出本进程
    let script_path = std::env::temp_dir().join(if cfg!(windows) {
        "pf-apply-update.bat"
    } else {
        "pf-apply-update.sh"
    });

    let content = if cfg!(windows) {
        portable_update::windows_update_script(&info.install_dir, &staging_dir, &exe_name)
    } else {
        portable_update::unix_update_script(&info.install_dir, &staging_dir, &exe_name)
    };
    std::fs::write(&script_path, content).map_err(|e| format!("写更新脚本失败: {e}"))?;

    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "/min", "", &script_path.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("启动更新脚本失败: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).map_err(|e| e.to_string())?;
        std::process::Command::new("sh")
            .arg(&script_path)
            .spawn()
            .map_err(|e| format!("启动更新脚本失败: {e}"))?;
    }

    // 退出，让脚本替换文件
    app.exit(0);
    Ok(())
}

/// 极简 zip 解压：只支持 stored / deflate，够用于我们自己的更新包。
fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    use std::io::Read;
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip 解析失败: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // 防路径穿越：只允许相对路径，且不能含 ..
        let name = entry.name().to_string();
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            continue;
        }
        let out_path = dest.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = out_path.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut out = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 当前应用版本号（来自 tauri.conf 的 version）。
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
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
        // compare_exchange 而非 swap：抢不到锁时不得改动标志，
        // 否则并发触发会让 scanning 永久卡在 true，扫描按钮再也点不动。
        if st
            .scanning
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("扫描正在进行中".into());
        }
        // 用 guard 保证任何退出路径（含 panic/提前 return）都会复位标志
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _reset = Reset(&st.scanning);
        do_scan(&st)
    })
    .await
    .map_err(|e| e.to_string())?;
    spawn_analysis(&app, "pending");
    result
}

/// 纯采集：遍历配置的根目录，读取项目信号，不触碰数据库。
/// 把文件遍历和 git 子进程调用全部放在锁外，避免长时间占用数据库锁。
fn collect_projects(cfg: &settings::Settings) -> Vec<db::NewProject> {
    let mut out = Vec::new();
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
            out.push(db::NewProject {
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
            });
        }
    }
    out
}

fn do_scan(st: &AppState) -> Result<ScanResult, String> {
    let cfg = settings::load(&st.data_dir);
    // 第一步：锁外采集（耗时的主要来源：目录遍历 + git 子进程）
    let found_projects = collect_projects(&cfg);
    let found = found_projects.len();

    // 第二步：逐个短锁写入，每写一条就释放，绝不跨项目持锁
    for p in &found_projects {
        let db = st.db.lock().map_err(|_| "数据库被占用")?;
        db.upsert(p)?;
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
                .set_analysis_typed(id, &a.description, &a.tags, &a.project_type, provider.name())
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAiResult {
    pub ok: bool,
    /// 展示给用户的文案（成功含模型回复，失败含原因）
    pub message: String,
    /// 请求耗时（毫秒），让用户知道确实是真连了一次
    pub elapsed_ms: u64,
    /// 测试通过后的配置指纹（前端可据此立即解锁开关，无需等保存）
    pub verified_fingerprint: String,
}

#[tauri::command]
pub async fn test_ai(app: AppHandle) -> Result<TestAiResult, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<TestAiResult, String> {
        let st = handle.state::<AppState>();
        let mut cfg = settings::load(&st.data_dir);
        if !settings::ai_configured(&cfg.ai) {
            return Err("请先填写 API 地址和 Key".into());
        }
        let provider = analyzer::from_config(&cfg.ai);
        let started = std::time::Instant::now();
        let a = provider.analyze("ping", "未知", &signals::Signals::empty())?;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        // 测试通过：记录本次配置指纹，允许开启开关
        cfg.ai.verified_fingerprint = settings::ai_fingerprint(&cfg.ai);
        settings::save(&st.data_dir, &cfg)?;
        Ok(TestAiResult {
            ok: true,
            message: format!(
                "连接成功（{}ms），模型返回：{}",
                elapsed_ms,
                a.description
            ),
            elapsed_ms,
            verified_fingerprint: cfg.ai.verified_fingerprint,
        })
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
//
// 注意：所有会碰数据库的命令都必须是 async。
// Tauri 2 中同步命令在主线程执行，一旦在扫描/分析持锁期间等待 Mutex，
// 主线程会被阻塞，整个窗口无响应。async 命令走异步运行时线程池，
// 即使短暂等待锁也只阻塞该任务，UI 保持可用。

/// 在阻塞线程池中执行「取 AppState 并锁库」的短任务。
/// 注意：AppHandle 是 Send + 'static，可安全移入 blocking 闭包。
async fn with_db<T, F>(app: AppHandle, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&db::Db) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let db = st.db.lock().map_err(|_| "数据库被占用")?;
        f(&db)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_projects(app: AppHandle) -> Result<Vec<db::ProjectRow>, String> {
    with_db(app, |db| db.list()).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facets {
    pub languages: Vec<FacetCount>,
    pub types: Vec<FacetCount>,
    pub untyped: usize,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetCount {
    pub name: String,
    pub count: usize,
}

#[tauri::command]
pub async fn project_facets(app: AppHandle) -> Result<Facets, String> {
    with_db(app, |db| {
        let (languages, types) = db.facets()?;
        let rows = db.list()?;
        let untyped = rows
            .iter()
            .filter(|r| r.status == "ok" && r.project_type.is_empty())
            .count();
        Ok(Facets {
            languages: languages
                .into_iter()
                .map(|(name, count)| FacetCount { name, count })
                .collect(),
            types: types
                .into_iter()
                .map(|(name, count)| FacetCount { name, count })
                .collect(),
            untyped,
        })
    })
    .await
}

/// 给"已有说明但没有类型"的老项目补分类（不重新生成描述与标签）。
#[tauri::command]
pub fn classify_untyped(app: AppHandle) -> Result<(), String> {
    let handle = app.clone();
    std::thread::spawn(move || {
        let st = handle.state::<AppState>();
        let cfg = settings::load(&st.data_dir);
        if !cfg.ai.enabled || !settings::ai_configured(&cfg.ai) {
            eprintln!("[classify] AI 未启用或未配置，跳过");
            return;
        }
        let pending = match st.db.lock() {
            Ok(db) => db.untyped_analyzed(10000).unwrap_or_default(),
            Err(_) => return,
        };
        eprintln!("[classify] 待补分类 {} 个", pending.len());
        for (id, description, tags) in pending {
            let local = infer_type_from_text(&description, &tags);
            if !local.is_empty() {
                if let Ok(db) = st.db.lock() {
                    let _ = db.set_project_type(id, &local);
                }
                continue;
            }
            // 文本启发式判断不出时，再问一次 AI（只用已有摘要，省 token）
            let sig = signals::Signals {
                deps: vec![],
                readme: Some(description.clone()),
                git_remote: None,
                commits: vec![],
                last_commit_ts: None,
                structure: vec![],
            };
            let provider = analyzer::from_config(&cfg.ai);
            match provider.analyze("", "", &sig) {
                Ok(a) if !a.project_type.is_empty() => {
                    if let Ok(db) = st.db.lock() {
                        let _ = db.set_project_type(id, &a.project_type);
                    }
                }
                _ => {}
            }
        }
        eprintln!("[classify] 完成");
    });
    Ok(())
}

/// 仅凭已有描述/标签做本地启发式归类，命中则无需再调 AI。
fn infer_type_from_text(description: &str, tags: &[String]) -> String {
    let mut text = description.to_string();
    for t in tags {
        text.push(' ');
        text.push_str(t);
    }
    analyzer::normalize_type(&text)
}

#[tauri::command]
pub async fn search_projects(
    app: AppHandle,
    query: String,
) -> Result<Vec<db::ProjectRow>, String> {
    with_db(app, move |db| {
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
    })
    .await
}

#[tauri::command]
pub async fn update_notes(app: AppHandle, id: i64, notes: String) -> Result<(), String> {
    with_db(app, move |db| db.update_notes(id, Some(&notes))).await
}

#[tauri::command]
pub async fn update_tags(app: AppHandle, id: i64, tags: Vec<String>) -> Result<(), String> {
    with_db(app, move |db| db.update_tags(id, &tags)).await
}

/// 仅从数据库移除记录，不动磁盘文件。
#[tauri::command]
pub async fn delete_project(app: AppHandle, id: i64) -> Result<(), String> {
    with_db(app, move |db| db.delete(id)).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeResult {
    /// 数据库记录是否已删除
    pub removed_from_db: bool,
    /// 文件夹是否真的被删掉了（不存在也算 true）
    pub removed_folder: bool,
}

/// 彻底删除：项目文件夹（快速删除）+ 数据库记录。
///
/// 安全措施：
/// 1. 路径必须通过 security::is_safe_delete_target（拒绝盘符根、用户主目录及其祖先、相对路径）
/// 2. 必须显式传 confirm=true，前端有二次确认弹窗
/// 3. 若传了 expected_path，必须与库中记录的路径完全一致（防止 id 错位删错项目）
/// 4. 只删该项目的根文件夹，不递归到父级
#[tauri::command]
pub async fn purge_project(
    app: AppHandle,
    id: i64,
    confirm: bool,
    expected_path: Option<String>,
) -> Result<PurgeResult, String> {
    if !confirm {
        return Err("未确认删除".into());
    }
    tauri::async_runtime::spawn_blocking(move || -> Result<PurgeResult, String> {
        let st = app.state::<AppState>();
        // 取出路径后立即释放锁，删除文件系统操作绝不持锁
        let path = {
            let db = st.db.lock().map_err(|_| "数据库被占用")?;
            let row = db.get(id)?.ok_or_else(|| "项目不存在".to_string())?;
            if let Some(expected) = expected_path {
                if !expected.is_empty() && expected != row.path {
                    return Err(format!(
                        "路径不匹配，已中止（库中为 {}，请求为 {}）",
                        row.path, expected
                    ));
                }
            }
            row.path
        };

        let home = dirs::home_dir();
        if !security::is_safe_delete_target(&path, home.as_deref()) {
            return Err(format!("拒绝删除不安全的路径：{path}"));
        }

        let target = std::path::Path::new(&path);
        let mut removed_folder = true;
        if target.exists() {
            // 快速删除：直接递归删除，不进回收站（Windows 上大目录明显更快）
            std::fs::remove_dir_all(target)
                .map_err(|e| format!("删除文件夹失败：{e}（文件可能被占用）"))?;
            removed_folder = !target.exists();
            if !removed_folder {
                return Err("文件夹删除后仍然存在，可能被占用".into());
            }
        }

        let removed_from_db = {
            let db = st.db.lock().map_err(|_| "数据库被占用")?;
            db.delete(id)?;
            db.get(id)?.is_none()
        };
        eprintln!("[purge] #{id} 已删除：{path}");
        Ok(PurgeResult {
            removed_from_db,
            removed_folder,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn confirm_candidate(app: AppHandle, id: i64) -> Result<(), String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let st = handle.state::<AppState>();
        st.db
            .lock()
            .map_err(|_| "数据库被占用")?
            .set_status(id, "pending")?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    spawn_analysis(&app, "pending");
    Ok(())
}

/// 手动添加扫描漏掉的项目（如无任何标记文件的老目录）。
#[tauri::command]
pub async fn add_manual_project(
    app: AppHandle,
    path: String,
) -> Result<db::ProjectRow, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<db::ProjectRow, String> {
        let p = PathBuf::from(path.trim());
        if !p.is_dir() {
            return Err("目录不存在".into());
        }
        let entries = scan::dir_entries(&p).ok_or("无法读取目录")?;
        let snaps: Vec<(&str, bool)> = entries.iter().map(|(n, d)| (n.as_str(), *d)).collect();
        let detection = detect::detect(&snaps);
        if !detection.is_project() && !detection.is_candidate() {
            return Err(
                "该目录没有任何项目标记文件（package.json、.git 等），无法识别为项目".into(),
            );
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
        // 信号采集（git 子进程）放在锁外
        let sig = signals::extract(&p, &lang);

        let st = handle.state::<AppState>();
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
            spawn_analysis(&handle, "pending");
        }
        Ok(row)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------- 打开动作 ----------

/// 列出本机实际可用的编辑器 / AI 智能体（供右键子菜单动态渲染）。
///
/// 不做缓存：用户可能在应用运行期间安装或卸载编辑器，
/// 每次展开菜单都重新检测，保证「移除了就真的消失」。
#[tauri::command]
pub async fn list_editors() -> Result<Vec<editors::EditorInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut available = Vec::new();
        for def in editors::EDITORS {
            if let Some(command) = editors::resolve_command(def, actions::command_available) {
                available.push(editors::EditorInfo {
                    id: def.id.to_string(),
                    label: def.label.to_string(),
                    command,
                });
            }
        }
        Ok(available)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_project(app: AppHandle, id: i64, target: String) -> Result<(), String> {
    // spawn_blocking：查库 + 启动外部进程都不该占用主线程，
    // 否则扫描期间点「打开文件夹」会让整个窗口卡死。
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let st = app.state::<AppState>();
        let (row_path, cfg) = {
            let db = st.db.lock().map_err(|_| "数据库被占用")?;
            let row = db.get(id)?.ok_or_else(|| "项目不存在".to_string())?;
            (row.path, settings::load(&st.data_dir))
        };
        open_path(&row_path, &target, &cfg)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn open_terminal(path: &str) -> Result<(), String> {
    let kind = actions::detect_terminal(actions::wt_available());

    // Linux 上终端模拟器不统一：按候选列表逐个尝试，用第一个能启动的
    if cfg!(all(unix, not(target_os = "macos"))) {
        for program in actions::linux_terminal_candidates() {
            if !actions::command_available(program) {
                continue;
            }
            let spec = actions::linux_terminal_spec(program, path);
            let mut cmd = std::process::Command::new(&spec.program);
            cmd.args(&spec.args);
            if cmd.spawn().is_ok() {
                return Ok(());
            }
        }
        return Err("未找到可用终端（尝试过 gnome-terminal、konsole、xterm 等）".into());
    }

    actions::terminal_command_for(kind, path)
        .spawn()
        .map_err(|e| format!("打开终端失败: {e}"))?;
    Ok(())
}

fn open_path(path: &str, target: &str, cfg: &settings::Settings) -> Result<(), String> {
    match target {
        // 设置里配置的默认编辑器（保留原有行为）
        "editor" => {
            let program = cfg.editor_cmd.split_whitespace().next().unwrap_or("code");
            if !actions::command_available(program) {
                return Err(format!(
                    "编辑器命令 \"{program}\" 不可用：请在设置里改成完整路径（如 ...\\code.cmd）"
                ));
            }
            actions::editor_command(path, &cfg.editor_cmd)
                .spawn()
                .map_err(|e| format!("启动编辑器失败（检查设置中的编辑器命令）: {e}"))?;
        }
        "folder" => {
            actions::folder_command(path)
                .spawn()
                .map_err(|e| format!("打开资源管理器失败: {e}"))?;
        }
        "terminal" => {
            open_terminal(path)?;
        }
        // 其余目标是注册表里的编辑器/智能体（vscode、trae、opencode、zcode、dsh…）
        other => {
            let def = editors::find_editor(other)
                .ok_or_else(|| format!("未知的打开方式: {other}"))?;
            let command = editors::resolve_command(def, actions::command_available)
                .ok_or_else(|| {
                    format!(
                        "未检测到 {}（尝试过命令: {}）",
                        def.label,
                        def.commands.join(", ")
                    )
                })?;
            actions::editor_command(path, &command)
                .spawn()
                .map_err(|e| format!("启动 {} 失败: {e}", def.label))?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn copy_path(app: AppHandle, id: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let st = app.state::<AppState>();
        let path = {
            let db = st.db.lock().map_err(|_| "数据库被占用")?;
            db.get(id)?.ok_or_else(|| "项目不存在".to_string())?.path
        };
        write_clipboard(&path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 把文本写入系统剪贴板。
/// Linux 上 wl-copy / xclip / xsel 不一定都装，按顺序找第一个可用的。
fn write_clipboard(text: &str) -> Result<(), String> {
    if cfg!(all(unix, not(target_os = "macos"))) {
        for program in actions::linux_clipboard_candidates() {
            if !actions::command_available(program) {
                continue;
            }
            let spec = actions::linux_clipboard_spec(program);
            if pipe_to_command(&spec, text).is_ok() {
                return Ok(());
            }
        }
        return Err("未找到剪贴板工具（可安装 wl-clipboard 或 xclip）".into());
    }

    let spec = actions::clipboard_spec();
    pipe_to_command(&spec, text)
}

fn pipe_to_command(spec: &actions::CmdSpec, text: &str) -> Result<(), String> {
    let mut child = std::process::Command::new(&spec.program)
        .args(&spec.args)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("复制失败: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("无法写入剪贴板")?;
    stdin
        .write_all(text.as_bytes())
        .map_err(|e| format!("复制失败: {e}"))?;
    drop(stdin);
    let status = child.wait().map_err(|e| format!("复制失败: {e}"))?;
    if !status.success() {
        return Err(format!("剪贴板工具 {} 退出异常", spec.program));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_projects(root: &std::path::Path, names: &[&str]) -> Vec<settings::RootConfig> {
        for n in names {
            let d = root.join(n);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("package.json"), r#"{"name":"x"}"#).unwrap();
        }
        vec![settings::RootConfig {
            path: root.to_string_lossy().to_string(),
            depth: 5,
        }]
    }

    #[test]
    fn collect_projects_reads_without_touching_db() {
        let tmp = tempfile::tempdir().unwrap();
        let roots = make_projects(tmp.path(), &["alpha", "beta"]);
        let cfg = settings::Settings {
            roots,
            ..Default::default()
        };
        let found = collect_projects(&cfg);
        assert_eq!(found.len(), 2);
        let mut names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
        assert!(found.iter().all(|p| p.language == "Node.js"));
    }

    #[test]
    fn collect_projects_handles_missing_root() {
        let cfg = settings::Settings {
            roots: vec![settings::RootConfig {
                path: "Z:/definitely/not/here".into(),
                depth: 3,
            }],
            ..Default::default()
        };
        assert!(collect_projects(&cfg).is_empty());
    }

    #[test]
    fn collect_projects_respects_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("a/b/c/d/deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("package.json"), "{}").unwrap();
        let cfg = settings::Settings {
            roots: vec![settings::RootConfig {
                path: tmp.path().to_string_lossy().to_string(),
                depth: 2,
            }],
            ..Default::default()
        };
        // 深度 2 够不到第 5 层
        assert!(collect_projects(&cfg).is_empty());

        let cfg_deep = settings::Settings {
            roots: vec![settings::RootConfig {
                path: tmp.path().to_string_lossy().to_string(),
                depth: 5,
            }],
            ..Default::default()
        };
        assert_eq!(collect_projects(&cfg_deep).len(), 1);
    }

    #[test]
    fn open_path_rejects_unknown_target() {
        let cfg = settings::Settings::default();
        assert!(open_path(r"C:\x", "nonsense", &cfg).is_err());
    }

    #[test]
    fn open_path_routes_registered_editors() {
        let cfg = settings::Settings::default();
        // 注册表里的工具若本机不存在，应报"未检测到"而不是"未知的打开方式"
        for id in ["vscode", "trae", "opencode", "zcode", "dsh"] {
            match open_path(r"C:\x", id, &cfg) {
                Ok(()) => { /* 本机确实装了并且启动成功 */ }
                Err(e) => assert!(
                    !e.contains("未知的打开方式"),
                    "{id} 应该被注册表识别，实际报错: {e}"
                ),
            }
        }
    }

    #[test]
    fn open_path_editor_reports_missing_binary() {
        let mut cfg = settings::Settings::default();
        cfg.editor_cmd = "definitely-not-a-real-binary-xyz".into();
        let err = open_path(r"C:\x", "editor", &cfg).unwrap_err();
        assert!(err.contains("不可用"), "{err}");
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// 复现原始 bug 的模型：模拟"扫描持锁时间长"。
    /// 验证点：其它命令在扫描期间必须能在合理时间内拿到锁，
    /// 而不是被扫描独占整段过程。
    #[test]
    fn short_locks_do_not_block_readers_during_long_scan() {
        let db = Arc::new(std::sync::Mutex::new(db::Db::open_in_memory().unwrap()));

        // 模拟重构后的 do_scan：逐条短锁写入，而不是整段持锁
        let writer = {
            let db = Arc::clone(&db);
            std::thread::spawn(move || {
                for i in 0..200 {
                    // 每条各自加锁、释放
                    let guard = db.lock().unwrap();
                    guard
                        .upsert(&db::NewProject {
                            path: format!(r"C:\p{i}"),
                            name: format!("p{i}"),
                            language: "Node.js".into(),
                            status: "pending".into(),
                            deps: vec![],
                            commits: vec![],
                            readme: None,
                            git_remote: None,
                            structure: vec![],
                            last_commit_at: None,
                        })
                        .unwrap();
                    drop(guard); // 关键：立即释放
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };

        // 读取方持续尝试拿锁，记录最长等待
        let max_wait = Arc::new(AtomicUsize::new(0));
        let reader = {
            let db = Arc::clone(&db);
            let max_wait = Arc::clone(&max_wait);
            std::thread::spawn(move || {
                let mut reads = 0;
                let start = Instant::now();
                while start.elapsed() < Duration::from_millis(800) {
                    let t = Instant::now();
                    {
                        let guard = db.lock().unwrap();
                        let _ = guard.list().unwrap_or_default();
                    }
                    let waited = t.elapsed().as_millis() as usize;
                    max_wait.fetch_max(waited, Ordering::SeqCst);
                    reads += 1;
                    std::thread::sleep(Duration::from_millis(2));
                }
                reads
            })
        };

        reader.join().unwrap();
        writer.join().unwrap();

        let worst = max_wait.load(Ordering::SeqCst);
        // 短锁下读者最长等待应远小于扫描总时长（200 条 ≈ 200ms+）
        assert!(
            worst < 150,
            "读取被阻塞过久：{worst}ms，说明存在长事务持锁"
        );
    }

    /// 验证：即使持锁方做了耗时的"采集"工作，只要按正确姿势分成短临界区，
    /// 读者就不会被长时间挡住。这条测试锁定了 do_scan 的重构契约。
    #[test]
    fn collecting_outside_lock_keeps_db_available() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..30 {
            let d = tmp.path().join(format!("proj{i}"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("package.json"), "{}").unwrap();
        }
        let cfg = settings::Settings {
            roots: vec![settings::RootConfig {
                path: tmp.path().to_string_lossy().to_string(),
                depth: 3,
            }],
            ..Default::default()
        };

        let db = Arc::new(std::sync::Mutex::new(db::Db::open_in_memory().unwrap()));
        let db2 = Arc::clone(&db);

        // 模拟 do_scan：先锁外采集，再逐条短锁写入
        let scanner = std::thread::spawn(move || {
            let collected = collect_projects(&cfg); // 锁外
            let mut n = 0;
            for p in &collected {
                let guard = db2.lock().unwrap();
                guard.upsert(p).unwrap();
                drop(guard);
                n += 1;
            }
            n
        });

        // 扫描期间读取方必须能持续拿到锁
        let max_wait = Arc::new(AtomicUsize::new(0));
        let reader = {
            let db = Arc::clone(&db);
            let max_wait = Arc::clone(&max_wait);
            std::thread::spawn(move || {
                let start = Instant::now();
                let mut reads = 0;
                while start.elapsed() < Duration::from_millis(500) {
                    let t = Instant::now();
                    {
                        let guard = db.lock().unwrap();
                        let _ = guard.list().unwrap_or_default();
                    }
                    max_wait.fetch_max(t.elapsed().as_millis() as usize, Ordering::SeqCst);
                    reads += 1;
                    std::thread::sleep(Duration::from_millis(3));
                }
                reads
            })
        };

        let read_count = reader.join().unwrap();
        let written = scanner.join().unwrap();

        assert_eq!(written, 30);
        assert!(read_count > 20, "读取方只成功读了 {read_count} 次，疑似被阻塞");
        let worst = max_wait.load(Ordering::SeqCst);
        assert!(worst < 200, "扫描期间读取最长阻塞 {worst}ms，仍然过久");
    }
}

#[cfg(test)]
mod scan_flag_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// 复现并锁定 bug：并发触发扫描时，抢不到标志的一方
    /// 不能把标志永久留在 true（否则扫描按钮永久失效）。
    #[test]
    fn losing_racer_does_not_corrupt_scanning_flag() {
        let flag = AtomicBool::new(false);

        // 第一个请求抢到
        assert!(flag
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok());

        // 第二个请求必须失败，且不得改变标志状态
        assert!(flag
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err());
        assert!(
            flag.load(Ordering::SeqCst),
            "抢锁失败方不应改动标志"
        );

        // 第一个请求完成后复位
        flag.store(false, Ordering::SeqCst);
        assert!(!flag.load(Ordering::SeqCst));

        // 后续请求仍能正常抢到（按钮可用）
        assert!(flag
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok());
    }

    /// 对比说明：swap 写法在并发下会把标志永久留在 true。
    /// 场景：A 正在扫描，B 也点了扫描 → B 的 swap 把标志写成 true 后
    /// 直接返回 Err（不复位）；随后 A 完成并把标志复位为 false；
    /// 若 B 的 swap 恰好发生在 A 复位之后，B 就会留下永久 true。
    #[test]
    fn swap_style_can_strand_the_flag() {
        let flag = AtomicBool::new(false);

        // A 抢占成功
        assert!(!flag.swap(true, Ordering::SeqCst));

        // B 抢锁失败（swap 返回 true），但 swap 仍把标志保持为 true
        assert!(flag.swap(true, Ordering::SeqCst));

        // A 完成，正常复位
        flag.store(false, Ordering::SeqCst);

        // B 的执行落到 A 复位之后：此时 swap 返回 false（"抢到了"），
        // 但 B 若是被前面 is_err 分支提前 return 的路径，
        // 就会留下一个再也不会被复位的 true。
        assert!(!flag.swap(true, Ordering::SeqCst));
        assert!(flag.load(Ordering::SeqCst), "这就是原 bug：标志卡死在 true");
    }

    /// Reset guard 在 panic 时也能复位
    #[test]
    fn reset_guard_restores_flag_on_panic() {
        let flag = AtomicBool::new(true);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            struct Reset<'a>(&'a AtomicBool);
            impl Drop for Reset<'_> {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::SeqCst);
                }
            }
            let _g = Reset(&flag);
            panic!("模拟扫描中途 panic");
        }));
        assert!(res.is_err());
        assert!(!flag.load(Ordering::SeqCst), "panic 后标志必须复位");
    }
}
