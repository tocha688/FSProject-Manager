export interface RootConfig {
  path: string;
  depth: number;
}

export interface AiConfig {
  provider: string;
  baseUrl: string;
  apiKey: string;
  model: string;
  enabled: boolean;
  concurrency: number;
  verifiedFingerprint: string;
}

export interface Settings {
  roots: RootConfig[];
  ai: AiConfig;
  shortcut: string;
  editorCmd: string;
  autostart: boolean;
}

/** get_settings 的返回：附带后端权威判定的"是否已验证" */
export interface SettingsView extends Settings {
  aiVerified: boolean;
}

export interface Project {
  id: number;
  path: string;
  name: string;
  language: string;
  status: string;
  description: string | null;
  tags: string[];
  notes: string | null;
  deps: string[];
  commits: string[];
  readme: string | null;
  gitRemote: string | null;
  structure: string[];
  lastCommitAt: number | null;
  analyzedAt: number | null;
  aiSource: string | null;
  updatedAt: number;
}

export interface AppStatus {
  scanning: boolean;
  analyzing: boolean;
}

export interface ScanResult {
  found: number;
  pruned: number;
  total: number;
}

export interface DataInfo {
  dbPath: string;
  configPath: string;
}

export type OpenTarget = "editor" | "folder" | "terminal";
