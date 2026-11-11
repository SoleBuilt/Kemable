/** Where a tooltip opens from its element: centred under it, from its start or end, or above it. */
export type TipPlace = "center" | "start" | "end" | "up";

/**
 * The attributes that draw a tooltip (`[data-tip]` in components.css). The
 * design system never uses `title`, which WebView2 draws late and in the
 * system's style (docs/decisions/0012-design-system.md).
 */
export function tipAttrs(tip: string | undefined, at: TipPlace = "center"): Record<string, string> {
  if (!tip) return {};
  return at === "center" ? { "data-tip": tip } : { "data-tip": tip, [`data-tip-${at}`]: "" };
}
