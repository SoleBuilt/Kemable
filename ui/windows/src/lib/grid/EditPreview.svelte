<script lang="ts">
  import { onMount, tick } from "svelte";
  import { errorTitle } from "../errors";
  import type { CoreError } from "../types";
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    /** The statements Save runs, in order, in one transaction. */
    statements: string[];
    busy: boolean;
    error: CoreError | null;
    onSave: () => void;
    onClose: () => void;
  }

  let { statements, busy, error, onSave, onClose }: Props = $props();
  let saveButton: HTMLButtonElement | undefined = $state();
  let copied = $state(false);
  const saveLabel = $derived(`Save ${statements.length} ${statements.length === 1 ? "change" : "changes"}`);

  onMount(() => void tick().then(() => saveButton?.focus()));

  function copy() {
    void navigator.clipboard.writeText(statements.map((statement) => `${statement};`).join("\n")).then(() => (copied = true));
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && !busy) onSave();
  }}
/>

<Dialog title={saveLabel} width={760} maxHeight="80vh" {busy} {onClose}>
  {#if error}<InfoBar tone="danger" title={errorTitle(error)} message={error.message} />{/if}
  <p class="pb-dialog-text">These statements run in one transaction. If one fails, or names a row that is gone, none of them are saved.</p>
  <pre class="pb-codebox">{statements.map((statement) => `${statement};`).join("\n")}</pre>

  {#snippet footer()}
    <Button variant="ghost" icon={copied ? "check" : "copy"} tip="Copy the statements" tipAt="start" onclick={copy}>{copied ? "Copied" : "Copy SQL"}</Button>
    <Spacer />
    <Button variant="ghost" onclick={onClose} disabled={busy}>Back to the edits</Button>
    <Button bind:ref={saveButton} variant="primary" icon={busy ? "loader" : undefined} spin={busy} disabled={busy} tip="Save  Ctrl+Enter" tipAt="end" onclick={onSave}
      >{busy ? "Saving…" : saveLabel}</Button
    >
  {/snippet}
</Dialog>
