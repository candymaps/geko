export interface Feature {
  geometry: { coordinates: [number, number] };
  properties: { label?: string; name: string; layer: string; confidence: number };
}

export interface Answer {
  features: Feature[];
  ms: string;
}

export async function query(route: string, params: Record<string, string | number>): Promise<Answer> {
  const q = new URLSearchParams(Object.entries(params).map(([k, v]) => [k, String(v)]));
  const res = await fetch(`/geocoder/v1/${route}?${q}`);
  if (!res.ok) throw new Error((await res.json().catch(() => null))?.error ?? res.statusText);
  const body = await res.json();
  return { features: body.features, ms: (body.geocoding.timing_us / 1000).toFixed(1) };
}

export async function credits(): Promise<string> {
  try {
    const a = await (await fetch("/v1/attribution")).json();
    const sources = (a.sources as { attribution: string }[]).map((s) => s.attribution).join(" · ");
    return `Results: ${sources} · <a href="/v1/attribution">licenses</a>`;
  } catch {
    return "";
  }
}
