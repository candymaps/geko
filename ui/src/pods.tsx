import { useState } from "preact/hooks";
import { useApi } from "./api";
import type { Series } from "./panel";

/** What one pod of a module answered: its own counters, or why there are none. */
export interface PodData<T> {
  pod: string;
  ready: boolean;
  data: T | null;
  error: string | null;
}

/**
 * `path` (the route of the module, without the leading slash) as each pod of `module` answers it.
 * The operator asks them all (`/admin/api/modules/<module>/pods/<path>`); a geko with no operator
 * has no such route, and then its one process is the only pod.
 */
export function usePods<T>(module: string, path: string, every = 0): { pods: PodData<T>[] | null; error: string | null } {
  const operator = useApi<PodData<T>[]>(`/admin/api/modules/${module}/pods/${path}`, every);
  const single = operator.error?.endsWith("answered 404") ?? false;
  const direct = useApi<T>(single ? `/${path}` : null, every);
  if (!single) return { pods: operator.data, error: operator.error };
  return { pods: direct.data ? [{ pod: "geko", ready: true, data: direct.data, error: null }] : null, error: direct.error };
}

/** The pod the dashboard focuses on: all are shown until one is picked, and picking it again shows all. */
export function usePodFilter(): { only: string | null; pick: (pod: string) => void } {
  const [only, setOnly] = useState<string | null>(null);
  return { only, pick: (pod) => setOnly((was) => (was === pod ? null : pod)) };
}

/** The colors of the pods are taken in order (the oldest pod is the first), so a pod keeps its own while it lives. */
export const TONES = 6;
export const tone = (index: number) => index % TONES;

/** The end of a pod's name: what tells replicas of one deployment apart. */
export const shortName = (pod: string) => pod.split("-").slice(-1)[0] || pod;

/** Above every panel of a view: which pods the panels below show. It is not drawn with one pod. */
export function PodFilter({ pods, only, onPick }: { pods: PodData<unknown>[]; only: string | null; onPick: (pod: string) => void }) {
  if (pods.length < 2) return null;
  return (
    <div class="gc-pods" role="group" aria-label="Pods shown">
      <span class="gc-label">Pods</span>
      {pods.map((p, i) => {
        const off = only !== null && only !== p.pod;
        const dead = p.data === null;
        return (
          <button
            key={p.pod}
            type="button"
            class={`gc-pod pod-${tone(i)}${off ? " off" : ""}${dead ? " dead" : ""}`}
            aria-pressed={!off && !dead}
            disabled={dead}
            title={dead ? `${p.pod}: ${p.error ?? "no data"}` : only === p.pod ? `${p.pod}: click to show all` : `${p.pod}: click to focus`}
            onClick={() => onPick(p.pod)}
          >
            <i />
            {shortName(p.pod)}
            {dead && <small>{p.ready ? "no data" : "not ready"}</small>}
          </button>
        );
      })}
    </div>
  );
}

/** A series that ends at `end` (a unix second), moved to end at `to`; what falls outside is `fill`. */
export function align<V>(data: V[], end: number, to: number, fill: V): V[] {
  const shift = to - end;
  return data.map((_, i) => data[i - shift] ?? fill);
}

interface Window<V> {
  end_unix: number;
  values: V[];
}

function merge<V>(windows: Window<V>[], join: (values: V[]) => V, fill: V): Window<V> {
  const end = Math.max(...windows.map((w) => w.end_unix));
  const aligned = windows.map((w) => align(w.values, w.end_unix, end, fill));
  return { end_unix: end, values: aligned[0].map((_, i) => join(aligned.map((a) => a[i]))) };
}

const worst = (values: (number | null)[]) => {
  const known = values.filter((v): v is number => v !== null);
  return known.length === 0 ? null : Math.max(...known);
};

/** The requests of several pods, second by second (or minute by minute): added, once aligned in time. */
export function sumWindow(windows: { end_unix: number; requests: number[] }[]): { end_unix: number; requests: number[] } {
  const { end_unix, values } = merge(
    windows.map((w) => ({ end_unix: w.end_unix, values: w.requests })),
    (v) => v.reduce((t, n) => t + n, 0),
    0,
  );
  return { end_unix, requests: values };
}

/** The p99 of several pods: the worst of them. A percentile of the whole cannot come from those of its parts. */
export function worstWindow(windows: { end_unix: number; p99: (number | null)[] }[]): { end_unix: number; p99: (number | null)[] } {
  const { end_unix, values } = merge(
    windows.map((w) => ({ end_unix: w.end_unix, values: w.p99 })),
    worst,
    null,
  );
  return { end_unix, p99: values };
}

/** Rows keyed by `key`, in the order they first appear; `join` makes one of those that share it. */
export function groupBy<R, K extends string | number>(rows: R[], key: (row: R) => K, join: (rows: R[]) => R): R[] {
  const groups = new Map<K, R[]>();
  for (const row of rows) groups.set(key(row), [...(groups.get(key(row)) ?? []), row]);
  return [...groups.values()].map((g) => (g.length === 1 ? g[0] : join(g)));
}

export const total = (values: number[]) => values.reduce((t, n) => t + n, 0);
export { worst };

/** A pod that is shown: its answer, and its place among the pods (which gives it its color). */
export interface Shown<T> {
  pod: string;
  index: number;
  data: T;
}

/**
 * The view of a module by pod: every pod, which of them the filter leaves on (the focused one, or all
 * when none is focused or it is gone), and the stats of those as a single one (`combine`), or `null`
 * when none is.
 */
export function usePodView<T>(module: string, path: string, combine: (list: T[]) => T) {
  const { pods, error } = usePods<T>(module, path, 1_000);
  const filter = usePodFilter();
  const focused = (pods ?? []).some((p) => p.pod === filter.only && p.data !== null) ? filter.only : null;
  const shown: Shown<T>[] = (pods ?? []).flatMap((p, index) => (p.data !== null && (focused === null || p.pod === focused) ? [{ pod: p.pod, index, data: p.data }] : []));
  return { pods, error, filter, shown, stats: shown.length > 0 ? combine(shown.map((s) => s.data)) : null };
}

/** One line for each pod shown, and the total of them when there are several and it means something. */
export function podSeries<T>(shown: Shown<T>[], read: (data: T) => { end_unix: number; values: number[] }, end: number, sum?: number[]): Series[] {
  const lines = shown.map<Series>((s) => {
    const { end_unix, values } = read(s.data);
    return { label: shortName(s.pod), tone: tone(s.index), data: align(values, end_unix, end, 0) };
  });
  return sum && shown.length > 1 ? [...lines, { label: "Total", data: sum }] : lines;
}

/** In place of the panels when no pod gives data to show. */
export function NoPods({ pods }: { pods: PodData<unknown>[] }) {
  const text = pods.length === 0 ? "No pod of this module is running." : pods.every((p) => p.data === null) ? "No pod of this module answers." : "No pod is selected.";
  return (
    <div class="gc-placeholder">
      <p>{text}</p>
    </div>
  );
}
