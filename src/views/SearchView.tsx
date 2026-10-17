import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, FilterOptions, SearchParams, SearchResult, SortDir, SortField, TagMode } from "../api";
import Highlight from "../components/Highlight";
import MultiSelect from "../components/MultiSelect";
import { Splitter, useStoredWidth } from "../components/Splitter";
import { TagChip } from "../components/Tags";
import { useI18n } from "../i18n";
import { sourceLabel, titleOrUntitled } from "../labels";
import SessionDetailView from "./SessionDetailView";

const MIN_LIST_WIDTH = 320;
const MIN_DETAIL_WIDTH = 360;

const SORT_FIELDS: SortField[] = ["updated", "started", "title", "project", "messages", "hits"];
const QUICK_RANGES = [7, 30, 90, 365] as const;
const SORT_STORAGE_KEY = "tsuzukikara.sort";

/** Local calendar date (YYYY-MM-DD) for a date input. */
function isoDate(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** From/To are inclusive local calendar days; the index stores UTC instants. */
function dateRange(from: string, to: string): Pick<SearchParams, "updated_from" | "updated_to"> {
  const start = from ? new Date(`${from}T00:00:00`).toISOString() : null;
  const end = to ? new Date(new Date(`${to}T00:00:00`).getTime() + 86_400_000).toISOString() : null;
  return { updated_from: start, updated_to: end };
}

function loadSort(): { field: SortField; dir: SortDir } {
  try {
    const v = JSON.parse(localStorage.getItem(SORT_STORAGE_KEY) ?? "null");
    if (v && SORT_FIELDS.includes(v.field) && (v.dir === "asc" || v.dir === "desc")) return v;
  } catch {
    // Fall through to the default when storage is unavailable or malformed.
  }
  return { field: "updated", dir: "desc" };
}

export default function SearchView({ revision, onRescan }: { revision: number; onRescan: () => void }) {
  const { t, te, formatDate } = useI18n();
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [sourceKeys, setSourceKeys] = useState<string[]>([]);
  const [projectIds, setProjectIds] = useState<string[]>([]);
  const [tagIds, setTagIds] = useState<string[]>([]);
  const [tagMode, setTagMode] = useState<TagMode>("and");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [sort, setSortState] = useState(loadSort);
  const [options, setOptions] = useState<FilterOptions | null>(null);
  const [result, setResult] = useState<SearchResult | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<number | null>(null);
  // Bumped when a session's tags or name change in the detail pane.
  const [localRevision, setLocalRevision] = useState(0);
  const layoutRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLElement>(null);
  const [listWidth, setListWidth] = useStoredWidth("tsuzukikara.listWidth");

  const setSort = (next: { field: SortField; dir: SortDir }) => {
    setSortState(next);
    try {
      localStorage.setItem(SORT_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // The sort order then lasts for this run only.
    }
  };

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(query), 250);
    return () => clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    api.filterOptions().then(setOptions).catch(setError);
  }, [revision, localRevision]);

  // Drop selections that no longer exist (deleted tags, projects gone after a rescan).
  useEffect(() => {
    if (!options) return;
    setTagIds((ids) => ids.filter((id) => options.tags.some((tag) => String(tag.id) === id)));
    setProjectIds((ids) => ids.filter((id) => options.projects.some((p) => String(p.id) === id)));
    setSourceKeys((keys) => keys.filter((k) => options.source_keys.includes(k)));
  }, [options]);

  const params: SearchParams = useMemo(
    () => ({
      query: debounced,
      source_keys: sourceKeys,
      project_ids: projectIds.map(Number),
      tag_ids: tagIds.map(Number),
      tag_mode: tagMode,
      ...dateRange(from, to),
      sort: sort.field,
      sort_dir: sort.dir,
      limit: 300,
    }),
    [debounced, sourceKeys, projectIds, tagIds, tagMode, from, to, sort],
  );

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api
      .search(params)
      .then((r) => {
        if (!cancelled) {
          setResult(r);
          setError(null);
        }
      })
      .catch((e) => !cancelled && setError(e))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [params, revision, localRevision]);

  // Projects of the selected environments only (all when none is selected).
  const projects = useMemo(
    () => (options?.projects ?? []).filter((p) => sourceKeys.length === 0 || sourceKeys.includes(p.source_key)),
    [options, sourceKeys],
  );
  const selectedTags = (options?.tags ?? []).filter((tag) => tagIds.includes(String(tag.id)));
  const empty = options && options.source_keys.length === 0;
  const onSessionChanged = useCallback(() => setLocalRevision((r) => r + 1), []);

  function quickRange(days: number) {
    setFrom(isoDate(new Date(Date.now() - (days - 1) * 86_400_000)));
    setTo(isoDate(new Date()));
  }

  return (
    <div
      className="search-layout"
      ref={layoutRef}
      style={
        listWidth != null
          ? // min() keeps the detail pane usable when the window is narrower than when resized.
            { gridTemplateColumns: `min(${listWidth}px, calc(100% - ${MIN_DETAIL_WIDTH}px)) auto 1fr` }
          : undefined
      }
    >
      <section className="search-pane" ref={listRef}>
        <div className="search-box">
          <input
            autoFocus
            type="search"
            placeholder={t("search.placeholder")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <div className="filters">
          <MultiSelect
            allLabel={t("search.allEnvironments")}
            label={t("search.environments")}
            options={(options?.source_keys ?? []).map((k) => ({ value: k, label: sourceLabel(t, k), title: k }))}
            selected={sourceKeys}
            onChange={setSourceKeys}
          />
          <MultiSelect
            allLabel={t("search.allProjects")}
            label={t("search.projects")}
            options={projects.map((p) => ({
              value: String(p.id),
              label: `${p.display_name} (${p.session_count})`,
              title: p.project_path,
            }))}
            selected={projectIds}
            onChange={setProjectIds}
          />
          <MultiSelect
            allLabel={t("search.allTags")}
            label={t("search.tags")}
            options={(options?.tags ?? []).map((tag) => ({
              value: String(tag.id),
              label: tag.name,
              render: (
                <>
                  <TagChip tag={tag} small /> <span className="muted small">{tag.session_count}</span>
                </>
              ),
            }))}
            selected={tagIds}
            onChange={setTagIds}
            header={
              <div className="segmented" role="radiogroup" aria-label={t("search.tagMode")}>
                {(["and", "or"] as TagMode[]).map((mode) => (
                  <button
                    key={mode}
                    role="radio"
                    aria-checked={tagMode === mode}
                    className={tagMode === mode ? "active" : ""}
                    onClick={() => setTagMode(mode)}
                  >
                    {t(mode === "and" ? "search.tagModeAnd" : "search.tagModeOr")}
                  </button>
                ))}
              </div>
            }
          />
        </div>
        <div className="filters">
          <span className="date-range">
            <label className="muted small" htmlFor="date-from">{t("search.from")}</label>
            <input id="date-from" type="date" value={from} max={to || undefined} onChange={(e) => setFrom(e.target.value)} />
            <span aria-hidden>〜</span>
            <label className="muted small" htmlFor="date-to">{t("search.to")}</label>
            <input id="date-to" type="date" value={to} min={from || undefined} onChange={(e) => setTo(e.target.value)} />
          </span>
          <select
            value=""
            aria-label={t("search.quickRange")}
            onChange={(e) => e.target.value && quickRange(Number(e.target.value))}
          >
            <option value="">{t("search.quickRange")}</option>
            {QUICK_RANGES.map((d) => (
              <option key={d} value={d}>{t(`search.period.${d}` as const)}</option>
            ))}
          </select>
          {(from || to) && (
            <button className="small-button" onClick={() => { setFrom(""); setTo(""); }}>
              {t("search.clearDates")}
            </button>
          )}
          <span className="sort-control">
            <label className="muted small" htmlFor="sort-field">{t("search.sortBy")}</label>
            <select
              id="sort-field"
              value={sort.field}
              onChange={(e) => setSort({ ...sort, field: e.target.value as SortField })}
            >
              {SORT_FIELDS.map((f) => (
                <option key={f} value={f} disabled={f === "hits" && !debounced.trim()}>
                  {t(`search.sort.${f}` as const)}
                </option>
              ))}
            </select>
            <button
              className="small-button"
              onClick={() => setSort({ ...sort, dir: sort.dir === "asc" ? "desc" : "asc" })}
              title={t(sort.dir === "asc" ? "search.sortAsc" : "search.sortDesc")}
              aria-label={t(sort.dir === "asc" ? "search.sortAsc" : "search.sortDesc")}
            >
              {sort.dir === "asc" ? "↑" : "↓"} {t(sort.dir === "asc" ? "search.sortAsc" : "search.sortDesc")}
            </button>
          </span>
        </div>
        {selectedTags.length > 0 && (
          <div className="tag-row filter-tags">
            {selectedTags.length > 1 && (
              <span className="muted small">{t(tagMode === "and" ? "search.tagModeAnd" : "search.tagModeOr")}:</span>
            )}
            {selectedTags.map((tag) => (
              <TagChip
                key={tag.id}
                tag={tag}
                onRemove={() => setTagIds((ids) => ids.filter((i) => i !== String(tag.id)))}
              />
            ))}
          </div>
        )}
        <div className="result-meta muted">
          {loading
            ? t("search.searching")
            : result
              ? result.total > result.hits.length
                ? t("search.countLimited", { total: result.total, shown: result.hits.length })
                : t("search.count", { total: result.total })
              : ""}
        </div>
        {error != null && <div className="error">{te(error)}</div>}
        {empty && (
          <div className="empty">
            <p>{t("search.emptyIndex")}</p>
            <button className="primary" onClick={onRescan}>{t("search.scanNow")}</button>
            <p className="muted">{t("search.wslHint")}</p>
          </div>
        )}
        <ul className="results">
          {result?.hits.map((h) => (
            <li
              key={h.session_id}
              className={selected === h.session_id ? "selected" : ""}
              onClick={() => setSelected(h.session_id)}
            >
              <div className="result-title">
                <span className={h.title ? "" : "muted"}>{titleOrUntitled(t, h.title)}</span>
              </div>
              {h.tags.length > 0 && (
                <div className="tag-row">
                  {h.tags.map((tag) => <TagChip key={tag.id} tag={tag} small />)}
                </div>
              )}
              <div className="result-sub">
                <span className={`badge env-${h.environment_type}`}>{sourceLabel(t, h.source_key)}</span>
                <span title={h.project_path ?? ""}>{h.project_name ?? "—"}</span>
                {h.git_branch && <span className="muted">⎇ {h.git_branch}</span>}
                <span className="muted">{formatDate(h.updated_at)}</span>
                {h.hit_count > 0 && <span className="muted">{t("search.hits", { count: h.hit_count })}</span>}
                {h.duplicates > 0 && (
                  <span className="badge warn" title={t("search.duplicatesTitle")}>
                    {t("search.duplicates", { count: h.duplicates })}
                  </span>
                )}
              </div>
              {h.snippet && (
                <div className="snippet">
                  <Highlight text={h.snippet} />
                </div>
              )}
            </li>
          ))}
        </ul>
      </section>
      <Splitter
        containerRef={layoutRef}
        leftRef={listRef}
        width={listWidth}
        onChange={setListWidth}
        minLeft={MIN_LIST_WIDTH}
        minRight={MIN_DETAIL_WIDTH}
        label={t("search.resizeHandle")}
      />
      <section className="detail-pane">
        {selected != null ? (
          <SessionDetailView id={selected} query={debounced} onChanged={onSessionChanged} />
        ) : (
          <div className="placeholder muted">{t("search.selectSession")}</div>
        )}
      </section>
    </div>
  );
}
