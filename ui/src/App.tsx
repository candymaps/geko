import { Menu } from "lucide-preact";
import type { ComponentType } from "preact";
import { useEffect, useState } from "preact/hooks";
import { useApi, type ModuleInfo } from "./api";
import { GeocoderPage } from "./geocoder/GeocoderPage";
import { TilesPage } from "./tiles/TilesPage";
import { useMedia, useRoute, useStored } from "./hooks";
import { useSource } from "./source";
import { PageHead } from "./PageHead";
import { capitalize, iconOf, Sidebar } from "./Sidebar";

const MAP = "map";

type MapPageProps = Parameters<typeof import("./MapPage").MapPage>[0];

export function App() {
  const { mode, up } = useSource();
  const { data, error } = useApi<ModuleInfo[]>("/admin/api/modules");
  const modules = data ?? (error !== null ? [] : null);
  const offline = mode === "live" && up === false;
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
    if (modules) import("./MapPage").then((m) => setMapPage(() => m.MapPage));
  }, [modules]);

  const current = route || MAP;
  const module = modules?.find((m) => m.name === current);
  const onMap = current === MAP || (modules !== null && !module);
  // The map talks to geko itself: it has no sample data.
  const geocoder = (modules?.some((m) => m.name === "geocoder") ?? false) && mode === "live";
  const tiles = (modules?.some((m) => m.name === "tiles" && !m.unavailable) ?? false) && mode === "live";

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
        <main>
          {onMap ? (
            small && (
              <button class="menu float" onClick={() => setDrawer(true)} aria-label="Menu">
                <Menu size={20} />
              </button>
            )
          ) : (
            <PageHead title={capitalize(current)} Icon={iconOf(current)} menu={small} onMenu={() => setDrawer(true)} />
          )}
          <div class="body">
            {offline && <p class="error">geko does not answer. Start it, or switch to Mock.</p>}
            {!offline && error && <p class="error">{error}</p>}
            {MapPage && (
              <div class="page" hidden={!onMap}>
                <MapPage key={mode} geocoder={geocoder} tiles={tiles} active={onMap} />
              </div>
            )}
            {!onMap && current === "geocoder" && <GeocoderPage version={module?.version} />}
            {!onMap && current === "tiles" && <TilesPage version={module?.version} />}
            {!onMap && current !== "geocoder" && current !== "tiles" && <div class="empty">Nothing here yet.</div>}
          </div>
        </main>
      </div>
    </div>
  );
}
