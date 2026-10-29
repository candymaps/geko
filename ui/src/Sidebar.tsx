import {
  BookOpen,
  Box,
  Earth,
  Layers,
  MapPinned,
  PanelLeftClose,
  PanelLeftOpen,
  type LucideIcon,
} from "lucide-preact";
import type { ModuleInfo } from "./api";
import { ModeToggle } from "./source";
import { VERSION } from "./version";

const ICONS: Record<string, LucideIcon> = {
  geocoder: MapPinned,
  tiles: Layers,
};

const modifier = /Mac/i.test(navigator.userAgent) ? "⌘" : "Ctrl";

export const capitalize = (name: string) =>
  name.charAt(0).toUpperCase() + name.slice(1);

export const iconOf = (module: string): LucideIcon => ICONS[module] ?? Box;

interface Props {
  modules: ModuleInfo[];
  route: string;
  open: boolean;
  drawer: boolean;
  onToggle: () => void;
  onNavigate: (route: string) => void;
}

export function Sidebar({
  modules,
  route,
  open,
  drawer,
  onToggle,
  onNavigate,
}: Props) {
  const link = (
    to: string,
    label: string,
    Icon: LucideIcon,
    warning?: string | null,
  ) => {
    const active = (route || "map") === to;
    return (
      <a
        href={`#/${to}`}
        class={`item${active ? " on" : ""}`}
        data-tip={open ? (warning ?? undefined) : label}
        aria-current={active ? "page" : undefined}
        onClick={() => onNavigate(to)}
      >
        <span class="icon">
          <Icon size={17} />
        </span>
        <span class="label">{label}</span>
        {warning && <span class="warn" data-tip={warning} />}
      </a>
    );
  };

  return (
    <aside class={`sidebar${open ? " open" : ""}${drawer ? " drawer" : ""}`}>
      <div class="rail">
        <div class="brand">
          <span class="wordmark">geko</span>
          <span class="mark">g</span>
          {!drawer && (
            <button
              class="toggle"
              onClick={onToggle}
              data-tip={`${open ? "Collapse" : "Expand"} (${modifier} + .)`}
              aria-label={open ? "Collapse" : "Expand"}
            >
              {open ? (
                <PanelLeftClose size={18} />
              ) : (
                <PanelLeftOpen size={18} />
              )}
            </button>
          )}
        </div>
        <nav>
          {link("map", "Map", Earth)}
          {modules.length > 0 && <div class="eyebrow">Modules</div>}
          {modules.map((m) =>
            link(m.name, capitalize(m.name), iconOf(m.name), m.unavailable),
          )}
        </nav>
        <div class="foot">
          <a
            class="item"
            href="https://candymaps.github.io/geko/"
            target="_blank"
            rel="noreferrer"
            data-tip={open ? undefined : "Documentation"}
          >
            <span class="icon">
              <BookOpen size={17} />
            </span>
            <span class="label">Documentation</span>
          </a>
          <div class="source">
            <ModeToggle compact={!open} />
            <span class="version" data-tip="Version of the dashboard">
              v{VERSION}
            </span>
          </div>
        </div>
      </div>
    </aside>
  );
}
