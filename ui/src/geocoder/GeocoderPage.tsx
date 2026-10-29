import { Activity, Bug, Database, Layers, Settings2, Zap, type LucideIcon } from "lucide-preact";
import { Bars, BuildSteps, Card, number, Placeholder, SettingsCard, Spark, Stat, Tabs, type Setting, type Tab } from "../panel";
import { useState } from "preact/hooks";
import { useApi } from "../api";
import { combine, type Stats, type Status } from "./api";
import { inRange, NoPods, PodFilter, podSeries, RangeSelect, stepOf, usePodView, useRange } from "../pods";
import { useTab } from "../hooks";
import { useSource, type Mode } from "../source";
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
const bytes = (n: number) => {
  const mb = n / 1048576;
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb.toFixed(1)} MB`;
};
const ms = (v: number | null) => (v === null ? "—" : `${v} ms`);
const percent = (part: number, whole: number) => (whole === 0 ? "—" : `${((part / whole) * 100).toFixed(1)} %`);

function Overview({ status, version }: { status: Status; version?: string }) {
  const entities = Object.values(status.entities ?? {});
  const totals = entities.reduce((t, c) => ({ a: t.a + c.addresses, s: t.s + c.streets, p: t.p + c.pois, l: t.l + c.places }), { a: 0, s: 0, p: 0, l: 0 });
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
      <Card title="Entities" wide aside="What fusion left in the bundle">
        <div class="gc-stats four">
          <Stat label="Addresses" value={entities.length ? number(totals.a) : "—"} />
          <Stat label="Streets" value={entities.length ? number(totals.s) : "—"} />
          <Stat label="POIs" value={entities.length ? number(totals.p) : "—"} />
          <Stat label="Places" value={entities.length ? number(totals.l) : "—"} />
        </div>
      </Card>
      <Card title="Last build" wide aside={status.last_build ? stamp(status.last_build.finished_unix) : undefined}>
        <BuildSteps build={status.last_build} />
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
  const { range, pick: pickRange } = useRange();
  const { pods, error, filter, shown, accent, stats } = usePodView<Stats>("geocoder", "geocoder/v1/stats", combine, range.every);
  if (!pods) return <Placeholder error={error} module="geocoder" />;
  const filtered = (
    <div class="gc-bar">
      <PodFilter pods={pods} only={filter.only} onPick={filter.pick} />
      <RangeSelect range={range} onPick={pickRange} />
    </div>
  );
  if (!stats) {
    return (
      <>
        {filtered}
        <NoPods pods={pods} />
      </>
    );
  }
  const firstResults = stats.confidence.reduce((t, n) => t + n, 0);
  const layerRows = [...stats.layers].sort((a, b) => b.count - a.count).map((l) => ({ name: l.layer, share: Math.round((l.count / Math.max(1, firstResults)) * 1000) / 10 }));
  const worstOf = shown.length > 1 ? "; with several pods, the worst of them" : "";
  const lookups = stats.cache.hits + stats.cache.misses;
  const uptime = Math.max(1, Date.now() / 1000 - stats.since_unix);
  const total = stats.routes.reduce((t, r) => t + r.requests, 0);
  const step = stepOf(range);
  const windowOf = (s: Stats) => (range.hour ? s.last_hour : s.last_minute);
  const latest = windowOf(stats);
  const per = (pick: (w: Stats["last_minute"]) => (number | null)[], scale = 1) => (data: Stats) => ({
    end_unix: windowOf(data).end_unix,
    values: inRange(range, pick(windowOf(data))).map((v) => (v ?? 0) / scale),
  });
  return (
    <>
    {filtered}
    <div class="gc-grid" style={accent ? { "--accent": accent } : undefined}>
      <Card title="Requests per second" aside={range.label}>
        <Spark series={podSeries(shown, per((w) => w.requests, step), latest.end_unix, inRange(range, latest.requests).map((v) => v / step), step)} unit="req/s" step={step} />
      </Card>
      <Card title={`p99 latency (ms)`} aside={range.label}>
        <Spark series={podSeries(shown, per((w) => w.p99_ms), latest.end_unix, undefined, step)} unit="ms" step={step} />
      </Card>
      <Card title="Health of the answers" wide aside={`Since the module started, ${stamp(stats.since_unix)}`}>
        <div class="gc-stats four">
          <Stat label="Budget exhausted" value={percent(stats.cut_short, stats.searches)} hint="searches cut by the time budget" />
          <Stat label="Without results" value={percent(stats.empty, stats.searches)} hint={`${number(stats.searches)} searches`} />
          <Stat label="Cache hits" value={stats.cache.enabled ? percent(stats.cache.hits, lookups) : "Off"} hint={stats.cache.enabled ? `${number(lookups)} lookups` : "serve.cache_entries is 0"} />
          <Stat label="Duplicates merged" value={number(stats.merged_duplicates)} hint="each one is a fusion miss" />
        </div>
      </Card>
      <Card title="Latency by route" wide aside={`Upper bound of the bucket each percentile falls in${worstOf}`}>
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
      <Card title="Confidence of the first result" aside="Text searches and autocompletes">
        <div class="gc-hist" role="img" aria-label="Distribution of confidence, from 0 to 1">
          {stats.confidence.map((v, i) => (
            <div key={i} class="col" style={{ height: `${(v / Math.max(1, ...stats.confidence)) * 100}%` }} data-tip={`${i / 10}–${(i + 1) / 10}: ${percent(v, firstResults)}`} />
          ))}
        </div>
        <div class="gc-axis">
          <span>0</span>
          <span>0.5</span>
          <span>1</span>
        </div>
      </Card>
      <Card title="Layer of the first result" aside="Text searches and autocompletes">
        {firstResults === 0 ? <p class="muted">Nothing has been asked yet.</p> : <Bars rows={layerRows} />}
      </Card>
      <Card title="Slowest queries" wide aside="Last hour, digits and links removed">
        {stats.slow_queries === null ? (
          <p class="muted">Off. Set serve.slow_queries in geko-geocoder.toml to list the slowest queries, anonymized and kept in memory only.</p>
        ) : stats.slow_queries.length === 0 ? (
          <p class="muted">Nothing has been asked yet.</p>
        ) : (
          <table class="gc-table">
            <tbody>
              {stats.slow_queries.map((q) => (
                <tr key={`${q.route} ${q.text}`}>
                  <td>{q.text}</td>
                  <td class="muted">{q.route}</td>
                  <td class="num">{q.ms} ms</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </div>
    </>
  );
}

interface Reading {
  probability: number;
  reading: { type: string; [field: string]: unknown };
  tried_us: number | null;
}

interface Reply {
  ms: number;
  countries: { country: string; readings: Reading[] }[];
  hits: { gid: string; label: string; layer: string; sources: string[]; match_type: string; confidence: number; score: number | null }[];
}

/** The query as any client sends it, with `debug=true` so that each result brings its score; in mock mode, the sample. */
async function ask(text: string, autocomplete: boolean, mode: Mode): Promise<Reply> {
  if (mode === "mock") {
    const d = mock.debug;
    return { ms: d.ms, countries: [], hits: d.hits.map((h) => ({ gid: h.label, label: h.label, layer: h.layer, sources: [h.source], match_type: "exact", confidence: h.confidence, score: null })) };
  }
  const params = new URLSearchParams({ text, size: "10", debug: "true" });
  const res = await fetch(`/geocoder/v1/${autocomplete ? "autocomplete" : "search"}?${params}`);
  const body = await res.json().catch(() => null);
  if (!res.ok) throw new Error(body?.error ?? `the geocoder answered ${res.status}`);
  return {
    ms: body.geocoding.timing_us / 1000,
    countries: body.geocoding.debug ?? [],
    hits: body.features.map((f: { properties: Omit<Reply["hits"][number], "sources"> & { sources?: string[] } }) => ({ ...f.properties, sources: f.properties.sources ?? [], score: f.properties.score ?? null })),
  };
}

/** A reading as a line: what the parser took each part of the query for. */
function describe(r: Reading["reading"]): string {
  const part = (label: string, v: unknown) => (typeof v === "string" && v ? `${label} “${v}”` : null);
  const parts =
    r.type === "address"
      ? [part("street", r.street), part("number", r.housenumber), part("postcode", r.postcode), part("locality", r.locality)]
      : r.type === "place"
        ? [part("place", r.text), part("within", r.within)]
        : r.type === "postcode"
          ? [part("postcode", r.postcode)]
          : [`coordinates ${r.lat}, ${r.lon}`];
  return [r.type === "address" ? "Address" : r.type === "place" ? "Place" : r.type === "postcode" ? "Postcode" : "Point", ...parts.filter(Boolean)].join(" · ");
}

function Debugger() {
  const { mode } = useSource();
  const [text, setText] = useState("");
  const [autocomplete, setAutocomplete] = useState(false);
  const [state, setState] = useState<{ reply: Reply | null; error: string | null; busy: boolean }>({ reply: null, error: null, busy: false });
  const run = async (e: Event) => {
    e.preventDefault();
    const query = text.trim();
    if (!query) return;
    setState((s) => ({ ...s, busy: true }));
    try {
      setState({ reply: await ask(query, autocomplete, mode), error: null, busy: false });
    } catch (err) {
      setState({ reply: null, error: (err as Error).message, busy: false });
    }
  };
  const { reply, error, busy } = state;
  return (
    <div class="gc-grid">
      <Card title="Query" wide aside={reply ? `${reply.ms.toFixed(1)} ms` : undefined}>
        <form class="gc-query" onSubmit={run}>
          <input value={text} onInput={(e) => setText(e.currentTarget.value)} placeholder="gran via 1, palma" aria-label="Query" />
          <label class="gc-check">
            <input type="checkbox" checked={autocomplete} onChange={(e) => setAutocomplete(e.currentTarget.checked)} /> Autocomplete
          </label>
          <button type="submit" disabled={busy || !text.trim()}>
            <Zap size={14} /> Run
          </button>
        </form>
        <p class="gc-scope">Asked like any client does, so it is counted in the statistics.</p>
        {error && <p class="error">{error}</p>}
      </Card>
      {reply && reply.countries.length > 0 && (
        <Card title="How it was read" wide aside="Likeliest reading first">
          <table class="gc-table">
            <thead>
              <tr>
                <th>Reading</th>
                <th>Country</th>
                <th class="num">Probability</th>
                <th class="num">Time</th>
              </tr>
            </thead>
            <tbody>
              {reply.countries.flatMap((c) =>
                c.readings.map((r, i) => (
                  <tr key={`${c.country} ${i}`} class={r.tried_us === null ? "muted" : undefined}>
                    <td>{describe(r.reading)}</td>
                    <td>{c.country}</td>
                    <td class="num">{percent(r.probability, 1)}</td>
                    <td class="num">{r.tried_us === null ? "skipped" : `${(r.tried_us / 1000).toFixed(2)} ms`}</td>
                  </tr>
                )),
              )}
            </tbody>
          </table>
        </Card>
      )}
      <Card title="Results" wide aside={reply ? `${reply.hits.length} found` : undefined}>
        {!reply ? (
          <p class="muted">Run a query to see what the geocoder answers.</p>
        ) : reply.hits.length === 0 ? (
          <p class="muted">No result.</p>
        ) : (
          <table class="gc-table">
            <thead>
              <tr>
                <th>Result</th>
                <th>Layer</th>
                <th>Match</th>
                <th>Sources</th>
                <th class="num">Score</th>
                <th class="num">Confidence</th>
              </tr>
            </thead>
            <tbody>
              {reply.hits.map((h) => (
                <tr key={h.gid}>
                  <td>{h.label}</td>
                  <td>{h.layer}</td>
                  <td>{h.match_type}</td>
                  <td class="muted">{h.sources.join(" + ") || "—"}</td>
                  <td class="num">{h.score === null ? "—" : h.score.toFixed(2)}</td>
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
        )}
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
