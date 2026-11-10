<script lang="ts">
  import { onMount } from "svelte";
  import { errorTitle } from "../errors";
  import type { CoreError } from "../types";
  import Icon from "../ui/Icon.svelte";

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

<div class="pb-overlay pb-overlay--fixed" role="presentation">
  <div class="pb-dialog" style="width: 760px; max-height: 80vh" role="dialog" aria-labelledby="preview-title">
    <div class="pb-dialog-head">
      <h2 class="pb-dialog-title" id="preview-title">Save {statements.length} {statements.length === 1 ? "change" : "changes"}</h2>
      <button type="button" class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label="Close" data-tip-end="" data-tip="Close  Esc" onclick={onClose} disabled={busy}
        ><Icon name="x" /></button
      >
    </div>
    <div class="pb-dialog-body" style="min-height: 0; overflow: auto">
      {#if error}
        <div class="pb-info pb-info--danger" role="alert">
          <Icon name="error" />
          <div class="pb-info-text"><span class="pb-info-title">{errorTitle(error)}</span><span class="pb-info-msg">{error.message}</span></div>
        </div>
      {/if}
      <p class="pb-dialog-text">These statements run in one transaction. If one fails, or names a row that is gone, none of them are saved.</p>
      <pre class="pb-codebox">{statements.map((statement) => `${statement};`).join("\n")}</pre>
    </div>
    <div class="pb-dialog-foot">
      <button type="button" class="pb-btn pb-btn--ghost" onclick={copy} data-tip-start="" data-tip="Copy the statements"
        ><Icon name={copied ? "check" : "copy"} />{copied ? "Copied" : "Copy SQL"}</button
      >
      <span class="pb-spacer"></span>
      <button type="button" class="pb-btn pb-btn--ghost" onclick={onClose} disabled={busy}>Back to the edits</button>
      <button bind:this={saveButton} type="button" class="pb-btn pb-btn--primary" onclick={onSave} disabled={busy} data-tip-end="" data-tip="Save  Ctrl+Enter">
        {#if busy}<Icon name="loader" spin />Saving…{:else}Save {statements.length} {statements.length === 1 ? "change" : "changes"}{/if}
      </button>
    </div>
  </div>
</div>
