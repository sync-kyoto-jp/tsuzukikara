mod db;
mod indexer;
mod parser;
mod resume;
mod settings;
mod sources;

use serde::Serialize;
use settings::Settings;
use sources::SourceRoot;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

struct AppState {
    db_path: PathBuf,
    settings_path: PathBuf,
    /// Connection for interactive queries. Scans use their own connection (WAL allows reads
    /// while a scan writes) so the UI stays responsive.
    conn: Mutex<rusqlite::Connection>,
    settings: Mutex<Settings>,
    /// Resolved WSL roots, cached so the distribution is asked for `$HOME` once per run.
    wsl_roots: Mutex<HashMap<String, SourceRoot>>,
    scanning: Mutex<bool>,
}

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct SourceView {
    key: String,
    label: String,
    environment_type: String,
    distro_name: Option<String>,
    root_path: Option<String>,
    enabled: bool,
    /// False for a WSL distribution the user has not allowed yet.
    configurable: bool,
    status: Option<db::SourceStatus>,
}

#[derive(Serialize)]
struct SourcesResponse {
    sources: Vec<SourceView>,
    wsl_error: Option<String>,
}

#[tauri::command]
fn list_sources(state: State<AppState>) -> CmdResult<SourcesResponse> {
    let settings = state.settings.lock().unwrap().clone();
    let statuses = db::source_statuses(&state.conn.lock().unwrap()).map_err(err)?;
    let mut out = Vec::new();

    if let Some(root) = sources::windows_root() {
        out.push(SourceView {
            label: "Windows (Claude Code)".into(),
            environment_type: "windows".into(),
            distro_name: None,
            root_path: Some(root.root_path.to_string_lossy().into_owned()),
            enabled: settings.windows_enabled,
            configurable: true,
            status: statuses.get(&root.key).cloned(),
            key: root.key,
        });
    }

    let (distros, wsl_error) = match sources::list_wsl_distros() {
        Ok(d) => (d, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let cached = state.wsl_roots.lock().unwrap().clone();
    let mut names = distros;
    for d in &settings.enabled_wsl_distros {
        if !names.contains(d) {
            names.push(d.clone());
        }
    }
    for distro in names {
        let key = format!("wsl:{distro}");
        let enabled = settings.enabled_wsl_distros.contains(&distro);
        let status = statuses.get(&key).cloned();
        out.push(SourceView {
            label: format!("WSL: {distro}"),
            environment_type: "wsl".into(),
            root_path: cached
                .get(&distro)
                .map(|r| r.root_path.to_string_lossy().into_owned())
                .or_else(|| status.as_ref().map(|s| s.root_path.clone())),
            distro_name: Some(distro),
            enabled,
            configurable: true,
            status,
            key,
        });
    }

    for path in &settings.extra_roots {
        let root = sources::custom_root(&PathBuf::from(path));
        out.push(SourceView {
            label: format!("追加フォルダ: {path}"),
            environment_type: "custom".into(),
            distro_name: None,
            root_path: Some(path.clone()),
            enabled: true,
            configurable: true,
            status: statuses.get(&root.key).cloned(),
            key: root.key,
        });
    }
    Ok(SourcesResponse { sources: out, wsl_error })
}

fn save_settings(state: &AppState, f: impl FnOnce(&mut Settings)) -> CmdResult<Settings> {
    let mut s = state.settings.lock().unwrap();
    f(&mut s);
    s.save(&state.settings_path)?;
    Ok(s.clone())
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_windows_enabled(state: State<AppState>, enabled: bool) -> CmdResult<Settings> {
    let s = save_settings(&state, |s| s.windows_enabled = enabled)?;
    if !enabled {
        indexer::remove_source(&state.conn.lock().unwrap(), "windows").map_err(err)?;
    }
    Ok(s)
}

#[tauri::command]
fn set_wsl_enabled(state: State<AppState>, distro: String, enabled: bool) -> CmdResult<Settings> {
    if !sources::is_valid_distro_name(&distro) {
        return Err("不正なディストリビューション名です".into());
    }
    let s = save_settings(&state, |s| {
        s.enabled_wsl_distros.retain(|d| d != &distro);
        if enabled {
            s.enabled_wsl_distros.push(distro.clone());
        }
    })?;
    if !enabled {
        state.wsl_roots.lock().unwrap().remove(&distro);
        indexer::remove_source(&state.conn.lock().unwrap(), &format!("wsl:{distro}")).map_err(err)?;
    }
    Ok(s)
}

#[tauri::command]
fn add_extra_root(state: State<AppState>, path: String) -> CmdResult<Settings> {
    let path = path.trim().to_string();
    if !PathBuf::from(&path).is_dir() {
        return Err(format!("フォルダが見つかりません: {path}"));
    }
    save_settings(&state, |s| {
        if !s.extra_roots.contains(&path) {
            s.extra_roots.push(path.clone());
        }
    })
}

#[tauri::command]
fn remove_extra_root(state: State<AppState>, path: String) -> CmdResult<Settings> {
    let s = save_settings(&state, |s| s.extra_roots.retain(|p| p != &path))?;
    let key = sources::custom_root(&PathBuf::from(&path)).key;
    indexer::remove_source(&state.conn.lock().unwrap(), &key).map_err(err)?;
    Ok(s)
}

#[tauri::command]
fn set_excluded_paths(state: State<AppState>, paths: Vec<String>) -> CmdResult<Settings> {
    let paths: Vec<String> = paths.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    save_settings(&state, |s| s.excluded_paths = paths)
}

#[derive(Clone, Serialize)]
struct ScanProgress {
    source_key: String,
    done: usize,
    total: usize,
}

#[derive(Serialize)]
struct RescanResponse {
    reports: Vec<indexer::ScanReport>,
}

/// Rescans every enabled source; only changed files are re-parsed.
#[tauri::command]
async fn rescan(app: tauri::AppHandle) -> CmdResult<RescanResponse> {
    {
        let state = app.state::<AppState>();
        let mut scanning = state.scanning.lock().unwrap();
        if *scanning {
            return Err("スキャン中です".into());
        }
        *scanning = true;
    }
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || run_scan(&handle))
        .await
        .map_err(err)
        .and_then(|r| r);
    *app.state::<AppState>().scanning.lock().unwrap() = false;
    result
}

fn run_scan(app: &tauri::AppHandle) -> CmdResult<RescanResponse> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let excludes: Vec<PathBuf> = settings.excluded_paths.iter().map(PathBuf::from).collect();
    let mut conn = db::open(&state.db_path).map_err(err)?;
    let mut reports = Vec::new();

    let mut roots: Vec<Result<SourceRoot, (String, String)>> = Vec::new();
    if settings.windows_enabled {
        if let Some(r) = sources::windows_root() {
            roots.push(Ok(r));
        }
    }
    for distro in &settings.enabled_wsl_distros {
        let cached = state.wsl_roots.lock().unwrap().get(distro).cloned();
        let root = match cached {
            Some(r) => Ok(r),
            None => sources::resolve_wsl_root(distro).inspect(|r| {
                state.wsl_roots.lock().unwrap().insert(distro.clone(), r.clone());
            }),
        };
        roots.push(root.map_err(|e| (format!("wsl:{distro}"), e)));
    }
    for path in &settings.extra_roots {
        roots.push(Ok(sources::custom_root(&PathBuf::from(path))));
    }

    for root in roots {
        match root {
            Ok(root) => {
                let key = root.key.clone();
                let mut last_emit = std::time::Instant::now();
                let report = indexer::scan_source(&mut conn, &root, &excludes, &mut |done, total| {
                    if done == total || last_emit.elapsed().as_millis() > 150 {
                        last_emit = std::time::Instant::now();
                        let _ = app.emit("scan-progress", ScanProgress { source_key: key.clone(), done, total });
                    }
                })
                .map_err(err)?;
                reports.push(report);
            }
            // A failing source (e.g. a WSL distribution that will not start) does not stop others.
            Err((key, message)) => reports.push(indexer::ScanReport {
                source_key: key,
                status: format!("error: {message}"),
                errors: 1,
                ..Default::default()
            }),
        }
    }
    Ok(RescanResponse { reports })
}

#[tauri::command]
fn search(state: State<AppState>, params: db::SearchParams) -> CmdResult<db::SearchResult> {
    db::search(&state.conn.lock().unwrap(), &params).map_err(err)
}

#[tauri::command]
fn get_session(state: State<AppState>, id: i64) -> CmdResult<db::SessionDetail> {
    db::session_detail(&state.conn.lock().unwrap(), id)
        .map_err(err)?
        .ok_or_else(|| "セッションが見つかりません".into())
}

#[derive(Serialize)]
struct FilterOptions {
    source_keys: Vec<String>,
    projects: Vec<db::ProjectView>,
    client_tags: Vec<String>,
}

#[tauri::command]
fn filter_options(state: State<AppState>) -> CmdResult<FilterOptions> {
    let conn = state.conn.lock().unwrap();
    Ok(FilterOptions {
        source_keys: db::indexed_source_keys(&conn).map_err(err)?,
        projects: db::list_projects(&conn).map_err(err)?,
        client_tags: db::client_tags(&conn).map_err(err)?,
    })
}

#[tauri::command]
fn set_project_tag(state: State<AppState>, project_path: String, tag: Option<String>) -> CmdResult<()> {
    db::set_project_tag(&state.conn.lock().unwrap(), &project_path, tag.as_deref()).map_err(err)
}

/// Checks run off the UI thread: they may start a WSL distribution.
#[tauri::command]
async fn resume_plan(app: tauri::AppHandle, id: i64) -> CmdResult<resume::ResumePlan> {
    let detail = get_session(app.state::<AppState>(), id)?;
    tauri::async_runtime::spawn_blocking(move || resume::plan(&detail)).await.map_err(err)
}

#[tauri::command]
async fn resume_session(app: tauri::AppHandle, id: i64) -> CmdResult<()> {
    let detail = get_session(app.state::<AppState>(), id)?;
    tauri::async_runtime::spawn_blocking(move || resume::launch(&detail)).await.map_err(err)?
}

#[tauri::command]
fn delete_index(state: State<AppState>) -> CmdResult<()> {
    if *state.scanning.lock().unwrap() {
        return Err("スキャン中は削除できません".into());
    }
    db::clear_index(&state.conn.lock().unwrap()).map_err(err)
}

#[derive(Serialize)]
struct AppInfo {
    version: String,
    data_dir: String,
    db_path: String,
}

#[tauri::command]
fn app_info(app: tauri::AppHandle, state: State<AppState>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: state.db_path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        db_path: state.db_path.to_string_lossy().into_owned(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("index.sqlite");
            let settings_path = dir.join("settings.json");
            let conn = db::open(&db_path)?;
            app.manage(AppState {
                settings: Mutex::new(Settings::load(&settings_path)),
                db_path,
                settings_path,
                conn: Mutex::new(conn),
                wsl_roots: Mutex::new(HashMap::new()),
                scanning: Mutex::new(false),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_sources,
            get_settings,
            set_windows_enabled,
            set_wsl_enabled,
            add_extra_root,
            remove_extra_root,
            set_excluded_paths,
            rescan,
            search,
            get_session,
            filter_options,
            set_project_tag,
            resume_plan,
            resume_session,
            delete_index,
            app_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
