<script lang="ts">
  import Button from "../ui/Button.svelte";
  import EmptyState from "../ui/EmptyState.svelte";

  /** What the window shows with nothing open: no sidebar; the palette holds the saved connections (D25). */
  interface Props {
    busy: boolean;
    onNew: () => void;
    onOpenFile: () => void;
    onMemory: () => void;
  }

  let { busy, onNew, onOpenFile, onMemory }: Props = $props();

  const KEYS = [
    { keys: ["Ctrl", "K"], label: "Search" },
    { keys: ["Ctrl", "N"], label: "New connection" },
    { keys: ["Ctrl", "O"], label: "Open file" },
  ];
</script>

<main class="pb-welcome">
  <EmptyState icon="database" title="No connection open" text="Search for a saved connection, make a new one, or open a SQLite file." keys={KEYS}>
    <Button variant="primary" icon="plus" disabled={busy} onclick={onNew}>New connection…</Button>
    <Button icon="file" disabled={busy} onclick={onOpenFile}>Open file…</Button>
    <Button variant="ghost" disabled={busy} onclick={onMemory}>Open an in-memory database</Button>
  </EmptyState>
</main>
