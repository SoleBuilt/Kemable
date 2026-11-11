<script lang="ts">
  import type { Snippet } from "svelte";
  import logo from "../../../src-tauri/icons/64x64.png";
  import WindowControls from "./WindowControls.svelte";

  /**
   * The window's top bar, in place of the system title bar: the logo, where
   * you are on the left, the search in the middle, the window's buttons on the
   * right. Its empty parts drag the window; a double-click maximizes it.
   */
  interface Props {
    /** The connection's colour token, drawn as the 3 px stripe on top; no stripe without one. */
    conn?: string | null;
    left?: Snippet;
    center?: Snippet;
  }

  let { conn = null, left, center }: Props = $props();
</script>

<header class={["pb-toolbar", "pb-toolbar--center", "pb-titlebar", conn && "has-conn"]} style:--conn={conn ? `var(--conn-${conn})` : undefined} data-tauri-drag-region>
  <div class="pb-tool-left" data-tauri-drag-region>
    <img class="pb-logo" src={logo} alt="Paboable" draggable="false" data-tauri-drag-region />
    {@render left?.()}
  </div>
  {@render center?.()}
  <div class="pb-tool-right" data-tauri-drag-region>
    <WindowControls />
  </div>
</header>
