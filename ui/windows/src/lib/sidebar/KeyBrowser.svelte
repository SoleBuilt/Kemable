<script lang="ts">
  import { untrack } from "svelte";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, KeyInfo, KeyKind } from "../types";
  import Combobox from "../ui/Combobox.svelte";
  import Badge from "../ui/Badge.svelte";
  import Button from "../ui/Button.svelte";
  import ErrorNote from "../ui/ErrorNote.svelte";
  import ListItem from "../ui/ListItem.svelte";
  import Note from "../ui/Note.svelte";
  import SearchField from "../ui/SearchField.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    conn: number;
    database: string | null;
    /** Bumped after a run that may have changed keys. */
    refreshKey: number;
    /** The key open in the results, to mark it. */
    openKey: string | null;
    onOpen: (key: KeyInfo) => void;
    /** Whether a scan is going, for the pane header's spinning icon. */
    onBusy?: (busy: boolean) => void;
  }

  let { conn, database, refreshKey, openKey, onOpen, onBusy }: Props = $props();

  /** Keys gathered per click: SCAN steps are a hint, and a step may find none. */
  const WANTED = 200;
  /** Steps per click at most, so a pattern that matches nothing still returns. */
  const MAX_STEPS = 50;

  const KINDS: { value: KeyKind | null; label: string }[] = [
    { value: null, label: "All types" },
    { value: "string", label: "String" },
    { value: "hash", label: "Hash" },
    { value: "list", label: "List" },
    { value: "set", label: "Set" },
    { value: "sortedSet", label: "Sorted set" },
    { value: "stream", label: "Stream" },
  ];
  const BADGES: Record<KeyKind, string> = {
    string: "string",
    hash: "hash",
    list: "list",
    set: "set",
    sortedSet: "zset",
    stream: "stream",
    other: "other",
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
    onBusy?.(true);
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
      if (mine === generation) {
        loading = false;
        onBusy?.(false);
      }
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
    if (ms === null) return "no expiry";
    const seconds = Math.ceil(ms / 1000);
    if (seconds < 120) return `expires in ${seconds} s`;
    if (seconds < 7200) return `expires in ${Math.round(seconds / 60)} min`;
    if (seconds < 172_800) return `expires in ${Math.round(seconds / 3600)} h`;
    return `expires in ${Math.round(seconds / 86_400)} days`;
  }

  const numberFormat = new Intl.NumberFormat();
</script>

<SearchField
  mono
  placeholder="Text, or a glob like user:*"
  label="Key pattern"
  bind:value={pattern}
  onkeydown={(event) => {
    if (event.key === "Enter") void scan(true);
  }}
>
  {#snippet end()}{#if database !== null}<Badge size="sm" outline>db {database}</Badge>{/if}{/snippet}
</SearchField>
<div class="pb-side-tools">
  <Combobox label="Key type" small width={128} options={KINDS} value={kind} onPick={(value) => (kind = value)} />
</div>
<div class="pb-side-body">
  {#if error}<ErrorNote {error} />{/if}
  <div class="pb-list">
    {#each keys as key (key.name)}
      <ListItem selected={openKey === key.name} onclick={() => onOpen(key)}>
        <span class="pb-list-line"
          ><Badge size="sm" outline class="pb-key-kind">{BADGES[key.kind]}</Badge><span class="pb-list-name pb-mono">{key.name}</span></span
        >
        <span class="pb-meta pb-key-meta">{ttl(key.ttlMs)}</span>
      </ListItem>
    {:else}
      {#if !loading && !error}<Note>No keys match.</Note>{/if}
    {/each}
  </div>
</div>
<div class="pb-side-foot">
  {#if loading}
    <span>Scanning…</span>
  {:else}
    <span>{numberFormat.format(keys.length)}{done ? "" : "+"} keys</span>
    <Spacer />
    {#if !done}<Button variant="link" onclick={() => scan(false)}>Scan more</Button>{/if}
  {/if}
</div>
