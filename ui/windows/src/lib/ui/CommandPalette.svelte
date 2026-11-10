<script lang="ts" module>
  import type { IconName } from "./Icon.svelte";

  /** One command: what it says, its key, and what it does. */
  export interface Command {
    id: string;
    group: string;
    label: string;
    meta?: string;
    icon?: IconName;
    /** A connection colour token's name, drawn as a dot in place of an icon. */
    dot?: string;
    keys?: string[];
    disabled?: boolean;
    run: () => void;
  }
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    commands: Command[];
    onClose: () => void;
  }

  let { commands, onClose }: Props = $props();

  let query = $state("");
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();

  /** The commands that match, grouped in the order their groups first appear. */
  const groups = $derived.by(() => {
    const words = query.trim().toLowerCase();
    const shown = commands.filter(
      (command) => !words || `${command.label} ${command.meta ?? ""} ${command.group}`.toLowerCase().includes(words),
    );
    const order: string[] = [];
    const byGroup = new Map<string, Command[]>();
    for (const command of shown) {
      if (!byGroup.has(command.group)) {
        order.push(command.group);
        byGroup.set(command.group, []);
      }
      byGroup.get(command.group)?.push(command);
    }
    return order.map((group) => ({ group, items: byGroup.get(group) ?? [] }));
  });
  const runnable = $derived(groups.flatMap((group) => group.items).filter((command) => !command.disabled));
  const activeId = $derived(runnable[Math.min(active, runnable.length - 1)]?.id ?? null);

  onMount(() => {
    void tick().then(() => input?.focus());
  });

  function run(command: Command | undefined) {
    if (!command || command.disabled) return;
    onClose();
    command.run();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (runnable.length === 0) return;
      active = (active + (event.key === "ArrowDown" ? 1 : -1) + runnable.length) % runnable.length;
      void tick().then(() => list?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" }));
    } else if (event.key === "Enter") {
      event.preventDefault();
      run(runnable.find((command) => command.id === activeId));
    }
  }
</script>

<div class="pb-cmdk-layer">
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="pb-cmdk-scrim" onclick={onClose}></div>
  <div class="pb-cmdk" role="dialog" aria-label="Search connections and commands">
    <div class="pb-cmdk-input">
      <Icon name="search" />
      <input
        bind:this={input}
        placeholder="Type a command or search…"
        aria-label="Type a command or search"
        spellcheck="false"
        autocomplete="off"
        bind:value={query}
        oninput={() => (active = 0)}
        onkeydown={onKeydown}
      />
      <kbd class="pb-kbd">Esc</kbd>
    </div>
    <div class="pb-cmdk-list" role="listbox" aria-label="Commands" bind:this={list}>
      {#each groups as group, index (group.group)}
        <div class="pb-cmdk-group" role="group" aria-label={group.group}>
          {#if index > 0}<div class="pb-cmdk-sep"></div>{/if}
          <div class="pb-cmdk-heading">{group.group}</div>
          {#each group.items as command (command.id)}
            <button
              type="button"
              class="pb-cmdk-item"
              class:is-disabled={command.disabled}
              role="option"
              aria-selected={command.id === activeId}
              aria-disabled={command.disabled}
              tabindex="-1"
              onclick={() => run(command)}
              onmousemove={() => {
                const at = runnable.findIndex((candidate) => candidate.id === command.id);
                if (at >= 0) active = at;
              }}
            >
              {#if command.dot}
                <span class="pb-dot" style:--conn="var(--conn-{command.dot})"></span>
              {:else if command.icon}
                <Icon name={command.icon} />
              {/if}
              <span class="pb-cmdk-label">{command.label}</span>
              {#if command.meta}<span class="pb-cmdk-meta">{command.meta}</span>{/if}
              {#if command.keys}
                <span class="pb-cmdk-end">
                  <span class="pb-keys">{#each command.keys as key (key)}<kbd class="pb-kbd">{key}</kbd>{/each}</span>
                </span>
              {/if}
            </button>
          {/each}
        </div>
      {:else}
        <div class="pb-cmdk-empty">No results found.</div>
      {/each}
    </div>
    <div class="pb-cmdk-foot">
      <span><span class="pb-keys"><kbd class="pb-kbd">↑</kbd><kbd class="pb-kbd">↓</kbd></span>to move</span>
      <span><kbd class="pb-kbd">↵</kbd>to open</span>
      <span><kbd class="pb-kbd">Esc</kbd>to close</span>
    </div>
  </div>
</div>
