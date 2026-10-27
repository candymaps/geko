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

export interface Latency {
  route: string;
  rps: number;
  p50: number;
  p95: number;
  p99: number;
  errors: number;
}

export interface Reading {
  text: string;
  kind: string;
  score: number;
  ms: number;
  winner: boolean;
}

export interface Hit {
  label: string;
  layer: string;
  confidence: number;
  source: string;
}

export type Origin = "default" | "file" | "sql";

export interface Setting {
  key: string;
  value: string;
  fallback: string;
  origin: Origin;
  apply: "live" | "rebuild";
  help: string;
}

const wave = (n: number, base: number, amp: number, seed: number) =>
  Array.from({ length: n }, (_, i) => Math.max(0, base + Math.sin(i / 4 + seed) * amp + Math.sin(i * 1.7 + seed * 3) * amp * 0.35));

export const mock = {
  index: {
    bundle: "2026.10.26-3f9a1c2",
    built: "2026-10-26 04:12 UTC",
    buildTime: "38 min",
    nextBuild: "in 6 days",
    ramHotMb: 412,
    mappedMb: 1630,
    diskMb: 2184,
    countries: [
      { code: "ES", name: "Spain", addresses: 11_840_221, streets: 612_408, pois: 1_204_377, places: 68_190, sizeMb: 1980 },
      { code: "PT", name: "Portugal", addresses: 3_214_905, streets: 188_220, pois: 311_042, places: 21_330, sizeMb: 204 },
    ] as Country[],
  },
  sources: [
    { name: "OpenStreetMap", scope: "All countries", enabled: true, release: "Geofabrik 2026-10-25", fetched: "2026-10-26", license: "ODbL 1.0", records: 9_902_114, options: { mirror: "auto" } },
    { name: "Overture Maps Places", scope: "POIs, all countries", enabled: true, release: "2026-09-23.1", fetched: "2026-10-26", license: "CDLA-Permissive-2.0", records: 1_498_730, options: { min_confidence: "0.8" } },
    { name: "CartoCiudad", scope: "Spain, addresses", enabled: true, release: "2026-10", fetched: "2026-10-24", license: "CC BY 4.0", records: 8_104_551, options: {} },
  ] as Source[],
  pipeline: [
    { stage: "Download", state: "done", note: "3 sources, cached", took: "4 min" },
    { stage: "Staging", state: "done", note: "Parquet per source", took: "9 min" },
    { stage: "Merge", state: "done", note: "−3.1 % duplicates", took: "14 min" },
    { stage: "Index", state: "done", note: "2 countries", took: "11 min" },
  ],
  performance: {
    window: "Last 60 minutes",
    rps: wave(60, 82, 24, 1),
    p99: wave(60, 4.1, 1.2, 2),
    budgetExhausted: 0.4,
    emptyRate: 3.2,
    cacheHit: 61,
    cacheEntries: 50_000,
    routes: [
      { route: "/v1/autocomplete", rps: 54.2, p50: 0.9, p95: 3.1, p99: 5.6, errors: 0.02 },
      { route: "/v1/search", rps: 21.8, p50: 1.4, p95: 3.4, p99: 4.0, errors: 0.1 },
      { route: "/v1/reverse", rps: 9.4, p50: 0.3, p95: 0.8, p99: 1.2, errors: 0 },
      { route: "/v1/place/{id}", rps: 2.1, p50: 0.1, p95: 0.2, p99: 0.4, errors: 0 },
      { route: "/v1/batch", rps: 0.6, p50: 8.4, p95: 22.0, p99: 41.0, errors: 0.5 },
    ] as Latency[],
    confidence: [2, 3, 4, 6, 8, 11, 15, 21, 34, 62],
    layers: [
      { name: "address", share: 58 },
      { name: "street", share: 21 },
      { name: "venue", share: 14 },
      { name: "locality", share: 7 },
    ],
    slow: [
      { text: "calle mayor", ms: 14.8, note: "420 candidate streets" },
      { text: "c/ san jose 12 3º b valencia", ms: 11.2, note: "4 readings" },
      { text: "avenida de la constitucion", ms: 9.7, note: "typo tolerance" },
    ],
  },
  debug: {
    query: "gran via 1, madrid",
    parsed: "street “gran via” · number 1 · locality “madrid”",
    ms: 2.3,
    readings: [
      { text: "gran vía 1, Madrid", kind: "address", score: 0.96, ms: 0.8, winner: true },
      { text: "gran vía, Madrid (street)", kind: "street", score: 0.71, ms: 0.5, winner: false },
      { text: "Gran Vía 1, Bilbao", kind: "address", score: 0.44, ms: 0.6, winner: false },
    ] as Reading[],
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
