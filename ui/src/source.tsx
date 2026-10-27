import { createContext, type ComponentChildren } from "preact";
import { useContext, useEffect, useState } from "preact/hooks";

export type Mode = "mock" | "live";

interface Source {
  mode: Mode;
  setMode: (mode: Mode) => void;
  /** Whether geko answers at `/health`; `null` until the first answer. */
  up: boolean | null;
}

const KEY = "geko.source";
// Sample data is for developing the views: the dashboard that geko serves is always live.
const DEV = import.meta.env.DEV;

const Context = createContext<Source>({ mode: "live", setMode: () => {}, up: true });

export const useSource = () => useContext(Context);

function stored(): Mode {
  try {
    return DEV && localStorage.getItem(KEY) === "mock" ? "mock" : "live";
  } catch {
    return "live";
  }
}

export function SourceProvider({ children }: { children: ComponentChildren }) {
  const [mode, setState] = useState<Mode>(stored);
  const [up, setUp] = useState<boolean | null>(DEV ? null : true);

  useEffect(() => {
    if (!DEV) return;
    let live = true;
    const ping = async () => {
      try {
        const res = await fetch("/health", { cache: "no-store" });
        if (live) setUp(res.ok);
      } catch {
        if (live) setUp(false);
      }
    };
    ping();
    const timer = setInterval(ping, 5000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, []);

  const setMode = (next: Mode) => {
    setState(next);
    try {
      localStorage.setItem(KEY, next);
    } catch {}
  };

  return <Context.Provider value={{ mode, setMode, up }}>{children}</Context.Provider>;
}

export function ModeToggle() {
  const { mode, setMode, up } = useSource();
  if (!DEV) return null;
  const live = mode === "live";
  const hint = up === null ? "Looking for geko…" : up ? "geko answers at /health" : "geko does not answer: start it, or use mock";
  return (
    <button class="mode" role="switch" aria-checked={live} data-mode={mode} title={`${live ? "Live" : "Mock"}: ${hint}`} onClick={() => setMode(live ? "mock" : "live")}>
      <span class="thumb" />
      <span class={`opt${live ? "" : " on"}`} data-text="Mock">
        Mock
      </span>
      <span class={`opt${live ? " on" : ""}`} data-text="Live">
        Live
      </span>
    </button>
  );
}
