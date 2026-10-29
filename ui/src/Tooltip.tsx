import { useLayoutEffect, useEffect, useRef, useState } from "preact/hooks";

const GAP = 8;
const MARGIN = 6;

interface Shown {
  text: string;
  rect: DOMRect;
  side: boolean;
}

/** One tooltip for the whole page, driven by `data-tip`: unlike `title`, it shows at once and is never clipped by an `overflow: hidden` ancestor. */
export function Tooltip() {
  const [shown, setShown] = useState<Shown | null>(null);
  const [at, setAt] = useState<{ left: number; top: number } | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const show = (e: Event) => {
      const el = (e.target as Element | null)?.closest?.("[data-tip]");
      const text = el?.getAttribute("data-tip");
      if (!el || !text) return setShown(null);
      setAt(null);
      setShown({ text, rect: el.getBoundingClientRect(), side: el.closest(".sidebar") !== null });
    };
    const hide = () => setShown(null);
    document.addEventListener("pointerover", show);
    document.addEventListener("focusin", show);
    document.addEventListener("pointerdown", hide);
    document.addEventListener("scroll", hide, true);
    document.addEventListener("focusout", hide);
    document.documentElement.addEventListener("pointerleave", hide);
    return () => {
      document.removeEventListener("pointerover", show);
      document.removeEventListener("focusin", show);
      document.removeEventListener("pointerdown", hide);
      document.removeEventListener("scroll", hide, true);
      document.removeEventListener("focusout", hide);
      document.documentElement.removeEventListener("pointerleave", hide);
    };
  }, []);

  useLayoutEffect(() => {
    if (!shown || !box.current) return;
    const { width, height } = box.current.getBoundingClientRect();
    const { rect, side } = shown;
    const clamp = (v: number, size: number, max: number) => Math.max(MARGIN, Math.min(v, max - size - MARGIN));
    if (side) {
      setAt({ left: rect.right + GAP, top: clamp(rect.top + (rect.height - height) / 2, height, innerHeight) });
    } else {
      const above = rect.top - GAP - height >= MARGIN;
      setAt({
        left: clamp(rect.left + (rect.width - width) / 2, width, innerWidth),
        top: above ? rect.top - GAP - height : rect.bottom + GAP,
      });
    }
  }, [shown]);

  if (!shown) return null;
  return (
    <div ref={box} class="tooltip" role="tooltip" style={{ left: at?.left ?? 0, top: at?.top ?? 0, visibility: at ? "visible" : "hidden" }}>
      {shown.text}
    </div>
  );
}
