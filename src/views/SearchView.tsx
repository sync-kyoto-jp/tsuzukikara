import { useEffect, useMemo, useState } from "react";
import { api, errorText, FilterOptions, formatDate, SearchParams, SearchResult, sourceLabel } from "../api";
import Highlight from "../components/Highlight";
import SessionDetailView from "./SessionDetailView";

type Period = "all" | "7" | "30" | "90" | "365" | "custom";

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
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [sourceKey, setSourceKey] = useState("");
  const [projectId, setProjectId] = useState("");
  const [clientTag, setClientTag] = useState("");
  const [period, setPeriod] = useState<Period>("all");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [options, setOptions] = useState<FilterOptions | null>(null);
  const [result, setResult] = useState<SearchResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<number | null>(null);
  const [tagRevision, setTagRevision] = useState(0);

  useEffect(() => {
    const t = setTimeout(() => setDebounced(query), 250);
    return () => clearTimeout(t);
  }, [query]);

  useEffect(() => {
    api.filterOptions().then(setOptions).catch((e) => setError(errorText(e)));
  }, [revision, tagRevision]);

  const params: SearchParams = useMemo(
    () => ({
      query: debounced,
      source_keys: sourceKey ? [sourceKey] : [],
      project_id: projectId ? Number(projectId) : null,
      client_tag: clientTag || null,
      ...periodRange(period, from, to),
      limit: 300,
    }),
    [debounced, sourceKey, projectId, clientTag, period, from, to],
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
      .catch((e) => !cancelled && setError(errorText(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [params, revision, tagRevision]);

  const projects = useMemo(
    () => (options?.projects ?? []).filter((p) => !sourceKey || p.source_key === sourceKey),
    [options, sourceKey],
  );
  const empty = options && options.source_keys.length === 0;

  return (
    <div className="search-layout">
      <section className="search-pane">
        <div className="search-box">
          <input
            autoFocus
            type="search"
            placeholder="キーワードで検索（スペース区切りで AND）"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <div className="filters">
          <select value={sourceKey} onChange={(e) => { setSourceKey(e.target.value); setProjectId(""); }}>
            <option value="">すべての環境</option>
            {options?.source_keys.map((k) => (
              <option key={k} value={k}>{sourceLabel(k)}</option>
            ))}
          </select>
          <select value={clientTag} onChange={(e) => setClientTag(e.target.value)}>
            <option value="">すべての案件タグ</option>
            {options?.client_tags.map((t) => (
              <option key={t} value={t}>{t}</option>
            ))}
          </select>
          <select value={projectId} onChange={(e) => setProjectId(e.target.value)} className="project-select">
            <option value="">すべてのプロジェクト</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id} title={p.project_path}>
                {p.display_name}{p.client_tag ? ` [${p.client_tag}]` : ""} ({p.session_count})
              </option>
            ))}
          </select>
          <select value={period} onChange={(e) => setPeriod(e.target.value as Period)}>
            <option value="all">全期間</option>
            <option value="7">7日以内</option>
            <option value="30">30日以内</option>
            <option value="90">90日以内</option>
            <option value="365">1年以内</option>
            <option value="custom">期間を指定</option>
          </select>
          {period === "custom" && (
            <span className="date-range">
              <input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
              〜
              <input type="date" value={to} onChange={(e) => setTo(e.target.value)} />
            </span>
          )}
        </div>
        <div className="result-meta muted">
          {loading ? "検索中…" : result ? `${result.total} 件${result.total > result.hits.length ? `（上位 ${result.hits.length} 件を表示）` : ""}` : ""}
        </div>
        {error && <div className="error">{error}</div>}
        {empty && (
          <div className="empty">
            <p>まだインデックスがありません。</p>
            <button className="primary" onClick={onRescan}>履歴をスキャン</button>
            <p className="muted">WSL の履歴を含めるには「ソース管理」でディストリビューションを有効にしてください。</p>
          </div>
        )}
        <ul className="results">
          {result?.hits.map((h) => (
            <li
              key={h.session_id}
              className={selected === h.session_id ? "selected" : ""}
              onClick={() => setSelected(h.session_id)}
            >
              <div className="result-title">{h.title}</div>
              <div className="result-sub">
                <span className={`badge env-${h.environment_type}`}>{sourceLabel(h.source_key)}</span>
                {h.client_tag && <span className="badge tag">{h.client_tag}</span>}
                <span title={h.project_path ?? ""}>{h.project_name ?? "—"}</span>
                {h.git_branch && <span className="muted">⎇ {h.git_branch}</span>}
                <span className="muted">{formatDate(h.updated_at)}</span>
                {h.hit_count > 0 && <span className="muted">{h.hit_count} 件ヒット</span>}
                {h.duplicates > 0 && <span className="badge warn" title="同じセッションIDの別ファイルがあります">重複 {h.duplicates}</span>}
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
      <section className="detail-pane">
        {selected != null ? (
          <SessionDetailView
            id={selected}
            query={debounced}
            onTagChanged={() => setTagRevision((r) => r + 1)}
          />
        ) : (
          <div className="placeholder muted">左の一覧からセッションを選択してください</div>
        )}
      </section>
    </div>
  );
}
