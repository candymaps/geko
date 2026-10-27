import { Activity, Bug, Database, Layers, Settings2, Zap, type LucideIcon } from "lucide-preact";
import { Bars, Card, number, Placeholder, SettingsCard, size, Spark, Stat, Tabs, type Setting, type Tab } from "../panel";
import { useApi } from "../api";
import type { Stats, Status } from "./api";
import { useTab } from "../hooks";
import { mock } from "./mock";

const TABS: Tab[] = [
  { id: "overview", label: "Overview", Icon: Database },
  { id: "sources", label: "Sources", Icon: Layers },
  { id: "performance", label: "Performance", Icon: Activity },
  { id: "debugger", label: "Query debugger", Icon: Bug },
  { id: "config", label: "Configuration", Icon: Settings2 },
];

const stamp = (unix: number) => `${new Date(unix * 1000).toISOString().slice(0, 16).replace("T", " ")} UTC`;
const region = (() => {
  try {
    const names = new Intl.DisplayNames(["en"], { type: "region" });
    return (code: string) => names.of(code) ?? code;
  } catch {
    return (code: string) => code;
  }
})();
const bytes = (n: number) => size(n / 1048576);
const ms = (v: number | null) => (v === null ? "—" : `${v} ms`);
const percent = (part: number, whole: number) => (whole === 0 ? "—" : `${((part / whole) * 100).toFixed(1)} %`);

function Sample() {
  return <span class="gc-pill off">Sample data</span>;
}

function Overview({ status, version }: { status: Status; version?: string }) {
  const i = mock.index;
  const totals = i.countries.reduce((t, c) => ({ a: t.a + c.addresses, s: t.s + c.streets, p: t.p + c.pois, l: t.l + c.places }), { a: 0, s: 0, p: 0, l: 0 });
  const countries = status.countries ?? [];
  const disk = countries.reduce((t, c) => t + c.bytes, 0);
  return (
    <div class="gc-grid">
      <Card
        title="Bundle"
        aside={
          status.ready ? (
            <span class="gc-pill ok">
              <i class="gc-dot" /> Serving
            </span>
          ) : (
            <span class="gc-pill warn">No bundle loaded</span>
          )
        }
      >
        <div class="gc-stats">
          {version && <Stat label="Module" value={version} />}
          <Stat label="Bundle" value={status.bundle?.version ?? "—"} />
          <Stat label="Built" value={status.bundle ? stamp(status.bundle.created_unix) : "—"} />
        </div>
      </Card>
      <Card title="Disk">
        <div class="gc-stats">
          <Stat label="On disk" value={bytes(disk)} hint="the index of the countries served" />
          <Stat label="Countries" value={String(countries.length)} />
        </div>
      </Card>
      <Card title="Countries served" wide>
        <table class="gc-table">
          <thead>
            <tr>
              <th>Country</th>
              <th class="num">On disk</th>
            </tr>
          </thead>
          <tbody>
            {countries.map((c) => (
              <tr key={c.code}>
                <td>
                  <span class="gc-code">{c.code}</span> {region(c.code)}
                </td>
                <td class="num">{bytes(c.bytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <Card title="Entities" wide aside={<Sample />}>
        <div class="gc-stats four">
          <Stat label="Addresses" value={number(totals.a)} />
          <Stat label="Streets" value={number(totals.s)} />
          <Stat label="POIs" value={number(totals.p)} />
          <Stat label="Places" value={number(totals.l)} />
        </div>
      </Card>
      <Card title="Last build" wide aside={<Sample />}>
        <ol class="gc-steps">
          {mock.pipeline.map((s) => (
            <li key={s.stage} class={s.state}>
              <strong>{s.stage}</strong>
              <span>{s.note}</span>
              <small>{s.took}</small>
            </li>
          ))}
        </ol>
      </Card>
    </div>
  );
}

function Sources({ status }: { status: Status }) {
  const sources = status.sources ?? [];
  const enabled = (id: string) => status.source_options[id]?.enabled !== false;
  const options = (id: string) => Object.entries(status.source_options[id] ?? {}).filter(([k]) => k !== "enabled");
  return (
    <div class="gc-grid">
      <Card title="Data sources of the index" wide aside="Merged into one record per entity">
        <table class="gc-table">
          <thead>
            <tr>
              <th>Source</th>
              <th>Status</th>
              <th>License</th>
              <th>Attribution</th>
              <th>Options</th>
            </tr>
          </thead>
          <tbody>
            {sources.map((s) => (
              <tr key={s.id}>
                <td>
                  {s.url ? (
                    <a href={s.url} target="_blank" rel="noreferrer">
                      {s.id}
                    </a>
                  ) : (
                    s.id
                  )}
                </td>
                <td>
                  <span class={`gc-pill ${enabled(s.id) ? "ok" : "off"}`}>{enabled(s.id) ? "Enabled" : "Disabled"}</span>
                </td>
                <td>{s.name}</td>
                <td class="muted">{s.attribution}</td>
                <td>
                  {options(s.id).map(([k, v]) => (
                    <code key={k} class="gc-opt">
                      {k} = {String(v)}
                    </code>
                  ))}
                  {options(s.id).length === 0 && <span class="muted">—</span>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function Performance() {
  const { data: stats, error } = useApi<Stats>("/geocoder/v1/stats", 1_000);
  const p = mock.performance;
  if (!stats) return <Placeholder error={error} module="geocoder" />;
  const lookups = stats.cache.hits + stats.cache.misses;
  const uptime = Math.max(1, Date.now() / 1000 - stats.since_unix);
  const total = stats.routes.reduce((t, r) => t + r.requests, 0);
  return (
    <div class="gc-grid">
      <Card title="Requests per second" aside="Last 60 seconds">
        <Spark data={stats.last_minute.requests} unit="req/s" />
      </Card>
      <Card title="p99 latency (ms)" aside="Last 60 seconds">
        <Spark data={stats.last_minute.p99_ms.map((v) => v ?? 0)} unit="ms" />
      </Card>
      <Card title="Health of the answers" wide aside={`Since the module started, ${stamp(stats.since_unix)}`}>
        <div class="gc-stats four">
          <Stat label="Budget exhausted" value={percent(stats.cut_short, stats.searches)} hint="searches cut by the time budget" />
          <Stat label="Without results" value={percent(stats.empty, stats.searches)} hint={`${number(stats.searches)} searches`} />
          <Stat label="Cache hits" value={stats.cache.enabled ? percent(stats.cache.hits, lookups) : "Off"} hint={stats.cache.enabled ? `${number(lookups)} lookups` : "serve.cache_entries is 0"} />
          <Stat label="Duplicates merged" value={number(stats.merged_duplicates)} hint="each one is a fusion miss" />
        </div>
      </Card>
      <Card title="Latency by route" wide aside="Upper bound of the bucket each percentile falls in">
        <table class="gc-table">
          <thead>
            <tr>
              <th>Route</th>
              <th class="num">Requests</th>
              <th class="num">req/s</th>
              <th class="num">p50</th>
              <th class="num">p95</th>
              <th class="num">p99</th>
              <th class="num">Errors</th>
            </tr>
          </thead>
          <tbody>
            {stats.routes.map((r) => (
              <tr key={r.route}>
                <td>
                  <code>/{r.route}</code>
                </td>
                <td class="num">{number(r.requests)}</td>
                <td class="num">{(r.requests / uptime).toFixed(2)}</td>
                <td class="num">{ms(r.p50_ms)}</td>
                <td class="num">{ms(r.p95_ms)}</td>
                <td class="num">{ms(r.p99_ms)}</td>
                <td class="num">{percent(r.errors, r.requests)}</td>
              </tr>
            ))}
            {total === 0 && (
              <tr>
                <td colSpan={7} class="muted">
                  Nothing has been asked yet.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </Card>
      <Card title="Confidence of the first result" aside={<Sample />}>
        <div class="gc-hist" role="img" aria-label="Distribution of confidence, from 0 to 1">
          {p.confidence.map((v, i) => (
            <div key={i} class="col" style={{ height: `${v}%` }} title={`${i / 10}–${(i + 1) / 10}: ${v} %`} />
          ))}
        </div>
        <div class="gc-axis">
          <span>0</span>
          <span>0.5</span>
          <span>1</span>
        </div>
      </Card>
      <Card title="Layer of the first result" aside={<Sample />}>
        <Bars rows={p.layers} />
      </Card>
      <Card title="Slowest queries" wide aside={<Sample />}>
        <table class="gc-table">
          <tbody>
            {p.slow.map((q) => (
              <tr key={q.text}>
                <td>{q.text}</td>
                <td class="muted">{q.note}</td>
                <td class="num">{q.ms} ms</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function Debugger() {
  const d = mock.debug;
  return (
    <div class="gc-grid">
      <Card title="Query" wide aside={`${d.ms} ms`}>
        <div class="gc-query">
          <input value={d.query} readOnly aria-label="Query" />
          <button disabled>
            <Zap size={14} /> Run
          </button>
        </div>
        <p class="gc-scope">Understood as {d.parsed}</p>
      </Card>
      <Card title="Readings tried" wide>
        <table class="gc-table">
          <thead>
            <tr>
              <th>Reading</th>
              <th>Kind</th>
              <th class="num">Score</th>
              <th class="num">Time</th>
            </tr>
          </thead>
          <tbody>
            {d.readings.map((r) => (
              <tr key={r.text} class={r.winner ? "win" : undefined}>
                <td>
                  {r.text} {r.winner && <span class="gc-pill ok">Chosen</span>}
                </td>
                <td>{r.kind}</td>
                <td class="num">{r.score.toFixed(2)}</td>
                <td class="num">{r.ms} ms</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <Card title="Results" wide>
        <table class="gc-table">
          <tbody>
            {d.hits.map((h) => (
              <tr key={h.label}>
                <td>{h.label}</td>
                <td>{h.layer}</td>
                <td class="muted">{h.source}</td>
                <td class="num">
                  <span class="gc-meter">
                    <i style={{ width: `${h.confidence * 100}%` }} />
                  </span>
                  {h.confidence.toFixed(2)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

const HELP: Record<string, string> = {
  "serve.countries": "Countries this node serves; empty means every country of the bundle.",
  "serve.search_budget_ms": "Time budget of a search; the likeliest reading always completes (0: none).",
  "serve.autocomplete_budget_ms": "Time budget of an autocomplete request (0: none).",
  "serve.slow_query_ms": "Requests slower than this are logged with their query (0: none).",
  "serve.same_entity_m": "Results with the same name closer than this, in metres, are merged.",
  "serve.batch_max": "Most requests in one batch.",
  "serve.cache_entries": "Answers kept for repeated queries, about 2.5 KB each (0: none).",
};

const shown = (v: unknown) => (Array.isArray(v) ? (v.length ? v.join(", ") : "all") : String(v));

function settingsOf(status: Status): Setting[] {
  const serve = status.serve.map<Setting>((s) => ({
    key: s.key,
    value: shown(s.value),
    fallback: shown(s.default),
    origin: shown(s.value) === shown(s.default) ? "default" : "file",
    apply: "restart",
    help: HELP[s.key] ?? "",
  }));
  const sources = Object.entries(status.source_options).flatMap(([id, table]) =>
    Object.entries(table).map<Setting>(([k, v]) => ({ key: `sources.${id}.${k}`, value: shown(v), fallback: shown(v), origin: "file", apply: "rebuild", help: "" })),
  );
  return [...serve, ...sources];
}

export function GeocoderPage({ version }: { version?: string }) {
  const [tab, setTab] = useTab("geko.tab.geocoder", TABS.map((t) => t.id));
  const { data: status, error } = useApi<Status>("/geocoder/v1/status", 30_000);
  const needsStatus = tab !== "performance" && tab !== "debugger";
  return (
    <div class="gc">
      <Tabs tabs={TABS} current={tab} onSelect={setTab} />
      {tab === "performance" && <Performance />}
      {tab === "debugger" && <Debugger />}
      {needsStatus && !status && <Placeholder error={error} module="geocoder" />}
      {status && tab === "overview" && <Overview status={status} version={version} />}
      {status && tab === "sources" && <Sources status={status} />}
      {status && tab === "config" && <SettingsCard settings={settingsOf(status)} file="geko-geocoder.toml" rebuilds="sources rebuild the index" />}
    </div>
  );
}
