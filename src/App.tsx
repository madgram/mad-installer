import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, asError, on, type AccountInfo, type AppError, type CertInfo, type DeviceStatus, type InstallRecord, type Release, type Step, type TfaParams } from "./api";
import { formatDate, formatMb, t } from "./i18n";
import TitleBar from "./components/TitleBar";
import UpdatePill from "./components/UpdatePill";
import { Button, Check, Emoji, Menu, ProgressBar, Row, Spinner } from "./components/ui";
import { CertsSheet, ErrorSheet, InfoSheet, LoginSheet } from "./screens/Sheets";

const REPO_URL = "https://github.com/yar2slav/mad-installer";
const APPLE_DEVICES_STORE = "ms-windows-store://pdp/?ProductId=9NP83LWLPZ9K";
const APPLE_DEVICES_WEB = "https://apps.microsoft.com/detail/9NP83LWLPZ9K";

const openDriverUpdates = () => openUrl("ms-settings:windowsupdate-optionalupdates").catch(() => openUrl("ms-settings:windowsupdate"));
const openAppleDevices = () => openUrl(APPLE_DEVICES_STORE).catch(() => openUrl(APPLE_DEVICES_WEB));

type Screen = "home" | "installing" | "done";

const STEPS: Exclude<Step, "done">[] = ["download", "prepare", "sign", "install"];
const WEIGHTS: Record<Exclude<Step, "done">, [number, number]> = {
  download: [0, 0.35],
  prepare: [0.35, 0.45],
  sign: [0.45, 0.7],
  install: [0.7, 1],
};

const daysLeft = (iso: string) => Math.ceil((new Date(iso).getTime() - Date.now()) / 86400000);

export default function App() {
  const [platform, setPlatform] = useState("macos");
  const [screen, setScreen] = useState<Screen>("home");
  const [device, setDevice] = useState<DeviceStatus | null>(null);
  const [account, setAccount] = useState<AccountInfo | null | undefined>(undefined);
  const [releases, setReleases] = useState<Release[] | null>(null);
  const [releasesFailed, setReleasesFailed] = useState(false);
  const [selectedTag, setSelectedTag] = useState<string | null>(null);
  const [record, setRecord] = useState<InstallRecord | null>(null);
  const [step, setStep] = useState<Exclude<Step, "done">>("download");
  const [stepProgress, setStepProgress] = useState(0);
  const [error, setError] = useState<AppError | null>(null);
  const [loginOpen, setLoginOpen] = useState(false);
  const [tfa, setTfa] = useState<TfaParams | null>(null);
  const [certs, setCerts] = useState<CertInfo[] | null>(null);
  const [infoOpen, setInfoOpen] = useState(false);
  const [downloads, setDownloads] = useState<Record<string, number | "failed">>({});
  const polling = useRef(false);
  const screenRef = useRef<Screen>("home");
  screenRef.current = screen;

  useEffect(() => {
    api.platform().then((p) => {
      setPlatform(p);
      document.documentElement.dataset.platform = p === "windows" ? "windows" : "mac";
    });
    api.accountStatus().then(setAccount).catch(() => setAccount(null));
    const unlisten = [
      on<TfaParams>("tfa-required", setTfa),
      on<CertInfo[]>("certs-limit", setCerts),
      on<{ step: Step; progress: number }>("install-step", ({ step, progress }) => {
        if (step === "done") return;
        setStep(step);
        setStepProgress(progress);
      }),
      on<{ tag: string; received: number; total: number }>("download-progress", ({ tag, received, total }) => {
        const p = total ? received / total : 0;
        setDownloads((d) => ({ ...d, [tag]: p }));
        if (screenRef.current === "installing") {
          setStep("download");
          setStepProgress(p);
        }
      }),
    ];
    return () => unlisten.forEach((u) => u.then((f) => f()));
  }, []);

  const loadReleases = useCallback(() => {
    setReleasesFailed(false);
    api
      .listReleases()
      .then((list) => {
        setReleases(list);
        setSelectedTag((tag) => tag ?? (list.find((r) => !r.prerelease) ?? list[0])?.tag ?? null);
      })
      .catch(() => setReleasesFailed(true));
  }, []);
  useEffect(loadReleases, [loadReleases]);

  useEffect(() => {
    if (screen !== "home") return;
    const tick = async () => {
      if (polling.current) return;
      polling.current = true;
      try {
        setDevice(await api.deviceStatus());
      } catch {
        setDevice({ state: "noService", message: "" });
      } finally {
        polling.current = false;
      }
    };
    tick();
    const id = setInterval(tick, 1500);
    return () => clearInterval(id);
  }, [screen]);

  const udid = device?.state === "ready" ? device.device.udid : null;
  useEffect(() => {
    if (!udid) return setRecord(null);
    api.installRecord(udid).then(setRecord);
  }, [udid]);

  const release = useMemo(() => releases?.find((r) => r.tag === selectedTag) ?? null, [releases, selectedTag]);

  const prefetch = useCallback((r: Release) => {
    setDownloads((d) => ({ ...d, [r.tag]: 0 }));
    api
      .downloadRelease(r)
      .then(() => setReleases((list) => list?.map((x) => (x.tag === r.tag ? { ...x, cached: true } : x)) ?? null))
      .catch(() => setDownloads((d) => ({ ...d, [r.tag]: "failed" })));
  }, []);

  useEffect(() => {
    if (release && !release.cached && downloads[release.tag] === undefined) prefetch(release);
  }, [release, downloads, prefetch]);
  const latestTag = useMemo(() => releases?.find((r) => !r.prerelease)?.tag, [releases]);

  const startInstall = async () => {
    if (!udid || !release) return;
    setStep("download");
    setStepProgress(0);
    setScreen("installing");
    try {
      const rec = await api.install(udid, release);
      setRecord(rec);
      setReleases((list) => list?.map((r) => (r.tag === release.tag ? { ...r, cached: true } : r)) ?? null);
      setScreen("done");
    } catch (e) {
      setError(asError(e));
      setScreen("home");
    }
  };

  const ready = device?.state === "ready" && !!account && !!release;
  const renew = !!record && record.email === account?.email;
  const hint = device?.state !== "ready" ? t.hintDevice : !account ? t.hintAccount : !release ? t.hintRelease : null;
  const left = record ? daysLeft(record.expiresAt) : null;
  const signature = record && left !== null ? `${t.signedUntil(formatDate(record.expiresAt))} · ${t.daysLeft(left)}` : null;

  return (
    <div className="window">
      <TitleBar>
        <UpdatePill />
      </TitleBar>
      <main className="content">
        <div className={`screen${screen === "home" ? " is-active" : ""}`} aria-hidden={screen !== "home"}>
          <Home
            platform={platform}
            device={device}
            account={account}
            releases={releases}
            releasesFailed={releasesFailed}
            release={release}
            latestTag={latestTag}
            download={release ? downloads[release.tag] : undefined}
            onRetryDownload={() => release && prefetch(release)}
            onRetryReleases={loadReleases}
            onSelectRelease={setSelectedTag}
            onSignIn={() => setLoginOpen(true)}
            onSignOut={() => api.logout().then(() => setAccount(null))}
          />
          <div className="actions">
            <Button variant="primary" size="large" wide disabled={!ready} onClick={startInstall}>
              {renew ? t.renew : t.install}
            </Button>
            <div className={`hint${hint || signature ? " is-visible" : ""}${!hint && left !== null && left <= 2 ? " is-warn" : ""}`}>{hint ?? signature ?? " "}</div>
          </div>
          <footer className="footer">
            <button type="button" className="link" onClick={() => openUrl(REPO_URL)}>{t.sourceCode}</button>
            <span className="dot-sep">·</span>
            <button type="button" className="link" onClick={() => setInfoOpen(true)}>{t.limits}</button>
          </footer>
        </div>

        <div className={`screen${screen === "installing" ? " is-active" : ""}`} aria-hidden={screen !== "installing"}>
          <Installing release={release} step={step} progress={stepProgress} />
        </div>

        <div className={`screen${screen === "done" ? " is-active" : ""}`} aria-hidden={screen !== "done"}>
          {record && (
            <Done
              record={record}
              developerMode={device?.state === "ready" ? device.device.developerMode : null}
              onClose={() => setScreen("home")}
            />
          )}
        </div>
      </main>

      <LoginSheet
        open={loginOpen}
        tfa={tfa}
        onTfaDone={() => setTfa(null)}
        onClose={() => {
          setLoginOpen(false);
          setTfa(null);
        }}
        onSuccess={(info) => {
          setAccount(info);
          setLoginOpen(false);
          setTfa(null);
        }}
      />
      <CertsSheet certs={certs} onDone={() => setCerts(null)} />
      <InfoSheet open={infoOpen} onClose={() => setInfoOpen(false)} />
      <ErrorSheet error={error} onClose={() => setError(null)} onRetry={() => { setError(null); startInstall(); }} />
    </div>
  );
}

function Home(props: {
    platform: string;
    device: DeviceStatus | null;
    account: AccountInfo | null | undefined;
    releases: Release[] | null;
    releasesFailed: boolean;
    release: Release | null;
    latestTag: string | undefined;
    download: number | "failed" | undefined;
    onRetryDownload: () => void;
    onRetryReleases: () => void;
    onSelectRelease: (tag: string) => void;
    onSignIn: () => void;
    onSignOut: () => void;
  }) {
    const { device, account, releases, release } = props;
    return (
      <>
        <div className="hero">
          <img className="app-icon" src="/logo.png" alt="" width={84} height={84} draggable={false} />
          <h1>{t.title}</h1>
          <p className="lead">{t.subtitle}</p>
        </div>
        <section className="group">
          <DeviceRow status={device} platform={props.platform} />
          <Row
            icon={<Emoji code="1f511" />}
            title={account ? account.name ?? account.email : t.account}
            subtitle={account === undefined ? t.accountChecking : account ? (account.name ? account.email : t.account) : t.accountSub}
            tone={account ? "ok" : undefined}
            accessory={
              account === undefined ? (
                <Spinner small />
              ) : account ? (
                <Button variant="plain" size="small" onClick={props.onSignOut}>{t.signOut}</Button>
              ) : (
                <Button variant="secondary" size="small" onClick={props.onSignIn}>{t.signIn}</Button>
              )
            }
          />
          <Row
            icon={<Emoji code="1f4e6" />}
            title={release ? t.release(release.version) : props.releasesFailed ? t.releaseFailed : t.releaseLoading}
            subtitle={
              release ? (
                <>
                  {release.prerelease ? t.releasePre : release.tag === props.latestTag ? t.releaseLatest : t.releaseOlder}
                  {" · "}
                  {release.cached
                    ? t.downloaded
                    : props.download === "failed"
                      ? t.downloadFailed
                      : props.download !== undefined
                        ? t.downloading(Math.round(props.download * 100), formatMb(release.size))
                        : formatMb(release.size)}
                  {!release.cached && typeof props.download === "number" && (
                    <span className="row-progress">
                      <ProgressBar value={props.download} />
                    </span>
                  )}
                </>
              ) : undefined
            }
            accessory={
              release && !release.cached && props.download === "failed" ? (
                <Button size="small" onClick={props.onRetryDownload}>{t.releaseRetry}</Button>
              ) : props.releasesFailed ? (
                <Button size="small" onClick={props.onRetryReleases}>{t.releaseRetry}</Button>
              ) : !releases ? (
                <Spinner small />
              ) : releases.length > 0 ? (
                <Menu
                  label={t.change}
                  onSelect={props.onSelectRelease}
                  items={releases.map((r) => ({
                    key: r.tag,
                    label: t.release(r.version),
                    detail: r.tag === props.latestTag ? t.releaseLatest : r.prerelease ? t.releasePre : r.publishedAt ? formatDate(r.publishedAt) : undefined,
                    checked: r.tag === release?.tag,
                  }))}
                />
              ) : null
            }
          />
        </section>
      </>
    );
}

function DeviceRow({ status, platform }: { status: DeviceStatus | null; platform: string }) {
  const win = platform === "windows";
  if (!status) return <Row icon={<Emoji code="1f4f1" />} title={t.deviceNone} subtitle={t.deviceNoneSub} accessory={<Spinner small />} />;
  switch (status.state) {
    case "ready":
      return (
        <Row
          icon={<Emoji code="1f4f1" />}
          title={status.device.name || "iPhone"}
          subtitle={t.deviceReadySub(status.device.version)}
          tone="ok"
          accessory={<Check />}
        />
      );
    case "noService":
      return (
        <Row
          icon={<Emoji code="1f50c" />}
          title={win ? t.deviceNoServiceWin : t.deviceNoServiceMac}
          subtitle={
            <>
              {win ? t.deviceNoServiceWinSub : t.deviceNoServiceMacSub}
              {status.message && <span className="row-detail">{status.message}</span>}
            </>
          }
          tone="warn"
          accessory={win ? <Button size="small" onClick={openAppleDevices}>{t.getDriver}</Button> : undefined}
        />
      );
    case "noDriver":
      return (
        <Row
          icon={<Emoji code="1f50c" />}
          title={t.deviceNoDriver}
          subtitle={t.deviceNoDriverSub}
          tone="warn"
          accessory={<Button size="small" onClick={openDriverUpdates}>{t.openSettings}</Button>}
        />
      );
    case "locked":
      return <Row icon={<Emoji code="1f512" />} title={t.deviceLocked} subtitle={t.deviceLockedSub} accessory={<Spinner small />} />;
    case "trust":
      return <Row icon={<Emoji code="1f91d" />} title={t.deviceTrust} subtitle={t.deviceTrustSub} accessory={<Spinner small />} />;
    case "denied":
      return <Row icon={<Emoji code="1f6ab" />} title={t.deviceDenied} subtitle={t.deviceDeniedSub} tone="warn" />;
    case "error":
      return (
        <Row
          icon={<Emoji code="26a0-fe0f" />}
          title={t.deviceError}
          subtitle={
            <>
              {win ? t.deviceErrorSubWin : t.deviceErrorSub}
              {status.message && <span className="row-detail">{status.message}</span>}
            </>
          }
          tone="warn"
          accessory={<Spinner small />}
        />
      );
    default:
      return <Row icon={<Emoji code="1f50c" />} title={t.deviceNone} subtitle={t.deviceNoneSub} accessory={<Spinner small />} />;
  }
}

function Installing({ release, step, progress }: { release: Release | null; step: Exclude<Step, "done">; progress: number }) {
  const index = STEPS.indexOf(step);
  const [from, to] = WEIGHTS[step];
  const overall = from + (to - from) * progress;
  const labels = { download: t.stepDownload, prepare: t.stepPrepare, sign: t.stepSign, install: t.stepInstall };
  const r = 50;
  const c = 2 * Math.PI * r;
  return (
    <div className="installing">
      <div className="ring-wrap">
        <svg className="ring" viewBox="0 0 112 112" width="112" height="112" aria-hidden>
          <circle className="ring-track" cx="56" cy="56" r={r} />
          <circle className="ring-fill" cx="56" cy="56" r={r} strokeDasharray={c} strokeDashoffset={c * (1 - overall)} />
        </svg>
        <span className="ring-pct">{Math.round(overall * 100)}<small>%</small></span>
      </div>
      <h1>{t.installingTitle(release?.version ?? "")}</h1>
      <p className="lead">{t.installingSub}</p>
      <section className="group steps">
        {STEPS.map((s, i) => (
          <div key={s} className={`step${i < index ? " is-done" : i === index ? " is-active" : ""}`}>
            <span className="step-mark">{i < index ? <Check /> : i === index ? <Spinner small /> : <span className="step-dot" />}</span>
            <span className="step-label">{labels[s]}</span>
            {i === index && (s === "download" || s === "install") && <span className="step-pct">{Math.round(progress * 100)}%</span>}
          </div>
        ))}
        <div className="steps-bar">
          <ProgressBar value={step === "prepare" ? null : progress} />
        </div>
      </section>
    </div>
  );
}

function Done({ record, developerMode, onClose }: { record: InstallRecord; developerMode: boolean | null; onClose: () => void }) {
  const items = [
    ...(developerMode === true ? [] : [{ emoji: "1f6e0-fe0f", title: t.doneDev, sub: t.doneDevSub }]),
    { emoji: "1f91d", title: t.doneTrust, sub: t.doneTrustSub(record.email) },
    { emoji: "1f389", title: t.doneOpen, sub: t.doneOpenSub },
  ];
  return (
    <div className="done">
      <div className="done-badge"><Emoji code="2705" size={56} /></div>
      <h1>{t.doneTitle}</h1>
      <p className="lead">{t.doneSub}</p>
      <section className="group">
        {items.map((it, i) => (
          <div className="todo" key={it.title} style={{ animationDelay: `${120 + i * 70}ms` }}>
            <span className="todo-num">{i + 1}</span>
            <div className="row-text">
              <div className="row-title">{it.title}</div>
              <div className="row-subtitle">{it.sub}</div>
            </div>
            <Emoji code={it.emoji} size={24} />
          </div>
        ))}
      </section>
      <p className="note">{t.doneExpiry(formatDate(record.expiresAt))}</p>
      <div className="actions">
        <Button variant="primary" size="large" wide onClick={onClose}>{t.doneButton}</Button>
      </div>
    </div>
  );
}
