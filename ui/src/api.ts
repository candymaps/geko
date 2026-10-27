import { useEffect, useRef, useState } from "preact/hooks";
import { mockGet } from "./mock";
import { useSource, type Mode } from "./source";

export interface ModuleInfo {
  name: string;
  version: string;
  unavailable: string | null;
}

async function get<T>(path: string, mode: Mode): Promise<T> {
  if (mode === "mock") return mockGet(path) as T;
  const res = await fetch(path);
  if (!res.ok) throw new Error(`${path} answered ${res.status}`);
  return res.json();
}

/** `path` read from geko, or from the sample data in mock mode; `every` polls it, in milliseconds. */
export function useApi<T>(path: string, every = 0): { data: T | null; error: string | null } {
  const { mode, up } = useSource();
  const [state, setState] = useState<{ data: T | null; error: string | null }>({ data: null, error: null });
  const last = useRef("");
  useEffect(() => {
    let live = true;
    const key = `${mode} ${path}`;
    // What was read in another mode, or from another route, is not an answer to this one.
    if (last.current !== key) setState({ data: null, error: null });
    last.current = key;
    const load = async () => {
      try {
        const data = await get<T>(path, mode);
        if (live) setState({ data, error: null });
      } catch (e) {
        if (live) setState((s) => ({ ...s, error: (e as Error).message }));
      }
    };
    load();
    const timer = every ? setInterval(load, every) : 0;
    return () => {
      live = false;
      clearInterval(timer);
    };
    // A geko that comes up is read again at once, not at the next poll.
  }, [path, every, mode, mode === "live" && up]);
  return state;
}
