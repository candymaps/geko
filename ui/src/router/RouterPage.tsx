import { Activity, Database, Layers, Settings2 } from "lucide-preact";
import { useTab } from "../hooks";
import { useApi } from "../api";
import { Bars, BuildSteps, Card, number, Placeholder, SettingsCard, Spark, Stat, Tabs, type Setting, type Tab } from "../panel";
import { combine, type Stats, type Status } from "./api";
import { inRange, NoPods, PodFilter, podSeries, RangeSelect, stepOf, usePodView, useRange } from "../pods";

const TABS: Tab[] = [
  { id: "overview", label: "Overview", Icon: Database },
  { id: "sources", label: "Sources", Icon: Layers },
  { id: "performance", label: "Performance", Icon: Activity },
  { id: "config", label: "Configuration", Icon: Settings2 },
];

const stamp = (unix: number) => `${new Date(unix * 1000).toISOString().slice(0, 16).replace("T", " ")} UTC`;
const micros = (v: number | null) => (v === null ? "—" : v >= 1000 ? `${(v / 1000).toFixed(1)} ms` : `${v} µs`);
const percent = (part: number, whole: number) => (whole === 0 ? "—" : `${((part / whole) * 100).toFixed(1)} %`);
const count = (n: number | null) => (n === null ? "—" : number(n));

function Overview({ status, version }: { status: Status; version?: string }) {
  const b = status.bounds;
  const t = status.traffic;
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
            <span class="gc-pill warn">No graph loaded</span>
          )
        }
      >
        <div class="gc-stats">
          {version && <Stat label="Module" value={version} />}
          <Stat label="Bundle" value={status.bundle?.version ?? "—"} />
          <Stat label="Built" value={status.bundle ? stamp(status.bundle.created_unix) : "—"} />
        </div>
      </Card>
      <Card title="Traffic">
        <div class="gc-stats">
          <Stat label="State" value={t ? "In use" : "None"} hint={t ? undefined : "routes are at free flow"} />
          <Stat label="Slowed edges" value={t ? number(t.slowed_edges) : "—"} />
          <Stat label="Closed edges" value={t ? number(t.closed_edges) : "—"} />
          {t?.age_s !== undefined && <Stat label="Age" value={`${t.age_s} s`} hint="of the live data" />}
        </div>
      </Card>
      <Card title="Graph" wide>
        <div class="gc-stats four">
          <Stat label="Profile" value={status.profile} />
          <Stat label="Nodes" value={count(status.nodes)} />
          <Stat label="Edges" value={count(status.edges)} hint="directed" />
          <Stat label="Turns" value={count(status.turns)} hint="allowed between edges" />
        </div>
      </Card>
      <Card title="Coverage" wide>
        <div class="gc-stats">
          <Stat label="South, west" value={b ? `${b[0].toFixed(2)}, ${b[1].toFixed(2)}` : "—"} hint="lat, lon" />
          <Stat label="North, east" value={b ? `${b[2].toFixed(2)}, ${b[3].toFixed(2)}` : "—"} hint="lat, lon" />
        </div>
      </Card>
      <Card title="Last build" wide aside={status.last_build ? stamp(status.last_build.finished_unix) : undefined}>
        <BuildSteps build={status.last_build} />
      </Card>
    </div>
  );
}

function Sources({ status }: { status: Status }) {
  return (
    <div class="gc-grid">
      <Card title="Data sources of the router" wide aside="Declared in the bundle manifest">
        <table class="gc-table">
          <thead>
            <tr>
              <th>Source</th>
              <th>License</th>
              <th>Attribution</th>
            </tr>
          </thead>
          <tbody>
            {(status.sources ?? []).map((s) => (
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
                <td>{s.name}</td>
                <td class="muted">{s.attribution}</td>
              </tr>
            ))}
            {!status.sources?.length && (
              <tr>
                <td colSpan={3} class="muted">
                  No bundle loaded.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function Performance() {
  const { range, pick: pickRange } = useRange();
  const { pods, error, filter, shown, accent, stats } = usePodView<Stats>("router", "router/v1/stats", combine, range.every);
  if (!pods) return <Placeholder error={error} module="router" />;
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
  const worstOf = shown.length > 1 ? "; with several pods, the worst of them" : "";
  const step = stepOf(range);
  const windowOf = (s: Stats) => (range.hour ? s.last_hour : s.last_minute);
  const latest = windowOf(stats);
  const per = (pick: (w: Stats["last_minute"]) => (number | null)[], scale = 1) => (data: Stats) => ({
    end_unix: windowOf(data).end_unix,
    values: inRange(range, pick(windowOf(data))).map((v) => (v ?? 0) / scale),
  });
  const uptime = Math.max(1, Date.now() / 1000 - stats.since_unix);
  const requests = stats.routes.reduce((t, r) => t + r.requests, 0);
  const errors = stats.routes.reduce((t, r) => t + r.errors, 0);
  const share = (n: number) => (requests === 0 ? 0 : Math.round((n / requests) * 1000) / 10);
  return (
    <>
      {filtered}
      <div class="gc-grid" style={accent ? { "--accent": accent } : undefined}>
        <Card title="Requests per second" aside={range.label}>
          <Spark series={podSeries(shown, per((w) => w.requests, step), latest.end_unix, inRange(range, latest.requests).map((v) => v / step), step)} unit="req/s" step={step} />
        </Card>
        <Card title="p99 time in the module (µs)" aside={`${range.label}${worstOf}`}>
          <Spark series={podSeries(shown, per((w) => w.p99_us), latest.end_unix, undefined, step)} unit="µs" step={step} />
        </Card>
        <Card title="Time in the module by route" wide aside={`Upper bound of the bucket each percentile falls in${worstOf}`}>
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
                  <td class="num">{micros(r.p50_us)}</td>
                  <td class="num">{micros(r.p95_us)}</td>
                  <td class="num">{micros(r.p99_us)}</td>
                  <td class="num">{percent(r.errors, r.requests)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
        <Card title="Responses" aside={`Since the module started, ${stamp(stats.since_unix)}`}>
          <Bars
            rows={[
              { name: "answered", share: share(requests - errors) },
              { name: "errors", share: share(errors) },
            ]}
          />
        </Card>
      </div>
    </>
  );
}

const HELP: Record<string, string> = {
  "osm.mirror": "Geofabrik, or a mirror with its layout (shared with the geocoder and the tiles).",
  "packs.registry": "Where the country packs are published, without the country.",
  "profiles.default": "The profile the graph is built for, one for each bundle.",
  "traffic.enabled": "Off, routes are at free flow and nothing is downloaded for traffic.",
  "traffic.live": "Poll the live sources of the country packs of the scope.",
  sources: "What changes in a source of a country pack, by its name.",
  "serve.preload": "Off, the graph loads with the first request that needs it, which waits for it.",
};

const shown = (value: unknown) => (typeof value === "object" ? JSON.stringify(value) : String(value));

function settingsOf(status: Status): Setting[] {
  return status.settings.map((s) => ({
    key: s.key,
    value: shown(s.value),
    fallback: shown(s.default),
    origin: shown(s.value) === shown(s.default) ? "default" : "file",
    apply: s.apply,
    help: HELP[s.key] ?? "",
  }));
}

export function RouterPage({ version }: { version?: string }) {
  const [tab, setTab] = useTab("geko.tab.router", TABS.map((t) => t.id));
  const { data: status, error } = useApi<Status>("/router/v1/status", 30_000);
  const needsStatus = tab !== "performance";
  return (
    <div class="gc">
      <Tabs tabs={TABS} current={tab} onSelect={setTab} />
      {tab === "performance" && <Performance />}
      {needsStatus && !status && <Placeholder error={error} module="router" />}
      {status && tab === "overview" && <Overview status={status} version={version} />}
      {status && tab === "sources" && <Sources status={status} />}
      {status && tab === "config" && <SettingsCard settings={settingsOf(status)} file="geko-router.toml" rebuilds="the build regenerate the graph" />}
    </div>
  );
}
