import { useEffect, useState } from "preact/hooks";

export function useMedia(query: string): boolean {
  const [matches, setMatches] = useState(() => matchMedia(query).matches);
  useEffect(() => {
    const list = matchMedia(query);
    const on = () => setMatches(list.matches);
    list.addEventListener("change", on);
    return () => list.removeEventListener("change", on);
  }, [query]);
  return matches;
}

/** The route is the hash (`#/tiles`): the host serves one page and needs no fallback for deep links. */
export function useRoute(): [string, (route: string) => void] {
  const read = () => location.hash.replace(/^#\/?/, "");
  const [route, setRoute] = useState(read);
  useEffect(() => {
    const on = () => setRoute(read());
    addEventListener("hashchange", on);
    return () => removeEventListener("hashchange", on);
  }, []);
  return [route, (next) => (location.hash = `/${next}`)];
}

/** The selected tab of a view, kept so that it is still there when coming back to the view. */
export function useTab(key: string, ids: string[]): [string, (id: string) => void] {
  const [tab, setTab] = useState(() => {
    try {
      const saved = localStorage.getItem(key);
      if (saved && ids.includes(saved)) return saved;
    } catch {}
    return ids[0];
  });
  return [
    tab,
    (id) => {
      setTab(id);
      try {
        localStorage.setItem(key, id);
      } catch {}
    },
  ];
}

export function useStored(key: string, initial: boolean): [boolean, (value: boolean) => void] {
  const [value, setValue] = useState(() => {
    try {
      const raw = localStorage.getItem(key);
      return raw === null ? initial : raw === "1";
    } catch {
      return initial;
    }
  });
  return [
    value,
    (next) => {
      setValue(next);
      try {
        localStorage.setItem(key, next ? "1" : "0");
      } catch {}
    },
  ];
}
