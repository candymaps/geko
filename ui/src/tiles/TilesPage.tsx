import { Activity, Database, Layers, Palette, Settings2 } from "lucide-preact";
import { useState } from "preact/hooks";
import { Bars, Card, number, SettingsCard, size, Spark, Stat, Tabs, type Tab } from "../panel";
import { mock } from "./mock";

const TABS: Tab[] = [
  { id: "overview", label: "Overview", Icon: Database },
  { id: "sources", label: "Sources", Icon: Layers },
  { id: "performance", label: "Performance", Icon: Activity },
  { id: "styles", label: "Styles", Icon: Palette },
  { id: "config", label: "Configuration", Icon: Settings2 },
];

function Overview({ version }: { version?: string }) {
  const i = mock.index;
  const most = Math.max(...mock.layers.map((l) => l.features));
  return (
    <div class="gc-grid">
      <Card
        title="Bundle"
        aside={
          <span class="gc-pill ok">
            <i class="gc-dot" /> Serving
          </span>
        }
      >
        <div class="gc-stats">
          {version && <Stat label="Module" value={version} />}
          <Stat label="Bundle" value={i.bundle} />
          <Stat label="Built" value={i.built} hint={`took ${i.buildTime}`} />
        </div>
      </Card>
      <Card title="Memory and disk">
        <div class="gc-stats">
          <Stat label="On disk" value={size(i.diskMb)} hint="tiles of the scope" />
          <Stat label="Pinned in RAM" value={size(i.ramMb)} hint="the index" />
          <Stat label="Per tile" value="≈ 2 µs" hint="one lookup, a copy of its bytes" />
        </div>
      </Card>
      <Card title="Tiles" wide>
        <div class="gc-stats four">
          <Stat label="Tiles" value={number(i.tiles)} />
          <Stat label="Format" value={i.format} />
          <Stat label="Schema" value={i.schema} />
          <Stat label="Zoom levels" value={i.zooms} />
        </div>
      </Card>
      <Card title="Coverage">
        <div class="gc-stats">
          <Stat label="Scope" value={i.scope.join(", ")} />
          <Stat label="Center" value={i.center} hint="lon, lat" />
          <Stat label="Bounds" value={i.bounds} hint="west, south, east, north" />
        </div>
        <p class="gc-scope gc-attr">{i.attribution}</p>
      </Card>
      <Card title="Layers">
        <table class="gc-table">
          <tbody>
            {mock.layers.map((l) => (
              <tr key={l.name}>
                <td>
                  <code>{l.name}</code>
                </td>
                <td class="num">{number(l.features)}</td>
                <td class="bar">
                  <span class="gc-meter wide">
                    <i style={{ width: `${(l.features / most) * 100}%` }} />
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <Card title="Last build" wide>
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

function Sources() {
  return (
    <div class="gc-grid">
      <Card title="Data and tools of the build" wide aside="Downloaded from their origin, checked against fixed checksums">
        <table class="gc-table">
          <thead>
            <tr>
              <th>Input</th>
              <th>Used for</th>
              <th>Version</th>
              <th>Downloaded</th>
              <th>License</th>
              <th class="num">Size</th>
            </tr>
          </thead>
          <tbody>
            {mock.inputs.map((n) => (
              <tr key={n.name}>
                <td>{n.name}</td>
                <td class="muted">{n.use}</td>
                <td>{n.version}</td>
                <td>{n.fetched}</td>
                <td>{n.license}</td>
                <td class="num">{size(n.sizeMb)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function Performance() {
  const t = mock.performance;
  return (
    <div class="gc-grid">
      <Card title="Tiles per second" aside={t.window}>
        <Spark data={t.tilesPerSecond} unit="tiles/s" />
      </Card>
      <Card title="p99 time to answer (µs)" aside={t.window}>
        <Spark data={t.p99Us} unit="µs" />
      </Card>
      <Card title="Served" wide>
        <div class="gc-stats four">
          <Stat label="Tiles served" value={t.served} hint={t.window.toLowerCase()} />
          <Stat label="Sent" value={`${t.sentGb} GB`} />
          <Stat label="Empty (204)" value={`${t.emptyShare} %`} hint="tiles with nothing to draw" />
          <Stat label="Revalidated" value={`${t.cachedShare} %`} hint="clients with a cached copy" />
        </div>
      </Card>
      <Card title="By zoom level" wide>
        <table class="gc-table">
          <thead>
            <tr>
              <th>Zoom</th>
              <th class="num">Distinct tiles</th>
              <th class="num">Empty</th>
              <th class="num">Average size</th>
              <th>Share of requests</th>
            </tr>
          </thead>
          <tbody>
            {t.byZoom.map((z) => (
              <tr key={z.zoom}>
                <td>z{z.zoom}</td>
                <td class="num">{number(z.tiles)}</td>
                <td class="num">{number(z.empty)}</td>
                <td class="num">{z.avgKb} KB</td>
                <td>
                  <span class="gc-meter wide">
                    <i style={{ width: `${z.share}%` }} />
                  </span>
                  <span class="gc-pct">{z.share} %</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <Card title="Responses">
        <Bars rows={t.responses} />
      </Card>
      <Card title="Clients">
        <Bars rows={t.clients} />
      </Card>
    </div>
  );
}

function Styles() {
  const s = mock.styles;
  return (
    <div class="gc-grid">
      <Card title="Flavors" wide aside={`${s.flavors.length} flavors · ${s.flavors[0].languages} languages`}>
        <div class="gc-chips">
          {s.flavors.map((f) => (
            <span key={f.name} class="gc-chip">
              <i class={`swatch ${f.name}`} /> {f.name}
              <small>{f.languages}</small>
            </span>
          ))}
        </div>
      </Card>
      <Card title="Most requested styles">
        <Bars rows={s.served} />
      </Card>
      <Card title="Assets and TileJSON">
        <div class="gc-stats">
          <Stat label="Fonts" value={String(s.fonts.length)} hint={s.fonts.join(", ")} />
          <Stat label="Sprites" value={String(s.sprites)} hint="one per flavor" />
          <Stat label="TileJSON" value={s.tilejson} />
        </div>
      </Card>
    </div>
  );
}

export function TilesPage({ version }: { version?: string }) {
  const [tab, setTab] = useState("overview");
  return (
    <div class="gc">
      <Tabs tabs={TABS} current={tab} onSelect={setTab} />
      {tab === "overview" && <Overview version={version} />}
      {tab === "sources" && <Sources />}
      {tab === "performance" && <Performance />}
      {tab === "styles" && <Styles />}
      {tab === "config" && <SettingsCard settings={mock.settings} file="geko-tiles.toml" rebuilds="the build regenerate the tiles" />}
    </div>
  );
}
