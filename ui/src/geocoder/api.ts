import { groupBy, sumWindow, total, worst, worstWindow } from "../pods";

import type { LastBuild } from "../panel";

export interface Status {
  ready: boolean;
  last_build?: LastBuild | null;
  bundle: { version: string; created_unix: number } | null;
  countries: { code: string; bytes: number }[] | null;
  sources: { id: string; name: string; attribution: string; url: string | null }[] | null;
  /** What fusion left in each country, by its code; missing in a bundle built before it was recorded. */
  entities?: Record<string, { streets: number; addresses: number; places: number; pois: number; postcodes: number }> | null;
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
  /** The slowest queries of the last hour, anonymized by the module; `null` when it keeps none. */
  slow_queries: { text: string; route: string; ms: number; at_unix: number }[] | null;
  /** The first result of each text search, by tenths of confidence, and by layer. */
  confidence: number[];
  layers: { layer: string; count: number }[];
  last_hour: { end_unix: number; requests: number[]; p99_ms: (number | null)[] };
  last_minute: { end_unix: number; requests: number[]; p99_ms: (number | null)[] };
}

/** The counters of several pods as one: what is added is added, and a percentile is the worst of the pods (see `worstWindow`). */
export function combine(list: Stats[]): Stats {
  const window = (w: (s: Stats) => Stats["last_hour"], step: number): Stats["last_hour"] => {
    const sum = sumWindow(list.map((s) => w(s)), step);
    const worst = worstWindow(list.map((s) => ({ end_unix: w(s).end_unix, p99: w(s).p99_ms })), step);
    return { end_unix: sum.end_unix, requests: sum.requests, p99_ms: worst.p99 };
  };
  const all = list.flatMap((s) => s.routes);
  return {
    since_unix: Math.min(...list.map((s) => s.since_unix)),
    routes: groupBy(all, (r) => r.route, (rows) => ({
      route: rows[0].route,
      requests: total(rows.map((r) => r.requests)),
      errors: total(rows.map((r) => r.errors)),
      p50_ms: worst(rows.map((r) => r.p50_ms)),
      p95_ms: worst(rows.map((r) => r.p95_ms)),
      p99_ms: worst(rows.map((r) => r.p99_ms)),
    })),
    searches: total(list.map((s) => s.searches)),
    empty: total(list.map((s) => s.empty)),
    cut_short: total(list.map((s) => s.cut_short)),
    merged_duplicates: total(list.map((s) => s.merged_duplicates)),
    slow_queries: list.every((s) => !s.slow_queries)
      ? null
      : list
          .flatMap((s) => s.slow_queries ?? [])
          .sort((a, b) => b.ms - a.ms)
          .slice(0, 10),
    confidence: Array.from({ length: 10 }, (_, i) => total(list.map((s) => s.confidence?.[i] ?? 0))),
    layers: groupBy(list.flatMap((s) => s.layers ?? []), (l) => l.layer, (rows) => ({ layer: rows[0].layer, count: total(rows.map((r) => r.count)) })),
    cache: { enabled: list.some((s) => s.cache.enabled), hits: total(list.map((s) => s.cache.hits)), misses: total(list.map((s) => s.cache.misses)) },
    last_hour: window((s) => s.last_hour, 60),
    last_minute: window((s) => s.last_minute, 1),
  };
}
