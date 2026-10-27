export interface Status {
  ready: boolean;
  bundle: { version: string; created_unix: number } | null;
  countries: { code: string; bytes: number }[] | null;
  sources: { id: string; name: string; attribution: string; url: string | null }[] | null;
  source_options: Record<string, Record<string, unknown>>;
  serve: { key: string; value: unknown; default: unknown }[];
}

export interface RouteStats {
  route: string;
  requests: number;
  errors: number;
  p50_ms: number | null;
  p95_ms: number | null;
  p99_ms: number | null;
}

export interface Stats {
  since_unix: number;
  routes: RouteStats[];
  searches: number;
  empty: number;
  cut_short: number;
  merged_duplicates: number;
  cache: { enabled: boolean; hits: number; misses: number };
  last_hour: { end_unix: number; requests: number[]; p99_ms: (number | null)[] };
}
