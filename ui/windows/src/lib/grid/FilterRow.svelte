<script lang="ts" module>
  import type { FilterOp } from "../types";

  /** One condition as the row edits it: a column by position, or -1 for a SQL condition. */
  export interface Condition {
    column: number;
    op: FilterOp;
    value: string;
    second: string;
  }

  /** The operators, worded as the design system's Filter row has them. */
  export const OPERATORS: { op: FilterOp; label: string }[] = [
    { op: "equals", label: "is" },
    { op: "notEquals", label: "is not" },
    { op: "contains", label: "contains" },
    { op: "notContains", label: "does not contain" },
    { op: "startsWith", label: "starts with" },
    { op: "endsWith", label: "ends with" },
    { op: "greater", label: "is greater than" },
    { op: "greaterOrEqual", label: "is at least" },
    { op: "less", label: "is less than" },
    { op: "lessOrEqual", label: "is at most" },
    { op: "in", label: "is one of" },
    { op: "between", label: "is between" },
    { op: "isNull", label: "is NULL" },
    { op: "isNotNull", label: "is not NULL" },
  ];

  export function needsValue(op: FilterOp): boolean {
    return op !== "isNull" && op !== "isNotNull";
  }
</script>

<script lang="ts">
  import { tick } from "svelte";
  import Combobox from "../ui/Combobox.svelte";
  import Button from "../ui/Button.svelte";
  import Input from "../ui/Input.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    columns: string[];
    /** The conditions in force, which the draft starts from. */
    conditions: Condition[];
    matchAny: boolean;
    /** A table tab may take a SQL condition; a query result cannot (docs/decisions/0011-filtering-a-result.md). */
    allowRaw?: boolean;
    busy?: boolean;
    onApply: (conditions: Condition[], matchAny: boolean) => void;
  }

  let { columns, conditions, matchAny, allowRaw = false, busy = false, onApply }: Props = $props();

  const RAW = -1;

  // The draft starts from what is in force; the row is made again for each page.
  // svelte-ignore state_referenced_locally
  let draft: Condition[] = $state(conditions.length > 0 ? conditions.map((condition) => ({ ...condition })) : [blank()]);
  // svelte-ignore state_referenced_locally
  let any = $state(matchAny);
  let host: HTMLDivElement | undefined = $state();

  const columnOptions = $derived([
    ...columns.map((name, index) => ({ value: index, label: name })),
    ...(allowRaw ? [{ value: RAW, label: "SQL condition…" }] : []),
  ]);
  const operatorOptions = OPERATORS.map((choice) => ({ value: choice.op, label: choice.label }));

  function blank(): Condition {
    return { column: 0, op: "contains", value: "", second: "" };
  }

  /** Puts the cursor in the first value box. */
  export async function focus() {
    await tick();
    host?.querySelector<HTMLInputElement>("input")?.focus();
  }

  async function add() {
    draft.push(blank());
    await tick();
    const boxes = host?.querySelectorAll<HTMLInputElement>("input[data-value]");
    boxes?.[boxes.length - 1]?.focus();
  }

  function apply() {
    const usable = draft.filter((condition) => !needsValue(condition.op) || condition.value.trim() !== "");
    onApply(
      usable.map((condition) => ({ ...condition })),
      any,
    );
  }

  function clearAll() {
    draft = [blank()];
    onApply([], any);
  }

  function pickColumn(condition: Condition, column: number) {
    if (column === RAW) condition.op = "raw";
    else if (condition.op === "raw") condition.op = "contains";
    condition.column = column;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && event.target instanceof HTMLInputElement) {
      event.preventDefault();
      apply();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="pb-filters pb-filters--rows" bind:this={host} onkeydown={onKeydown}>
  {#each draft as condition, index (index)}
    <div class="pb-cond">
      {#if index === 0}
        <span class="pb-cond-lead">Where</span>
      {:else}
        <!-- One rule joins every condition, as the core's filters do (all or any): the buttons switch together. -->
        <Button size="sm" class="pb-cond-lead" tip="AND or OR. Click to switch" tipAt="start" onclick={() => (any = !any)}>{any ? "OR" : "AND"}</Button>
      {/if}
      <Combobox
        label="Column"
        small
        width={140}
        listWidth={220}
        searchable={columns.length > 8}
        searchPlaceholder="Find a column"
        options={columnOptions}
        value={condition.op === "raw" ? RAW : condition.column}
        onPick={(column) => pickColumn(condition, column)}
      />
      {#if condition.op === "raw"}
        <Input
          data-value
          size="sm"
          mono
          style="flex: 1; min-width: 200px"
          placeholder="total > 100 and status <> 'void'"
          aria-label="SQL condition"
          bind:value={condition.value}
        />
      {:else}
        <Combobox label="Condition" small width={160} options={operatorOptions} value={condition.op} onPick={(op) => (condition.op = op)} />
        {#if needsValue(condition.op)}
          <Input
            data-value
            size="sm"
            style="width: 180px"
            placeholder={condition.op === "between" ? "From" : condition.op === "in" ? "a, b, c" : "Value"}
            aria-label="Value"
            bind:value={condition.value}
          />
        {/if}
        {#if condition.op === "between"}
          <Input size="sm" style="width: 140px" placeholder="To" aria-label="Upper value" bind:value={condition.second} />
        {/if}
      {/if}
      {#if index > 0}
        <Button variant="ghost" size="sm" icon="x" label="Remove the condition" tip="Remove" onclick={() => draft.splice(index, 1)} />
      {/if}
    </div>
  {/each}
  <div class="pb-cond">
    <Button variant="ghost" size="sm" icon="plus" onclick={add}>Condition</Button>
    <Spacer />
    <Button variant="ghost" size="sm" disabled={busy} onclick={clearAll}>Clear</Button>
    <Button variant="primary" size="sm" disabled={busy} tip="Apply  Enter" tipAt="end" onclick={apply}>Apply</Button>
  </div>
</div>
