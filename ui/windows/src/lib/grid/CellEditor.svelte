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
  class="absolute z-20 flex border-2 border-accent bg-surface shadow-lg"
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
    class="min-w-0 flex-1 resize-none bg-surface px-1.5 py-0.5 font-mono text-[12px] leading-[18px] outline-none select-text"
    placeholder={isNull ? "NULL" : ""}
    spellcheck="false"
    aria-label="Cell value"
  ></textarea>
  <button
    type="button"
    class="shrink-0 px-1.5 text-[10.5px] text-muted hover:text-fg"
    title="Set NULL (Ctrl+Shift+N)"
    onmousedown={(event) => event.preventDefault()}
    onclick={() => commit(null, "down")}>NULL</button
  >
</div>
