import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";

const editable = (target: EventTarget | null) =>
  target instanceof HTMLElement && (target.isContentEditable || target.matches("input, textarea, pre, pre *"));

window.addEventListener("contextmenu", (e) => {
  if (!editable(e.target)) e.preventDefault();
});

window.addEventListener(
  "keydown",
  (e) => {
    const key = e.key.toLowerCase();
    const mod = e.ctrlKey || e.metaKey;
    const blocked =
      key === "f5" ||
      key === "f7" ||
      key === "f12" ||
      (mod && ["s", "p", "f", "g", "r", "u", "j", "h", "o", "n", "t", "+", "=", "-", "0"].includes(key)) ||
      (mod && e.shiftKey && ["i", "j", "c"].includes(key)) ||
      (e.altKey && (key === "arrowleft" || key === "arrowright"));
    if (blocked) e.preventDefault();
  },
  true,
);

createRoot(document.getElementById("root")!).render(<App />);
