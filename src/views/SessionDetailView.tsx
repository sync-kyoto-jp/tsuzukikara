import { useEffect, useMemo, useState } from "react";
import { api, errorText, formatDate, ResumePlan, SessionDetail, sourceLabel } from "../api";
import { HighlightTerms } from "../components/Highlight";
import { maskSecrets } from "../secrets";

interface Props {
  id: number;
  query: string;
  onTagChanged: () => void;
}

export default function SessionDetailView({ id, query, onTagChanged }: Props) {
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reveal, setReveal] = useState(false);
  const [plan, setPlan] = useState<ResumePlan | null>(null);
  const [checking, setChecking] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [tagDraft, setTagDraft] = useState("");

  useEffect(() => {
    setDetail(null);
    setPlan(null);
    setConfirming(false);
    setNotice(null);
    setReveal(false);
    api
      .getSession(id)
      .then((d) => {
        setDetail(d);
        setTagDraft(d.client_tag ?? "");
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
  }, [id]);

  const terms = useMemo(() => query.split(/\s+/).filter(Boolean), [query]);
  const rendered = useMemo(() => {
    if (!detail) return { messages: [], masked: 0 };
    let masked = 0;
    const messages = detail.messages.map((m) => {
      if (reveal) return { ...m };
      const r = maskSecrets(m.text);
      masked += r.masked;
      return { ...m, text: r.text };
    });
    return { messages, masked };
  }, [detail, reveal]);

  async function checkResume() {
    setChecking(true);
    setNotice(null);
    try {
      const p = await api.resumePlan(id);
      setPlan(p);
      setConfirming(p.available);
    } catch (e) {
      setNotice(errorText(e));
    } finally {
      setChecking(false);
    }
  }

  async function runResume() {
    setConfirming(false);
    try {
      await api.resumeSession(id);
      setNotice("新しいターミナルでセッションを再開しました");
    } catch (e) {
      setNotice(`再開できませんでした: ${errorText(e)}`);
    }
  }

  async function saveTag() {
    if (!detail?.cwd) return;
    try {
      await api.setProjectTag(detail.cwd, tagDraft.trim() || null);
      setDetail({ ...detail, client_tag: tagDraft.trim() || null });
      onTagChanged();
    } catch (e) {
      setNotice(errorText(e));
    }
  }

  if (error) return <div className="error">{error}</div>;
  if (!detail) return <div className="placeholder muted">読み込み中…</div>;

  return (
    <div className="detail">
      <div className="detail-header">
        <h2>{detail.title}</h2>
        <dl className="meta">
          <dt>環境</dt>
          <dd>{sourceLabel(detail.source_key)}</dd>
          <dt>プロジェクト</dt>
          <dd className="mono">{detail.cwd ?? "—"}</dd>
          {detail.git_branch && (
            <>
              <dt>ブランチ</dt>
              <dd className="mono">{detail.git_branch}</dd>
            </>
          )}
          <dt>期間</dt>
          <dd>
            {formatDate(detail.started_at)} 〜 {formatDate(detail.updated_at)}
          </dd>
          <dt>セッションID</dt>
          <dd className="mono">{detail.external_session_id ?? "—"}</dd>
          <dt>保存元</dt>
          <dd className="mono small">{detail.file_path}</dd>
          <dt>案件タグ</dt>
          <dd className="tag-edit">
            <input
              value={tagDraft}
              placeholder="例: 顧客A / 案件X"
              onChange={(e) => setTagDraft(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && saveTag()}
              disabled={!detail.cwd}
            />
            <button onClick={saveTag} disabled={!detail.cwd || tagDraft === (detail.client_tag ?? "")}>
              保存
            </button>
            <span className="muted small">同じプロジェクトの全セッションに付きます</span>
          </dd>
        </dl>
        <div className="actions">
          <button className="primary" onClick={checkResume} disabled={checking}>
            {checking ? "確認中…" : "セッションを再開…"}
          </button>
          {plan && !plan.available && <span className="error inline">再開できません: {plan.reason}</span>}
          {notice && <span className="muted">{notice}</span>}
        </div>
        {confirming && plan && (
          <div className="confirm">
            <p>次のコマンドを新しいターミナルで実行します。よろしいですか？</p>
            <pre className="mono">{plan.command_display}</pre>
            {plan.working_dir && <p className="muted small">作業ディレクトリ: {plan.working_dir}</p>}
            <div className="actions">
              <button className="primary" onClick={runResume}>
                実行する
              </button>
              <button onClick={() => setConfirming(false)}>キャンセル</button>
            </div>
          </div>
        )}
        <div className="warning">
          ⚠ 履歴には API キーやパスワードなどの秘密情報が含まれている可能性があります。画面共有中の閲覧に注意してください。
          {rendered.masked > 0 && !reveal && <> 秘密情報らしき文字列 {rendered.masked} 件を伏せています。</>}
          <label className="reveal">
            <input type="checkbox" checked={reveal} onChange={(e) => setReveal(e.target.checked)} />
            伏せ字を解除
          </label>
        </div>
        {detail.bad_lines > 0 && (
          <div className="muted small">解析できなかった {detail.bad_lines} 行はスキップしました。</div>
        )}
      </div>
      <ol className="messages">
        {rendered.messages.map((m) => (
          <li key={m.sequence_no} className={`message role-${m.role}`}>
            <div className="message-head">
              <span className="role">{m.role === "user" ? "ユーザー" : "アシスタント"}</span>
              <span className="muted small">{formatDate(m.timestamp)}</span>
            </div>
            <div className="message-body">
              <HighlightTerms text={m.text} terms={terms} />
            </div>
          </li>
        ))}
      </ol>
    </div>
  );
}
