<script lang="ts">
  import { untrack } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, KeyInfo, KeyKind } from "../types";

  interface Props {
    conn: number;
    database: string | null;
    /** Bumped after a run that may have changed keys. */
    refreshKey: number;
    /** The key open in the results, to mark it. */
    openKey: string | null;
    onOpen: (key: KeyInfo) => void;
  }

  let { conn, database, refreshKey, openKey, onOpen }: Props = $props();

  /** Keys gathered per click: SCAN steps are a hint, and a step may find none. */
  const WANTED = 200;
  /** Steps per click at most, so a pattern that matches nothing still returns. */
  const MAX_STEPS = 50;

  const KINDS: { kind: KeyKind | null; label: string }[] = [
    { kind: null, label: "All types" },
    { kind: "string", label: "String" },
    { kind: "hash", label: "Hash" },
    { kind: "list", label: "List" },
    { kind: "set", label: "Set" },
    { kind: "sortedSet", label: "Sorted set" },
    { kind: "stream", label: "Stream" },
  ];
  const BADGES: Record<KeyKind, string> = {
    string: "STR",
    hash: "HASH",
    list: "LIST",
    set: "SET",
    sortedSet: "ZSET",
    stream: "XADD",
    other: "?",
  };

  let pattern = $state("");
  let kind: KeyKind | null = $state(null);
  let keys: KeyInfo[] = $state.raw([]);
  let cursor: string | null = $state(null);
  let done = $state(false);
  let loading = $state(false);
  let error: CoreError | null = $state(null);
  /** Bumped for each new scan, so a slow old one cannot add its keys. */
  let generation = 0;

  /** What is typed as a glob; plain text matches anywhere in the name. */
  function glob(text: string): string {
    const trimmed = text.trim();
    if (trimmed === "") return "*";
    return /[*?[]/.test(trimmed) ? trimmed : `*${trimmed}*`;
  }

  async function scan(fresh: boolean) {
    const mine = fresh ? ++generation : generation;
    if (fresh) {
      keys = [];
      cursor = null;
      done = false;
    }
    loading = true;
    error = null;
    let found: KeyInfo[] = [];
    let at = cursor;
    try {
      for (let step = 0; step < MAX_STEPS && found.length < WANTED; step++) {
        const page = await ipc.scanKeys(conn, database, { pattern: glob(pattern), cursor: at, count: 500, kind });
        if (mine !== generation) return;
        found = found.concat(page.keys);
        at = page.cursor;
        if (at === null) break;
      }
      keys = keys.concat(found);
      cursor = at;
      done = at === null;
    } catch (caught) {
      if (mine === generation) error = asCoreError(caught);
    } finally {
      if (mine === generation) loading = false;
    }
  }

  /** Scans again from the start, after a change elsewhere. */
  export function refresh() {
    void scan(true);
  }

  $effect(() => {
    void refreshKey;
    void conn;
    void database;
    void kind;
    // The pattern applies on Enter, not as it is typed.
    untrack(() => void scan(true));
  });

  function ttl(ms: number | null): string {
    if (ms === null) return "";
    const seconds = Math.ceil(ms / 1000);
    if (seconds < 120) return `${seconds} s`;
    if (seconds < 7200) return `${Math.round(seconds / 60)} min`;
    if (seconds < 172_800) return `${Math.round(seconds / 3600)} h`;
    return `${Math.round(seconds / 86_400)} d`;
  }

  const numberFormat = new Intl.NumberFormat();
</script>

<div class="flex h-full flex-col text-[12px]">
  <div class="flex flex-col gap-1.5 border-b border-line p-2">
    <input
      class="h-7 w-full rounded border border-line bg-surface px-2 font-mono outline-none select-text placeholder:font-sans placeholder:text-muted focus:border-accent"
      placeholder="Filter keys: text, or a glob like user:*"
      spellcheck="false"
      bind:value={pattern}
      onkeydown={(event) => {
        if (event.key === "Enter") void scan(true);
      }}
    />
    <div class="flex items-center gap-2">
      <select class="h-6 rounded border border-line bg-surface px-1 text-[11.5px]" bind:value={kind} aria-label="Key type">
        {#each KINDS as choice (choice.label)}
          <option value={choice.kind}>{choice.label}</option>
        {/each}
      </select>
      <span class="ml-auto text-[11.5px] text-muted">
        {numberFormat.format(keys.length)}{done ? "" : "+"} keys
      </span>
      <button class="rounded px-1.5 text-[11.5px] hover:bg-surface-3" onclick={() => scan(true)} disabled={loading} title="Scan again (Ctrl+R)"
        >↻</button
      >
    </div>
  </div>

  <div class="min-h-0 flex-1 overflow-auto py-1">
    {#if error}
      <div class="px-3 py-2">
        <div class="font-semibold text-danger">{errorTitle(error)}</div>
        <div class="text-[11.5px] wrap-break-word text-muted">{error.message}</div>
      </div>
    {/if}
    {#each keys as key (key.name)}
      <button
        class="flex w-full items-center gap-2 px-2 py-0.5 text-left hover:bg-surface-3"
        class:bg-surface-3={openKey === key.name}
        onclick={() => onOpen(key)}
        title={key.name}
      >
        <span class="w-9 shrink-0 rounded bg-surface px-1 text-center text-[10px] font-semibold text-muted">{BADGES[key.kind]}</span>
        <span class="min-w-0 flex-1 truncate font-mono">{key.name}</span>
        {#if key.ttlMs !== null}<span class="shrink-0 text-[10.5px] text-muted" title="Expires in">{ttl(key.ttlMs)}</span>{/if}
      </button>
    {:else}
      {#if !loading && !error}<div class="p-3 text-muted">No keys match.</div>{/if}
    {/each}
    {#if loading}
      <div class="px-3 py-2 text-muted">Scanning…</div>
    {:else if !done}
      <button class="w-full py-2 text-accent hover:bg-surface-3" onclick={() => scan(false)}>Load more</button>
    {/if}
  </div>
</div>
