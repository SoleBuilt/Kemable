<script lang="ts" module>
  import type { IconName } from "./Icon.svelte";

  /** One choice of a Combobox. */
  export interface ComboOption<T> {
    value: T;
    label: string;
    /** A second, muted line under the label. */
    meta?: string;
    icon?: IconName;
    /** A connection colour token's name ("red", "teal"), drawn as a dot before the label. */
    dot?: string;
    disabled?: boolean;
    /** A button at the option's end that acts on it without picking it (Connect now). */
    action?: { icon: IconName; label: string; run: () => void };
  }
</script>

<script lang="ts" generics="T">
  import { tick } from "svelte";
  import { placeBy } from "./place";
  import Button from "./Button.svelte";
  import Dot from "./Dot.svelte";
  import Icon from "./Icon.svelte";
  import SearchField from "./SearchField.svelte";
  import { tipAttrs } from "./tip";

  interface Props {
    options: ComboOption<T>[];
    value: T;
    onPick: (value: T) => void;
    /** The accessible name of the trigger and its list. */
    label: string;
    /** The trigger's width in px. */
    width?: number;
    small?: boolean;
    /** A text box at the top of the list that filters it. */
    searchable?: boolean;
    searchPlaceholder?: string;
    /** Open toward the left edge (near the window's right side) or upward (near the bottom). */
    placement?: "start" | "end" | "up" | "up-end";
    /** The list's width when it differs from the trigger's. */
    listWidth?: number;
    disabled?: boolean;
    /** Text for when the value matches no option. */
    placeholder?: string;
    tip?: string;
    /** Called as the list opens, to read fresh options. */
    onOpen?: () => void;
    /**
     * With `searchable`, what the typed text picks when no option is named
     * so: a value not in the list, such as a database typed by hand.
     */
    custom?: (text: string) => ComboOption<T>;
  }

  let {
    options,
    value,
    onPick,
    label,
    width = 160,
    small = false,
    searchable = false,
    searchPlaceholder = "Find",
    placement = "start",
    listWidth,
    disabled = false,
    placeholder = "",
    tip,
    onOpen,
    custom,
  }: Props = $props();

  let open = $state(false);
  let query = $state("");
  let active = $state(0);
  let root: HTMLDivElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();
  let popStyle = $state("");
  let list: HTMLDivElement | undefined = $state();

  const current = $derived(options.find((option) => option.value === value));
  const matching = $derived(
    query.trim() === ""
      ? options
      : options.filter((option) => `${option.label} ${option.meta ?? ""}`.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const typed = $derived.by(() => {
    const text = query.trim();
    if (!custom || text === "" || options.some((option) => option.label.toLowerCase() === text.toLowerCase())) return null;
    return custom(text);
  });
  const shown = $derived(typed ? [typed, ...matching] : matching);

  async function show() {
    if (disabled || !root) return;
    onOpen?.();
    // Fixed to the window, so a bar that clips (the page bar) neither cuts the list off nor scrolls to show it.
    const box = root.getBoundingClientRect();
    popStyle = placeBy(box, placement, listWidth ? { width: listWidth } : { minWidth: box.width });
    open = true;
    query = "";
    active = Math.max(0, options.findIndex((option) => option.value === value));
    await tick();
    if (searchable) list?.querySelector<HTMLInputElement>("input")?.focus({ preventScroll: true });
    else list?.querySelector<HTMLElement>("[role=option][aria-selected=true], [role=option]")?.focus({ preventScroll: true });
    // In a long list the picked option may sit below the fold: bring it into the list's view.
    list?.querySelector<HTMLElement>("[role=option][aria-selected=true]")?.scrollIntoView({ block: "nearest" });
  }

  function close(refocus = true) {
    open = false;
    if (refocus) trigger?.focus();
  }

  function choose(option: ComboOption<T> | undefined) {
    if (!option || option.disabled) return;
    close();
    if (option.value !== value) onPick(option.value);
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (shown.length === 0) return;
      active = (active + (event.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length;
      list?.querySelectorAll<HTMLElement>("[role=option]")[active]?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(shown[active]);
    } else if (event.key === "Tab") {
      close(false);
    }
  }
</script>

<svelte:window
  onresize={() => {
    if (open) close(false);
  }}
/>

<div class="pb-combo" style:width="{width}px" bind:this={root}>
  <button
    bind:this={trigger}
    type="button"
    class="pb-input pb-select pb-combo-trigger"
    class:pb-input--sm={small}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={label}
    {...tipAttrs(tip)}
    {disabled}
    onclick={() => (open ? close() : void show())}
    onkeydown={(event) => {
      if (!open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
        event.preventDefault();
        void show();
      }
    }}
  >
    {#if current?.dot}<Dot color={current.dot} />{/if}
    {#if current?.icon}<Icon name={current.icon} class="pb-combo-icon" />{/if}
    <span class="pb-combo-text" class:is-empty={!current}>{current?.label ?? placeholder}</span>
  </button>
  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="pb-clickaway" onclick={() => close()}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="pb-pop" style={popStyle} bind:this={list} onkeydown={onKeydown}>
      <div class="pb-menu" role="listbox" aria-label={label}>
        {#if searchable}
          <SearchField variant="menu" placeholder={searchPlaceholder} bind:value={query} oninput={() => (active = 0)} />
        {/if}
        {#each shown as option, index (option.value)}
          <div
            class="pb-option"
            class:is-active={index === active}
            role="option"
            tabindex="-1"
            aria-selected={option.value === value}
            aria-disabled={option.disabled}
            onclick={() => choose(option)}
            onkeydown={() => {}}
            onmousemove={() => (active = index)}
          >
            {#if option.dot}<Dot color={option.dot} />{/if}
            {#if option.icon}<Icon name={option.icon} class="pb-option-icon" />{/if}
            {#if option.meta}
              <span class="pb-option-text"><span>{option.label}</span><span>{option.meta}</span></span>
            {:else}
              <span class="pb-option-label">{option.label}</span>
            {/if}
            {#if option.value === value}<Icon name="check" />{/if}
            {#if option.action}
              {@const action = option.action}
              <Button
                variant="ghost"
                size="sm"
                class="pb-option-action"
                icon={action.icon}
                label={action.label}
                tip={action.label}
                tipAt="end"
                onclick={(event) => {
                  event.stopPropagation();
                  close(false);
                  action.run();
                }}
              />
            {/if}
          </div>
        {:else}
          <div class="pb-cmdk-empty" style="padding: 12px">Nothing matches.</div>
        {/each}
      </div>
    </div>
  {/if}
</div>
