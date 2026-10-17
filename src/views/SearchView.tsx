import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, FilterOptions, SearchParams, SearchResult } from "../api";
import Highlight from "../components/Highlight";
import { Splitter, useStoredWidth } from "../components/Splitter";
import { TagChip } from "../components/Tags";
import { useI18n } from "../i18n";
import { sourceLabel, titleOrUntitled } from "../labels";
import SessionDetailView from "./SessionDetailView";

const MIN_LIST_WIDTH = 320;
const MIN_DETAIL_WIDTH = 360;

type Period = "all" | "7" | "30" | "90" | "365" | "custom";
const PERIODS: Period[] = ["all", "7", "30", "90", "365", "custom"];

function periodRange(period: Period, from: string, to: string): Pick<SearchParams, "updated_from" | "updated_to"> {
  if (period === "all") return {};
  if (period === "custom") {
    // Dates are local calendar days; the index stores UTC instants.
    const start = from ? new Date(`${from}T00:00:00`).toISOString() : null;
    const end = to ? new Date(new Date(`${to}T00:00:00`).getTime() + 86_400_000).toISOString() : null;
    return { updated_from: start, updated_to: end };
  }
  return { updated_from: new Date(Date.now() - Number(period) * 86_400_000).toISOString() };
}

export default function SearchView({ revision, onRescan }: { revision: number; onRescan: () => void }) {
  const { t, te, formatDate } = useI18n();
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [sourceKey, setSourceKey] = useState("");
  const [projectId, setProjectId] = useState("");
  const [tagIds, setTagIds] = useState<number[]>([]);
  const [period, setPeriod] = useState<Period>("all");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
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

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(query), 250);
    return () => clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    api.filterOptions().then(setOptions).catch(setError);
  }, [revision, localRevision]);

  // Drop filters on tags that were deleted.
  useEffect(() => {
    if (options) setTagIds((ids) => ids.filter((id) => options.tags.some((tag) => tag.id === id)));
  }, [options]);

  const params: SearchParams = useMemo(
    () => ({
      query: debounced,
      source_keys: sourceKey ? [sourceKey] : [],
      project_id: projectId ? Number(projectId) : null,
      tag_ids: tagIds,
      ...periodRange(period, from, to),
      limit: 300,
    }),
    [debounced, sourceKey, projectId, tagIds, period, from, to],
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

  const projects = useMemo(
    () => (options?.projects ?? []).filter((p) => !sourceKey || p.source_key === sourceKey),
    [options, sourceKey],
  );
  const selectedTags = (options?.tags ?? []).filter((tag) => tagIds.includes(tag.id));
  const unselectedTags = (options?.tags ?? []).filter((tag) => !tagIds.includes(tag.id));
  const empty = options && options.source_keys.length === 0;
  const onSessionChanged = useCallback(() => setLocalRevision((r) => r + 1), []);

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
          <select value={sourceKey} onChange={(e) => { setSourceKey(e.target.value); setProjectId(""); }}>
            <option value="">{t("search.allEnvironments")}</option>
            {options?.source_keys.map((k) => (
              <option key={k} value={k}>{sourceLabel(t, k)}</option>
            ))}
          </select>
          <select value={projectId} onChange={(e) => setProjectId(e.target.value)} className="project-select">
            <option value="">{t("search.allProjects")}</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id} title={p.project_path}>
                {p.display_name} ({p.session_count})
              </option>
            ))}
          </select>
          <select value={period} onChange={(e) => setPeriod(e.target.value as Period)}>
            {PERIODS.map((p) => (
              <option key={p} value={p}>{t(`search.period.${p}` as const)}</option>
            ))}
          </select>
          {period === "custom" && (
            <span className="date-range">
              <input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
              〜
              <input type="date" value={to} onChange={(e) => setTo(e.target.value)} />
            </span>
          )}
          {unselectedTags.length > 0 && (
            <select
              value=""
              onChange={(e) => e.target.value && setTagIds((ids) => [...ids, Number(e.target.value)])}
            >
              <option value="">{t("search.addTagFilter")}</option>
              {unselectedTags.map((tag) => (
                <option key={tag.id} value={tag.id}>
                  {tag.name} ({tag.session_count})
                </option>
              ))}
            </select>
          )}
        </div>
        {selectedTags.length > 0 && (
          <div className="tag-row filter-tags">
            {selectedTags.map((tag) => (
              <TagChip key={tag.id} tag={tag} onRemove={() => setTagIds((ids) => ids.filter((i) => i !== tag.id))} />
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
