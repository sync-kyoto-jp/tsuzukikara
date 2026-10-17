import { useCallback, useEffect, useState } from "react";
import { api, AppInfo, Settings, TagUsage } from "../api";
import { ColorPicker, TagChip } from "../components/Tags";
import { useI18n } from "../i18n";
import { LOCALES } from "../i18n/locales";

type Message = "excludesSaved" | "indexDeleted" | { error: unknown };

export default function SettingsView({ onChanged }: { onChanged: () => void }) {
  const { t, te, preference, setPreference } = useI18n();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [excludes, setExcludes] = useState("");
  const [message, setMessage] = useState<Message | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const showError = useCallback((e: unknown) => setMessage({ error: e }), []);

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
      setMessage("excludesSaved");
    } catch (e) {
      showError(e);
    }
  }

  async function deleteIndex() {
    setConfirmDelete(false);
    try {
      await api.deleteIndex();
      setMessage("indexDeleted");
      onChanged();
    } catch (e) {
      showError(e);
    }
  }

  return (
    <div className="page">
      <h2>{t("settings.title")}</h2>
      {message && (
        <div className={typeof message === "string" ? "notice" : "notice error"}>
          {message === "excludesSaved"
            ? t("settings.excludesSaved")
            : message === "indexDeleted"
              ? t("settings.indexDeleted")
              : te(message.error)}
        </div>
      )}

      <section>
        <h3>{t("settings.language")}</h3>
        <select value={preference ?? ""} onChange={(e) => setPreference(e.target.value || null).catch(showError)}>
          <option value="">{t("settings.languageSystem")}</option>
          {LOCALES.map((l) => (
            <option key={l.code} value={l.code}>
              {l.nativeName}
            </option>
          ))}
        </select>
      </section>

      <TagManager onChanged={onChanged} onError={showError} />

      <section>
        <h3>{t("settings.privacy")}</h3>
        <ul className="policy">
          <li>{t("settings.privacy.noNetwork")}</li>
          <li>{t("settings.privacy.readOnly")}</li>
          <li>{t("settings.privacy.noToolOutput")}</li>
          <li>{t("settings.privacy.indexCopies")}</li>
        </ul>
      </section>

      <section>
        <h3>{t("settings.excludes")}</h3>
        <p className="muted small">{t("settings.excludesHint")}</p>
        <textarea
          className="mono"
          rows={5}
          value={excludes}
          onChange={(e) => setExcludes(e.target.value)}
          placeholder={"C:\\Users\\me\\.claude\\projects\\C--work-secret-client"}
        />
        <div className="actions">
          <button onClick={saveExcludes} disabled={!settings || excludes === settings.excluded_paths.join("\n")}>
            {t("settings.save")}
          </button>
        </div>
      </section>

      <section>
        <h3>{t("settings.data")}</h3>
        <dl className="meta">
          <dt>{t("settings.dataDir")}</dt>
          <dd className="mono small">{info?.data_dir ?? "—"}</dd>
          <dt>{t("settings.indexFile")}</dt>
          <dd className="mono small">{info?.db_path ?? "—"}</dd>
        </dl>
        {!confirmDelete ? (
          <button className="danger" onClick={() => setConfirmDelete(true)}>
            {t("settings.deleteIndex")}
          </button>
        ) : (
          <div className="confirm">
            <p>{t("settings.confirmDeleteIndex")}</p>
            <div className="actions">
              <button className="danger" onClick={deleteIndex}>{t("settings.delete")}</button>
              <button onClick={() => setConfirmDelete(false)}>{t("session.cancel")}</button>
            </div>
          </div>
        )}
      </section>

      <section>
        <h3>{t("settings.about")}</h3>
        <p>Tsuzukikara {info?.version ?? ""} — MIT License</p>
      </section>
    </div>
  );
}

function TagManager({ onChanged, onError }: { onChanged: () => void; onError: (e: unknown) => void }) {
  const { t } = useI18n();
  const [tags, setTags] = useState<TagUsage[]>([]);
  const [editing, setEditing] = useState<{ id: number; name: string; color: string } | null>(null);
  const [deleting, setDeleting] = useState<TagUsage | null>(null);

  const load = useCallback(() => {
    api.listTags().then(setTags).catch(onError);
  }, [onError]);

  useEffect(load, [load]);

  async function save() {
    if (!editing) return;
    try {
      await api.updateTag(editing.id, editing.name, editing.color);
      setEditing(null);
      load();
      onChanged();
    } catch (e) {
      onError(e);
    }
  }

  async function remove(tag: TagUsage) {
    setDeleting(null);
    try {
      await api.deleteTag(tag.id);
      load();
      onChanged();
    } catch (e) {
      onError(e);
    }
  }

  return (
    <section>
      <h3>{t("tags.manage")}</h3>
      <p className="muted small">{t("tags.manageHint")}</p>
      {tags.length === 0 && <p className="muted">{t("tags.noTags")}</p>}
      <ul className="tag-list">
        {tags.map((tag) => (
          <li key={tag.id}>
            {editing?.id === tag.id ? (
              <div className="tag-edit">
                <input
                  value={editing.name}
                  maxLength={40}
                  onChange={(e) => setEditing({ ...editing, name: e.target.value })}
                  onKeyDown={(e) => e.key === "Enter" && save()}
                />
                <ColorPicker value={editing.color} onChange={(color) => setEditing({ ...editing, color })} />
                <div className="actions">
                  <TagChip tag={{ id: tag.id, name: editing.name || tag.name, color: editing.color }} />
                  <button className="primary" onClick={save} disabled={!editing.name.trim()}>
                    {t("settings.save")}
                  </button>
                  <button onClick={() => setEditing(null)}>{t("session.cancel")}</button>
                </div>
              </div>
            ) : (
              <div className="tag-line">
                <button className="link-button" onClick={() => setEditing({ id: tag.id, name: tag.name, color: tag.color })}>
                  <TagChip tag={tag} />
                </button>
                <span className="muted small">{t("tags.sessions", { count: tag.session_count })}</span>
                <button className="small-button" onClick={() => setDeleting(tag)}>
                  {t("tags.delete")}
                </button>
              </div>
            )}
            {deleting?.id === tag.id && (
              <div className="confirm">
                <p>{t("tags.confirmDelete", { name: tag.name, count: tag.session_count })}</p>
                <div className="actions">
                  <button className="danger" onClick={() => remove(tag)}>{t("tags.delete")}</button>
                  <button onClick={() => setDeleting(null)}>{t("session.cancel")}</button>
                </div>
              </div>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
