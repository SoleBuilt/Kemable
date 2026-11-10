<script lang="ts">
  import { onMount } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, SavedQuery } from "../types";
  import Icon from "../ui/Icon.svelte";

  interface Props {
    /** The query to edit, or a new one with its SQL filled in. */
    query: SavedQuery;
    /** The saved connection open now; null when it is not a saved one. */
    connectionId: string | null;
    onSaved: () => void;
    onClose: () => void;
  }

  let { query, connectionId, onSaved, onClose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let name = $state(query.name);
  // svelte-ignore state_referenced_locally
  let sql = $state(query.sql);
  // svelte-ignore state_referenced_locally
  let everywhere = $state(query.connectionId === null && query.id !== "" ? true : connectionId === null);
  let error: CoreError | null = $state(null);
  let busy = $state(false);
  let nameInput: HTMLInputElement;

  onMount(() => nameInput.focus());

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    try {
      await ipc.saveQuery({ ...query, name: name.trim(), sql, connectionId: everywhere ? null : connectionId });
      onSaved();
      onClose();
    } catch (caught) {
      error = asCoreError(caught);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape" && !busy) onClose();
  }}
/>

<div class="pb-overlay pb-overlay--fixed" role="presentation">
  <form class="pb-dialog" style="width: 560px" onsubmit={submit} aria-labelledby="save-query-title">
    <div class="pb-dialog-head">
      <h2 class="pb-dialog-title" id="save-query-title">{query.id ? "Edit saved query" : "Save query"}</h2>
      <button type="button" class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label="Close" data-tip-end="" data-tip="Close  Esc" onclick={onClose}
        ><Icon name="x" /></button
      >
    </div>
    <div class="pb-dialog-body">
      <label class="pb-field">
        <span class="pb-label">Name</span>
        <input class="pb-input" bind:this={nameInput} bind:value={name} required spellcheck="false" />
      </label>
      <label class="pb-field">
        <span class="pb-label">SQL</span>
        <textarea class="pb-input pb-mono pb-textarea" bind:value={sql} rows="8" spellcheck="false"></textarea>
        <span class="pb-hint">The editor's selection, or all of its text when nothing is selected.</span>
      </label>
      <label class="pb-check">
        <input type="checkbox" bind:checked={everywhere} disabled={connectionId === null} />
        Show it for every connection{#if connectionId === null}<span class="pb-meta">&nbsp;(this connection is not saved)</span>{/if}
      </label>
      {#if error}
        <div class="pb-info pb-info--danger" role="alert">
          <Icon name="error" />
          <div class="pb-info-text"><span class="pb-info-title">{errorTitle(error)}</span><span class="pb-info-msg">{error.message}</span></div>
        </div>
      {/if}
    </div>
    <div class="pb-dialog-foot">
      <span class="pb-spacer"></span>
      <button type="button" class="pb-btn pb-btn--ghost" onclick={onClose} disabled={busy}>Cancel</button>
      <button type="submit" class="pb-btn pb-btn--primary" disabled={busy || name.trim() === ""} data-tip-end="" data-tip="Save  Enter">Save</button>
    </div>
  </form>
</div>
