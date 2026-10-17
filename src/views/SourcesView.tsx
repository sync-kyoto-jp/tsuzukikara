import { useCallback, useEffect, useState } from "react";
import { api, errorText, formatDate, SourceView } from "../api";

export default function SourcesView({ revision, onChanged }: { revision: number; onChanged: () => void }) {
  const [sources, setSources] = useState<SourceView[]>([]);
  const [wslError, setWslError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [newRoot, setNewRoot] = useState("");
  const [pendingWsl, setPendingWsl] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .listSources()
      .then((r) => {
        setSources(r.sources);
        setWslError(r.wsl_error);
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  useEffect(load, [load, revision]);

  async function run(action: () => Promise<unknown>) {
    setError(null);
    try {
      await action();
      load();
      onChanged();
    } catch (e) {
      setError(errorText(e));
    }
  }

  function toggle(s: SourceView) {
    if (s.environment_type === "windows") return run(() => api.setWindowsEnabled(!s.enabled));
    if (s.environment_type === "wsl" && s.distro_name) {
      // Enabling a distribution starts it and reads its files, so ask first.
      if (!s.enabled) return setPendingWsl(s.distro_name);
      return run(() => api.setWslEnabled(s.distro_name!, false));
    }
    if (s.environment_type === "custom" && s.root_path) return run(() => api.removeExtraRoot(s.root_path!));
  }

  return (
    <div className="page">
      <h2>ソース管理</h2>
      <p className="muted">
        履歴ファイルは読み取り専用で扱い、変更・削除はしません。変更があった後は右上の「再スキャン」でインデックスを更新します。
      </p>
      {error && <div className="error">{error}</div>}
      {wslError && <div className="muted">WSL: {wslError}</div>}

      {pendingWsl && (
        <div className="confirm">
          <p>
            WSL ディストリビューション <b>{pendingWsl}</b> の既定ユーザーのホームにある
            <code> ~/.claude/projects </code>を読み取ります。
            停止中の場合はディストリビューションが起動します。許可しますか？
          </p>
          <div className="actions">
            <button
              className="primary"
              onClick={() => {
                const d = pendingWsl;
                setPendingWsl(null);
                run(() => api.setWslEnabled(d, true));
              }}
            >
              許可する
            </button>
            <button onClick={() => setPendingWsl(null)}>キャンセル</button>
          </div>
        </div>
      )}

      <table className="sources">
        <thead>
          <tr>
            <th>ソース</th>
            <th>保存先</th>
            <th>状態</th>
            <th>セッション</th>
            <th>最終スキャン</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {sources.map((s) => (
            <tr key={s.key} className={s.enabled ? "" : "disabled"}>
              <td>{s.label}</td>
              <td className="mono small">{s.root_path ?? (s.enabled ? "次回スキャン時に確認" : "—")}</td>
              <td>
                {!s.enabled ? (
                  <span className="muted">無効</span>
                ) : s.status ? (
                  <span className={`status status-${s.status.status.split(":")[0]}`}>{statusLabel(s.status.status)}</span>
                ) : (
                  <span className="muted">未スキャン</span>
                )}
                {s.status && s.status.error_count > 0 && (
                  <details className="errors">
                    <summary>エラー {s.status.error_count} 件</summary>
                    <ul>
                      {s.status.recent_errors.map((e, i) => (
                        <li key={i} className="mono small">{e}</li>
                      ))}
                    </ul>
                  </details>
                )}
              </td>
              <td>{s.status?.session_count ?? "—"}</td>
              <td>{formatDate(s.status?.last_scan_at ?? null)}</td>
              <td>
                <button onClick={() => toggle(s)}>
                  {s.environment_type === "custom" ? "削除" : s.enabled ? "無効にする" : "有効にする"}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <h3>フォルダを追加</h3>
      <p className="muted small">
        プロジェクトごとのフォルダに <code>*.jsonl</code> が入った、<code>.claude/projects</code> 形式のフォルダを指定します（バックアップなど）。
      </p>
      <div className="inline-form">
        <input
          className="mono"
          placeholder="例: D:\backup\claude\projects"
          value={newRoot}
          onChange={(e) => setNewRoot(e.target.value)}
        />
        <button
          disabled={!newRoot.trim()}
          onClick={() =>
            run(async () => {
              await api.addExtraRoot(newRoot);
              setNewRoot("");
            })
          }
        >
          追加
        </button>
      </div>
    </div>
  );
}

function statusLabel(status: string): string {
  if (status === "ok") return "正常";
  if (status === "not_found") return "履歴フォルダなし";
  if (status === "pending") return "未スキャン";
  return status.startsWith("error") ? "エラー" : status;
}
