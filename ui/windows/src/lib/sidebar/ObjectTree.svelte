<script lang="ts">
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, ObjectInfo, ObjectKind, OpenedConnection, SchemaInfo } from "../types";

  interface Props {
    connection: OpenedConnection;
    /** Bumped by the parent after a run that may have changed the schema. */
    refreshKey: number;
    onOpen: (object: ObjectInfo) => void;
  }

  let { connection, refreshKey, onOpen }: Props = $props();

  interface Group {
    schema: SchemaInfo;
    objects: ObjectInfo[];
    open: boolean;
  }

  let groups: Group[] = $state([]);
  let filter = $state("");
  let error: CoreError | null = $state(null);
  let loading = $state(false);
  let selected: string | null = $state(null);

  const GLYPHS: Partial<Record<ObjectKind, string>> = {
    table: "▦",
    view: "◫",
    materializedView: "◫",
    index: "↯",
    trigger: "ϟ",
    function: "ƒ",
    procedure: "ƒ",
  };

  async function load(conn: number) {
    loading = true;
    error = null;
    try {
      const schemas = await ipc.listSchemas(conn);
      const lists = await Promise.all(schemas.map((schema) => ipc.listObjects(conn, schema.name)));
      const wasOpen = new Map(groups.map((group) => [group.schema.name, group.open]));
      groups = schemas.map((schema, index) => ({
        schema,
        objects: lists[index] ?? [],
        open: wasOpen.get(schema.name) ?? schema.isDefault,
      }));
    } catch (caught) {
      error = asCoreError(caught);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void refreshKey;
    void load(connection.id);
  });

  const needle = $derived(filter.trim().toLowerCase());

  function visible(objects: ObjectInfo[]): ObjectInfo[] {
    return needle ? objects.filter((object) => object.name.toLowerCase().includes(needle)) : objects;
  }

  function key(object: ObjectInfo): string {
    return `${object.schema ?? ""}.${object.kind}.${object.name}`;
  }
</script>

<div class="flex h-full flex-col">
  <div class="border-b border-line p-2">
    <input
      class="h-7 w-full rounded border border-line bg-surface px-2 text-[12px] outline-none select-text placeholder:text-muted focus:border-accent"
      placeholder="Filter"
      spellcheck="false"
      bind:value={filter}
    />
  </div>

  <div class="min-h-0 flex-1 overflow-auto py-1 text-[12.5px]" role="tree">
    {#if error}
      <div class="px-3 py-2 text-danger">
        <div class="font-semibold">{errorTitle(error)}</div>
        <div class="mt-1 text-[11.5px] break-words text-muted">{error.message}</div>
      </div>
    {/if}

    {#each groups as group (group.schema.name)}
      {@const objects = visible(group.objects)}
      <button
        class="flex h-6 w-full items-center gap-1 px-2 text-left font-semibold text-muted hover:bg-surface-3"
        onclick={() => (group.open = !group.open)}
        aria-expanded={group.open}
      >
        <span class="w-3 text-[10px]">{group.open ? "▾" : "▸"}</span>
        <span class="truncate">{group.schema.name}</span>
        <span class="ml-auto text-[11px] font-normal">{objects.length}</span>
      </button>
      {#if group.open}
        {#each objects as object (key(object))}
          <button
            class="flex h-6 w-full items-center gap-2 pr-2 pl-6 text-left hover:bg-surface-3 {selected === key(object)
              ? 'bg-surface-3'
              : ''}"
            role="treeitem"
            aria-selected={selected === key(object)}
            title={`${object.kind} ${object.name}`}
            onclick={() => (selected = key(object))}
            ondblclick={() => onOpen(object)}
          >
            <span class="w-3 text-center text-muted">{GLYPHS[object.kind] ?? "·"}</span>
            <span class="truncate">{object.name}</span>
          </button>
        {/each}
      {/if}
    {/each}

    {#if !loading && !error && groups.every((group) => group.objects.length === 0)}
      <div class="px-3 py-2 text-muted">No tables yet.</div>
    {/if}
  </div>
</div>
