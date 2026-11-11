<script lang="ts">
  import Badge from "../ui/Badge.svelte";
  import Button from "../ui/Button.svelte";
  import Separator from "../ui/Separator.svelte";

  /**
   * A table page's tools on the page bar (D15, docs/decisions/0010-editing-rows.md):
   * Filter, Row, Delete rows and Copy SQL; while edits wait, their summary,
   * Discard and Preview & save.
   */
  interface Props {
    /** The table has a primary key, so its rows can be edited. */
    editable: boolean;
    /** Editing is possible now: the page has loaded and nothing runs. */
    canEdit: boolean;
    hasSelection: boolean;
    /** Every page row selected is marked for delete, so the button unmarks them. */
    selectionDeleted: boolean;
    filtersOpen: boolean;
    filterCount: number;
    /** What the unsaved edits are ("2 changed · 1 new"); empty when there are none. */
    editSummary: string;
    running: boolean;
    /** A cell is being edited: its value is not in the edits yet. */
    editing: boolean;
    onToggleFilters: () => void;
    onAddRow: () => void;
    onDeleteRows: () => void;
    onCopySql: () => void;
    onDiscard: () => void;
    onPreview: () => void;
  }

  let {
    editable,
    canEdit,
    hasSelection,
    selectionDeleted,
    filtersOpen,
    filterCount,
    editSummary,
    running,
    editing,
    onToggleFilters,
    onAddRow,
    onDeleteRows,
    onCopySql,
    onDiscard,
    onPreview,
  }: Props = $props();
</script>

{#if editSummary === ""}
  {#if !editable}
    <Badge tone="warning" icon="lock" tip="No primary key, so a row cannot be named for an edit">Read-only</Badge>
  {/if}
  <Button variant="ghost" size="sm" icon="filter" open={filtersOpen} aria-pressed={filtersOpen} tip="Filter  Ctrl+F" onclick={onToggleFilters}
    >Filter{#if filterCount > 0}<Badge size="sm">{filterCount}</Badge>{/if}</Button
  >
  {#if editable}
    <Button variant="ghost" size="sm" icon="plus" disabled={!canEdit} tip="Add a row  Alt+Insert" onclick={onAddRow}>Row</Button>
    <Button variant="ghost" size="sm" icon="trash" disabled={!canEdit || !hasSelection} tip="Mark the selected rows for delete  Delete" onclick={onDeleteRows}>Delete rows</Button>
  {/if}
  <Separator />
  <Button variant="ghost" size="sm" icon="copy" tip="Copy the SELECT this page ran, to edit and run it yourself" tipAt="end" onclick={onCopySql}>Copy SQL</Button>
{:else}
  <Button
    variant="ghost"
    size="sm"
    icon="trash"
    disabled={!canEdit || !hasSelection}
    tip={selectionDeleted ? "Unmark the selected rows  Delete" : "Mark the selected rows for delete  Delete"}
    onclick={onDeleteRows}>{selectionDeleted ? "Keep rows" : "Delete rows"}</Button
  >
  <Badge dot>{editSummary}</Badge>
  <Button size="sm" disabled={running} onclick={onDiscard}>Discard</Button>
  <Button variant="primary" size="sm" disabled={running || editing} tip="See the statements, then save  Ctrl+S" tipAt="end" onclick={onPreview}>Preview &amp; save</Button>
{/if}
