<script lang="ts">
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Spacer from "../ui/Spacer.svelte";

  /** Asked as a `.sql` file's page closes with text not saved (docs/decisions/0016-sql-files.md). */
  interface Props {
    /** The file's name. */
    name: string;
    onAnswer: (answer: "save" | "drop" | "cancel") => void;
  }

  let { name, onAnswer }: Props = $props();
</script>

<Dialog title={`Save changes to ${name}?`} width={440} onClose={() => onAnswer("cancel")} onsubmit={() => onAnswer("save")}>
  <p class="pb-dialog-text">The page has changes that are not in the file. They are lost if you close it without saving.</p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => onAnswer("drop")}>Don't save</Button>
    <Spacer />
    <Button variant="ghost" onclick={() => onAnswer("cancel")}>Cancel</Button>
    <Button variant="primary" type="submit" tip="Save  Enter" tipAt="end">Save</Button>
  {/snippet}
</Dialog>
