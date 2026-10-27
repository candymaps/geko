import type { Setting } from "../panel";

export interface Input {
  name: string;
  use: string;
  version: string;
  fetched: string;
  license: string;
  sizeMb: number;
}

export interface Flavor {
  name: string;
  languages: number;
}

export interface ZoomRow {
  zoom: number;
  tiles: number;
  empty: number;
  avgKb: number;
  share: number;
}

const wave = (n: number, base: number, amp: number, seed: number) =>
  Array.from({ length: n }, (_, i) => Math.max(0, base + Math.sin(i / 5 + seed) * amp + Math.sin(i * 1.3 + seed * 2) * amp * 0.3));

export const mock = {
  index: {
    bundle: "2026.10.26-3f9a1c2",
    built: "2026-10-26 03:48 UTC",
    buildTime: "14 min",
    format: "MVT",
    schema: "Protomaps Basemap",
    zooms: "0 – 14",
    tiles: 8_412_960,
    diskMb: 1198,
    ramMb: 9,
    bounds: "−18.2, 27.6, 4.4, 43.9",
    scope: ["ES", "PT"],
    center: "−3.70, 40.42",
    attribution: "© OpenStreetMap contributors · Protomaps · Natural Earth",
  },
  layers: [
    { name: "earth", features: 18_204 },
    { name: "water", features: 1_204_330 },
    { name: "landuse", features: 3_880_122 },
    { name: "natural", features: 904_551 },
    { name: "roads", features: 9_620_410 },
    { name: "buildings", features: 21_330_904 },
    { name: "places", features: 412_880 },
    { name: "pois", features: 2_104_220 },
    { name: "boundaries", features: 88_410 },
    { name: "transit", features: 301_448 },
  ],
  pipeline: [
    { stage: "Download", state: "done", note: "OSM extracts, cached", took: "2 min" },
    { stage: "Cut to scope", state: "done", note: "water and land polygons", took: "1 min" },
    { stage: "Planetiler", state: "done", note: "4 GB heap, 8 threads", took: "10 min" },
    { stage: "Index", state: "done", note: "pinned in RAM", took: "1 min" },
  ],
  inputs: [
    { name: "OpenStreetMap", use: "Roads, buildings, land use, places", version: "Geofabrik 2026-10-25", fetched: "2026-10-26", license: "ODbL 1.0", sizeMb: 1180 },
    { name: "Natural Earth", use: "Low-zoom boundaries and coastline", version: "5.1.2", fetched: "2026-10-19", license: "Public domain", sizeMb: 212 },
    { name: "OSM water polygons", use: "Oceans", version: "2026-10-24", fetched: "2026-10-26", license: "ODbL 1.0", sizeMb: 842 },
    { name: "OSM land polygons", use: "Land mask", version: "2026-10-24", fetched: "2026-10-26", license: "ODbL 1.0", sizeMb: 912 },
    { name: "QRank", use: "Orders places and POIs", version: "2026-10-20", fetched: "2026-10-26", license: "CC0", sizeMb: 38 },
    { name: "Planetiler", use: "Generates the tiles (build only)", version: "0.9.1", fetched: "2026-09-02", license: "Apache-2.0", sizeMb: 71 },
    { name: "Noto Sans", use: "Fonts of the styles", version: "basemaps-assets@a41c9f", fetched: "2026-09-02", license: "OFL 1.1", sizeMb: 46 },
    { name: "Tangram icons", use: "Sprites of the styles", version: "basemaps-assets@a41c9f", fetched: "2026-09-02", license: "MIT", sizeMb: 1 },
  ] as Input[],
  performance: {
    window: "Last 60 minutes",
    tilesPerSecond: wave(60, 640, 190, 1),
    p99Us: wave(60, 22, 6, 3),
    emptyShare: 31,
    cachedShare: 94,
    served: "2.4 M tiles",
    sentGb: 3.1,
    byZoom: [
      { zoom: 0, tiles: 1, empty: 0, avgKb: 12, share: 1 },
      { zoom: 4, tiles: 14, empty: 2, avgKb: 38, share: 2 },
      { zoom: 8, tiles: 220, empty: 41, avgKb: 71, share: 6 },
      { zoom: 10, tiles: 3_480, empty: 970, avgKb: 54, share: 12 },
      { zoom: 12, tiles: 28_100, empty: 9_400, avgKb: 29, share: 26 },
      { zoom: 14, tiles: 96_300, empty: 31_800, avgKb: 14, share: 53 },
    ] as ZoomRow[],
    responses: [
      { name: "200 tile", share: 66 },
      { name: "204 empty", share: 31 },
      { name: "304 cached", share: 2 },
      { name: "404 / 5xx", share: 1 },
    ],
    clients: [
      { name: "MapLibre GL JS", share: 78 },
      { name: "Mobile SDKs", share: 15 },
      { name: "Other", share: 7 },
    ],
  },
  styles: {
    flavors: [
      { name: "light", languages: 41 },
      { name: "dark", languages: 41 },
      { name: "white", languages: 41 },
      { name: "grayscale", languages: 41 },
      { name: "black", languages: 41 },
    ] as Flavor[],
    fonts: ["Noto Sans Regular", "Noto Sans Medium", "Noto Sans Italic"],
    sprites: 5,
    tilejson: "/tiles/tilejson.json",
    served: [
      { name: "light / en", share: 44 },
      { name: "dark / en", share: 21 },
      { name: "light / es", share: 19 },
      { name: "dark / es", share: 9 },
      { name: "Others", share: 7 },
    ],
  },
  settings: [
    { key: "format", value: "mvt", fallback: "mvt", origin: "default", apply: "rebuild", help: "MLT is about 10 % smaller but needs MapLibre GL JS 5.12 or later." },
    { key: "url", value: "https://maps.example.com/tiles", fallback: "/tiles", origin: "file", apply: "live", help: "Where clients fetch tiles, fonts and sprites: absolute behind a CDN or proxy." },
    { key: "osm.mirror", value: "auto", fallback: "auto", origin: "default", apply: "rebuild", help: "Geofabrik, or a mirror with its layout (shared with the geocoder)." },
    { key: "planetiler.memory", value: "6g", fallback: "4g", origin: "sql", apply: "rebuild", help: "Heap of the Java process; changes build speed and memory, never the tiles." },
    { key: "planetiler.threads", value: "0", fallback: "0", origin: "default", apply: "rebuild", help: "0 is one per core." },
    { key: "planetiler.storage", value: "mmap", fallback: "mmap", origin: "default", apply: "rebuild", help: "Where Planetiler keeps its temporary data: mmap or ram." },
    { key: "planetiler.qrank", value: "true", fallback: "true", origin: "default", apply: "rebuild", help: "Download QRank to order places and POIs." },
  ] as Setting[],
};
