<script lang="ts">
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, SavedConnection } from "../types";

  interface Props {
    /** Bumped by the parent after a save, so the list reads the library again. */
    refreshKey: number;
    busy: boolean;
    onOpen: (saved: SavedConnection) => void;
    onEdit: (saved: SavedConnection) => void;
  }

  let { refreshKey, busy, onOpen, onEdit }: Props = $props();

  let saved: SavedConnection[] = $state.raw([]);
  let error: CoreError | null = $state(null);
  let confirming: string | null = $state(null);

  const ENGINE_LABELS: Record<string, string> = { postgres: "PG", mssql: "MS", sqlite: "SQ" };

  // Most recently opened first, then by name.
  const ordered = $derived(
    [...saved].sort((a, b) => (b.lastUsed ?? 0) - (a.lastUsed ?? 0) || a.name.localeCompare(b.name)),
  );

  async function load() {
    try {
      saved = await ipc.listSaved();
      error = null;
    } catch (caught) {
      error = asCoreError(caught);
    }
  }

  $effect(() => {
    void refreshKey;
    void load();
  });

  async function remove(entry: SavedConnection) {
    confirming = null;
    try {
      await ipc.deleteSaved(entry.id);
      await load();
    } catch (caught) {
      error = asCoreError(caught);
    }
  }

  function detail(entry: SavedConnection): string {
    const target = entry.config.target;
    if (target.kind === "file") return target.path;
    if (target.kind === "http") return target.endpoint;
    const who = target.user ? `${target.user}@` : entry.config.engine === "mssql" ? "Windows login · " : "";
    const where = target.port ? `${target.host}:${target.port}` : target.host;
    return `${who}${where}${target.database ? `/${target.database}` : ""}`;
  }
</script>

<div class="flex h-full flex-col">
  <div class="border-b border-line px-3 py-2 text-[11.5px] font-semibold tracking-wide text-muted uppercase">
    Saved connections
  </div>
  <div class="min-h-0 flex-1 overflow-auto py-1 text-[12.5px]" role="listbox" aria-label="Saved connections">
    {#if error}
      <div class="px-3 py-2">
        <div class="font-semibold text-danger">{errorTitle(error)}</div>
        <div class="text-[11.5px] wrap-break-word text-muted">{error.message}</div>
      </div>
    {/if}
    {#each ordered as entry (entry.id)}
      <div class="group flex items-center gap-2 px-2 hover:bg-surface-3">
        <button
          class="flex min-w-0 flex-1 items-center gap-2 py-1.5 text-left"
          role="option"
          aria-selected="false"
          disabled={busy}
          title="Double-click or press Enter to connect"
          ondblclick={() => onOpen(entry)}
          onkeydown={(event) => {
            if (event.key === "Enter") onOpen(entry);
          }}
        >
          <span class="w-6 shrink-0 rounded bg-surface-3 text-center text-[10px] font-semibold text-muted">
            {ENGINE_LABELS[entry.config.engine] ?? "··"}
          </span>
          <span class="min-w-0">
            <span class="block truncate">{entry.name}</span>
            <span class="block truncate text-[11px] text-muted">{detail(entry)}</span>
          </span>
        </button>
        {#if confirming === entry.id}
          <span class="flex shrink-0 items-center gap-1 text-[11.5px]">
            Delete?
            <button class="rounded px-1.5 text-danger hover:bg-surface" onclick={() => remove(entry)}>Yes</button>
            <button class="rounded px-1.5 hover:bg-surface" onclick={() => (confirming = null)}>No</button>
          </span>
        {:else}
          <span class="hidden shrink-0 gap-0.5 group-focus-within:flex group-hover:flex">
            <button class="rounded px-1 text-muted hover:text-fg" title="Edit" aria-label="Edit {entry.name}" onclick={() => onEdit(entry)}>✎</button>
            <button
              class="rounded px-1 text-muted hover:text-danger"
              title="Delete"
              aria-label="Delete {entry.name}"
              onclick={() => (confirming = entry.id)}>✕</button
            >
          </span>
        {/if}
      </div>
    {:else}
      {#if !error}
        <div class="px-3 py-2 leading-relaxed text-muted">
          No saved connections yet. Connect to PostgreSQL or SQL Server with "Save this connection" ticked, and it
          shows here.
        </div>
      {/if}
    {/each}
  </div>
</div>
