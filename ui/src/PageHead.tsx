import { Menu, type LucideIcon } from "lucide-preact";

interface Props {
  title: string;
  Icon: LucideIcon;
  menu: boolean;
  onMenu: () => void;
}

export function PageHead({ title, Icon, menu, onMenu }: Props) {
  return (
    <header class="page-head">
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
    </header>
  );
}
