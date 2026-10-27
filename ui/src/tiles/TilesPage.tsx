import { Activity, Database, Layers, Palette, Settings2 } from "lucide-preact";
import { useState } from "preact/hooks";
import { useApi } from "../api";
import { Bars, Card, number, Placeholder, SettingsCard, size, Spark, Stat, Tabs, type Setting, type Tab } from "../panel";
import { mock } from "./mock";
import type { Stats, Status } from "./api";

const TABS: Tab[] = [
  { id: "overview", label: "Overview", Icon: Database },
  { id: "sources", label: "Sources", Icon: Layers },
  { id: "performance", label: "Performance", Icon: Activity },
  { id: "styles", label: "Styles", Icon: Palette },
  { id: "config", label: "Configuration", Icon: Settings2 },
];

const stamp = (unix: number) => `${new Date(unix * 1000).toISOString().slice(0, 16).replace("T", " ")} UTC`;
const bytes = (n: number) => size(n / 1048576);
const micros = (v: number | null) => (v === null ? "—" : `${v} µs`);
const percent = (part: number, whole: number) => (whole === 0 ? "—" : `${((part / whole) * 100).toFixed(1)} %`);
const plain = (html: string) => html.replace(/<[^>]+>/g, "");
const degrees = (v: number[]) => v.map((n) => n.toFixed(2)).join(", ");

function Sample() {
  return <span class="gc-pill off">Sample data</span>;
}

function Overview({ status, version }: { status: Status; version?: string }) {
  const t = status.tiles;
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
      <Card title="Memory and disk">
        <div class="gc-stats">
          <Stat label="Tile data" value={t ? bytes(t.data_bytes) : "—"} hint="memory-mapped from disk" />
          <Stat label="Index in RAM" value={t ? bytes(t.index_bytes) : "—"} hint="16 bytes per tile" />
        </div>
      </Card>
      <Card title="Tiles" wide>
        <div class="gc-stats four">
          <Stat label="Tiles" value={t ? number(t.count) : "—"} hint={t ? `${number(t.distinct)} different contents` : undefined} />
          <Stat label="Format" value={t ? t.format.toUpperCase() : "—"} hint={t ? `${t.compression} compression` : undefined} />
          <Stat label="Zoom levels" value={t ? `${t.min_zoom} – ${t.max_zoom}` : "—"} />
          <Stat label="Schema" value="Protomaps Basemap" />
        </div>
      </Card>
      <Card title="Coverage">
        <div class="gc-stats">
          <Stat label="Center" value={t?.center ? degrees(t.center.slice(0, 2)) : "—"} hint="lon, lat" />
          <Stat label="Bounds" value={t?.bounds ? degrees(t.bounds) : "—"} hint="west, south, east, north" />
        </div>
        {t?.attribution && <p class="gc-scope gc-attr">{plain(t.attribution)}</p>}
      </Card>
      <Card title="Layers" aside={t ? `${t.layers.length} layers` : undefined}>
        <div class="gc-chips">
          {(t?.layers ?? []).map((l) => (
            <span key={l} class="gc-chip">
              <code>{l}</code>
            </span>
          ))}
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
  return (
    <div class="gc-grid">
      <Card title="Data sources of the tiles" wide aside="Declared in the bundle manifest">
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
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function Performance() {
  const { data: stats, error } = useApi<Stats>("/tiles/stats.json", 10_000);
  if (!stats) return <Placeholder error={error} module="tiles" />;
  const uptime = Math.max(1, Date.now() / 1000 - stats.since_unix);
  const served = stats.zooms.reduce((t, z) => t + z.tiles, 0);
  const empty = stats.zooms.reduce((t, z) => t + z.empty, 0);
  const sent = stats.zooms.reduce((t, z) => t + z.bytes, 0);
  const requests = stats.routes.reduce((t, r) => t + r.requests, 0);
  const errors = stats.routes.reduce((t, r) => t + r.errors, 0);
  const share = (n: number) => (requests === 0 ? 0 : Math.round((n / requests) * 1000) / 10);
  return (
    <div class="gc-grid">
      <Card title="Requests per second" aside="Last 60 minutes">
        <Spark data={stats.last_hour.requests.map((n) => n / 60)} unit="req/s" />
      </Card>
      <Card title="p99 time in the module (µs)" aside="Last 60 minutes">
        <Spark data={stats.last_hour.p99_us.map((v) => v ?? 0)} unit="µs" />
      </Card>
      <Card title="Served" wide aside={`Since the module started, ${stamp(stats.since_unix)}`}>
        <div class="gc-stats four">
          <Stat label="Tiles served" value={number(served)} />
          <Stat label="Sent" value={bytes(sent)} hint="tiles, as stored" />
          <Stat label="Empty (204)" value={percent(empty, served + empty)} hint="tiles with nothing to draw" />
          <Stat label="Errors" value={percent(errors, requests)} hint={`${number(requests)} requests in all`} />
        </div>
      </Card>
      <Card title="By zoom level" wide>
        <table class="gc-table">
          <thead>
            <tr>
              <th>Zoom</th>
              <th class="num">Tiles</th>
              <th class="num">Empty</th>
              <th class="num">Average size</th>
              <th>Share of the tiles asked</th>
            </tr>
          </thead>
          <tbody>
            {stats.zooms.map((z) => (
              <tr key={z.zoom}>
                <td>z{z.zoom}</td>
                <td class="num">{number(z.tiles)}</td>
                <td class="num">{number(z.empty)}</td>
                <td class="num">{z.tiles === 0 ? "—" : `${(z.bytes / z.tiles / 1024).toFixed(1)} KB`}</td>
                <td>
                  <span class="gc-meter wide">
                    <i style={{ width: `${((z.tiles + z.empty) / Math.max(1, served + empty)) * 100}%` }} />
                  </span>
                  <span class="gc-pct">{percent(z.tiles + z.empty, served + empty)}</span>
                </td>
              </tr>
            ))}
            {stats.zooms.length === 0 && (
              <tr>
                <td colSpan={5} class="muted">
                  No tile has been asked yet.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </Card>
      <Card title="Time in the module by route" wide aside="Upper bound of the bucket each percentile falls in">
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
      <Card title="Responses">
        <Bars
          rows={[
            { name: "200 tile", share: share(served) },
            { name: "204 empty", share: share(empty) },
            { name: "errors", share: share(errors) },
          ]}
        />
      </Card>
    </div>
  );
}

function Styles({ status }: { status: Status }) {
  const { data: stats } = useApi<Stats>("/tiles/stats.json", 30_000);
  const s = status.styles;
  const top = stats?.styles ?? [];
  const total = top.reduce((t, x) => t + x.requests, 0);
  return (
    <div class="gc-grid">
      <Card title="Flavors" wide aside={s ? `${s.flavors.length} flavors · ${s.languages} languages` : undefined}>
        <div class="gc-chips">
          {(s?.flavors ?? []).map((f) => (
            <span key={f} class="gc-chip">
              <i class={`swatch ${f}`} /> {f}
            </span>
          ))}
        </div>
      </Card>
      <Card title="Most requested styles" aside="Since the module started">
        {top.length === 0 ? (
          <p class="gc-scope">No style has been asked yet.</p>
        ) : (
          <Bars rows={top.slice(0, 6).map((x) => ({ name: x.style, share: Math.round((x.requests / total) * 100) }))} />
        )}
      </Card>
      <Card title="Assets and TileJSON">
        <div class="gc-stats">
          <Stat label="Fonts" value={s ? String(s.fonts.length) : "—"} hint={s?.fonts.join(", ")} />
          <Stat label="Sprites" value={s ? String(s.sprites) : "—"} />
          <Stat label="TileJSON" value="/tiles/tilejson.json" />
        </div>
      </Card>
    </div>
  );
}

const HELP: Record<string, string> = {
  format: "MLT is about 10 % smaller but needs MapLibre GL JS 5.12 or later.",
  url: "Where clients fetch tiles, fonts and sprites: absolute behind a CDN or proxy.",
  "osm.mirror": "Geofabrik, or a mirror with its layout (shared with the geocoder).",
  "planetiler.memory": "Heap of the Java process; changes build speed and memory, never the tiles.",
  "planetiler.threads": "0 is one per core.",
  "planetiler.storage": "Where Planetiler keeps its temporary data: mmap or ram.",
  "planetiler.qrank": "Download QRank to order places and POIs.",
};

function settingsOf(status: Status): Setting[] {
  return status.settings.map((s) => ({
    key: s.key,
    value: String(s.value),
    fallback: String(s.default),
    origin: String(s.value) === String(s.default) ? "default" : "file",
    apply: s.apply,
    help: HELP[s.key] ?? "",
  }));
}

export function TilesPage({ version }: { version?: string }) {
  const [tab, setTab] = useState("overview");
  const { data: status, error } = useApi<Status>("/tiles/status.json", 30_000);
  const needsStatus = tab !== "performance";
  return (
    <div class="gc">
      <Tabs tabs={TABS} current={tab} onSelect={setTab} />
      {tab === "performance" && <Performance />}
      {needsStatus && !status && <Placeholder error={error} module="tiles" />}
      {status && tab === "overview" && <Overview status={status} version={version} />}
      {status && tab === "sources" && <Sources status={status} />}
      {status && tab === "styles" && <Styles status={status} />}
      {status && tab === "config" && <SettingsCard settings={settingsOf(status)} file="geko-tiles.toml" rebuilds="the build regenerate the tiles" />}
    </div>
  );
}
