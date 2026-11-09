<script lang="ts">
  import { onMount } from "svelte";
  import { errorTitle } from "../errors";
  import type { CoreError } from "../types";

  interface Props {
    /** The statements Save runs, in order, in one transaction. */
    statements: string[];
    busy: boolean;
    error: CoreError | null;
    onSave: () => void;
    onClose: () => void;
  }

  let { statements, busy, error, onSave, onClose }: Props = $props();
  let saveButton: HTMLButtonElement;
  let copied = $state(false);

  onMount(() => saveButton.focus());

  function copy() {
    void navigator.clipboard.writeText(statements.map((statement) => `${statement};`).join("\n")).then(() => (copied = true));
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape" && !busy) onClose();
    else if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && !busy) onSave();
  }}
/>

<div class="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[10vh]" role="presentation">
  <div class="flex max-h-[75vh] w-180 flex-col rounded-lg border border-line bg-surface p-5 shadow-xl" role="dialog" aria-labelledby="preview-title">
    <h2 id="preview-title" class="mb-1 text-[14px] font-semibold">
      Save {statements.length} {statements.length === 1 ? "change" : "changes"}
    </h2>
    <p class="mb-3 text-[12px] text-muted">
      These run in one transaction: all of them, or none if one fails or names a row that is gone.
    </p>
    <pre class="min-h-0 flex-1 overflow-auto rounded border border-line bg-surface-2 p-3 font-mono text-[12px] leading-relaxed select-text">{statements
        .map((statement) => `${statement};`)
        .join("\n")}</pre>
    {#if error}
      <div class="mt-3 rounded border border-line bg-surface-2 p-2 text-[12px]">
        <div class="font-semibold text-danger">{errorTitle(error)}</div>
        <div class="mt-1 whitespace-pre-wrap wrap-break-word text-muted select-text">{error.message}</div>
      </div>
    {/if}
    <div class="mt-4 flex items-center justify-end gap-2">
      <button type="button" class="toolbar-button mr-auto" onclick={copy}>{copied ? "Copied" : "Copy SQL"}</button>
      <button type="button" class="toolbar-button" onclick={onClose} disabled={busy}>Cancel</button>
      <button
        bind:this={saveButton}
        type="button"
        class="toolbar-button font-semibold text-accent"
        onclick={onSave}
        disabled={busy}
        title="Run these statements (Ctrl+Enter)">{busy ? "Saving…" : "Save"}</button
      >
    </div>
  </div>
</div>
