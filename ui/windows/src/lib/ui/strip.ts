// A strip of tabs that scrolls sideways: the page bar's and the result bar's.

/** A mouse wheel scrolls the tabs sideways, as a browser's tab strip does. */
export function wheelSideways(event: WheelEvent) {
  const strip = event.currentTarget as HTMLElement;
  if (event.deltaY !== 0 && strip.scrollWidth > strip.clientWidth) strip.scrollLeft += event.deltaY;
}

/** Scrolls the selected tab into the strip's view, if any of it is past an edge. */
export function showSelected(strip: HTMLElement | undefined) {
  strip?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
}
