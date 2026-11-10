<script lang="ts" module>
  /** Where the cursor goes after a commit. */
  export type Move = "down" | "right" | "left" | null;
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import type { CellBox } from "./engine";

  interface Props {
    box: CellBox;
    /** The text to start from; null starts from NULL, shown empty. */
    initial: string | null;
    /** Typed over the value (a key started the edit): the cursor goes to the end. */
    replaced: boolean;
    onCommit: (value: string | null, move: Move) => void;
    onCancel: () => void;
  }

  let { box, initial, replaced, onCommit, onCancel }: Props = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(initial ?? "");
  // svelte-ignore state_referenced_locally
  let isNull = $state(initial === null && !replaced);
  let area: HTMLTextAreaElement;
  /** Set once the edit is committed or cancelled, so the blur that follows does nothing. */
  let closed = false;

  function commit(value: string | null, move: Move) {
    if (closed) return;
    closed = true;
    onCommit(value, move);
  }

  function cancel() {
    if (closed) return;
    closed = true;
    onCancel();
  }

  onMount(() => {
    area.focus();
    if (replaced) area.setSelectionRange(text.length, text.length);
    else area.select();
  });

  function onKeydown(event: KeyboardEvent) {
    // Keys stop here, so the grid behind does not move while a cell is edited.
    event.stopPropagation();
    if (event.key === "Escape") {
      event.preventDefault();
      cancel();
    } else if (event.key === "Enter" && !event.shiftKey && !event.altKey) {
      event.preventDefault();
      commit(isNull ? null : text, "down");
    } else if (event.key === "Tab") {
      event.preventDefault();
      commit(isNull ? null : text, event.shiftKey ? "left" : "right");
    } else if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === "n") {
      event.preventDefault();
      commit(null, "down");
    }
  }
</script>

<div
  class="pb-cell-editor"
  style:left="{box.left}px"
  style:top="{box.top}px"
  style:min-width="{Math.max(box.width, 160)}px"
  style:max-width="480px"
>
  <textarea
    bind:this={area}
    bind:value={text}
    oninput={() => (isNull = false)}
    onkeydown={onKeydown}
    onblur={() => commit(isNull ? null : text, null)}
    rows={Math.min(8, Math.max(1, text.split("\n").length))}
    class="pb-cell-editor-input"
    placeholder={isNull ? "NULL" : ""}
    spellcheck="false"
    aria-label="Cell value"
  ></textarea>
  <button
    type="button"
    class="pb-btn pb-btn--ghost pb-btn--sm pb-cell-editor-null"
    data-tip-end=""
    data-tip="Set NULL  Ctrl+Shift+N"
    onmousedown={(event) => event.preventDefault()}
    onclick={() => commit(null, "down")}>NULL</button
  >
</div>
