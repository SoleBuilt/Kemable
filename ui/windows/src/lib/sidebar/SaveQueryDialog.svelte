<script lang="ts">
  import { onMount } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, SavedQuery } from "../types";

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

<div class="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[12vh]" role="presentation">
  <form class="w-130 rounded-lg border border-line bg-surface p-5 shadow-xl" onsubmit={submit}>
    <h2 class="mb-4 text-[14px] font-semibold">{query.id ? "Edit saved query" : "Save query"}</h2>
    <label class="mb-3 flex flex-col gap-1 text-[12px]">
      <span class="text-muted">Name</span>
      <input
        bind:this={nameInput}
        bind:value={name}
        required
        class="h-7 rounded border border-line bg-surface px-2 outline-none select-text focus:border-accent"
        spellcheck="false"
      />
    </label>
    <label class="mb-3 flex flex-col gap-1 text-[12px]">
      <span class="text-muted">SQL</span>
      <textarea
        bind:value={sql}
        rows="8"
        class="rounded border border-line bg-surface p-2 font-mono outline-none select-text focus:border-accent"
        spellcheck="false"
      ></textarea>
    </label>
    <label class="mb-3 flex items-center gap-2 text-[12px]">
      <input type="checkbox" bind:checked={everywhere} disabled={connectionId === null} />
      Show it for every connection
      {#if connectionId === null}<span class="text-muted">(this connection is not saved)</span>{/if}
    </label>
    {#if error}
      <div class="mb-3 text-[12px]">
        <span class="font-semibold text-danger">{errorTitle(error)}</span>
        <span class="text-muted">{error.message}</span>
      </div>
    {/if}
    <div class="flex justify-end gap-2">
      <button type="button" class="toolbar-button" onclick={onClose} disabled={busy}>Cancel</button>
      <button type="submit" class="toolbar-button font-semibold text-accent" disabled={busy || name.trim() === ""}>Save</button>
    </div>
  </form>
</div>
