import { invoke } from "@tauri-apps/api/core";
import type {
  AppStatus,
  DataInfo,
  OpenTarget,
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
export const openProject = (id: number, target: OpenTarget) =>
  invoke<void>("open_project", { id, target });
export const copyPath = (id: number) => invoke<void>("copy_path", { id });
export const addManualProject = (path: string) =>
  invoke<Project>("add_manual_project", { path });
export const confirmCandidate = (id: number) =>
  invoke<void>("confirm_candidate", { id });
export const deleteProject = (id: number) => invoke<void>("delete_project", { id });
export const analyzeProject = (id: number) => invoke<void>("analyze_project", { id });
export const analyzePending = (mode: "pending" | "all" = "pending") =>
  invoke<void>("analyze_pending", { mode });
export const testAi = () => invoke<string>("test_ai");
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
