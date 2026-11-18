import { useEffect, useRef, useState, type ReactNode } from "react";

export function Emoji({ code, size = 22 }: { code: string; size?: number }) {
  return <img className="emoji" src={`/emoji/${code}.png`} width={size} height={size} alt="" draggable={false} />;
}

type ButtonProps = {
  children: ReactNode;
  onClick?: () => void;
  variant?: "primary" | "secondary" | "plain" | "destructive";
  size?: "regular" | "large" | "small";
  disabled?: boolean;
  loading?: boolean;
  wide?: boolean;
  type?: "button" | "submit";
};

export function Button({ children, onClick, variant = "secondary", size = "regular", disabled, loading, wide, type = "button" }: ButtonProps) {
  return (
    <button
      type={type}
      className={`btn btn-${variant} btn-${size}${wide ? " btn-wide" : ""}${loading ? " is-loading" : ""}`}
      onClick={onClick}
      disabled={disabled || loading}
    >
      <span className="btn-label">{children}</span>
      {loading && <Spinner small light={variant === "primary" || variant === "destructive"} />}
    </button>
  );
}

export function Spinner({ small, light }: { small?: boolean; light?: boolean }) {
  return (
    <span className={`spinner${small ? " spinner-small" : ""}${light ? " spinner-light" : ""}`} aria-hidden>
      {Array.from({ length: 12 }, (_, i) => (
        <i key={i} style={{ transform: `rotate(${i * 30}deg)`, opacity: 0.15 + (0.85 * i) / 11 }} />
      ))}
    </span>
  );
}

export function Check() {
  return (
    <svg className="check" viewBox="0 0 20 20" width="20" height="20" aria-hidden>
      <circle cx="10" cy="10" r="10" />
      <path d="M5.8 10.3l2.7 2.7 5.7-6" />
    </svg>
  );
}

export function Chevron({ open }: { open?: boolean }) {
  return (
    <svg className={`chevron${open ? " is-open" : ""}`} viewBox="0 0 10 6" width="9" height="6" aria-hidden>
      <path d="M1 1l4 4 4-4" />
    </svg>
  );
}

export function Sheet({ open, onClose, children, width = 380 }: { open: boolean; onClose?: () => void; children: ReactNode; width?: number }) {
  const [mounted, setMounted] = useState(open);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (open) {
      setMounted(true);
      const id = requestAnimationFrame(() => requestAnimationFrame(() => setVisible(true)));
      return () => cancelAnimationFrame(id);
    }
    setVisible(false);
    const id = setTimeout(() => setMounted(false), 260);
    return () => clearTimeout(id);
  }, [open]);
  useEffect(() => {
    if (!open || !onClose) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);
  if (!mounted) return null;
  return (
    <div className={`sheet-backdrop${visible ? " is-visible" : ""}`} onMouseDown={(e) => e.target === e.currentTarget && onClose?.()}>
      <div className="sheet" style={{ width }} role="dialog" aria-modal>
        {children}
      </div>
    </div>
  );
}

export type MenuItem = { key: string; label: string; detail?: string; checked?: boolean };

export function Menu({ items, onSelect, label }: { items: MenuItem[]; onSelect: (key: string) => void; label: ReactNode }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const key = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <div className="menu-wrap" ref={ref}>
      <button type="button" className={`popup${open ? " is-open" : ""}`} onClick={() => setOpen((o) => !o)}>
        <span>{label}</span>
        <Chevron open={open} />
      </button>
      <div className={`menu${open ? " is-open" : ""}`} role="menu">
        {items.map((item) => (
          <button
            type="button"
            key={item.key}
            className="menu-item"
            role="menuitemradio"
            aria-checked={item.checked}
            onClick={() => {
              setOpen(false);
              onSelect(item.key);
            }}
          >
            <span className="menu-check">{item.checked ? "✓" : ""}</span>
            <span className="menu-label">{item.label}</span>
            {item.detail && <span className="menu-detail">{item.detail}</span>}
          </button>
        ))}
      </div>
    </div>
  );
}

export function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button type="button" role="checkbox" aria-checked={checked} className={`checkbox${checked ? " is-on" : ""}`} onClick={() => onChange(!checked)}>
      <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden>
        <path d="M2.5 6.3l2.2 2.2 4.8-5" />
      </svg>
    </button>
  );
}

export function ProgressBar({ value }: { value: number | null }) {
  return (
    <div className={`progress${value === null ? " is-indeterminate" : ""}`}>
      <div className="progress-fill" style={value === null ? undefined : { transform: `scaleX(${Math.max(0, Math.min(1, value))})` }} />
    </div>
  );
}

export function Row({ icon, title, subtitle, accessory, tone }: { icon: ReactNode; title: ReactNode; subtitle?: ReactNode; accessory?: ReactNode; tone?: "warn" | "ok" }) {
  return (
    <div className={`row${tone ? ` row-${tone}` : ""}`}>
      <div className="row-icon">{icon}</div>
      <div className="row-text">
        <div className="row-title" key={String(title)}>{title}</div>
        {subtitle && <div className="row-subtitle">{subtitle}</div>}
      </div>
      {accessory && <div className="row-accessory">{accessory}</div>}
    </div>
  );
}
