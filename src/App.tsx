import { useCallback, useEffect, useState } from "react";
import "./App.css";
import { api, ScanProgress } from "./api";
import { useI18n } from "./i18n";
import { sourceLabel } from "./labels";
import SearchView from "./views/SearchView";
import SourcesView from "./views/SourcesView";
import SettingsView from "./views/SettingsView";

type Tab = "search" | "sources" | "settings";

type ScanMessage =
  | { kind: "done"; updated: number; removed: number; errors: number }
  | { kind: "failed"; error: unknown };

export default function App() {
  const { t, te } = useI18n();
  const [tab, setTab] = useState<Tab>("search");
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  // Kept as data, not text, so it re-renders in the new language after a language switch.
  const [scanMessage, setScanMessage] = useState<ScanMessage | null>(null);
  // Bumped after each scan or index change so views reload their data.
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    const un = api.onScanProgress(setProgress);
    return () => {
      un.then((f) => f());
    };
  }, []);

  const rescan = useCallback(async () => {
    setScanning(true);
    setScanMessage(null);
    try {
      const { reports } = await api.rescan();
      setScanMessage({
        kind: "done",
        updated: reports.reduce((n, r) => n + r.updated, 0),
        removed: reports.reduce((n, r) => n + r.removed, 0),
        errors: reports.reduce((n, r) => n + r.errors, 0),
      });
    } catch (e) {
      setScanMessage({ kind: "failed", error: e });
    } finally {
      setScanning(false);
      setProgress(null);
      setRevision((r) => r + 1);
    }
  }, []);

  const bump = useCallback(() => setRevision((r) => r + 1), []);

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">Tsuzukikara</div>
        <nav className="tabs">
          {(["search", "sources", "settings"] as Tab[]).map((key) => (
            <button key={key} className={tab === key ? "active" : ""} onClick={() => setTab(key)}>
              {t(`app.tabs.${key}` as const)}
            </button>
          ))}
        </nav>
        <div className="scan">
          {scanning && progress && (
            <span className="muted">
              {sourceLabel(t, progress.source_key)} {progress.done}/{progress.total}
            </span>
          )}
          {!scanning && scanMessage?.kind === "done" && (
            <span className="muted">
              {t("app.scanSummary", { updated: scanMessage.updated, removed: scanMessage.removed })}
              {scanMessage.errors > 0 && t("app.scanErrors", { errors: scanMessage.errors })}
            </span>
          )}
          {!scanning && scanMessage?.kind === "failed" && (
            <span className="error inline">{t("app.scanFailed", { error: te(scanMessage.error) })}</span>
          )}
          <button className="primary" onClick={rescan} disabled={scanning}>
            {scanning ? t("app.scanning") : t("app.rescan")}
          </button>
        </div>
      </header>
      <main className="content">
        {tab === "search" && <SearchView revision={revision} onRescan={rescan} />}
        {tab === "sources" && <SourcesView revision={revision} onChanged={bump} />}
        {tab === "settings" && <SettingsView onChanged={bump} />}
      </main>
    </div>
  );
}
