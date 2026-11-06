import { Route as RouteIcon } from "lucide-preact";
import maplibregl, { type GeoJSONSource, type Map as MlMap, type MapMouseEvent, type Marker } from "maplibre-gl";
import { useEffect, useRef, useState } from "preact/hooks";
import { number } from "../panel";
import { findRoutes, graphBounds, type Point, type Routes } from "./api";

const LINE = "geko-route";
const ALTERNATIVES = 2;
const NONE = { from: null, to: null };

type Ends = { from: Point | null; to: Point | null };

const length = (m: number) => (m >= 1000 ? `${(m / 1000).toFixed(1)} km` : `${Math.round(m)} m`);
const time = (s: number) => {
  const minutes = Math.round(s / 60);
  return minutes >= 60 ? `${Math.floor(minutes / 60)} h ${minutes % 60} min` : `${minutes} min`;
};

/** The lines of `routes`, the one chosen last so that it is drawn on top. */
function draw(map: MlMap, routes: Routes["routes"], chosen: number) {
  const data = {
    type: "FeatureCollection" as const,
    features: routes
      .map((r, i) => ({ type: "Feature" as const, properties: { chosen: i === chosen }, geometry: r.geometry }))
      .sort((a, b) => Number(a.properties.chosen) - Number(b.properties.chosen)),
  };
  const source = map.getSource(LINE) as GeoJSONSource | undefined;
  if (source) {
    source.setData(data);
    return;
  }
  // The style may still be loading: there is nothing to add until a route is found, after a click.
  if (routes.length === 0) return;
  map.addSource(LINE, { type: "geojson", data });
  map.addLayer({
    id: LINE,
    type: "line",
    source: LINE,
    layout: { "line-cap": "round", "line-join": "round" },
    paint: { "line-color": ["case", ["get", "chosen"], "#2a7de1", "#9aa3ad"], "line-width": ["case", ["get", "chosen"], 5, 4] },
  });
}

interface Props {
  map: MlMap;
  on: boolean;
  onToggle: (on: boolean) => void;
}

/** Directions on the map: with it on, a click sets the start, the next the end, and another starts over. */
export function RoutePanel({ map, on, onToggle }: Props) {
  const [ends, setEnds] = useState<Ends>(NONE);
  const [found, setFound] = useState<Routes | null>(null);
  const [chosen, setChosen] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const markers = useRef<{ from?: Marker; to?: Marker }>({});
  const latest = useRef(0);
  // A click frames the route; dragging an end leaves the map where it is.
  const frame = useRef(false);

  useEffect(() => {
    if (!on) {
      setEnds(NONE);
      return;
    }
    const click = (e: MapMouseEvent) => {
      const at: Point = [e.lngLat.lng, e.lngLat.lat];
      frame.current = true;
      setEnds((now) => (now.from && !now.to ? { from: now.from, to: at } : { from: at, to: null }));
    };
    const canvas = map.getCanvas();
    canvas.style.cursor = "crosshair";
    map.on("click", click);
    return () => {
      canvas.style.cursor = "";
      map.off("click", click);
    };
  }, [map, on]);

  // The map opens anywhere and a route needs the roads of the scope: if they are out of sight, go there.
  useEffect(() => {
    if (!on) return;
    let gone = false;
    graphBounds().then(
      (b) => {
        if (gone || !b) return;
        const box = new maplibregl.LngLatBounds([b[1], b[0]], [b[3], b[2]]);
        if (!box.contains(map.getCenter())) map.fitBounds(box, { padding: 40 });
      },
      () => {},
    );
    return () => {
      gone = true;
    };
  }, [map, on]);

  useEffect(() => {
    for (const [end, color] of [["from", "#2e9e5b"], ["to", "#d64545"]] as const) {
      const at = ends[end];
      const marker = markers.current[end];
      if (!at) {
        marker?.remove();
        delete markers.current[end];
      } else if (marker) {
        marker.setLngLat(at);
      } else {
        const made = new maplibregl.Marker({ color, draggable: true }).setLngLat(at).addTo(map);
        made.on("dragend", () => {
          const p = made.getLngLat();
          setEnds((now) => ({ ...now, [end]: [p.lng, p.lat] }));
        });
        markers.current[end] = made;
      }
    }
  }, [ends, map]);

  useEffect(() => {
    const id = ++latest.current;
    setError(null);
    if (!ends.from || !ends.to) {
      setFound(null);
      return;
    }
    findRoutes(ends.from, ends.to, ALTERNATIVES).then(
      (routes) => {
        if (id !== latest.current) return;
        setFound(routes);
        setChosen(0);
        if (frame.current) {
          const line = routes.routes[0].geometry.coordinates;
          const box = line.reduce((b, p) => b.extend(p), new maplibregl.LngLatBounds(line[0], line[0]));
          map.fitBounds(box, { padding: 80, maxZoom: 16 });
          frame.current = false;
        }
      },
      (e: Error) => {
        if (id !== latest.current) return;
        setFound(null);
        setError(e.message);
      },
    );
  }, [ends, map]);

  useEffect(() => draw(map, found?.routes ?? [], chosen), [found, chosen, map]);

  useEffect(
    () => () => {
      markers.current.from?.remove();
      markers.current.to?.remove();
      markers.current = {};
      // The map may have gone first, and its line with it.
      try {
        draw(map, [], 0);
      } catch {}
    },
    [map],
  );

  const hint = !ends.from ? "Click the map to set the start." : !ends.to ? "Click the map to set the end." : null;
  return (
    <div class="route-panel">
      <button class={`route-toggle${on ? " on" : ""}`} aria-pressed={on} onClick={() => onToggle(!on)}>
        <RouteIcon size={15} /> Directions
      </button>
      {on && (
        <div class="route-body">
          {hint && <p>{hint}</p>}
          {error && <p class="route-error">{error}</p>}
          {found && (
            <ul>
              {found.routes.map((r, i) => (
                <li key={i}>
                  <button class={i === chosen ? "on" : ""} aria-pressed={i === chosen} onClick={() => setChosen(i)}>
                    <b>{time(r.duration_s)}</b> {length(r.distance_m)}
                    {i > 0 && <small> alternative</small>}
                  </button>
                </li>
              ))}
            </ul>
          )}
          {found && <p class="route-note">{number(found.compute_us)} µs on the server. Drag an end to move it.</p>}
          {ends.from && (
            <button class="link" onClick={() => setEnds(NONE)}>
              Clear
            </button>
          )}
        </div>
      )}
    </div>
  );
}
