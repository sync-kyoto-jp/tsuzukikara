mod db;
mod error;
mod indexer;
mod parser;
mod resume;
mod settings;
mod sources;

use error::AppError;
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

type CmdResult<T> = Result<T, AppError>;

#[derive(Serialize)]
struct SourceView {
    key: String,
    environment_type: String,
    distro_name: Option<String>,
    root_path: Option<String>,
    enabled: bool,
    status: Option<db::SourceStatus>,
}

#[derive(Serialize)]
struct SourcesResponse {
    sources: Vec<SourceView>,
    wsl_error: Option<AppError>,
}

#[tauri::command]
fn list_sources(state: State<AppState>) -> CmdResult<SourcesResponse> {
    let settings = state.settings.lock().unwrap().clone();
    let statuses = db::source_statuses(&state.conn.lock().unwrap())?;
    let mut out = Vec::new();

    if let Some(root) = sources::windows_root() {
        out.push(SourceView {
            environment_type: "windows".into(),
            distro_name: None,
            root_path: Some(root.root_path.to_string_lossy().into_owned()),
            enabled: settings.windows_enabled,
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
        let status = statuses.get(&key).cloned();
        out.push(SourceView {
            environment_type: "wsl".into(),
            root_path: cached
                .get(&distro)
                .map(|r| r.root_path.to_string_lossy().into_owned())
                .or_else(|| status.as_ref().map(|s| s.root_path.clone())),
            enabled: settings.enabled_wsl_distros.contains(&distro),
            distro_name: Some(distro),
            status,
            key,
        });
    }

    for path in &settings.extra_roots {
        let root = sources::custom_root(&PathBuf::from(path));
        out.push(SourceView {
            environment_type: "custom".into(),
            distro_name: None,
            root_path: Some(path.clone()),
            enabled: true,
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
fn set_language(state: State<AppState>, language: Option<String>) -> CmdResult<Settings> {
    if let Some(l) = &language {
        if !Settings::is_valid_language(l) {
            return Err(AppError::with("invalid_language", l));
        }
    }
    save_settings(&state, |s| s.language = language)
}

#[tauri::command]
fn set_windows_enabled(state: State<AppState>, enabled: bool) -> CmdResult<Settings> {
    let s = save_settings(&state, |s| s.windows_enabled = enabled)?;
    if !enabled {
        indexer::remove_source(&state.conn.lock().unwrap(), "windows")?;
    }
    Ok(s)
}

#[tauri::command]
fn set_wsl_enabled(state: State<AppState>, distro: String, enabled: bool) -> CmdResult<Settings> {
    if !sources::is_valid_distro_name(&distro) {
        return Err(AppError::new("invalid_distro"));
    }
    let s = save_settings(&state, |s| {
        s.enabled_wsl_distros.retain(|d| d != &distro);
        if enabled {
            s.enabled_wsl_distros.push(distro.clone());
        }
    })?;
    if !enabled {
        state.wsl_roots.lock().unwrap().remove(&distro);
        indexer::remove_source(&state.conn.lock().unwrap(), &format!("wsl:{distro}"))?;
    }
    Ok(s)
}

#[tauri::command]
fn add_extra_root(state: State<AppState>, path: String) -> CmdResult<Settings> {
    let path = path.trim().to_string();
    if !PathBuf::from(&path).is_dir() {
        return Err(AppError::with("folder_not_found", &path));
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
    indexer::remove_source(&state.conn.lock().unwrap(), &key)?;
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
            return Err(AppError::new("scan_in_progress"));
        }
        *scanning = true;
    }
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || run_scan(&handle))
        .await
        .map_err(AppError::internal)
        .and_then(|r| r);
    *app.state::<AppState>().scanning.lock().unwrap() = false;
    result
}

fn run_scan(app: &tauri::AppHandle) -> CmdResult<RescanResponse> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let excludes: Vec<PathBuf> = settings.excluded_paths.iter().map(PathBuf::from).collect();
    let mut conn = db::open(&state.db_path)?;
    let mut reports = Vec::new();

    let mut roots: Vec<Result<SourceRoot, (String, AppError)>> = Vec::new();
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
                })?;
                reports.push(report);
            }
            // A failing source (e.g. a WSL distribution that will not start) does not stop others.
            Err((key, error)) => reports.push(indexer::ScanReport {
                source_key: key,
                status: "error".into(),
                errors: 1,
                error: Some(error),
                ..Default::default()
            }),
        }
    }
    Ok(RescanResponse { reports })
}

#[tauri::command]
fn search(state: State<AppState>, params: db::SearchParams) -> CmdResult<db::SearchResult> {
    Ok(db::search(&state.conn.lock().unwrap(), &params)?)
}

#[tauri::command]
fn get_session(state: State<AppState>, id: i64) -> CmdResult<db::SessionDetail> {
    get_session_inner(&state, id)
}

fn external_id_of(state: &AppState, id: i64) -> CmdResult<String> {
    get_session_inner(state, id)?
        .external_session_id
        .ok_or_else(|| AppError::new("session_id_missing"))
}

fn get_session_inner(state: &AppState, id: i64) -> CmdResult<db::SessionDetail> {
    db::session_detail(&state.conn.lock().unwrap(), id)?.ok_or_else(|| AppError::new("session_not_found"))
}

/// Sets the app-only display name of a session (`None` or blank restores the agent's title).
#[tauri::command]
fn rename_session(state: State<AppState>, id: i64, title: Option<String>) -> CmdResult<()> {
    let ext = external_id_of(&state, id)?;
    if title.as_deref().is_some_and(|t| t.chars().count() > 200) {
        return Err(AppError::new("title_too_long"));
    }
    Ok(db::rename_session(&state.conn.lock().unwrap(), &ext, title.as_deref())?)
}

#[tauri::command]
fn list_tags(state: State<AppState>) -> CmdResult<Vec<db::TagUsage>> {
    Ok(db::list_tags(&state.conn.lock().unwrap())?)
}

#[tauri::command]
fn create_tag(state: State<AppState>, name: String, color: String) -> CmdResult<db::TagView> {
    db::create_tag(&state.conn.lock().unwrap(), &name, &color)
}

#[tauri::command]
fn update_tag(state: State<AppState>, id: i64, name: String, color: String) -> CmdResult<db::TagView> {
    db::update_tag(&state.conn.lock().unwrap(), id, &name, &color)
}

#[tauri::command]
fn delete_tag(state: State<AppState>, id: i64) -> CmdResult<()> {
    Ok(db::delete_tag(&state.conn.lock().unwrap(), id)?)
}

#[tauri::command]
fn set_session_tags(state: State<AppState>, id: i64, tag_ids: Vec<i64>) -> CmdResult<()> {
    let ext = external_id_of(&state, id)?;
    Ok(db::set_session_tags(&mut state.conn.lock().unwrap(), &ext, &tag_ids)?)
}

#[derive(Serialize)]
struct FilterOptions {
    source_keys: Vec<String>,
    projects: Vec<db::ProjectView>,
    tags: Vec<db::TagUsage>,
}

#[tauri::command]
fn filter_options(state: State<AppState>) -> CmdResult<FilterOptions> {
    let conn = state.conn.lock().unwrap();
    Ok(FilterOptions {
        source_keys: db::indexed_source_keys(&conn)?,
        projects: db::list_projects(&conn)?,
        tags: db::list_tags(&conn)?,
    })
}

/// Checks run off the UI thread: they may start a WSL distribution.
#[tauri::command]
async fn resume_plan(app: tauri::AppHandle, id: i64) -> CmdResult<resume::ResumePlan> {
    let detail = get_session_inner(&app.state::<AppState>(), id)?;
    tauri::async_runtime::spawn_blocking(move || resume::plan(&detail)).await.map_err(AppError::internal)
}

#[tauri::command]
async fn resume_session(app: tauri::AppHandle, id: i64) -> CmdResult<()> {
    let detail = get_session_inner(&app.state::<AppState>(), id)?;
    tauri::async_runtime::spawn_blocking(move || resume::launch(&detail)).await.map_err(AppError::internal)?
}

#[tauri::command]
fn delete_index(state: State<AppState>) -> CmdResult<()> {
    if *state.scanning.lock().unwrap() {
        return Err(AppError::new("scan_in_progress"));
    }
    Ok(db::clear_index(&state.conn.lock().unwrap())?)
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
            set_language,
            set_windows_enabled,
            set_wsl_enabled,
            add_extra_root,
            remove_extra_root,
            set_excluded_paths,
            rescan,
            search,
            get_session,
            rename_session,
            list_tags,
            create_tag,
            update_tag,
            delete_tag,
            set_session_tags,
            filter_options,
            resume_plan,
            resume_session,
            delete_index,
            app_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
