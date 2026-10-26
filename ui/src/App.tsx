import type { ComponentType } from "preact";
import { useEffect, useState } from "preact/hooks";
import { listModules, type ModuleInfo } from "./api";

const MAP = "map";

type MapPageProps = Parameters<typeof import("./MapPage").MapPage>[0];

export function App() {
  const [modules, setModules] = useState<ModuleInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState(MAP);
  // MapLibre is the heavy part: it loads apart, once the modules are known.
  const [MapPage, setMapPage] = useState<ComponentType<MapPageProps> | null>(null);

  useEffect(() => {
    listModules().then(setModules, (e: Error) => {
      setError(e.message);
      setModules([]);
    });
  }, []);

  useEffect(() => {
    if (modules) import("./MapPage").then((m) => setMapPage(() => m.MapPage));
  }, [modules]);

  const geocoder = modules?.some((m) => m.name === "geocoder") ?? false;
  const tiles = modules?.some((m) => m.name === "tiles" && !m.unavailable) ?? false;

  return (
    <div class="shell">
      <nav class="tabs">
        <button class={tab === MAP ? "on" : ""} onClick={() => setTab(MAP)}>
          Map
        </button>
        {modules?.map((m) => (
          <button key={m.name} class={tab === m.name ? "on" : ""} onClick={() => setTab(m.name)}>
            {m.name}
          </button>
        ))}
        <a class="docs" href="/docs" title="API reference (OpenAPI)">
          API docs
        </a>
      </nav>
      <main>
        {error && <p class="error">{error}</p>}
        {MapPage && (
          <div class="page" hidden={tab !== MAP}>
            <MapPage geocoder={geocoder} tiles={tiles} active={tab === MAP} />
          </div>
        )}
        {tab !== MAP && <section class="panel" />}
      </main>
    </div>
  );
}
