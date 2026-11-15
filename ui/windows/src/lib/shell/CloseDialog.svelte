<script lang="ts">
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Spacer from "../ui/Spacer.svelte";

  /**
   * Asked as the window closes while closing would stop or lose work: the same list an
   * update reads before it closes the app (docs/decisions/0028-updates.md).
   */
  interface Props {
    /** One sentence per thing that stops or is lost. */
    items: string[];
    onBack: () => void;
    onClose: () => void;
  }

  let { items, onBack, onClose }: Props = $props();
</script>

<Dialog title="Close Kemable?" width={460} onClose={onBack} onsubmit={onClose}>
  <p class="pb-dialog-text">Closing Kemable now:</p>
  <ul class="pb-dialog-list">
    {#each items as item (item)}<li>{item}</li>{/each}
  </ul>
  <p class="pb-dialog-text">Go back to save or stop them, or close anyway.</p>
  {#snippet footer()}
    <Spacer />
    <Button variant="ghost" onclick={onBack} tip="Back  Esc" tipAt="end">Back</Button>
    <Button variant="danger" type="submit" tip="Close anyway  Enter" tipAt="end">Close anyway</Button>
  {/snippet}
</Dialog>
