import type { StyleSpecification } from "maplibre-gl";

export interface Base {
  style: StyleSpecification;
  center: [number, number];
  zoom: number;
  /** The scope the tiles cover: the map neither pans nor zooms out past it, where it would show nothing. */
  bounds?: [number, number, number, number];
}

interface Catalog {
  flavors: string[];
  languages: string[];
  style: string;
}

interface TileJson {
  tiles: string[];
  minzoom: number;
  maxzoom: number;
  bounds?: [number, number, number, number];
  center?: [number, number, number];
  attribution: string;
}

const SPAIN: Pick<Base, "center" | "zoom"> = { center: [-3.7, 40.4], zoom: 6 };

async function getJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: ${res.status}`);
  return res.json();
}

// MapLibre asks for the tiles and the sprite from a worker, where `/tiles/…` does not resolve, and
// `new URL` would escape the `{z}` of a template: the origin is joined by hand.
const absolute = (url: string) => (/^[a-z][a-z0-9+.-]*:/i.test(url) ? url : location.origin + url);

export function osmBase(): Base {
  return {
    ...SPAIN,
    style: {
      version: 8,
      sources: {
        osm: {
          type: "raster",
          tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
          tileSize: 256,
          maxzoom: 19,
          attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
        },
      },
      layers: [{ id: "osm", type: "raster", source: "osm" }],
    },
  };
}

async function tilesBase(): Promise<Base> {
  const [catalog, tilejson] = await Promise.all([getJson<Catalog>("/tiles/styles.json"), getJson<TileJson>("/tiles/tilejson.json")]);
  const dark = catalog.flavors.includes("dark") && matchMedia("(prefers-color-scheme: dark)").matches;
  const lang = navigator.languages.flatMap((l) => [l, l.split("-")[0]]).find((l) => catalog.languages.includes(l)) ?? "en";
  const style = await getJson<any>(catalog.style.replace("{flavor}", dark ? "dark" : "light").replace("{lang}", lang));
  const { url: _url, ...source } = style.sources.protomaps;
  style.sources.protomaps = {
    ...source,
    tiles: tilejson.tiles.map(absolute),
    minzoom: tilejson.minzoom,
    maxzoom: tilejson.maxzoom,
    ...(tilejson.bounds ? { bounds: tilejson.bounds } : {}),
    attribution: tilejson.attribution,
  };
  style.glyphs = absolute(style.glyphs);
  style.sprite = absolute(style.sprite);
  // The tiles cover the scope, not the whole of Spain: start there.
  const [lon, lat, zoom] = tilejson.center ?? [...SPAIN.center, SPAIN.zoom];
  return { style, center: [lon, lat], zoom, bounds: tilejson.bounds && pad(tilejson.bounds) };
}

// A margin, so that the coast of the scope is not on the very edge of the screen.
function pad([west, south, east, north]: [number, number, number, number]): [number, number, number, number] {
  const dx = (east - west) * 0.1;
  const dy = (north - south) * 0.1;
  return [west - dx, south - dy, east + dx, north + dy];
}

/** The base map is the tiles module's own if it is active and answers, or else OpenStreetMap's. */
export async function loadBase(tiles: boolean): Promise<Base> {
  if (!tiles) return osmBase();
  try {
    return await tilesBase();
  } catch {
    return osmBase();
  }
}
