<script lang="ts">
  // A newer Kemable, as its release announces it (docs/decisions/0028-updates.md):
  // what changed, then Update, Later or Skip this version. Update first says what
  // closing the app would stop, then downloads with its progress, and the
  // installer ends the app and starts the new version. A critical fix offers
  // Update alone: the app is not used until it is installed.
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, UpdateInfo, UpdateProgress } from "../types";
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Progress from "../ui/Progress.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    update: UpdateInfo;
    /** What closing the app now would stop or leave unsaved, read when Update is pressed. */
    blockers: () => string[];
    /** Writes what the app keeps; the installer ends the app without asking. */
    beforeInstall: () => Promise<void>;
    onLater: () => void;
    onSkip: () => void;
    /** A critical fix's way out: the app closes, and asks again at the next launch. */
    onQuit: () => void;
  }

  let { update, blockers, beforeInstall, onLater, onSkip, onQuit }: Props = $props();

  type Step = { kind: "offer" } | { kind: "confirm"; items: string[] } | { kind: "download"; progress: UpdateProgress | null } | { kind: "failed"; error: CoreError };
  let step = $state<Step>({ kind: "offer" });
  const busy = $derived(step.kind === "download");

  const megabytes = new Intl.NumberFormat(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const mb = (bytes: number) => megabytes.format(bytes / 1_048_576);

  function begin() {
    if (busy) return;
    const items = blockers();
    if (items.length > 0) step = { kind: "confirm", items };
    else void install();
  }

  async function install() {
    step = { kind: "download", progress: null };
    try {
      await beforeInstall();
      await ipc.installUpdate((progress) => {
        if (step.kind === "download") step = { kind: "download", progress };
      });
      // The installer ends the app; reaching here means it could not start.
      step = { kind: "failed", error: { code: "internal", message: "The installer did not start." } };
    } catch (caught) {
      step = { kind: "failed", error: asCoreError(caught) };
    }
  }

  const title = $derived(update.critical ? `Kemable ${update.version} is a critical fix` : `Kemable ${update.version} is available`);
</script>

<Dialog {title} width={520} {busy} closable={!update.critical} onClose={onLater} onsubmit={step.kind === "confirm" ? () => void install() : begin}>
  {#if step.kind === "offer" || step.kind === "failed"}
    <p class="pb-dialog-text">
      You have Kemable {update.currentVersion}.
      {#if update.critical}This version fixes a serious problem, so Kemable updates before it is used again.{/if}
      Updating closes Kemable, installs {update.version} and opens it again. Your connections, pages and settings stay.
    </p>
    {#if update.notes}<pre class="pb-notes">{update.notes}</pre>{/if}
    {#if step.kind === "failed"}
      <InfoBar tone="danger" title={`The update could not be installed: ${errorTitle(step.error)}`} message={step.error.message} />
    {/if}
  {:else if step.kind === "confirm"}
    <p class="pb-dialog-text">Updating closes Kemable now. Before it does:</p>
    <ul class="pb-dialog-list">
      {#each step.items as item (item)}<li>{item}</li>{/each}
    </ul>
    <p class="pb-dialog-text">Go back to save or stop them, or update anyway.</p>
  {:else}
    {@const progress = step.progress}
    <p class="pb-dialog-text">
      {#if progress === null}Getting ready…{:else if progress.total !== null && progress.downloaded >= progress.total}Starting the installer…{:else if progress.total !== null}Downloading {mb(progress.downloaded)} of {mb(progress.total)} MB…{:else}Downloading {mb(progress.downloaded)} MB…{/if}
    </p>
    <Progress label="Downloading the update" value={progress?.total ? progress.downloaded / progress.total : null} />
  {/if}

  {#snippet footer()}
    {#if step.kind === "confirm"}
      <Spacer />
      <Button variant="ghost" onclick={() => (step = { kind: "offer" })}>Back</Button>
      <Button variant="primary" type="submit" tip="Update anyway  Enter" tipAt="end">Update anyway</Button>
    {:else if step.kind === "download"}
      <Spacer />
      <Button variant="primary" disabled>Updating…</Button>
    {:else if update.critical}
      <Button variant="ghost" onclick={onQuit}>Close Kemable</Button>
      <Spacer />
      <Button variant="primary" type="submit" tip="Update  Enter" tipAt="end">{step.kind === "failed" ? "Try again" : "Update"}</Button>
    {:else}
      <Button variant="ghost" onclick={onSkip}>Skip this version</Button>
      <Spacer />
      <Button variant="ghost" onclick={onLater} tip="Ask again in 3 days  Esc" tipAt="end">Later</Button>
      <Button variant="primary" type="submit" tip="Update  Enter" tipAt="end">{step.kind === "failed" ? "Try again" : "Update"}</Button>
    {/if}
  {/snippet}
</Dialog>
