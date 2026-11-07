import { invoke } from "@tauri-apps/api/core";
import type {
  AppStatus,
  DataInfo,

  Project,
  ScanResult,
  Settings,
  SettingsView,
} from "./types";

export const getSettings = () => invoke<SettingsView>("get_settings");
export const saveSettings = (s: Settings) => invoke<void>("save_settings", { s });
export const scanNow = () => invoke<ScanResult>("scan_now");
export const listProjects = () => invoke<Project[]>("list_projects");
export const classifyUntyped = () => invoke<void>("classify_untyped");
export const searchProjects = (query: string) =>
  invoke<Project[]>("search_projects", { query });
export const updateNotes = (id: number, notes: string) =>
  invoke<void>("update_notes", { id, notes });
export const updateTags = (id: number, tags: string[]) =>
  invoke<void>("update_tags", { id, tags });
export const openProject = (id: number, target: string) =>
  invoke<void>("open_project", { id, target });

export interface EditorInfo {
  id: string;
  label: string;
  command: string;
}

/** 本机实际可用的编辑器 / AI 智能体（后端做 PATH 检测）。 */
export const listEditors = () => invoke<EditorInfo[]>("list_editors");
export const copyPath = (id: number) => invoke<void>("copy_path", { id });
export const addManualProject = (path: string) =>
  invoke<Project>("add_manual_project", { path });
export const confirmCandidate = (id: number) =>
  invoke<void>("confirm_candidate", { id });
export const deleteProject = (id: number) => invoke<void>("delete_project", { id });
export const analyzeProject = (id: number) => invoke<void>("analyze_project", { id });
export const analyzePending = (mode: "pending" | "all" = "pending") =>
  invoke<void>("analyze_pending", { mode });

export interface TestAiResult {
  ok: boolean;
  message: string;
  elapsedMs: number;
  verifiedFingerprint: string;
}

export const testAi = () => invoke<TestAiResult>("test_ai");
export const getStatus = () => invoke<AppStatus>("get_status");
export const getDataInfo = () => invoke<DataInfo>("data_info");
export const setAutostart = (enable: boolean) =>
  invoke<void>("set_autostart", { enable });
export const hideQuick = () => invoke<void>("hide_quick");

// ---------- 锁屏 ----------

export interface LockState {
  hasPassword: boolean;
  locked: boolean;
  idleLockMinutes: number;
  idleSeconds: number;
}

export const getLockState = () => invoke<LockState>("get_lock_state");
export const checkAutoLock = () => invoke<LockState>("check_auto_lock");
export const touchActivity = () => invoke<void>("touch_activity");
export const lockNow = () => invoke<void>("lock_now");
export const unlock = (password: string) => invoke<boolean>("unlock", { password });
export const setPassword = (
  oldPassword: string | null,
  newPassword: string | null,
  idleLockMinutes?: number,
) => invoke<void>("set_password", { oldPassword, newPassword, idleLockMinutes });

// ---------- 删除 ----------

export interface PurgeResult {
  removedFromDb: boolean;
  removedFolder: boolean;
}

/** 彻底删除项目文件夹 + 数据库记录。必须显式 confirm。 */
export const purgeProject = (id: number, expectedPath: string) =>
  invoke<PurgeResult>("purge_project", { id, confirm: true, expectedPath });

/** 当前应用版本号 */
export const getAppVersion = () => invoke<string>("app_version");

// ---------- 便携版更新 ----------

export interface PortableInfo {
  isPortable: boolean;
  exePath: string;
  installDir: string;
  /** 与 latest.json 对应的平台键，如 windows-x86_64 */
  platformKey: string;
  /** 当前应用版本 */
  version: string;
}

export interface PortableUpdateResult {
  version: string;
  stagingDir: string;
}

export const portableInfo = () => invoke<PortableInfo>("portable_info");
export const downloadPortableUpdate = (url: string, version: string) =>
  invoke<PortableUpdateResult>("download_portable_update", { url, version });
export const applyPortableUpdate = (stagingDir: string) =>
  invoke<void>("apply_portable_update", { stagingDir });

// ---------- 更新清单（必须走��端，WebView 里 fetch 外部 URL 会被拦）----------

export interface UpdateManifest {
  version: string;
  notes: string;
  portableUrl: string;
}

export const fetchUpdateManifest = (url: string, mirrorPrefix?: string) =>
  invoke<UpdateManifest>("fetch_update_manifest", { url, mirrorPrefix: mirrorPrefix ?? null });

/** 清理所有标记为「已丢失」的项目记录。 */
export const purgeMissing = () => invoke<number>("purge_missing_projects");

// ---------- 镜像源 ----------

export interface MirrorSpeed {
  id: string;
  label: string;
  ok: boolean;
  elapsedMs: number;
  error: string;
}

export interface MirrorInfo {
  id: string;
  label: string;
  prefix: string;
}

export const listMirrors = () => invoke<MirrorInfo[]>("list_mirrors");
export const speedTestMirrors = (probeUrl: string) =>
  invoke<MirrorSpeed[]>("speed_test_mirrors", { probeUrl });
export const mirrorUrl = (prefix: string, url: string) =>
  invoke<string>("mirror_url", { prefix, url });
