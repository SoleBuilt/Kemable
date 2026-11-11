/** Between a trigger and its panel, and between a panel and the window's edge. */
const GAP = 6;
const MARGIN = 8;
/** Room enough for a panel to open on the side asked for: about seven options. */
const ENOUGH = 240;

/** Which edge of its trigger a panel lines up with, and whether it opens upward (it flips when the window has no room). */
export type Placement = "start" | "end" | "up" | "up-end";

/**
 * Where a menu or a list goes, fixed to the window by its trigger's box. A
 * panel placed inside its trigger's parents would be cut off by any of them
 * that clips (the page bar does), and focusing an item in it would scroll
 * that parent to show it, sliding the whole bar out of sight.
 */
export function placeBy(anchor: DOMRect, placement: Placement, size: { width?: number; minWidth?: number } = {}): string {
  const viewWidth = document.documentElement.clientWidth;
  const viewHeight = document.documentElement.clientHeight;
  // The room each way, keeping a margin from the window's edge.
  const below = viewHeight - anchor.bottom - GAP - MARGIN;
  const above = anchor.top - GAP - MARGIN;
  // The side asked for, unless it is short and the other side has more room:
  // a picker near the bottom of a dialog opens upward instead of off the window.
  const asked = placement === "up" || placement === "up-end";
  const up = asked ? !(above < ENOUGH && below > above) : below < ENOUGH && above > below;
  const end = placement === "end" || placement === "up-end";
  const parts = [
    "position: fixed",
    up ? `top: auto; bottom: ${viewHeight - anchor.top + GAP}px` : `top: ${anchor.bottom + GAP}px; bottom: auto`,
    end ? `left: auto; right: ${viewWidth - anchor.right}px` : `left: ${anchor.left}px; right: auto`,
    // A list taller than the room scrolls inside it (.pb-pop > .pb-menu).
    `--pop-max: ${Math.max(120, Math.floor(up ? above : below))}px`,
  ];
  if (size.width) parts.push(`width: ${size.width}px`);
  else if (size.minWidth) parts.push(`min-width: ${size.minWidth}px`);
  return parts.join("; ");
}
