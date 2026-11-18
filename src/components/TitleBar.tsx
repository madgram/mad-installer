import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState, type ReactNode } from "react";
import { t } from "../i18n";

const win = getCurrentWindow();

export default function TitleBar({ children }: { children?: ReactNode }) {
  const [focused, setFocused] = useState(true);

  useEffect(() => {
    const unFocus = win.onFocusChanged(({ payload }) => setFocused(payload));
    const sync = () => win.isMaximized().then((m) => document.documentElement.classList.toggle("is-maximized", m));
    sync();
    const unResize = win.onResized(sync);
    return () => {
      unFocus.then((f) => f());
      unResize.then((f) => f());
    };
  }, []);

  return (
    <header className={`titlebar${focused ? "" : " is-blurred"}`} data-tauri-drag-region onDoubleClick={(e) => e.target === e.currentTarget && win.toggleMaximize()}>
      <div className="traffic">
        <button type="button" className="light light-close" aria-label={t.closeWindow} onClick={() => win.close()}>
          <svg viewBox="0 0 12 12" aria-hidden><path d="M3.6 3.6l4.8 4.8M8.4 3.6l-4.8 4.8" /></svg>
        </button>
        <button type="button" className="light light-min" aria-label={t.minimize} onClick={() => win.minimize()}>
          <svg viewBox="0 0 12 12" aria-hidden><path d="M2.8 6h6.4" /></svg>
        </button>
        <button type="button" className="light light-max" aria-label={t.maximize} onClick={() => win.toggleMaximize()}>
          <svg viewBox="0 0 12 12" aria-hidden><path className="fill" d="M3.3 4.4v4.3h4.3zM8.7 7.6V3.3H4.4z" /></svg>
        </button>
      </div>
      <div className="titlebar-end">{children}</div>
    </header>
  );
}
