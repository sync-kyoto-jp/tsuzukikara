import { useEffect, useMemo, useState } from "react";
import { api, ResumePlan, SessionDetail, TagView } from "../api";
import { HighlightTerms } from "../components/Highlight";
import { TagChip, TagPicker } from "../components/Tags";
import { useI18n } from "../i18n";
import { sourceLabel, titleOrUntitled } from "../labels";
import { maskSecrets } from "../secrets";

interface Props {
  id: number;
  query: string;
  /** Called after the session's name or tags change, so the list can refresh. */
  onChanged: () => void;
}

type Notice = { kind: "resumed" } | { kind: "error"; error: unknown } | { kind: "resumeFailed"; error: unknown };

export default function SessionDetailView({ id, query, onChanged }: Props) {
  const { t, te, formatDate } = useI18n();
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [reveal, setReveal] = useState(false);
  const [plan, setPlan] = useState<ResumePlan | null>(null);
  const [checking, setChecking] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState("");

  useEffect(() => {
    setDetail(null);
    setPlan(null);
    setConfirming(false);
    setNotice(null);
    setReveal(false);
    setEditingTitle(false);
    api
      .getSession(id)
      .then((d) => {
        setDetail(d);
        setLoadError(null);
      })
      .catch(setLoadError);
  }, [id]);

  const terms = useMemo(() => query.split(/\s+/).filter(Boolean), [query]);
  const maskLabel = t("session.masked");
  const rendered = useMemo(() => {
    if (!detail) return { messages: [], masked: 0 };
    let masked = 0;
    const messages = detail.messages.map((m) => {
      if (reveal) return m;
      const r = maskSecrets(m.text, maskLabel);
      masked += r.masked;
      return { ...m, text: r.text };
    });
    return { messages, masked };
  }, [detail, reveal, maskLabel]);

  async function checkResume() {
    setChecking(true);
    setNotice(null);
    try {
      const p = await api.resumePlan(id);
      setPlan(p);
      setConfirming(p.available);
    } catch (e) {
      setNotice({ kind: "error", error: e });
    } finally {
      setChecking(false);
    }
  }

  async function runResume() {
    setConfirming(false);
    try {
      await api.resumeSession(id);
      setNotice({ kind: "resumed" });
    } catch (e) {
      setNotice({ kind: "resumeFailed", error: e });
    }
  }

  async function saveTitle(title: string | null) {
    if (!detail) return;
    try {
      await api.renameSession(id, title);
      const trimmed = title?.trim() ?? "";
      setDetail({ ...detail, title: trimmed || detail.original_title, renamed: trimmed !== "" });
      setEditingTitle(false);
      onChanged();
    } catch (e) {
      setNotice({ kind: "error", error: e });
    }
  }

  async function saveTags(tags: TagView[]) {
    if (!detail) return;
    try {
      await api.setSessionTags(id, tags.map((tag) => tag.id));
      setDetail({ ...detail, tags: [...tags].sort((a, b) => a.name.localeCompare(b.name)) });
      onChanged();
    } catch (e) {
      setNotice({ kind: "error", error: e });
    }
  }

  if (loadError != null) return <div className="error detail">{te(loadError)}</div>;
  if (!detail) return <div className="placeholder muted">{t("session.loading")}</div>;
  const canEdit = detail.external_session_id != null;

  return (
    <div className="detail">
      <div className="detail-header">
        {editingTitle ? (
          <div className="title-edit">
            <input
              autoFocus
              value={titleDraft}
              maxLength={200}
              onChange={(e) => setTitleDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveTitle(titleDraft);
                if (e.key === "Escape") setEditingTitle(false);
              }}
            />
            <div className="actions">
              <button className="primary" onClick={() => saveTitle(titleDraft)} disabled={!titleDraft.trim()}>
                {t("session.save")}
              </button>
              <button onClick={() => setEditingTitle(false)}>{t("session.cancel")}</button>
              {detail.renamed && <button onClick={() => saveTitle(null)}>{t("session.resetTitle")}</button>}
            </div>
            <p className="muted small">{t("session.renameHint")}</p>
          </div>
        ) : (
          <div className="title-row">
            <h2>{titleOrUntitled(t, detail.title)}</h2>
            {canEdit && (
              <button
                className="icon-button"
                title={t("session.rename")}
                aria-label={t("session.rename")}
                onClick={() => {
                  setTitleDraft(detail.title);
                  setEditingTitle(true);
                }}
              >
                ✎
              </button>
            )}
          </div>
        )}
        {detail.renamed && !editingTitle && (
          <p className="muted small">
            {t("session.originalTitle", { title: titleOrUntitled(t, detail.original_title) })}
          </p>
        )}

        <div className="tag-row detail-tags">
          {detail.tags.map((tag) => (
            <TagChip
              key={tag.id}
              tag={tag}
              onRemove={canEdit ? () => saveTags(detail.tags.filter((x) => x.id !== tag.id)) : undefined}
            />
          ))}
          {canEdit && (
            <TagPicker assigned={detail.tags} onChange={saveTags} onError={(e) => setNotice({ kind: "error", error: e })} />
          )}
        </div>

        <dl className="meta">
          <dt>{t("session.environment")}</dt>
          <dd>{sourceLabel(t, detail.source_key)}</dd>
          <dt>{t("session.project")}</dt>
          <dd className="mono">{detail.cwd ?? "—"}</dd>
          {detail.git_branch && (
            <>
              <dt>{t("session.branch")}</dt>
              <dd className="mono">{detail.git_branch}</dd>
            </>
          )}
          <dt>{t("session.period")}</dt>
          <dd>
            {formatDate(detail.started_at)} 〜 {formatDate(detail.updated_at)}
          </dd>
          <dt>{t("session.sessionId")}</dt>
          <dd className="mono">{detail.external_session_id ?? "—"}</dd>
          <dt>{t("session.file")}</dt>
          <dd className="mono small">{detail.file_path}</dd>
        </dl>

        <div className="actions">
          <button className="primary" onClick={checkResume} disabled={checking}>
            {checking ? t("session.checking") : t("session.resume")}
          </button>
          {plan && !plan.available && plan.reason && (
            <span className="error inline">{t("session.cannotResume", { reason: te(plan.reason) })}</span>
          )}
          {notice?.kind === "resumed" && <span className="muted">{t("session.resumed")}</span>}
          {notice?.kind === "resumeFailed" && (
            <span className="error inline">{t("session.resumeFailed", { error: te(notice.error) })}</span>
          )}
          {notice?.kind === "error" && <span className="error inline">{te(notice.error)}</span>}
        </div>
        {confirming && plan && (
          <div className="confirm">
            <p>{t("session.confirmResume")}</p>
            <pre className="mono">{plan.command_display}</pre>
            {plan.working_dir && <p className="muted small">{t("session.workingDir", { dir: plan.working_dir })}</p>}
            <p className="muted small">{t("session.firstRunNote")}</p>
            <div className="actions">
              <button className="primary" onClick={runResume}>
                {t("session.run")}
              </button>
              <button onClick={() => setConfirming(false)}>{t("session.cancel")}</button>
            </div>
          </div>
        )}
        <div className="warning">
          ⚠ {t("session.secretWarning")}
          {rendered.masked > 0 && !reveal && <> {t("session.maskedCount", { count: rendered.masked })}</>}
          <label className="reveal">
            <input type="checkbox" checked={reveal} onChange={(e) => setReveal(e.target.checked)} />
            {t("session.reveal")}
          </label>
        </div>
        {detail.bad_lines > 0 && (
          <div className="muted small">{t("session.skippedLines", { count: detail.bad_lines })}</div>
        )}
      </div>
      <ol className="messages">
        {rendered.messages.map((m) => (
          <li key={m.sequence_no} className={`message role-${m.role}`}>
            <div className="message-head">
              <span className="role">{m.role === "user" ? t("session.roleUser") : t("session.roleAssistant")}</span>
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
