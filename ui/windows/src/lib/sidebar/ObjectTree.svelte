<script lang="ts">
  import { untrack } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, ObjectInfo, ObjectKind, OpenedConnection, SchemaInfo } from "../types";

  interface Props {
    connection: OpenedConnection;
    /** The database the editor runs in; its node is marked and opened. */
    database: string | null;
    /** Bumped by the parent after a run that may have changed the schema. */
    refreshKey: number;
    onOpen: (object: ObjectInfo, database: string | null) => void;
    /** Double-clicking a database makes the editor run in it. */
    onUseDatabase: (database: string) => void;
  }

  let { connection, database, refreshKey, onOpen, onUseDatabase }: Props = $props();

  interface Group {
    schema: SchemaInfo;
    objects: ObjectInfo[];
    open: boolean;
  }

  /** A database, or for an engine without databases the one unnamed root. */
  interface DatabaseNode {
    name: string | null;
    open: boolean;
    loading: boolean;
    error: CoreError | null;
    groups: Group[] | null;
  }

  let nodes: DatabaseNode[] = $state([]);
  let topError: CoreError | null = $state(null);
  let filter = $state("");
  let selected: string | null = $state(null);

  const GLYPHS: Partial<Record<ObjectKind, string>> = {
    table: "▦",
    view: "◫",
    materializedView: "◫",
    index: "↯",
    trigger: "ϟ",
    function: "ƒ",
    procedure: "ƒ",
    sequence: "#",
  };

  /** Reads the tree again, keeping which nodes were open. */
  export async function refresh() {
    const conn = connection.id;
    topError = null;
    const before = new Map(nodes.map((node) => [node.name, node]));
    try {
      if (connection.engine.hasDatabases) {
        const databases = await ipc.listDatabases(conn);
        nodes = databases.map((info) => ({
          name: info.name,
          open: (before.get(info.name)?.open ?? false) || info.name === database,
          loading: false,
          error: null,
          groups: null,
        }));
      } else {
        nodes = [{ name: null, open: true, loading: false, error: null, groups: null }];
      }
    } catch (caught) {
      topError = asCoreError(caught);
      return;
    }
    await Promise.all(
      nodes.filter((node) => node.open).map((node) => loadNode(node, before.get(node.name)?.groups ?? null)),
    );
  }

  async function loadNode(node: DatabaseNode, previous: Group[] | null) {
    node.loading = true;
    node.error = null;
    try {
      const schemas = await ipc.listSchemas(connection.id, node.name);
      const lists = await Promise.all(schemas.map((schema) => ipc.listObjects(connection.id, node.name, schema.name)));
      const wasOpen = new Map((previous ?? []).map((group) => [group.schema.name, group.open]));
      node.groups = schemas.map((schema, index) => ({
        schema,
        objects: lists[index] ?? [],
        open: wasOpen.get(schema.name) ?? schema.isDefault,
      }));
    } catch (caught) {
      node.error = asCoreError(caught);
    } finally {
      node.loading = false;
    }
  }

  function toggle(node: DatabaseNode) {
    node.open = !node.open;
    if (node.open && node.groups === null && !node.loading) void loadNode(node, null);
  }

  // Reload on a new connection or a refresh request only. `refresh` reads the
  // tree it replaces, so it runs untracked, or every load would trigger another.
  $effect(() => {
    void refreshKey;
    void connection.id;
    untrack(() => void refresh());
  });

  // A database the editor moves to opens in the tree.
  $effect(() => {
    const current = database;
    untrack(() => {
      const node = nodes.find((candidate) => candidate.name === current);
      if (node && !node.open) toggle(node);
    });
  });

  const needle = $derived(filter.trim().toLowerCase());

  function visible(objects: ObjectInfo[]): ObjectInfo[] {
    return needle ? objects.filter((object) => object.name.toLowerCase().includes(needle)) : objects;
  }

  function key(node: DatabaseNode, object: ObjectInfo): string {
    return `${node.name ?? ""}.${object.schema ?? ""}.${object.kind}.${object.name}`;
  }
</script>

<div class="flex h-full flex-col">
  <div class="flex gap-1 border-b border-line p-2">
    <input
      class="h-7 min-w-0 flex-1 rounded border border-line bg-surface px-2 text-[12px] outline-none select-text placeholder:text-muted focus:border-accent"
      placeholder="Filter"
      spellcheck="false"
      bind:value={filter}
    />
    <button
      class="h-7 w-7 shrink-0 rounded text-muted hover:bg-surface-3 hover:text-fg"
      title="Refresh (Ctrl+R)"
      aria-label="Refresh"
      onclick={() => void refresh()}>↻</button
    >
  </div>

  <div class="min-h-0 flex-1 overflow-auto py-1 text-[12.5px]" role="tree">
    {#if topError}
      <div class="px-3 py-2 text-danger">
        <div class="font-semibold">{errorTitle(topError)}</div>
        <div class="mt-1 text-[11.5px] wrap-break-word text-muted">{topError.message}</div>
      </div>
    {/if}

    {#each nodes as node (node.name ?? "")}
      {@const depth = node.name === null ? 0 : 1}
      {#if node.name !== null}
        {@const name = node.name}
        <button
          class="flex h-6 w-full items-center gap-1 px-2 text-left hover:bg-surface-3"
          class:font-semibold={name === database}
          role="treeitem"
          aria-expanded={node.open}
          aria-selected={name === database}
          title={name === database ? `${name}: the editor runs here` : `${name}: double-click to run the editor here`}
          onclick={() => toggle(node)}
          ondblclick={() => onUseDatabase(name)}
        >
          <span class="w-3 text-[10px] text-muted">{node.open ? "▾" : "▸"}</span>
          <span class="w-3 text-center text-muted">⛁</span>
          <span class="truncate">{name}</span>
          {#if name === database}<span class="ml-auto size-1.5 shrink-0 rounded-full bg-accent"></span>{/if}
        </button>
      {/if}

      {#if node.open}
        {#if node.loading && node.groups === null}
          <div class="h-6 text-muted" style:padding-left="{20 + depth * 14}px">Loading…</div>
        {/if}
        {#if node.error}
          <div class="py-1 pr-2 text-danger" style:padding-left="{20 + depth * 14}px">
            <div class="font-semibold">{errorTitle(node.error)}</div>
            <div class="text-[11.5px] wrap-break-word text-muted">{node.error.message}</div>
          </div>
        {/if}
        {#each node.groups ?? [] as group (group.schema.name)}
          {@const objects = visible(group.objects)}
          <button
            class="flex h-6 w-full items-center gap-1 pr-2 text-left font-semibold text-muted hover:bg-surface-3"
            style:padding-left="{8 + depth * 14}px"
            onclick={() => (group.open = !group.open)}
            aria-expanded={group.open}
          >
            <span class="w-3 text-[10px]">{group.open ? "▾" : "▸"}</span>
            <span class="truncate">{group.schema.name}</span>
            <span class="ml-auto text-[11px] font-normal">{objects.length}</span>
          </button>
          {#if group.open}
            {#each objects as object (key(node, object))}
              <button
                class="flex h-6 w-full items-center gap-2 pr-2 text-left hover:bg-surface-3 {selected ===
                key(node, object)
                  ? 'bg-surface-3'
                  : ''}"
                style:padding-left="{24 + depth * 14}px"
                role="treeitem"
                aria-selected={selected === key(node, object)}
                title={`${object.kind} ${object.name}`}
                onclick={() => (selected = key(node, object))}
                ondblclick={() => onOpen(object, node.name)}
              >
                <span class="w-3 text-center text-muted">{GLYPHS[object.kind] ?? "·"}</span>
                <span class="truncate">{object.name}</span>
              </button>
            {/each}
          {/if}
        {/each}
        {#if node.groups && node.groups.every((group) => group.objects.length === 0)}
          <div class="h-6 text-muted" style:padding-left="{20 + depth * 14}px">No tables yet.</div>
        {/if}
      {/if}
    {/each}
  </div>
</div>
