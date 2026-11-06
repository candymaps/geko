import { groupBy, sumWindow, total, worst, worstWindow } from "../pods";

import type { LastBuild } from "../panel";

export interface Status {
  ready: boolean;
  last_build?: LastBuild | null;
  bundle: { version: string; created_unix: number } | null;
  profile: string;
  nodes: number | null;
  edges: number | null;
  turns: number | null;
  /** `[min lat, min lon, max lat, max lon]` of the graph. */
  bounds: [number, number, number, number] | null;
  traffic: { slowed_edges: number; closed_edges: number; age_s?: number } | null;
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
    last_hour: window((s) => s.last_hour, 60),
    last_minute: window((s) => s.last_minute, 1),
  };
}

/** `[lon, lat]`, as MapLibre and GeoJSON have it. */
export type Point = [number, number];

/** The share of a route known by live data, by what is usual, and by nothing but its road class. */
export interface Coverage {
  live: number;
  historical: number;
  free_flow: number;
}

export interface Route {
  duration_s: number;
  distance_m: number;
  geometry: { type: "LineString"; coordinates: Point[] };
  /** Absent with traffic off. */
  coverage?: Coverage;
}

export interface Routes {
  /** The fastest first. */
  routes: Route[];
  compute_us: number;
}

/** `[min lat, min lon, max lat, max lon]` of the graph, null without one. */
export async function graphBounds(): Promise<Status["bounds"]> {
  const res = await fetch("/router/v1/status");
  return res.ok ? (await res.json()).bounds : null;
}

/** The fastest route and up to `alternatives` others; the error is what the module says of it. */
export async function findRoutes(from: Point, to: Point, alternatives: number): Promise<Routes> {
  const q = new URLSearchParams({ from: `${from[1]},${from[0]}`, to: `${to[1]},${to[0]}`, alternatives: String(alternatives) });
  const res = await fetch(`/router/v1/route?${q}`);
  if (!res.ok) throw new Error((await res.json().catch(() => null))?.error ?? res.statusText);
  const body = await res.json();
  // The first route says it in `traffic`, beside the rest of the traffic; the alternatives, on their own.
  return { routes: [{ ...body, coverage: body.traffic?.coverage }, ...(body.alternatives ?? [])], compute_us: body.compute_us };
}
