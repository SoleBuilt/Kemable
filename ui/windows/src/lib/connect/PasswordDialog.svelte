<script lang="ts">
  import { onMount, tick } from "svelte";
  import type { SavedConnection } from "../types";
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Field from "../ui/Field.svelte";
  import Input from "../ui/Input.svelte";
  import Spacer from "../ui/Spacer.svelte";

  /** Asks for the password or token of a saved connection that does not keep it. */
  interface Props {
    saved: SavedConnection;
    /** Where the connection points now, when its kept password was kept for elsewhere (ADR 0033). */
    movedTo?: string | null;
    onSubmit: (password: string) => void;
    onClose: () => void;
  }

  let { saved, movedTo = null, onSubmit, onClose }: Props = $props();
  let password = $state("");
  let input: HTMLInputElement | undefined = $state();
  const secret = $derived(saved.config.target.kind === "http" ? "API token" : "Password");

  onMount(() => void tick().then(() => input?.focus()));
</script>

<Dialog title="Password for {saved.name}" width={420} {onClose} onsubmit={() => onSubmit(password)}>
  {#if movedTo}
    <p class="pb-dialog-text">
      This connection now points to {movedTo}, not where its {secret === "API token" ? "API token" : "password"} was kept for, so the kept one was not sent. Enter it to connect there{saved.savePassword ? "; it is kept for this server from now on" : ""}.
    </p>
  {:else}
    <p class="pb-dialog-text">This connection does not keep its {secret === "API token" ? "API token" : "password"}.</p>
  {/if}
  <Field label={secret}><Input type="password" bind:ref={input} bind:value={password} /></Field>
  {#snippet footer()}
    <Spacer />
    <Button variant="ghost" onclick={onClose}>Cancel</Button>
    <Button variant="primary" type="submit" tip="Connect  Enter" tipAt="end">Connect</Button>
  {/snippet}
</Dialog>
