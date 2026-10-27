import { Map as MapIcon } from "lucide-preact";
import type { ComponentType } from "preact";
import { useEffect, useState } from "preact/hooks";
import { listModules, type ModuleInfo } from "./api";
import { GeocoderPage } from "./geocoder/GeocoderPage";
import { useMedia, useRoute, useStored } from "./hooks";
import { capitalize, iconOf, Sidebar } from "./Sidebar";
import { Topbar } from "./Topbar";

const MAP = "map";

type MapPageProps = Parameters<typeof import("./MapPage").MapPage>[0];

export function App() {
  const [modules, setModules] = useState<ModuleInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [route, go] = useRoute();
  const [wide, setWide] = useStored("geko.sidebar", true);
  const [peek, setPeek] = useState(false);
  const [drawer, setDrawer] = useState(false);
  // Below lg the rail floats over the page, collapsed unless peeked; below md it is a drawer.
  const large = useMedia("(min-width: 1024px)");
  const small = useMedia("(max-width: 767px)");
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

  const current = route || MAP;
  const module = modules?.find((m) => m.name === current);
  const onMap = current === MAP || (modules !== null && !module);
  const geocoder = modules?.some((m) => m.name === "geocoder") ?? false;
  const tiles = modules?.some((m) => m.name === "tiles" && !m.unavailable) ?? false;

  const open = small ? true : large ? wide : peek;
  const toggle = () => (large ? setWide(!wide) : setPeek(!peek));
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (!large || !(e.ctrlKey || e.metaKey) || e.key !== ".") return;
      e.preventDefault();
      setWide(!wide);
    };
    addEventListener("keydown", on);
    return () => removeEventListener("keydown", on);
  }, [large, wide]);

  const navigate = (to: string) => {
    go(to);
    setPeek(false);
    setDrawer(false);
  };

  return (
    <div class="app">
      <Sidebar modules={modules ?? []} route={current} open={open} drawer={small && drawer} onToggle={toggle} onNavigate={navigate} />
      {small && drawer && <div class="scrim" onClick={() => setDrawer(false)} />}
      <div class="column">
        <Topbar
          title={onMap ? "Map" : capitalize(current)}
          version={current === "geocoder" ? undefined : module?.version}
          Icon={onMap ? MapIcon : iconOf(current)}
          menu={small}
          onMenu={() => setDrawer(true)}
        />
        <main>
          {error && <p class="error">{error}</p>}
          {MapPage && (
            <div class="page" hidden={!onMap}>
              <MapPage geocoder={geocoder} tiles={tiles} active={onMap} />
            </div>
          )}
          {!onMap && current === "geocoder" && <GeocoderPage version={module?.version} />}
          {!onMap && current !== "geocoder" && <div class="empty">Nothing here yet.</div>}
        </main>
      </div>
    </div>
  );
}
