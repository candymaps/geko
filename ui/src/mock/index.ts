import type { ModuleInfo } from "../api";
import type { RouteStats, Stats, Status } from "../geocoder/api";
import type { RouteStats as TileRouteStats, Stats as TileStats, Status as TileStatus } from "../tiles/api";

const now = () => Math.floor(Date.now() / 1000);

const modules: ModuleInfo[] = [
  { name: "geocoder", version: "0.1.0", unavailable: null },
  { name: "tiles", version: "0.1.0", unavailable: null },
];

const status = (): Status => ({
  ready: true,
  bundle: { version: "v2026-10-27T161913", created_unix: now() - 7200 },
  countries: [
    { code: "ES", bytes: 2_076_000_000 },
    { code: "PT", bytes: 214_000_000 },
  ],
  entities: {
    ES: { streets: 612_408, addresses: 11_840_221, places: 68_190, pois: 1_204_377, postcodes: 11_000 },
    PT: { streets: 188_220, addresses: 3_214_905, places: 21_330, pois: 311_042, postcodes: 4_800 },
  },
  last_build: {
    finished_unix: now() - 7200,
    stages: [
      { stage: "fetch and stage", elapsed_ms: 240_000, peak_mb: 2_100, reused: false },
      { stage: "fuse ES", elapsed_ms: 840_000, peak_mb: 5_400, reused: false },
      { stage: "index ES", elapsed_ms: 660_000, peak_mb: 3_900, reused: false },
      { stage: "reverse ES", elapsed_ms: 12_000, peak_mb: 800, reused: true },
    ],
  },
  sources: [
    { id: "osm", name: "ODbL 1.0", attribution: "© OpenStreetMap contributors", url: "https://www.openstreetmap.org/copyright" },
    { id: "overture", name: "CDLA-Permissive-2.0", attribution: "© Overture Maps Foundation, places", url: null },
    { id: "cartociudad", name: "CC BY 4.0", attribution: "CartoCiudad CC BY 4.0 scne.es", url: null },
  ],
  source_options: { overture: { enabled: true, min_confidence: 0.8 }, osm: { mirror: "auto" } },
  serve: [
    { key: "serve.countries", value: ["ES", "PT"], default: [] },
    { key: "serve.search_budget_ms", value: 15, default: 15 },
    { key: "serve.autocomplete_budget_ms", value: 5, default: 5 },
    { key: "serve.slow_query_ms", value: 50, default: 50 },
    { key: "serve.same_entity_m", value: 100, default: 100 },
    { key: "serve.batch_max", value: 100, default: 100 },
    { key: "serve.cache_entries", value: 50_000, default: 0 },
  ],
});

const route = (name: string, requests: number, errors: number, p50: number | null, p95: number | null, p99: number | null): RouteStats => ({
  route: name,
  requests,
  errors,
  p50_ms: p50,
  p95_ms: p95,
  p99_ms: p99,
});

const stats = (): Stats => ({
  since_unix: now() - 5400,
  routes: [
    route("v1/search", 13_204, 12, 1, 5, 10),
    route("v1/autocomplete", 41_880, 3, 0.5, 2.5, 5),
    route("v1/search/structured", 120, 0, 2.5, 10, 25),
    route("v1/reverse", 5_902, 0, 0.25, 0.5, 1),
    route("v1/place/{id}", 811, 4, 0.1, 0.25, 0.5),
    route("v1/batch", 0, 0, null, null, null),
  ],
  searches: 55_084,
  empty: 1_760,
  cut_short: 221,
  merged_duplicates: 0,
  cache: { enabled: true, hits: 33_100, misses: 21_984 },
  slow_queries: [
    { text: "calle mayor", route: "search", ms: 14.8, at_unix: now() - 600 },
    { text: "c/ san jose # #º b valencia", route: "autocomplete", ms: 11.2, at_unix: now() - 1200 },
    { text: "avenida de la constitucion", route: "search", ms: 9.7, at_unix: now() - 2400 },
  ],
  confidence: [200, 300, 400, 600, 800, 1_100, 1_500, 2_100, 3_400, 6_200],
  layers: [
    { layer: "address", count: 6_400 },
    { layer: "street", count: 2_300 },
    { layer: "poi", count: 1_500 },
    { layer: "locality", count: 800 },
  ],
  last_hour: {
    end_unix: now(),
    requests: Array.from({ length: 60 }, (_, i) => Math.round(900 + Math.sin(i / 5) * 260 + Math.random() * 120)),
    p99_ms: Array.from({ length: 60 }, (_, i) => 5 + Math.round((Math.sin(i / 4) + 1) * 2.5)),
  },
  last_minute: {
    end_unix: now(),
    requests: Array.from({ length: 60 }, (_, i) => Math.round(15 + Math.sin(i / 6) * 5 + Math.random() * 4)),
    p99_ms: Array.from({ length: 60 }, (_, i) => 5 + Math.round((Math.sin(i / 7) + 1) * 2.5)),
  },
});

const tileStatus = (): TileStatus => ({
  ready: true,
  bundle: { version: "v2026-10-27T161913", created_unix: now() - 7200 },
  tiles: {
    count: 8_412_960,
    distinct: 6_101_332,
    data_bytes: 1_198_000_000,
    index_bytes: 134_607_360,
    format: "mvt",
    compression: "gzip",
    min_zoom: 0,
    max_zoom: 14,
    description: "Vector tiles of geko, with the schema of the Protomaps Basemap 4.9.0",
    attribution: '<a href="https://github.com/protomaps/basemaps">Protomaps</a> © <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>',
    bounds: [-18.2, 27.6, 4.4, 43.9],
    center: [-6.9, 35.75, 6],
    layers: ["earth", "water", "landuse", "natural", "roads", "buildings", "places", "pois", "boundaries", "transit"],
  },
  last_build: {
    finished_unix: now() - 7200,
    stages: [
      { stage: "fetch and stage", elapsed_ms: 240_000, peak_mb: 2_100, reused: false },
      { stage: "fuse ES", elapsed_ms: 840_000, peak_mb: 5_400, reused: false },
      { stage: "index ES", elapsed_ms: 660_000, peak_mb: 3_900, reused: false },
      { stage: "reverse ES", elapsed_ms: 12_000, peak_mb: 800, reused: true },
    ],
  },
  styles: { flavors: ["light", "dark", "white", "grayscale", "black"], languages: 41, fonts: ["Noto Sans Regular", "Noto Sans Medium", "Noto Sans Italic"], sprites: 5 },
  sources: [
    { id: "osm", name: "ODbL 1.0", attribution: "© OpenStreetMap contributors", url: "https://www.openstreetmap.org/copyright" },
    { id: "natural-earth", name: "Public domain", attribution: "Made with Natural Earth", url: null },
    { id: "protomaps", name: "BSD-3-Clause", attribution: "Map styles of Protomaps", url: null },
  ],
  settings: [
    { key: "format", value: "mvt", default: "mvt", apply: "rebuild" },
    { key: "url", value: "https://maps.example.com/tiles", default: "/tiles", apply: "restart" },
    { key: "osm.mirror", value: "auto", default: "auto", apply: "rebuild" },
    { key: "planetiler.memory", value: "6g", default: "4g", apply: "rebuild" },
    { key: "planetiler.threads", value: 0, default: 0, apply: "rebuild" },
    { key: "planetiler.storage", value: "mmap", default: "mmap", apply: "rebuild" },
    { key: "planetiler.qrank", value: true, default: true, apply: "rebuild" },
  ],
});

const tileRoute = (name: string, requests: number, errors: number, p50: number | null, p95: number | null, p99: number | null): TileRouteStats => ({
  route: name,
  requests,
  errors,
  p50_us: p50,
  p95_us: p95,
  p99_us: p99,
});

const tileStats = (): TileStats => ({
  since_unix: now() - 5400,
  routes: [
    tileRoute("{z}/{x}/{y}", 2_400_120, 310, 2, 5, 25),
    tileRoute("tilejson.json", 1_204, 0, 5, 10, 25),
    tileRoute("styles.json", 88, 0, 5, 10, 25),
    tileRoute("styles/{flavor}/{lang}", 940, 1, 25, 100, 250),
    tileRoute("fonts/{fontstack}/{range}", 31_880, 40, 5, 25, 50),
    tileRoute("sprites/{name}", 3_020, 0, 5, 10, 25),
  ],
  zooms: [
    { zoom: 0, tiles: 1_200, empty: 0, bytes: 14_400_000 },
    { zoom: 4, tiles: 9_800, empty: 120, bytes: 372_000_000 },
    { zoom: 8, tiles: 104_000, empty: 9_900, bytes: 7_400_000_000 },
    { zoom: 12, tiles: 610_000, empty: 190_000, bytes: 17_700_000_000 },
    { zoom: 14, tiles: 960_000, empty: 515_000, bytes: 13_400_000_000 },
  ],
  styles: [
    { style: "light/en", requests: 410 },
    { style: "dark/en", requests: 220 },
    { style: "light/es", requests: 180 },
    { style: "dark/es", requests: 90 },
    { style: "white/en", requests: 40 },
  ],
  last_hour: {
    end_unix: now(),
    requests: Array.from({ length: 60 }, (_, i) => Math.round(36_000 + Math.sin(i / 5) * 9_000 + Math.random() * 4_000)),
    p99_us: Array.from({ length: 60 }, (_, i) => (i % 17 === 0 ? 50 : 25)),
  },
  last_minute: {
    end_unix: now(),
    requests: Array.from({ length: 60 }, (_, i) => Math.round(600 + Math.sin(i / 6) * 150 + Math.random() * 70)),
    p99_us: Array.from({ length: 60 }, (_, i) => (i % 23 === 0 ? 50 : 25)),
  },
});

const routes: Record<string, () => unknown> = {
  "/admin/api/modules": () => modules,
  "/geocoder/v1/status": status,
  "/geocoder/v1/stats": stats,
  "/tiles/status.json": tileStatus,
  "/tiles/stats.json": tileStats,
};

/** Two replicas of a module, each with its own counters: the sample of one, moved a little for the other. */
function mockPods(module: string, path: string): unknown {
  const one = mockGet(`/${path}`) as { last_minute: { requests: number[] }; last_hour: { requests: number[] } };
  const other = structuredClone(one);
  const scale = (list: number[], by: number) => list.map((v) => Math.round(v * by));
  other.last_minute.requests = scale(other.last_minute.requests, 0.4);
  other.last_hour.requests = scale(other.last_hour.requests, 0.4);
  const pod = (suffix: string, data: unknown) => ({ pod: `geko-${module}-5d9c7b-${suffix}`, ready: true, data, error: null });
  return [pod("k7x2p", one), pod("q4w8m", other)];
}

export function mockGet(path: string): unknown {
  const pods = /^\/admin\/api\/modules\/([^/]+)\/pods\/(.+)$/.exec(path);
  if (pods) return mockPods(pods[1], pods[2]);
  const answer = routes[path];
  if (!answer) throw new Error(`${path} has no sample data`);
  return answer();
}
