import { Info, Lock, type LucideIcon } from "lucide-preact";
import type { ComponentChildren } from "preact";

export const number = (n: number) => n.toLocaleString("en");
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

export function Spark({ data, unit }: { data: number[]; unit: string }) {
  const max = Math.max(...data) * 1.1;
  const w = 300;
  const h = 64;
  const points = data.map((v, i) => `${((i / (data.length - 1)) * w).toFixed(1)},${(h - (v / max) * h).toFixed(1)}`);
  return (
    <svg class="gc-spark" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" role="img" aria-label={`Last hour, peak ${Math.max(...data).toFixed(1)} ${unit}`}>
      <polygon points={`0,${h} ${points.join(" ")} ${w},${h}`} class="area" />
      <polyline points={points.join(" ")} class="line" />
    </svg>
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
  return (
    <nav class="gc-tabs" role="tablist">
      {tabs.map(({ id, label, Icon }) => (
        <button key={id} role="tab" aria-selected={current === id} class={current === id ? "on" : undefined} onClick={() => onSelect(id)}>
          <Icon size={15} /> {label}
        </button>
      ))}
    </nav>
  );
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
                  <span class={`gc-pill ${s.apply === "live" ? "ok" : "warn"}`}>{s.apply === "live" ? "Live" : "Rebuild"}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}
