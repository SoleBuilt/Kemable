<script lang="ts">
  import { errorTitle } from "../errors";
  import type { CoreError } from "../types";

  /** An error inside a narrow pane: its words, then the engine's message. */
  let { error }: { error: CoreError } = $props();
</script>

<div class="pb-tree-error" role="alert"><span class="pb-tree-error-title">{errorTitle(error)}</span><span>{error.message}</span></div>
