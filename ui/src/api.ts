export interface ModuleInfo {
  name: string;
  version: string;
  unavailable: string | null;
}

export async function listModules(): Promise<ModuleInfo[]> {
  const res = await fetch("/admin/api/modules");
  if (!res.ok) throw new Error(`/admin/api/modules answered ${res.status}`);
  return res.json();
}
