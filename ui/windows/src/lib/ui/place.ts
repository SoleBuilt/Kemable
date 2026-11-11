/** Which edge of its trigger a panel lines up with, and whether it opens upward. */
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
  const up = placement === "up" || placement === "up-end";
  const end = placement === "end" || placement === "up-end";
  const parts = [
    "position: fixed",
    up ? `top: auto; bottom: ${viewHeight - anchor.top + 6}px` : `top: ${anchor.bottom + 6}px; bottom: auto`,
    end ? `left: auto; right: ${viewWidth - anchor.right}px` : `left: ${anchor.left}px; right: auto`,
  ];
  if (size.width) parts.push(`width: ${size.width}px`);
  else if (size.minWidth) parts.push(`min-width: ${size.minWidth}px`);
  return parts.join("; ");
}
