import { Activity, Bug, Database, Layers, Settings2, Zap, type LucideIcon } from "lucide-preact";
import { Bars, Card, number, SettingsCard, size, Spark, Stat, Tabs, type Tab } from "../panel";
import { useState } from "preact/hooks";
import { mock } from "./mock";

const TABS: Tab[] = [
  { id: "overview", label: "Overview", Icon: Database },
  { id: "sources", label: "Sources", Icon: Layers },
  { id: "performance", label: "Performance", Icon: Activity },
  { id: "debugger", label: "Query debugger", Icon: Bug },
  { id: "config", label: "Configuration", Icon: Settings2 },
];

function Overview({ version }: { version?: string }) {
  const i = mock.index;
  const totals = i.countries.reduce((t, c) => ({ a: t.a + c.addresses, s: t.s + c.streets, p: t.p + c.pois, l: t.l + c.places }), { a: 0, s: 0, p: 0, l: 0 });
  return (
    <div class="gc-grid">
      <Card title="Bundle" aside={<span class="gc-pill ok"><i class="gc-dot" /> Serving</span>}>
        <div class="gc-stats">
          {version && <Stat label="Module" value={version} />}
          <Stat label="Bundle" value={i.bundle} />
          <Stat label="Built" value={i.built} hint={`took ${i.buildTime}`} />
          <Stat label="Next build" value={i.nextBuild} />
        </div>
      </Card>
      <Card title="Memory and disk">
        <div class="gc-stats">
          <Stat label="Pinned in RAM" value={size(i.ramHotMb)} hint="hot part of the index" />
          <Stat label="Memory-mapped" value={size(i.mappedMb)} hint="cold part" />
          <Stat label="On disk" value={size(i.diskMb)} />
        </div>
      </Card>
      <Card title="Entities" wide>
        <div class="gc-stats four">
          <Stat label="Addresses" value={number(totals.a)} />
          <Stat label="Streets" value={number(totals.s)} />
          <Stat label="POIs" value={number(totals.p)} />
          <Stat label="Places" value={number(totals.l)} />
        </div>
      </Card>
      <Card title="Countries served" wide>
        <table class="gc-table">
          <thead>
            <tr>
              <th>Country</th>
              <th class="num">Addresses</th>
              <th class="num">Streets</th>
              <th class="num">POIs</th>
              <th class="num">Places</th>
              <th class="num">Size</th>
            </tr>
          </thead>
          <tbody>
            {i.countries.map((c) => (
              <tr key={c.code}>
                <td>
                  <span class="gc-code">{c.code}</span> {c.name}
                </td>
                <td class="num">{number(c.addresses)}</td>
                <td class="num">{number(c.streets)}</td>
                <td class="num">{number(c.pois)}</td>
                <td class="num">{number(c.places)}</td>
                <td class="num">{size(c.sizeMb)}</td>
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
      <Card title="Data sources of the index" wide aside="Merged into one record per entity">
        <table class="gc-table">
          <thead>
            <tr>
              <th>Source</th>
              <th>Covers</th>
              <th>Status</th>
              <th>Release</th>
              <th>Downloaded</th>
              <th>License</th>
              <th class="num">Records</th>
              <th>Options</th>
            </tr>
          </thead>
          <tbody>
            {mock.sources.map((s) => (
              <tr key={s.name}>
                <td>{s.name}</td>
                <td class="muted">{s.scope}</td>
                <td>
                  <span class={`gc-pill ${s.enabled ? "ok" : "off"}`}>{s.enabled ? "Enabled" : "Disabled"}</span>
                </td>
                <td>{s.release}</td>
                <td>{s.fetched}</td>
                <td>{s.license}</td>
                <td class="num">{number(s.records)}</td>
                <td>
                  {Object.entries(s.options).map(([k, v]) => (
                    <code key={k} class="gc-opt">
                      {k} = {v}
                    </code>
                  ))}
                  {Object.keys(s.options).length === 0 && <span class="muted">—</span>}
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
  const p = mock.performance;
  return (
    <div class="gc-grid">
      <Card title="Requests per second" aside={p.window}>
        <Spark data={p.rps} unit="req/s" />
      </Card>
      <Card title="p99 latency (ms)" aside={p.window}>
        <Spark data={p.p99} unit="ms" />
      </Card>
      <Card title="Health of the answers" wide>
        <div class="gc-stats four">
          <Stat label="Budget exhausted" value={`${p.budgetExhausted} %`} hint="queries cut by the time budget" />
          <Stat label="Without results" value={`${p.emptyRate} %`} />
          <Stat label="Cache hits" value={`${p.cacheHit} %`} hint={`${number(p.cacheEntries)} entries`} />
          <Stat label="Duplicates merged" value="0" hint="each one is a fusion miss" />
        </div>
      </Card>
      <Card title="Latency by route" wide>
        <table class="gc-table">
          <thead>
            <tr>
              <th>Route</th>
              <th class="num">req/s</th>
              <th class="num">p50</th>
              <th class="num">p95</th>
              <th class="num">p99</th>
              <th class="num">Errors</th>
            </tr>
          </thead>
          <tbody>
            {p.routes.map((r) => (
              <tr key={r.route}>
                <td>
                  <code>{r.route}</code>
                </td>
                <td class="num">{r.rps}</td>
                <td class="num">{r.p50} ms</td>
                <td class="num">{r.p95} ms</td>
                <td class="num">{r.p99} ms</td>
                <td class="num">{r.errors} %</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <Card title="Confidence of the first result">
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
      <Card title="Layer of the first result">
        <Bars rows={p.layers} />
      </Card>
      <Card title="Slowest queries" wide aside="No text is kept unless enabled">
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

export function GeocoderPage({ version }: { version?: string }) {
  const [tab, setTab] = useState("overview");
  return (
    <div class="gc">
      <Tabs tabs={TABS} current={tab} onSelect={setTab} />
      {tab === "overview" && <Overview version={version} />}
      {tab === "sources" && <Sources />}
      {tab === "performance" && <Performance />}
      {tab === "debugger" && <Debugger />}
      {tab === "config" && <SettingsCard settings={mock.settings} file="geko-geocoder.toml" rebuilds="sources rebuild the index" />}
    </div>
  );
}
