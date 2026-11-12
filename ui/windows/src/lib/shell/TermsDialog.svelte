<script lang="ts" module>
  /**
   * The terms in force. Raise it when the Terms of Service or the Privacy Policy
   * change, and everyone is asked once more, on their next launch.
   */
  export const TERMS_VERSION = "2026-11-12";
</script>

<script lang="ts">
  // The terms, asked once before the app is used, by the app alone, installed or copied
  // (docs/decisions/0019-data-folder.md): one sentence with its two links, as CapCut asks.
  import { ipc } from "../ipc";
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";

  interface Props {
    onAgree: () => void;
    /** The app closes: it is not used without the terms. */
    onDisagree: () => void;
  }

  let { onAgree, onDisagree }: Props = $props();
</script>

<Dialog title="Terms of Service and Privacy Policy" width={440} closable={false} onClose={() => {}} onsubmit={onAgree}>
  <p class="pb-dialog-text">
    By clicking “Agree and continue”, you agree to our <Button variant="link" onclick={() => void ipc.openTerms("terms")}>Terms of Service</Button> and acknowledge
    that you have read our <Button variant="link" onclick={() => void ipc.openTerms("privacy")}>Privacy Policy</Button>.
  </p>

  {#snippet footer()}
    <Button variant="ghost" onclick={onDisagree}>Disagree and close</Button>
    <Button variant="primary" type="submit">Agree and continue</Button>
  {/snippet}
</Dialog>
