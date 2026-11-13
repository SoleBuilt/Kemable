<script lang="ts">
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, SqlFolder, SqlFolderEntry } from "../types";
  import { fileName } from "../pages";
  import Button from "../ui/Button.svelte";
  import ContextMenu from "../ui/ContextMenu.svelte";
  import EmptyState from "../ui/EmptyState.svelte";
  import ErrorNote from "../ui/ErrorNote.svelte";
  import Menu from "../ui/Menu.svelte";
  import MenuItem from "../ui/MenuItem.svelte";
  import MenuLabel from "../ui/MenuLabel.svelte";
  import SearchField from "../ui/SearchField.svelte";
  import TreeRow from "../ui/TreeRow.svelte";

  /**
   * Folders of `.sql` files linked into the sidebar (docs/decisions/0016-sql-files.md):
   * each lists the scripts below it; a click opens one on a query page.
   */
  interface Props {
    /** The folders linked, in the order they were. */
    folders: string[];
    /** Bumped to read the folders again: the pane opened, the window came to the front, a file was saved. */
    refreshKey: number;
    /** The file the page shown is, marked in the list. */
    openPath: string | null;
    onOpen: (path: string) => void;
    onLink: () => void;
    onUnlink: (folder: string) => void;
    /** How many files the folders list, for the pane's header. */
    onCount?: (count: number) => void;
  }

  let { folders, refreshKey, openPath, onOpen, onLink, onUnlink, onCount }: Props = $props();

  type Listing = { folder: SqlFolder } | { error: CoreError };
  let listings: Record<string, Listing> = $state.raw({});
  let filter = $state("");
  /** Linked folders folded shut, and folders below them opened (closed until opened). */
  let shutRoots: string[] = $state([]);
  let openFolders: string[] = $state([]);
  let menu: { folder: string; x: number; y: number } | null = $state(null);

  $effect(() => {
    void refreshKey;
    const wanted = [...folders];
    void Promise.all(
      wanted.map(async (folder): Promise<[string, Listing]> => {
        try {
          return [folder, { folder: await ipc.listSqlFolder(folder) }];
        } catch (caught) {
          return [folder, { error: asCoreError(caught) }];
        }
      }),
    ).then((read) => {
      listings = Object.fromEntries(read);
      onCount?.(read.reduce((total, [, listing]) => total + ("folder" in listing ? listing.folder.entries.filter((entry) => !entry.folder).length : 0), 0));
    });
  });

  const same = (a: string | null, b: string) => a !== null && a.toLowerCase() === b.toLowerCase();

  /** A folder's row is shown when every folder above it is open. */
  function shown(entry: SqlFolderEntry, entries: SqlFolderEntry[]): boolean {
    const parts = entry.relative.split("/");
    for (let depth = 1; depth < parts.length; depth++) {
      const above = parts.slice(0, depth).join("/");
      const folder = entries.find((other) => other.folder && other.relative === above);
      if (folder && !openFolders.includes(folder.path)) return false;
    }
    return true;
  }

  function toggle(list: string[], item: string): string[] {
    return list.includes(item) ? list.filter((other) => other !== item) : [...list, item];
  }

  const needle = $derived(filter.trim().toLowerCase());
</script>

<SearchField placeholder="Find a file" bind:value={filter} />
<div class="pb-side-body">
  {#if folders.length === 0}
    <EmptyState compact icon="folder" title="No folder linked" text="Link a folder of .sql files to open its scripts from here.">
      <Button size="sm" icon="folder-plus" label="Link folder…" onclick={onLink} />
    </EmptyState>
  {:else}
    <div class="pb-tree" role="tree" aria-label="SQL files">
      {#each folders as folder (folder)}
        {@const listing = listings[folder]}
        {@const rootOpen = !shutRoots.includes(folder)}
        {@const entries = listing && "folder" in listing ? listing.folder.entries : []}
        <TreeRow
          depth={0}
          open={needle ? true : rootOpen}
          icon="folder"
          meta={entries.filter((entry) => !entry.folder).length}
          tip={`${folder}\nRight-click to refresh or remove it`}
          onclick={() => (shutRoots = toggle(shutRoots, folder))}
          oncontextmenu={(event) => {
            event.preventDefault();
            menu = { folder, x: event.clientX, y: event.clientY };
          }}>{fileName(folder)}</TreeRow
        >
        {#if listing && "error" in listing}
          <ErrorNote error={listing.error} />
        {:else if needle}
          {#each entries.filter((entry) => !entry.folder && entry.relative.toLowerCase().includes(needle)) as entry (entry.path)}
            <TreeRow depth={1} icon="file" selected={same(openPath, entry.path)} tip={entry.path} onclick={() => onOpen(entry.path)}>{entry.relative}</TreeRow>
          {/each}
        {:else if rootOpen}
          {#each entries as entry (entry.path)}
            {#if shown(entry, entries)}
              {#if entry.folder}
                <TreeRow depth={entry.depth + 1} open={openFolders.includes(entry.path)} icon="folder" onclick={() => (openFolders = toggle(openFolders, entry.path))}
                  >{fileName(entry.relative)}</TreeRow
                >
              {:else}
                <TreeRow depth={entry.depth + 1} icon="file" selected={same(openPath, entry.path)} tip={entry.path} onclick={() => onOpen(entry.path)}
                  >{fileName(entry.relative)}</TreeRow
                >
              {/if}
            {/if}
          {/each}
          {#if listing && "folder" in listing && listing.folder.truncated}
            <div class="pb-tree-note" style:--depth={1}>Only the first 5,000 files are listed</div>
          {:else if listing && entries.length === 0}
            <div class="pb-tree-note" style:--depth={1}>No .sql file in this folder</div>
          {/if}
        {/if}
      {/each}
    </div>
  {/if}
</div>

{#if menu}
  {@const target = menu}
  <ContextMenu x={target.x} y={target.y} onClose={() => (menu = null)}>
    <Menu label={fileName(target.folder)} minWidth={220}>
      <MenuLabel>{fileName(target.folder)}</MenuLabel>
      <MenuItem
        icon="refresh"
        onclick={() => {
          menu = null;
          void ipc.listSqlFolder(target.folder).then(
            (folder) => (listings = { ...listings, [target.folder]: { folder } }),
            (caught: unknown) => (listings = { ...listings, [target.folder]: { error: asCoreError(caught) } }),
          );
        }}>Refresh</MenuItem
      >
      <MenuItem
        icon="folder-plus"
        onclick={() => {
          menu = null;
          onLink();
        }}>Link another folder…</MenuItem
      >
      <MenuItem
        icon="x"
        onclick={() => {
          menu = null;
          onUnlink(target.folder);
        }}>Remove from the sidebar</MenuItem
      >
    </Menu>
  </ContextMenu>
{/if}
