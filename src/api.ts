import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export const HIT_START = "";
export const HIT_END = "";

export interface SearchParams {
  query: string;
  source_keys: string[];
  project_id?: number | null;
  client_tag?: string | null;
  updated_from?: string | null;
  updated_to?: string | null;
  limit?: number;
}

export interface SearchHit {
  session_id: number;
  external_session_id: string | null;
  title: string;
  started_at: string | null;
  updated_at: string | null;
  source_key: string;
  environment_type: string;
  distro_name: string | null;
  project_id: number | null;
  project_path: string | null;
  project_name: string | null;
  client_tag: string | null;
  git_branch: string | null;
  message_count: number;
  hit_count: number;
  duplicates: number;
  snippet: string | null;
}

export interface SearchResult {
  hits: SearchHit[];
  total: number;
}

export interface MessageView {
  sequence_no: number;
  role: string;
  timestamp: string | null;
  text: string;
}

export interface SessionDetail {
  session_id: number;
  external_session_id: string | null;
  title: string;
  cwd: string | null;
  git_branch: string | null;
  started_at: string | null;
  updated_at: string | null;
  file_path: string;
  source_key: string;
  environment_type: string;
  distro_name: string | null;
  client_tag: string | null;
  bad_lines: number;
  messages: MessageView[];
}

export interface ProjectView {
  id: number;
  source_key: string;
  project_path: string;
  display_name: string;
  client_tag: string | null;
  session_count: number;
  last_updated_at: string | null;
}

export interface FilterOptions {
  source_keys: string[];
  projects: ProjectView[];
  client_tags: string[];
}

export interface SourceStatus {
  key: string;
  root_path: string;
  last_scan_at: string | null;
  status: string;
  session_count: number;
  recent_errors: string[];
  error_count: number;
}

export interface SourceView {
  key: string;
  label: string;
  environment_type: string;
  distro_name: string | null;
  root_path: string | null;
  enabled: boolean;
  configurable: boolean;
  status: SourceStatus | null;
}

export interface SourcesResponse {
  sources: SourceView[];
  wsl_error: string | null;
}

export interface Settings {
  windows_enabled: boolean;
  enabled_wsl_distros: string[];
  extra_roots: string[];
  excluded_paths: string[];
}

export interface ScanReport {
  source_key: string;
  status: string;
  files: number;
  updated: number;
  unchanged: number;
  removed: number;
  errors: number;
}

export interface ScanProgress {
  source_key: string;
  done: number;
  total: number;
}

export interface ResumePlan {
  available: boolean;
  reason: string | null;
  command_display: string;
  working_dir: string | null;
}

export interface AppInfo {
  version: string;
  data_dir: string;
  db_path: string;
}

export const api = {
  search: (params: SearchParams) => invoke<SearchResult>("search", { params }),
  getSession: (id: number) => invoke<SessionDetail>("get_session", { id }),
  filterOptions: () => invoke<FilterOptions>("filter_options"),
  setProjectTag: (projectPath: string, tag: string | null) =>
    invoke<void>("set_project_tag", { projectPath, tag }),
  listSources: () => invoke<SourcesResponse>("list_sources"),
  getSettings: () => invoke<Settings>("get_settings"),
  setWindowsEnabled: (enabled: boolean) => invoke<Settings>("set_windows_enabled", { enabled }),
  setWslEnabled: (distro: string, enabled: boolean) =>
    invoke<Settings>("set_wsl_enabled", { distro, enabled }),
  addExtraRoot: (path: string) => invoke<Settings>("add_extra_root", { path }),
  removeExtraRoot: (path: string) => invoke<Settings>("remove_extra_root", { path }),
  setExcludedPaths: (paths: string[]) => invoke<Settings>("set_excluded_paths", { paths }),
  rescan: () => invoke<{ reports: ScanReport[] }>("rescan"),
  resumePlan: (id: number) => invoke<ResumePlan>("resume_plan", { id }),
  resumeSession: (id: number) => invoke<void>("resume_session", { id }),
  deleteIndex: () => invoke<void>("delete_index"),
  appInfo: () => invoke<AppInfo>("app_info"),
  onScanProgress: (cb: (p: ScanProgress) => void): Promise<UnlistenFn> =>
    listen<ScanProgress>("scan-progress", (e) => cb(e.payload)),
};

export function sourceLabel(key: string): string {
  if (key === "windows") return "Windows";
  if (key.startsWith("wsl:")) return `WSL ${key.slice(4)}`;
  if (key.startsWith("custom:")) return "追加フォルダ";
  return key;
}

export function formatDate(iso: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString("ja-JP", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
