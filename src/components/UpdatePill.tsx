import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { t } from "../i18n";
import { Spinner } from "./ui";

export default function UpdatePill() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (import.meta.env.DEV) return;
    check()
      .then(setUpdate)
      .catch(() => setUpdate(null));
  }, []);

  if (!update) return null;

  const run = async () => {
    setFailed(false);
    setProgress(0);
    let total = 0;
    let received = 0;
    try {
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        if (e.event === "Progress") {
          received += e.data.chunkLength;
          setProgress(total ? received / total : 0);
        }
        if (e.event === "Finished") setProgress(1);
      });
      await relaunch();
    } catch {
      setProgress(null);
      setFailed(true);
    }
  };

  return (
    <button type="button" className={`update-pill${failed ? " is-failed" : ""}`} onClick={run} disabled={progress !== null}>
      {progress !== null ? (
        <>
          <Spinner small light />
          <span>{t.updating(Math.round(progress * 100))}</span>
        </>
      ) : (
        <span>{failed ? t.updateFailed : t.updateAvailable(update.version)}</span>
      )}
    </button>
  );
}
