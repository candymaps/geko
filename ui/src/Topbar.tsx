import { Menu, type LucideIcon } from "lucide-preact";
import type { ComponentChildren } from "preact";

interface Props {
  title: string;
  Icon: LucideIcon;
  menu: boolean;
  onMenu: () => void;
  actions?: ComponentChildren;
}

export function Topbar({ title, Icon, menu, onMenu, actions }: Props) {
  return (
    <header class="topbar">
      {menu && (
        <button class="menu" onClick={onMenu} aria-label="Menu">
          <Menu size={20} />
        </button>
      )}
      <span class="badge">
        <Icon size={18} />
      </span>
      <div class="crumbs">
        <h1>{title}</h1>
      </div>
      {actions && <div class="actions">{actions}</div>}
    </header>
  );
}
