import { useEffect, useRef, useState, type FormEvent } from "react";
import { api, asError, type AccountInfo, type AppError, type CertInfo, type TfaParams } from "../api";
import { formatDate, t } from "../i18n";
import { Button, Emoji, Sheet, Toggle } from "../components/ui";

const friendly = (e: AppError) => t.errors[e.kind] ?? t.errors.unknown;

export function LoginSheet({
  open,
  tfa,
  onTfaDone,
  onClose,
  onSuccess,
}: {
  open: boolean;
  tfa: TfaParams | null;
  onTfaDone: () => void;
  onClose: () => void;
  onSuccess: (info: AccountInfo) => void;
}) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const emailRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setError(null);
      setPassword("");
      setTimeout(() => emailRef.current?.focus(), 80);
    }
  }, [open]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!email.trim() || !password) return;
    setBusy(true);
    setError(null);
    try {
      const info = await api.login(email, password);
      setPassword("");
      onSuccess(info);
    } catch (err) {
      const ae = asError(err);
      if (!/cancel|abort/i.test(ae.message)) setError(friendly(ae));
      onTfaDone();
    } finally {
      setBusy(false);
    }
  };

  const cancel = () => {
    if (tfa) api.answerTfa({ type: "cancel" });
    setPassword("");
    onClose();
  };

  return (
    <Sheet open={open} onClose={busy ? undefined : cancel}>
      {tfa ? (
        <TfaForm params={tfa} onCancel={cancel} onSubmitted={onTfaDone} />
      ) : (
        <form onSubmit={submit} className="sheet-body">
          <img className="app-icon sheet-icon" src="/logo.png" alt="" width={56} height={56} draggable={false} />
          <h2>{t.loginTitle}</h2>
          <p className="sheet-sub">{t.loginSub}</p>
          <div className="fields">
            <input
              ref={emailRef}
              className="field"
              type="email"
              autoComplete="username"
              autoCapitalize="off"
              spellCheck={false}
              placeholder={t.email}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              disabled={busy}
            />
            <input
              className="field"
              type="password"
              autoComplete="current-password"
              placeholder={t.password}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={busy}
            />
          </div>
          <div className={`form-error${error ? " is-visible" : ""}`}>{error ?? " "}</div>
          <p className="fine">
            <Emoji code="1f512" size={14} /> {t.passwordNote}
          </p>
          <div className="sheet-buttons">
            <Button onClick={cancel} disabled={busy}>{t.cancel}</Button>
            <Button variant="primary" type="submit" loading={busy} disabled={!email.trim() || !password}>{t.next}</Button>
          </div>
        </form>
      )}
    </Sheet>
  );
}

function TfaForm({ params, onCancel, onSubmitted }: { params: TfaParams; onCancel: () => void; onSubmitted: () => void }) {
  const [code, setCode] = useState("");
  const [sent, setSent] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setCode("");
    setSent(false);
    setTimeout(() => inputRef.current?.focus(), 60);
  }, [params]);

  const selected = params.numbers.find((n) => n.id === params.selectedNumberId);
  const subtitle = params.unknown ? t.tfaUnknown : params.sms && selected ? t.tfaSms(selected.numberWithDialCode) : t.tfaDevices;

  const send = (answer: Parameters<typeof api.answerTfa>[0]) => {
    setSent(true);
    api.answerTfa(answer);
    onSubmitted();
  };

  const onChange = (v: string) => {
    const digits = v.replace(/\D/g, "").slice(0, 6);
    setCode(digits);
    if (digits.length === 6 && !sent) send({ type: "code", code: digits });
  };

  return (
    <div className="sheet-body">
      <div className="sheet-emoji"><Emoji code="1f4f2" size={48} /></div>
      <h2>{t.tfaTitle}</h2>
      <p className="sheet-sub">{subtitle}</p>
      {!params.unknown && (
        <label className="code" onClick={() => inputRef.current?.focus()}>
          <input
            ref={inputRef}
            className="code-input"
            inputMode="numeric"
            autoComplete="one-time-code"
            value={code}
            onChange={(e) => onChange(e.target.value)}
            disabled={sent}
          />
          {Array.from({ length: 6 }, (_, i) => (
            <span key={i} className={`code-cell${i === code.length && !sent ? " is-caret" : ""}${code[i] ? " is-filled" : ""}`}>
              {code[i] ?? ""}
            </span>
          ))}
        </label>
      )}
      <div className={`form-error${params.lastError ? " is-visible" : ""}`}>{params.lastError ?? " "}</div>
      <div className="tfa-links">
        {!params.unknown && <button type="button" className="link" onClick={() => send({ type: "resend" })}>{t.tfaResend}</button>}
        {(params.sms || params.unknown) && <button type="button" className="link" onClick={() => send({ type: "devices" })}>{t.tfaToDevices}</button>}
        {params.numbers
          .filter((n) => n.id !== params.selectedNumberId || !params.sms)
          .map((n) => (
            <button type="button" key={n.id} className="link" onClick={() => send({ type: "sms", id: n.id })}>
              {t.tfaToSms(n.numberWithDialCode)}
            </button>
          ))}
      </div>
      <div className="sheet-buttons">
        <Button onClick={onCancel}>{t.cancel}</Button>
        <Button variant="primary" loading={sent} disabled={code.length !== 6} onClick={() => send({ type: "code", code })}>{t.next}</Button>
      </div>
    </div>
  );
}

export function CertsSheet({ certs, onDone }: { certs: CertInfo[] | null; onDone: () => void }) {
  const [picked, setPicked] = useState<string[]>([]);
  useEffect(() => setPicked([]), [certs]);
  const answer = (serials: string[] | null) => {
    api.answerCerts(serials);
    onDone();
  };
  return (
    <Sheet open={!!certs} width={420}>
      <div className="sheet-body">
        <div className="sheet-emoji"><Emoji code="26a0-fe0f" size={48} /></div>
        <h2>{t.certsTitle}</h2>
        <p className="sheet-sub">{t.certsSub}</p>
        <section className="group group-tight">
          {certs?.map((c) => (
            <label className="cert" key={c.serial}>
              <Toggle checked={picked.includes(c.serial)} onChange={(v) => setPicked((p) => (v ? [...p, c.serial] : p.filter((s) => s !== c.serial)))} />
              <div className="row-text">
                <div className="row-title">{c.machine ?? c.name ?? t.certsUnknown}</div>
                <div className="row-subtitle">{c.expires ? formatDate(c.expires) : c.serial}</div>
              </div>
            </label>
          ))}
        </section>
        <div className="sheet-buttons">
          <Button onClick={() => answer(null)}>{t.cancel}</Button>
          <Button variant="destructive" disabled={!picked.length} onClick={() => answer(picked)}>{t.certsRevoke}</Button>
        </div>
      </div>
    </Sheet>
  );
}

export function InfoSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const items = [
    { e: "23f3", title: t.info7, sub: t.info7Sub },
    { e: "1f4f1", title: t.info3, sub: t.info3Sub },
    { e: "1f4e6", title: t.info10, sub: t.info10Sub },
    { e: "1f512", title: t.infoPrivacy, sub: t.infoPrivacySub },
  ];
  return (
    <Sheet open={open} onClose={onClose} width={440}>
      <div className="sheet-body">
        <h2>{t.infoTitle}</h2>
        <div className="info-list">
          {items.map((it) => (
            <div className="info-item" key={it.title}>
              <Emoji code={it.e} size={28} />
              <div className="row-text">
                <div className="row-title">{it.title}</div>
                <div className="row-subtitle">{it.sub}</div>
              </div>
            </div>
          ))}
        </div>
        <div className="sheet-buttons sheet-buttons-single">
          <Button variant="primary" wide onClick={onClose}>{t.ok}</Button>
        </div>
      </div>
    </Sheet>
  );
}

export function ErrorSheet({ error, onClose, onRetry }: { error: AppError | null; onClose: () => void; onRetry: () => void }) {
  const [details, setDetails] = useState(false);
  useEffect(() => setDetails(false), [error]);
  return (
    <Sheet open={!!error} onClose={onClose} width={400}>
      <div className="sheet-body">
        <div className="sheet-emoji"><Emoji code="26a0-fe0f" size={48} /></div>
        <h2>{t.errorTitle}</h2>
        <p className="sheet-sub">{error ? friendly(error) : ""}</p>
        {error && (
          <>
            <button type="button" className="link link-small" onClick={() => setDetails((d) => !d)}>{t.errorDetails}</button>
            <pre className={`details${details ? " is-open" : ""}`}>{error.message}</pre>
          </>
        )}
        <div className="sheet-buttons">
          <Button onClick={onClose}>{t.close}</Button>
          <Button variant="primary" onClick={onRetry}>{t.errorRetry}</Button>
        </div>
      </div>
    </Sheet>
  );
}
