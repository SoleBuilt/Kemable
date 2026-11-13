<script lang="ts">
  import Badge from "../ui/Badge.svelte";
  import Button from "../ui/Button.svelte";
  import Separator from "../ui/Separator.svelte";
  import type { LiveSession } from "./session.svelte";

  /**
   * A Live page's tools on the page bar (docs/decisions/0020-live-tab.md):
   * Listen and Stop side by side, each disabled when it cannot act, so the
   * bar does not move as a listening starts and ends; then Follow, Filter,
   * Clear and Row detail.
   */
  interface Props {
    session: LiveSession;
    detailOpen: boolean;
    onListen: () => void;
    onToggleFilters: () => void;
    onToggleDetail: () => void;
  }

  let { session, detailOpen, onListen, onToggleFilters, onToggleDetail }: Props = $props();
</script>

<div class="pb-runbar pb-runbar--inline" role="toolbar" aria-label="Live">
  <Button variant="primary" size="sm" icon="play" disabled={session.busy || !session.hasTopics} tip="Listen  F5" onclick={onListen}>Listen</Button>
  <Button size="sm" icon="stop" disabled={session.status !== "starting" && session.status !== "listening"} tip="Stop listening  Shift+F5" onclick={() => session.stop()}>Stop</Button>
  <Separator />
  <Button
    variant="ghost"
    size="sm"
    icon="to-end"
    open={session.follow}
    aria-pressed={session.follow}
    tip={session.follow ? "Following the newest event. Click to stay where you scroll" : "Keep the newest event in view"}
    onclick={() => (session.follow = !session.follow)}>Follow</Button
  >
  <Button
    variant="ghost"
    size="sm"
    icon="filter"
    open={session.filtersOpen}
    aria-pressed={session.filtersOpen}
    disabled={session.result === null}
    tip="Filter the events held  Ctrl+F"
    onclick={onToggleFilters}>Filter{#if session.filter}<Badge size="sm">{session.filter.conditions.length}</Badge>{/if}</Button
  >
  <Button variant="ghost" size="sm" icon="trash" disabled={session.busy || session.result === null} tip="Forget the events heard" onclick={() => session.clear()}>Clear</Button>
  <Button
    variant="ghost"
    size="sm"
    icon="pane"
    label="Row detail"
    open={detailOpen}
    aria-pressed={detailOpen}
    tip="Row detail  Ctrl+Alt+I"
    tipAt="end"
    onclick={onToggleDetail}
  />
</div>
