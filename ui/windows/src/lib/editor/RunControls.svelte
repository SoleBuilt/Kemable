<script lang="ts">
  import Button from "../ui/Button.svelte";
  import Combobox from "../ui/Combobox.svelte";
  import Menu from "../ui/Menu.svelte";
  import MenuItem from "../ui/MenuItem.svelte";
  import Popover from "../ui/Popover.svelte";
  import Separator from "../ui/Separator.svelte";
  import { ROW_CAPS } from "../ui/SettingsDialog.svelte";

  /**
   * A query page's tools on the page bar (D16): Run with its menu, and Stop,
   * side by side and each disabled when it cannot act, so the bar never
   * moves as a quick run starts and ends; then the row limit and Save query.
   */
  interface Props {
    running: boolean;
    onRun: (mode: "statement" | "all") => void;
    onStop: () => void;
    rowCap: number | null;
    onRowCap: (cap: number | null) => void;
    /** A key-value console has no row limit and no saved queries. */
    keyValue?: boolean;
    saveDisabled: boolean;
    onSave: () => void;
  }

  let { running, onRun, onStop, rowCap, onRowCap, keyValue = false, saveDisabled, onSave }: Props = $props();

  const numberFormat = new Intl.NumberFormat();
  const limits = ROW_CAPS.map((cap) => ({ value: cap, label: cap === null ? "No limit" : `Limit ${numberFormat.format(cap)}` }));
</script>

<div class="pb-runbar pb-runbar--inline" role="toolbar" aria-label="Run">
  <Popover placement="end">
    {#snippet trigger({ open, toggle })}
      <span class="pb-split">
        <Button variant="primary" size="sm" icon="play" disabled={running} tip="Run statement  Ctrl+Enter" onclick={() => onRun("statement")}>Run</Button>
        <Button variant="primary" size="sm" icon="chev-down" label="Run options" disabled={running} aria-haspopup="menu" aria-expanded={open} onclick={toggle} />
      </span>
    {/snippet}
    {#snippet children(close)}
      <Menu label="Run" minWidth={280}>
        <MenuItem
          icon="play"
          keys={["Ctrl", "↵"]}
          onclick={() => {
            close();
            onRun("statement");
          }}>Run statement</MenuItem
        >
        <MenuItem
          icon="play"
          keys={["Ctrl", "Shift", "↵"]}
          onclick={() => {
            close();
            onRun("all");
          }}>Run all</MenuItem
        >
      </Menu>
    {/snippet}
  </Popover>
  <Button size="sm" icon="stop" disabled={!running} tip="Stop the run  Esc" onclick={onStop}>Stop</Button>
  {#if !keyValue}
    <Combobox label="Row limit" small width={132} placement="end" listWidth={180} options={limits} value={rowCap} onPick={onRowCap} tip="Rows to fetch before stopping" />
    <Separator />
    <Button variant="ghost" size="sm" icon="save" disabled={saveDisabled} tip="Save this query  Ctrl+Shift+D" tipAt="end" onclick={onSave}>Save query</Button>
  {/if}
</div>
