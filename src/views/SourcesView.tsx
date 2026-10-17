import { useCallback, useEffect, useState } from "react";
import { api, AppError, SourceView } from "../api";
import { MessageKey, useI18n } from "../i18n";

const STATUS_KEYS: Record<string, MessageKey> = {
  ok: "sources.status.ok",
  not_found: "sources.status.not_found",
  pending: "sources.status.pending",
  error: "sources.status.error",
};

export default function SourcesView({ revision, onChanged }: { revision: number; onChanged: () => void }) {
  const { t, te, formatDate } = useI18n();
  const [sources, setSources] = useState<SourceView[]>([]);
  const [wslError, setWslError] = useState<AppError | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [newRoot, setNewRoot] = useState("");
  const [pendingWsl, setPendingWsl] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .listSources()
      .then((r) => {
        setSources(r.sources);
        setWslError(r.wsl_error);
      })
      .catch(setError);
  }, []);

  useEffect(load, [load, revision]);

  async function run(action: () => Promise<unknown>) {
    setError(null);
    try {
      await action();
      load();
      onChanged();
    } catch (e) {
      setError(e);
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

  function label(s: SourceView): string {
    if (s.environment_type === "windows") return t("source.windowsLabel");
    if (s.environment_type === "wsl") return t("source.wslLabel", { distro: s.distro_name ?? "" });
    return t("source.customLabel", { path: s.root_path ?? "" });
  }

  return (
    <div className="page">
      <h2>{t("sources.title")}</h2>
      <p className="muted">{t("sources.intro")}</p>
      {error != null && <div className="error">{te(error)}</div>}
      {wslError && <div className="muted">{t("sources.wslError", { error: te(wslError) })}</div>}

      {pendingWsl && (
        <div className="confirm">
          <p>{t("sources.confirmWsl", { distro: pendingWsl })}</p>
          <div className="actions">
            <button
              className="primary"
              onClick={() => {
                const d = pendingWsl;
                setPendingWsl(null);
                run(() => api.setWslEnabled(d, true));
              }}
            >
              {t("sources.allow")}
            </button>
            <button onClick={() => setPendingWsl(null)}>{t("session.cancel")}</button>
          </div>
        </div>
      )}

      <table className="sources">
        <thead>
          <tr>
            <th>{t("sources.col.source")}</th>
            <th>{t("sources.col.location")}</th>
            <th>{t("sources.col.status")}</th>
            <th>{t("sources.col.sessions")}</th>
            <th>{t("sources.col.lastScan")}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {sources.map((s) => (
            <tr key={s.key} className={s.enabled ? "" : "disabled"}>
              <td>{label(s)}</td>
              <td className="mono small">{s.root_path ?? (s.enabled ? t("sources.checkOnScan") : "—")}</td>
              <td>
                {!s.enabled ? (
                  <span className="muted">{t("sources.disabled")}</span>
                ) : s.status ? (
                  <span className={`status status-${s.status.status}`}>
                    {STATUS_KEYS[s.status.status] ? t(STATUS_KEYS[s.status.status]) : s.status.status}
                  </span>
                ) : (
                  <span className="muted">{t("sources.notScanned")}</span>
                )}
                {s.status && s.status.error_count > 0 && (
                  <details className="errors">
                    <summary>{t("sources.errors", { count: s.status.error_count })}</summary>
                    <ul>
                      {s.status.recent_errors.map((e, i) => (
                        <li key={i} className="mono small">
                          {e.file_path ? `${e.file_path}: ` : ""}
                          {te(e)}
                        </li>
                      ))}
                    </ul>
                  </details>
                )}
              </td>
              <td>{s.status?.session_count ?? "—"}</td>
              <td>{formatDate(s.status?.last_scan_at ?? null)}</td>
              <td>
                <button onClick={() => toggle(s)}>
                  {s.environment_type === "custom"
                    ? t("sources.remove")
                    : s.enabled
                      ? t("sources.disable")
                      : t("sources.enable")}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <h3>{t("sources.addFolder")}</h3>
      <p className="muted small">{t("sources.addFolderHint")}</p>
      <div className="inline-form">
        <input
          className="mono"
          placeholder="D:\backup\claude\projects"
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
          {t("sources.add")}
        </button>
      </div>
    </div>
  );
}
