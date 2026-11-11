<script lang="ts">
  import { errorTitle } from "../errors";
  import type { CoreError } from "../types";

  import { tipAttrs } from "./tip";

  /**
   * An error inside a narrow pane: its words, then the engine's message.
   * `clamp` keeps it to three lines, with the whole message in its tip.
   */
  let { error, clamp = false }: { error: CoreError; clamp?: boolean } = $props();
</script>

<div class={["pb-tree-error", clamp && "is-clamped"]} role="alert" {...tipAttrs(clamp ? error.message : undefined, "start")}>
  <span class="pb-tree-error-title">{errorTitle(error)}</span><span>{error.message}</span>
</div>
