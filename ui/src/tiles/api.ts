import { groupBy, sumWindow, total, worst, worstWindow } from "../pods";

import type { LastBuild } from "../panel";

export interface Status {
  ready: boolean;
  last_build?: LastBuild | null;
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

/** The counters of several pods as one: what is added is added, and a percentile is the worst of the pods (see `worstWindow`). */
export function combine(list: Stats[]): Stats {
  const window = (w: (s: Stats) => Stats["last_hour"], step: number): Stats["last_hour"] => {
    const sum = sumWindow(list.map((s) => w(s)), step);
    const slowest = worstWindow(list.map((s) => ({ end_unix: w(s).end_unix, p99: w(s).p99_us })), step);
    return { end_unix: sum.end_unix, requests: sum.requests, p99_us: slowest.p99 };
  };
  return {
    since_unix: Math.min(...list.map((s) => s.since_unix)),
    routes: groupBy(list.flatMap((s) => s.routes), (r) => r.route, (rows) => ({
      route: rows[0].route,
      requests: total(rows.map((r) => r.requests)),
      errors: total(rows.map((r) => r.errors)),
      p50_us: worst(rows.map((r) => r.p50_us)),
      p95_us: worst(rows.map((r) => r.p95_us)),
      p99_us: worst(rows.map((r) => r.p99_us)),
    })),
    zooms: groupBy(list.flatMap((s) => s.zooms), (z) => z.zoom, (rows) => ({
      zoom: rows[0].zoom,
      tiles: total(rows.map((z) => z.tiles)),
      empty: total(rows.map((z) => z.empty)),
      bytes: total(rows.map((z) => z.bytes)),
    })).sort((a, b) => a.zoom - b.zoom),
    styles: groupBy(list.flatMap((s) => s.styles), (s) => s.style, (rows) => ({ style: rows[0].style, requests: total(rows.map((s) => s.requests)) })),
    last_hour: window((s) => s.last_hour, 60),
    last_minute: window((s) => s.last_minute, 1),
  };
}
