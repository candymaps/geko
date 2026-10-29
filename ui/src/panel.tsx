import { CloudOff, Hourglass, Info, Lock, PlugZap, type LucideIcon } from "lucide-preact";
import type { ComponentChildren } from "preact";
import { useLayoutEffect, useRef, useState } from "preact/hooks";

export const number = (n: number) => n.toLocaleString("en");

/** The stages of the last build of a module, as it wrote them. */
export interface LastBuild {
  finished_unix: number;
  stages: { stage: string; elapsed_ms: number; peak_mb: number | null; reused: boolean }[];
}

const took = (ms: number) => (ms < 1000 ? `${ms} ms` : ms < 60_000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.floor(ms / 60_000)} min ${Math.round((ms % 60_000) / 1000)} s`);

export function BuildSteps({ build }: { build: LastBuild | null | undefined }) {
  if (!build?.stages?.length) return <p class="muted">No build has left its stages yet.</p>;
  return (
    <ol class="gc-steps">
      {build.stages.map((s) => (
        <li key={s.stage} class={s.reused ? "reused" : "done"}>
          <strong>{s.stage}</strong>
          <span>{s.reused ? "up to date" : s.peak_mb === null ? "built" : `built, peak ${number(s.peak_mb)} MB`}</span>
          <small>{took(s.elapsed_ms)}</small>
        </li>
      ))}
    </ol>
  );
}
export const size = (mb: number) => (mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`);

export function Card({ title, aside, children, wide }: { title: string; aside?: ComponentChildren; children: ComponentChildren; wide?: boolean }) {
  return (
    <section class={`gc-card${wide ? " wide" : ""}`}>
      <header>
        <h3>{title}</h3>
        {aside && <span class="gc-aside">{aside}</span>}
      </header>
      {children}
    </section>
  );
}

export function Stat({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div class="gc-stat">
      <span class="gc-label">{label}</span>
      <strong>{value}</strong>
      {hint && <small>{hint}</small>}
    </div>
  );
}

const reading = (v: number) => v.toLocaleString("en", { maximumFractionDigits: v < 10 ? 2 : v < 100 ? 1 : 0 });

/** A line of a chart: `tone` is the color of its pod, and none is the total of them. */
export interface Series {
  label: string;
  tone?: number;
  data: number[];
}

/** One value per `step` seconds (1 or 60), the last one being the current one; a line for each series. */
export function Spark({ series, unit, step = 1 }: { series: Series[]; unit: string; step?: number }) {
  const [hover, setHover] = useState<number | null>(null);
  const peak = Math.max(0, ...series.flatMap((s) => s.data));
  const max = peak * 1.1 || 1;
  const w = 300;
  const h = 64;
  const last = Math.max(1, ...series.map((s) => s.data.length)) - 1;
  const line = (data: number[]) => data.map((v, i) => `${((i / last) * w).toFixed(1)},${(h - (v / max) * h).toFixed(1)}`);
  const lone = series.length === 1;
  const at = (e: PointerEvent) => {
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setHover(Math.max(0, Math.min(last, Math.round(((e.clientX - box.left) / box.width) * last))));
  };
  const x = hover === null ? 0 : (hover / last) * 100;
  const ago = hover === null ? 0 : last - hover;
  const cls = (s: Series) => (s.tone === undefined ? "total" : `pod-${s.tone}`);
  return (
    <div class="gc-spark-wrap" onPointerMove={at} onPointerLeave={() => setHover(null)}>
      <svg class="gc-spark" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" role="img" aria-label={`Last ${last + 1} ${step === 1 ? "seconds" : "minutes"}, peak ${peak.toFixed(1)} ${unit}`}>
        {lone && <polygon points={`0,${h} ${line(series[0].data).join(" ")} ${w},${h}`} class="area" />}
        {series.map((s) => (
          <polyline key={s.label} points={line(s.data).join(" ")} class={`line ${lone ? "" : cls(s)}`} />
        ))}
      </svg>
      {hover !== null && (
        <>
          <span class="guide" style={{ left: `${x}%` }} />
          {series.map((s) => (
            <span key={s.label} class={`dot ${lone ? "" : cls(s)}`} style={{ left: `${x}%`, top: `${(1 - (s.data[hover] ?? 0) / max) * 100}%` }} />
          ))}
          <span class={`tip${x > 60 ? " left" : ""}`} style={{ left: `${x}%` }}>
            {series.map((s) => (
              <strong key={s.label} class={lone ? "" : cls(s)}>
                {!lone && <i />}
                {!lone && `${s.label} `}
                {reading(s.data[hover] ?? 0)} {unit}
              </strong>
            ))}
            <small>{step === 1 ? (ago === 0 ? "This second" : `${ago} s ago`) : ago === 0 ? "This minute" : `${ago} min ago`}</small>
          </span>
        </>
      )}
    </div>
  );
}

export function Bars({ rows }: { rows: { name: string; share: number }[] }) {
  return (
    <div class="gc-bars">
      {rows.map((r) => (
        <div class="row" key={r.name}>
          <span>{r.name}</span>
          <div class="track">
            <div class="fill" style={{ width: `${r.share}%` }} />
          </div>
          <em>{r.share} %</em>
        </div>
      ))}
    </div>
  );
}

export interface Tab {
  id: string;
  label: string;
  Icon: LucideIcon;
}

export function Tabs({ tabs, current, onSelect }: { tabs: Tab[]; current: string; onSelect: (id: string) => void }) {
  const nav = useRef<HTMLElement>(null);
  const [bar, setBar] = useState<{ left: number; width: number } | null>(null);
  // The first placement does not slide in from the corner.
  const [slide, setSlide] = useState(false);
  useLayoutEffect(() => {
    const el = nav.current;
    if (!el) return;
    const place = () => {
      const on = el.querySelector<HTMLElement>("button.on");
      if (on) setBar({ left: on.offsetLeft, width: on.offsetWidth });
    };
    place();
    const frame = requestAnimationFrame(() => setSlide(true));
    const watch = new ResizeObserver(place);
    watch.observe(el);
    return () => {
      cancelAnimationFrame(frame);
      watch.disconnect();
    };
  }, [current, tabs]);
  return (
    <nav ref={nav} class="gc-tabs" role="tablist">
      {tabs.map(({ id, label, Icon }) => (
        <button key={id} role="tab" aria-selected={current === id} class={current === id ? "on" : undefined} onClick={() => onSelect(id)}>
          <Icon size={15} /> {label}
        </button>
      ))}
      <span class={`bar${slide ? " slide" : ""}`} aria-hidden="true" style={bar ? { transform: `translateX(${bar.left}px)`, width: `${bar.width}px` } : { width: 0 }} />
    </nav>
  );
}

export type Origin = "default" | "file" | "sql";

export interface Setting {
  key: string;
  value: string;
  fallback: string;
  origin: Origin;
  apply: "live" | "restart" | "rebuild";
  help: string;
}

const APPLIES = { live: "Live", restart: "Restart", rebuild: "Rebuild" };
const ORIGINS: Record<Origin, string> = { default: "Default", file: "File", sql: "Database" };

export function SettingsCard({ settings, file, rebuilds }: { settings: Setting[]; file: string; rebuilds: string }) {
  return (
    <div class="gc-grid">
      <Card
        title="Effective configuration"
        wide
        aside={
          <span class="gc-pill off">
            <Lock size={11} /> Read only
          </span>
        }
      >
        <p class="gc-note">
          <Info size={14} /> Each value comes from the default, from <code>{file}</code> or, later, from the database. Changes that touch {rebuilds}.
        </p>
        <table class="gc-table settings">
          <thead>
            <tr>
              <th>Option</th>
              <th>Value</th>
              <th>Origin</th>
              <th>Applies</th>
            </tr>
          </thead>
          <tbody>
            {settings.map((s) => (
              <tr key={s.key}>
                <td>
                  <code>{s.key}</code>
                  <small>{s.help}</small>
                </td>
                <td>
                  <input value={s.value} readOnly disabled aria-label={s.key} />
                  {s.value !== s.fallback && <small>default: {s.fallback}</small>}
                </td>
                <td>
                  <span class={`gc-pill origin-${s.origin}`}>{ORIGINS[s.origin]}</span>
                </td>
                <td>
                  <span class={`gc-pill ${s.apply === "live" ? "ok" : "warn"}`}>{APPLIES[s.apply]}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

/** What a view shows in place of its content while it loads or when geko cannot give it. */
export function Placeholder({ error, module }: { error: string | null; module: string }) {
  if (error === null) {
    return (
      <div class="gc-placeholder">
        <Hourglass size={28} />
        <p>Loading…</p>
      </div>
    );
  }
  const [path, status] = /^(\S+) answered (\d+)$/.exec(error)?.slice(1) ?? [];
  const missing = status === "404";
  const Icon = missing ? PlugZap : CloudOff;
  return (
    <div class="gc-placeholder">
      <Icon size={28} />
      <h3>{missing ? `The ${module} module does not report its state yet` : `Could not read the ${module} module`}</h3>
      <p>
        {missing ? (
          <>
            The running version has no <code>{path}</code>. Update the module to one that has it.
          </>
        ) : (
          <code>{error}</code>
        )}
      </p>
    </div>
  );
}
