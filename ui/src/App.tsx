import { useEffect, useState } from "preact/hooks";
import { listModules, type ModuleInfo } from "./api";

const MAP = "map";

export function App() {
  const [modules, setModules] = useState<ModuleInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState(MAP);

  useEffect(() => {
    listModules().then(setModules, (e: Error) => setError(e.message));
  }, []);

  return (
    <div class="shell">
      <nav class="tabs">
        <button class={tab === MAP ? "on" : ""} onClick={() => setTab(MAP)}>
          Map
        </button>
        {modules.map((m) => (
          <button key={m.name} class={tab === m.name ? "on" : ""} onClick={() => setTab(m.name)}>
            {m.name}
          </button>
        ))}
      </nav>
      <main>
        {error && <p class="error">{error}</p>}
        {tab === MAP ? <section class="map" /> : <section class="panel" />}
      </main>
    </div>
  );
}
