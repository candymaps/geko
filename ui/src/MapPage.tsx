import maplibregl, { type Map as MlMap, type Marker } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { useEffect, useRef, useState } from "preact/hooks";
import { loadBase } from "./base";
import { credits, query, type Feature } from "./geocoder";

const HINT = "Type to autocomplete, Enter to search. Click the map to reverse geocode; drag the pin to move the focus.";

const stored = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const store = (key: string, value: string) => {
  try {
    localStorage.setItem(key, value);
  } catch {}
};

interface Props {
  geocoder: boolean;
  tiles: boolean;
  active: boolean;
}

export function MapPage({ geocoder, tiles, active }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MlMap | null>(null);
  const pinRef = useRef<Marker | null>(null);
  const dots = useRef<Marker[]>([]);
  const latest = useRef(0);
  const [ready, setReady] = useState(false);
  const [text, setText] = useState("");
  const [results, setResults] = useState<Feature[]>([]);
  const [status, setStatus] = useState(HINT);
  const [focus, setFocus] = useState(() => stored("geko.focus") !== "off");
  const [bounded, setBounded] = useState(() => stored("geko.bbox") === "on");

  async function ask(route: string, params: Record<string, string | number>) {
    const map = mapRef.current;
    const pin = pinRef.current;
    if (!map || !pin) return null;
    const id = ++latest.current;
    // A reverse query has its own point: the focus only biases searches.
    const searching = route !== "reverse";
    const c = pin.getLngLat();
    const b = map.getBounds();
    try {
      const answer = await query(route, {
        ...params,
        ...(searching && focus ? { "focus.point.lat": c.lat, "focus.point.lon": c.lng } : {}),
        ...(searching && bounded
          ? { "boundary.rect.min_lat": b.getSouth(), "boundary.rect.min_lon": b.getWest(), "boundary.rect.max_lat": b.getNorth(), "boundary.rect.max_lon": b.getEast() }
          : {}),
        size: 8,
      });
      if (id !== latest.current) return null;
      setResults(answer.features);
      setStatus(`${answer.features.length} results · ${answer.ms} ms on the server`);
      return answer.features;
    } catch (e) {
      if (id === latest.current) setStatus((e as Error).message);
      return null;
    }
  }
  // The handlers of the map are bound once: they reach what is current through these.
  const askRef = useRef(ask);
  askRef.current = ask;
  const liveGeocoder = useRef(geocoder);
  liveGeocoder.current = geocoder;
  const liveBounded = useRef(bounded);
  liveBounded.current = bounded;
  const liveText = useRef(text);
  liveText.current = text;

  const refresh = () => {
    if (liveText.current.trim()) void askRef.current("autocomplete", { text: liveText.current });
  };

  useEffect(refresh, [focus, bounded]);

  useEffect(() => {
    let gone = false;
    let map: MlMap | undefined;
    Promise.all([loadBase(tiles), credits()]).then(([base, credit]) => {
      if (gone || !box.current) return;
      map = new maplibregl.Map({
        container: box.current,
        style: base.style,
        center: base.center,
        zoom: base.zoom,
        maxZoom: 19,
        attributionControl: false,
      });
      map.addControl(new maplibregl.AttributionControl({ compact: true, customAttribution: credit || undefined }));
      map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "bottom-right");
      map.on("click", (e) => {
        if (liveGeocoder.current) void askRef.current("reverse", { "point.lat": e.lngLat.lat, "point.lon": e.lngLat.lng });
      });
      // With the box on, moving the map changes what can be found.
      map.on("moveend", () => {
        if (liveBounded.current) refresh();
      });
      let start: [number, number] = base.center;
      try {
        const saved = JSON.parse(stored("geko.pin") ?? "null");
        if (saved && Number.isFinite(saved.lat) && Number.isFinite(saved.lng)) start = [saved.lng, saved.lat];
      } catch {}
      const pin = new maplibregl.Marker({ draggable: true }).setLngLat(start);
      pin.getElement().title = "Focus of the search: drag it";
      pin.on("dragend", () => {
        const p = pin.getLngLat();
        store("geko.pin", JSON.stringify({ lat: p.lat, lng: p.lng }));
        refresh();
      });
      mapRef.current = map;
      pinRef.current = pin;
      setReady(true);
    });
    return () => {
      gone = true;
      map?.remove();
      mapRef.current = null;
      pinRef.current = null;
      setReady(false);
    };
  }, [tiles]);

  useEffect(() => {
    const map = mapRef.current;
    const pin = pinRef.current;
    if (!map || !pin) return;
    if (geocoder && focus) pin.addTo(map);
    else pin.remove();
  }, [ready, geocoder, focus]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    dots.current.forEach((d) => d.remove());
    dots.current = results.map((f) => {
      const el = document.createElement("div");
      el.className = "dot-pin";
      el.title = f.properties.label ?? f.properties.name;
      return new maplibregl.Marker({ element: el }).setLngLat(f.geometry.coordinates).addTo(map);
    });
  }, [results, ready]);

  useEffect(() => {
    if (active) mapRef.current?.resize();
  }, [active, ready]);

  const clear = () => {
    ++latest.current;
    setResults([]);
    setStatus(HINT);
  };

  const flyTo = (f: Feature, zoom: number) => mapRef.current?.flyTo({ center: f.geometry.coordinates, zoom });

  return (
    <div class="map-page">
      <div class="map" ref={box} />
      {geocoder && (
        <div class="search">
          <input
            placeholder="Search an address, a street, a place…"
            autocomplete="off"
            value={text}
            onInput={(e) => {
              const value = e.currentTarget.value;
              setText(value);
              if (value.trim()) void ask("autocomplete", { text: value });
              else clear();
            }}
            onKeyDown={async (e) => {
              if (e.key !== "Enter" || !text.trim()) return;
              const found = await ask("search", { text });
              if (found?.length) flyTo(found[0], 16);
            }}
          />
          <label>
            <input
              type="checkbox"
              checked={focus}
              onChange={(e) => {
                setFocus(e.currentTarget.checked);
                store("geko.focus", e.currentTarget.checked ? "on" : "off");
              }}
            />
            Focus on the pin (drag it on the map)
          </label>
          <label>
            <input
              type="checkbox"
              checked={bounded}
              onChange={(e) => {
                setBounded(e.currentTarget.checked);
                store("geko.bbox", e.currentTarget.checked ? "on" : "off");
              }}
            />
            Only within the visible map
          </label>
          <ul>
            {results.map((f, i) => (
              <li key={i} onClick={() => flyTo(f, 17)}>
                {f.properties.label ?? f.properties.name}
                <small>{` ${f.properties.layer} · ${f.properties.confidence}`}</small>
              </li>
            ))}
          </ul>
          <div class="status">
            <span>{status}</span>
            {status !== HINT && (
              <button class="link" onClick={clear}>
                Clear suggestions
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
