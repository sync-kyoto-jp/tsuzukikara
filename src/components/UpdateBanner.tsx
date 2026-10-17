import { useEffect, useState } from "react";
import { api, UpdateStatus } from "../api";
import { useI18n } from "../i18n";

/**
 * Top-of-window notice: asks once whether to enable the opt-in update check, and announces a
 * newer release when one is found. Runs the automatic check on startup (the backend skips it
 * unless enabled and more than a day has passed).
 */
export default function UpdateBanner() {
  const { t, te } = useI18n();
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [hidden, setHidden] = useState(false);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    api.checkForUpdates(false).then(setStatus).catch(() => setStatus(null));
  }, []);

  if (!status || hidden) return null;

  async function enable(enabled: boolean) {
    try {
      await api.setUpdateCheck(enabled);
      if (enabled) setStatus(await api.checkForUpdates(true));
      else setHidden(true);
    } catch (e) {
      setError(e);
    }
  }

  if (status.enabled === null) {
    return (
      <div className="banner">
        <span>{t("update.ask")}</span>
        <div className="actions">
          <button className="primary" onClick={() => enable(true)}>{t("update.enable")}</button>
          <button onClick={() => enable(false)}>{t("update.decline")}</button>
        </div>
        {error != null && <span className="error inline">{te(error)}</span>}
      </div>
    );
  }

  if (!status.available || !status.latest_version || !status.release_url) return null;
  const { latest_version, release_url } = status;

  return (
    <div className="banner update">
      <span>{t("update.available", { version: latest_version, current: status.current_version })}</span>
      <div className="actions">
        <button className="primary" onClick={() => api.openReleasePage(release_url).catch(setError)}>
          {t("update.open")}
        </button>
        <button onClick={() => setHidden(true)}>{t("update.later")}</button>
        <button
          onClick={() =>
            api
              .skipUpdateVersion(latest_version)
              .then(() => setHidden(true))
              .catch(setError)
          }
        >
          {t("update.skip")}
        </button>
      </div>
      {error != null && <span className="error inline">{te(error)}</span>}
    </div>
  );
}
