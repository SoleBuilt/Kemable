<script lang="ts" module>
  import type { CoreError } from "../types";

  /** What a run said besides its rows. */
  export type Message =
    | { kind: "command"; statement: number; rowsAffected: number | null; changesSchema?: boolean }
    | { kind: "notice"; statement: number | null; text: string }
    | { kind: "switched"; statement: number; database: string }
    | { kind: "error"; statement: number | null; error: CoreError };
</script>

<script lang="ts">
  import { errorTitle } from "../errors";
  import Icon from "../ui/Icon.svelte";
  import Note from "../ui/Note.svelte";

  /** The Messages tab: each command's outcome, notices, database switches and errors, with the statement they came from. */
  interface Props {
    messages: Message[];
    /** The first words of statement `index`, to show under its message. */
    statement: (index: number) => string;
  }

  let { messages, statement }: Props = $props();
  const numberFormat = new Intl.NumberFormat();
</script>

<div class="pb-messages" role="log">
  {#each messages as message, index (index)}
    <div class="pb-msg" class:is-error={message.kind === "error"}>
      {#if message.kind === "error"}
        <Icon name="error" class="pb-bad" />
      {:else if message.kind === "command"}
        <Icon name="check-circle" class="pb-ok" />
      {:else}
        <Icon name="info" />
      {/if}
      <div class="pb-msg-text">
        {#if message.kind === "error"}
          <span><span class="pb-msg-title">{errorTitle(message.error)}</span> <span class="pb-msg-detail">{message.error.message}</span></span>
        {:else if message.kind === "command"}
          <span>{message.rowsAffected === null ? "Done" : `${numberFormat.format(message.rowsAffected)} rows affected`}</span>
        {:else if message.kind === "switched"}
          <span>Switched to {message.database}; the statements after this run there.</span>
        {:else}
          <span>{message.text}</span>
        {/if}
        {#if message.statement !== null}<span class="pb-msg-stmt">{statement(message.statement)}</span>{/if}
      </div>
    </div>
  {:else}
    <Note>No messages.</Note>
  {/each}
</div>
