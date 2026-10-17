import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export const HIT_START = "";
export const HIT_END = "";

/** Error returned by backend commands; `code` is translated via `error.<code>` messages. */
export interface AppError {
  code: string;
  detail: string | null;
}

export function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && typeof (e as AppError).code === "string";
}

export interface TagView {
  id: number;
  name: string;
  color: string;
}

export interface TagUsage extends TagView {
  session_count: number;
}

export interface SearchParams {
  query: string;
  source_keys: string[];
  project_id?: number | null;
  tag_ids: number[];
  updated_from?: string | null;
  updated_to?: string | null;
  limit?: number;
}

export interface SearchHit {
  session_id: number;
  external_session_id: string | null;
  title: string;
  renamed: boolean;
  started_at: string | null;
  updated_at: string | null;
  source_key: string;
  environment_type: string;
  distro_name: string | null;
  project_id: number | null;
  project_path: string | null;
  project_name: string | null;
  git_branch: string | null;
  message_count: number;
  hit_count: number;
  duplicates: number;
  tags: TagView[];
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
  original_title: string;
  renamed: boolean;
  cwd: string | null;
  git_branch: string | null;
  started_at: string | null;
  updated_at: string | null;
  file_path: string;
  source_key: string;
  environment_type: string;
  distro_name: string | null;
  bad_lines: number;
  tags: TagView[];
  messages: MessageView[];
}

export interface ProjectView {
  id: number;
  source_key: string;
  project_path: string;
  display_name: string;
  session_count: number;
  last_updated_at: string | null;
}

export interface FilterOptions {
  source_keys: string[];
  projects: ProjectView[];
  tags: TagUsage[];
}

export interface ScanErrorView {
  file_path: string | null;
  code: string;
  detail: string | null;
}

export interface SourceStatus {
  key: string;
  root_path: string;
  last_scan_at: string | null;
  status: string;
  session_count: number;
  recent_errors: ScanErrorView[];
  error_count: number;
}

export interface SourceView {
  key: string;
  environment_type: string;
  distro_name: string | null;
  root_path: string | null;
  enabled: boolean;
  status: SourceStatus | null;
}

export interface SourcesResponse {
  sources: SourceView[];
  wsl_error: AppError | null;
}

export interface Settings {
  windows_enabled: boolean;
  enabled_wsl_distros: string[];
  extra_roots: string[];
  excluded_paths: string[];
  language: string | null;
}

export interface ScanReport {
  source_key: string;
  status: string;
  files: number;
  updated: number;
  unchanged: number;
  removed: number;
  errors: number;
  error: AppError | null;
}

export interface ScanProgress {
  source_key: string;
  done: number;
  total: number;
}

export interface ResumePlan {
  available: boolean;
  reason: AppError | null;
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
  renameSession: (id: number, title: string | null) => invoke<void>("rename_session", { id, title }),
  filterOptions: () => invoke<FilterOptions>("filter_options"),
  listTags: () => invoke<TagUsage[]>("list_tags"),
  createTag: (name: string, color: string) => invoke<TagView>("create_tag", { name, color }),
  updateTag: (id: number, name: string, color: string) => invoke<TagView>("update_tag", { id, name, color }),
  deleteTag: (id: number) => invoke<void>("delete_tag", { id }),
  setSessionTags: (id: number, tagIds: number[]) => invoke<void>("set_session_tags", { id, tagIds }),
  listSources: () => invoke<SourcesResponse>("list_sources"),
  getSettings: () => invoke<Settings>("get_settings"),
  setLanguage: (language: string | null) => invoke<Settings>("set_language", { language }),
  setWindowsEnabled: (enabled: boolean) => invoke<Settings>("set_windows_enabled", { enabled }),
  setWslEnabled: (distro: string, enabled: boolean) => invoke<Settings>("set_wsl_enabled", { distro, enabled }),
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
