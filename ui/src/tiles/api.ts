export interface Status {
  ready: boolean;
  bundle: { version: string; created_unix: number } | null;
  tiles: {
    count: number;
    distinct: number;
    data_bytes: number;
    index_bytes: number;
    format: string;
    compression: string;
    min_zoom: number;
    max_zoom: number;
    description: string | null;
    attribution: string | null;
    bounds: [number, number, number, number] | null;
    center: [number, number, number] | null;
    layers: string[];
  } | null;
  styles: { flavors: string[]; languages: number; fonts: string[]; sprites: number } | null;
  sources: { id: string; name: string; attribution: string; url: string | null }[] | null;
  settings: { key: string; value: unknown; default: unknown; apply: "live" | "restart" | "rebuild" }[];
}

export interface RouteStats {
  route: string;
  requests: number;
  errors: number;
  p50_us: number | null;
  p95_us: number | null;
  p99_us: number | null;
}

export interface Stats {
  since_unix: number;
  routes: RouteStats[];
  zooms: { zoom: number; tiles: number; empty: number; bytes: number }[];
  styles: { style: string; requests: number }[];
  last_hour: { end_unix: number; requests: number[]; p99_us: (number | null)[] };
  last_minute: { end_unix: number; requests: number[]; p99_us: (number | null)[] };
}
