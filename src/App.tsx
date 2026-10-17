import { useCallback, useEffect, useState } from "react";
import "./App.css";
import { api, errorText, ScanProgress, sourceLabel } from "./api";
import SearchView from "./views/SearchView";
import SourcesView from "./views/SourcesView";
import SettingsView from "./views/SettingsView";

type Tab = "search" | "sources" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("search");
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [scanMessage, setScanMessage] = useState<string | null>(null);
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
      const updated = reports.reduce((n, r) => n + r.updated, 0);
      const removed = reports.reduce((n, r) => n + r.removed, 0);
      const errors = reports.reduce((n, r) => n + r.errors, 0);
      setScanMessage(
        `更新 ${updated} 件 / 削除 ${removed} 件` + (errors ? ` / エラー ${errors} 件（ソース管理で確認）` : ""),
      );
    } catch (e) {
      setScanMessage(`スキャンに失敗しました: ${errorText(e)}`);
    } finally {
      setScanning(false);
      setProgress(null);
      setRevision((r) => r + 1);
    }
  }, []);

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">Tsuzukikara</div>
        <nav className="tabs">
          <button className={tab === "search" ? "active" : ""} onClick={() => setTab("search")}>
            検索
          </button>
          <button className={tab === "sources" ? "active" : ""} onClick={() => setTab("sources")}>
            ソース管理
          </button>
          <button className={tab === "settings" ? "active" : ""} onClick={() => setTab("settings")}>
            設定
          </button>
        </nav>
        <div className="scan">
          {scanning && progress && (
            <span className="muted">
              {sourceLabel(progress.source_key)} {progress.done}/{progress.total}
            </span>
          )}
          {!scanning && scanMessage && <span className="muted">{scanMessage}</span>}
          <button className="primary" onClick={rescan} disabled={scanning}>
            {scanning ? "スキャン中…" : "再スキャン"}
          </button>
        </div>
      </header>
      <main className="content">
        {tab === "search" && <SearchView revision={revision} onRescan={rescan} />}
        {tab === "sources" && <SourcesView revision={revision} onChanged={() => setRevision((r) => r + 1)} />}
        {tab === "settings" && <SettingsView onChanged={() => setRevision((r) => r + 1)} />}
      </main>
    </div>
  );
}
