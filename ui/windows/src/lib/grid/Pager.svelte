<script lang="ts" module>
  /** The rows-per-page presets (docs/decisions/0012-design-system.md, D18). */
  export const PAGE_SIZES = [5, 10, 20, 100, 500, 1_000];
</script>

<script lang="ts">
  import Combobox from "../ui/Combobox.svelte";
  import Button from "../ui/Button.svelte";
  import Input from "../ui/Input.svelte";
  import Separator from "../ui/Separator.svelte";

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
  <Button variant="ghost" class="pb-pager-btn" icon="chevs-left" label="First page" tip="First page  Ctrl+Alt+Up" disabled={held || page === 1} onclick={() => go(1)} />
  <Button variant="ghost" class="pb-pager-btn" icon="chev-left" label="Previous page" tip="Previous page  Ctrl+[" disabled={held || page === 1} onclick={() => go(page - 1)} />
  <span class="pb-pager-label">Page</span>
  <Input
    class="pb-pager-input pb-num"
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
  <Button variant="ghost" class="pb-pager-btn" icon="chev-right" label="Next page" tip="Next page  Ctrl+]" disabled={held || !hasNext} onclick={() => go(page + 1)} />
  <Button
    variant="ghost"
    class="pb-pager-btn"
    icon="chevs-right"
    label="Last page"
    tip="Last page  Ctrl+Alt+Down"
    tipAt="end"
    disabled={held || pages === null || page >= pages}
    onclick={() => pages !== null && go(pages)}
  />
  <Separator />
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
