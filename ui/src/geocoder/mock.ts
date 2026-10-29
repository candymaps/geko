export interface Country {
  code: string;
  name: string;
  addresses: number;
  streets: number;
  pois: number;
  places: number;
  sizeMb: number;
}

export interface Source {
  name: string;
  scope: string;
  enabled: boolean;
  release: string;
  fetched: string;
  license: string;
  records: number;
  options: Record<string, string>;
}

export interface Hit {
  label: string;
  layer: string;
  confidence: number;
  source: string;
}

import type { Setting } from "../panel";

export const mock = {
  debug: {
    ms: 2.3,
    hits: [
      { label: "Gran Vía, 1, 28013 Madrid", layer: "address", confidence: 0.98, source: "CartoCiudad + OSM" },
      { label: "Gran Vía, Madrid", layer: "street", confidence: 0.74, source: "OSM" },
      { label: "Gran Vía, 1, 48001 Bilbao", layer: "address", confidence: 0.41, source: "CartoCiudad" },
    ] as Hit[],
  },
  settings: [
    { key: "serve.search_budget_ms", value: "15", fallback: "15", origin: "default", apply: "live", help: "Time budget of a search; the likeliest reading always completes." },
    { key: "serve.autocomplete_budget_ms", value: "5", fallback: "5", origin: "default", apply: "live", help: "Time budget of an autocomplete request." },
    { key: "serve.cache_entries", value: "50000", fallback: "0", origin: "file", apply: "live", help: "Answers kept for repeated queries, about 2.5 KB each." },
    { key: "serve.countries", value: "ES, PT", fallback: "all", origin: "sql", apply: "live", help: "Countries this node serves; empty means every country of the bundle." },
    { key: "sources.overture.min_confidence", value: "0.8", fallback: "0.8", origin: "default", apply: "rebuild", help: "Places Overture is less sure exist are left out." },
    { key: "sources.overture.release", value: "2026-09-23.1", fallback: "latest", origin: "sql", apply: "rebuild", help: "A fixed Overture release instead of the latest." },
    { key: "sources.osm.mirror", value: "auto", fallback: "auto", origin: "default", apply: "rebuild", help: "A mirror with Geofabrik's layout." },
  ] as Setting[],
};
