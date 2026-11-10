<script lang="ts" module>
  /** The rows-per-page presets (docs/decisions/0012-design-system.md, D18). */
  export const PAGE_SIZES = [5, 10, 20, 100, 500, 1_000];
</script>

<script lang="ts">
  import Combobox from "../ui/Combobox.svelte";
  import Icon from "../ui/Icon.svelte";

  /**
   * A table tab's pager, at the status bar's right end: first, previous, a page box,
   * next, last, and rows per page.
   */
  interface Props {
    offset: number;
    limit: number;
    /** Rows on this page once it has loaded; null while it loads. */
    rowsOnPage: number | null;
    /** Rows the filters match, when counted. */
    count: number | null;
    /** Paging is held: a run is going, or unsaved edits wait (docs/decisions/0010-editing-rows.md). */
    held: boolean;
    onPage: (offset: number) => void;
    onLimit: (limit: number) => void;
  }

  let { offset, limit, rowsOnPage, count, held, onPage, onLimit }: Props = $props();

  const numberFormat = new Intl.NumberFormat();
  const page = $derived(Math.floor(offset / limit) + 1);
  const pages = $derived(count === null ? null : Math.max(1, Math.ceil(count / limit)));
  const hasNext = $derived(
    rowsOnPage !== null && (pages !== null ? page < pages : rowsOnPage >= limit),
  );
  let draft: string | null = $state(null);

  function go(target: number) {
    draft = null;
    if (held) return;
    const last = pages ?? Number.POSITIVE_INFINITY;
    const clamped = Math.max(1, Math.min(last, target));
    if (clamped !== page) onPage((clamped - 1) * limit);
  }

  /** Ctrl+[ and Ctrl+] step, Ctrl+Alt+Up and Ctrl+Alt+Down go to the ends (the owner routes the keys here). */
  export function step(where: "first" | "previous" | "next" | "last") {
    if (where === "first") go(1);
    else if (where === "previous") go(page - 1);
    else if (where === "next" && hasNext) go(page + 1);
    else if (where === "last" && pages !== null) go(pages);
  }
</script>

<div class="pb-pager" role="navigation" aria-label="Pages">
  <button class="pb-btn pb-btn--ghost pb-pager-btn" onclick={() => go(1)} disabled={held || page === 1} aria-label="First page" data-tip="First page  Ctrl+Alt+Up"
    ><Icon name="chevs-left" /></button
  >
  <button
    class="pb-btn pb-btn--ghost pb-pager-btn"
    onclick={() => go(page - 1)}
    disabled={held || page === 1}
    aria-label="Previous page"
    data-tip="Previous page  Ctrl+["><Icon name="chev-left" /></button
  >
  <span class="pb-pager-label">Page</span>
  <input
    class="pb-input pb-pager-input pb-num"
    inputmode="numeric"
    aria-label="Page number"
    disabled={held}
    value={draft ?? String(page)}
    oninput={(event) => (draft = event.currentTarget.value.replace(/[^0-9]/g, ""))}
    onkeydown={(event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        const target = Number.parseInt(draft ?? "", 10);
        if (target > 0) go(target);
        else draft = null;
      } else if (event.key === "Escape") {
        draft = null;
      }
    }}
    onblur={() => (draft = null)}
  />
  <span class="pb-pager-label">{pages === null ? "" : `of ${numberFormat.format(pages)}`}</span>
  <button class="pb-btn pb-btn--ghost pb-pager-btn" onclick={() => go(page + 1)} disabled={held || !hasNext} aria-label="Next page" data-tip="Next page  Ctrl+]"
    ><Icon name="chev-right" /></button
  >
  <button
    class="pb-btn pb-btn--ghost pb-pager-btn"
    onclick={() => pages !== null && go(pages)}
    disabled={held || pages === null || page >= pages}
    aria-label="Last page"
    data-tip-end=""
    data-tip="Last page  Ctrl+Alt+Down"><Icon name="chevs-right" /></button
  >
  <span class="pb-sep"></span>
  <Combobox
    label="Rows per page"
    small
    width={112}
    placement="up-end"
    listWidth={150}
    disabled={held}
    options={PAGE_SIZES.map((size) => ({ value: size, label: `${numberFormat.format(size)} rows` }))}
    value={limit}
    onPick={onLimit}
  />
</div>
