<script lang="ts">
  import { tick } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, DatabaseInfo } from "../types";

  interface Props {
    conn: number;
    /** The database the editor runs in. */
    database: string | null;
    onPick: (database: string) => void;
  }

  let { conn, database, onPick }: Props = $props();

  let isOpen = $state(false);
  let filter = $state("");
  let databases: DatabaseInfo[] = $state.raw([]);
  let error: CoreError | null = $state(null);
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();

  const shown = $derived.by(() => {
    const needle = filter.trim().toLowerCase();
    return needle ? databases.filter((info) => info.name.toLowerCase().includes(needle)) : databases;
  });

  /** Opens the list, for the toolbar button and Ctrl+K. */
  export async function open() {
    isOpen = true;
    filter = "";
    error = null;
    await tick();
    input?.focus();
    try {
      databases = await ipc.listDatabases(conn);
      active = Math.max(0, databases.findIndex((info) => info.name === database));
    } catch (caught) {
      error = asCoreError(caught);
    }
  }

  function close() {
    isOpen = false;
  }

  function pick(name: string) {
    close();
    if (name !== database) onPick(name);
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      close();
    } else if (event.key === "ArrowDown") {
      active = Math.min(shown.length - 1, active + 1);
    } else if (event.key === "ArrowUp") {
      active = Math.max(0, active - 1);
    } else if (event.key === "Enter") {
      const target = shown[active];
      if (target) pick(target.name);
    } else {
      return;
    }
    event.preventDefault();
  }
</script>

<div class="relative">
  <button
    class="toolbar-button flex max-w-[220px] items-center gap-1"
    onclick={() => (isOpen ? close() : void open())}
    title="Switch database (Ctrl+K)"
  >
    <span class="text-muted">⛁</span>
    <span class="truncate">{database ?? "—"}</span>
    <span class="text-[10px] text-muted">▾</span>
  </button>

  {#if isOpen}
    <button class="fixed inset-0 z-40 cursor-default" aria-label="Close" tabindex="-1" onclick={close}></button>
    <div class="absolute top-8 left-0 z-50 w-[280px] rounded-md border border-line bg-surface p-1.5 shadow-xl">
      <input
        bind:this={input}
        bind:value={filter}
        oninput={() => (active = 0)}
        onkeydown={onKeydown}
        class="mb-1 h-7 w-full rounded border border-line bg-surface px-2 text-[12px] outline-none select-text focus:border-accent"
        placeholder="Switch database…"
        spellcheck="false"
      />
      {#if error}
        <div class="px-2 py-1.5 text-[12px]">
          <div class="font-semibold text-danger">{errorTitle(error)}</div>
          <div class="text-muted">{error.message}</div>
        </div>
      {/if}
      <div class="max-h-[320px] overflow-auto" role="listbox">
        {#each shown as info, index (info.name)}
          <button
            class="flex h-7 w-full items-center gap-2 rounded px-2 text-left text-[12.5px]"
            class:bg-surface-3={index === active}
            role="option"
            aria-selected={index === active}
            onmouseenter={() => (active = index)}
            onclick={() => pick(info.name)}
          >
            <span class="truncate" class:font-semibold={info.name === database}>{info.name}</span>
            {#if info.name === database}<span class="ml-auto text-[11px] text-muted">current</span>{/if}
          </button>
        {:else}
          {#if !error}<div class="px-2 py-1.5 text-[12px] text-muted">No database matches.</div>{/if}
        {/each}
      </div>
    </div>
  {/if}
</div>
