import { useEffect, useState } from "react";
import { api, AppInfo, errorText, Settings } from "../api";

export default function SettingsView({ onChanged }: { onChanged: () => void }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [excludes, setExcludes] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    api.getSettings().then((s) => {
      setSettings(s);
      setExcludes(s.excluded_paths.join("\n"));
    });
    api.appInfo().then(setInfo);
  }, []);

  async function saveExcludes() {
    try {
      const s = await api.setExcludedPaths(excludes.split(/\r?\n/));
      setSettings(s);
      setExcludes(s.excluded_paths.join("\n"));
      setMessage("除外設定を保存しました。次回の再スキャンで反映されます。");
    } catch (e) {
      setMessage(errorText(e));
    }
  }

  async function deleteIndex() {
    setConfirmDelete(false);
    try {
      await api.deleteIndex();
      setMessage("インデックスを削除しました。履歴ファイル本体は変更していません。");
      onChanged();
    } catch (e) {
      setMessage(errorText(e));
    }
  }

  return (
    <div className="page">
      <h2>設定</h2>
      {message && <div className="notice">{message}</div>}

      <section>
        <h3>プライバシー</h3>
        <ul className="policy">
          <li>このアプリは外部へ一切通信しません（テレメトリ・分析・クラウド API なし）。画面からの外部通信も CSP で遮断しています。</li>
          <li>履歴ファイルは読み取り専用で扱い、変更・削除しません。</li>
          <li>ツールの実行結果（コマンド出力・ファイル内容など）と思考過程はインデックスに含めません。</li>
          <li>ただし会話本文には秘密情報が含まれ得るため、インデックスにも複製されます。共有 PC では注意してください。</li>
        </ul>
      </section>

      <section>
        <h3>除外するフォルダ・ファイル</h3>
        <p className="muted small">1 行に 1 パス。指定したフォルダ配下のセッションはインデックスから外れます。</p>
        <textarea
          className="mono"
          rows={5}
          value={excludes}
          onChange={(e) => setExcludes(e.target.value)}
          placeholder={"例: C:\\Users\\me\\.claude\\projects\\C--work-secret-client"}
        />
        <div className="actions">
          <button onClick={saveExcludes} disabled={!settings || excludes === settings.excluded_paths.join("\n")}>
            保存
          </button>
        </div>
      </section>

      <section>
        <h3>データ</h3>
        <dl className="meta">
          <dt>保存先</dt>
          <dd className="mono small">{info?.data_dir ?? "—"}</dd>
          <dt>インデックス</dt>
          <dd className="mono small">{info?.db_path ?? "—"}</dd>
        </dl>
        {!confirmDelete ? (
          <button className="danger" onClick={() => setConfirmDelete(true)}>
            インデックスを削除…
          </button>
        ) : (
          <div className="confirm">
            <p>検索インデックス（会話本文の複製）をすべて削除します。案件タグと設定は残ります。よろしいですか？</p>
            <div className="actions">
              <button className="danger" onClick={deleteIndex}>削除する</button>
              <button onClick={() => setConfirmDelete(false)}>キャンセル</button>
            </div>
          </div>
        )}
      </section>

      <section>
        <h3>バージョン情報</h3>
        <p>Tsuzukikara {info?.version ?? ""} — MIT License（予定）</p>
      </section>
    </div>
  );
}
