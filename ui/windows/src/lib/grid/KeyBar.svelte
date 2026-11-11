<script lang="ts">
  import type { KeyDetail } from "../types";
  import Badge from "../ui/Badge.svelte";
  import Button from "../ui/Button.svelte";
  import Count from "../ui/Count.svelte";
  import Spacer from "../ui/Spacer.svelte";

  /** A Redis key open in the results: its name, type, size and expiry (docs/decisions/0008-key-value-engines.md). */
  interface Props {
    name: string;
    detail: KeyDetail;
    busy: boolean;
    onReload: () => void;
    onCopy: () => void;
  }

  let { name, detail, busy, onReload, onCopy }: Props = $props();

  const UNITS: Record<KeyDetail["kind"], string> = {
    string: "bytes",
    hash: "fields",
    list: "items",
    set: "members",
    sortedSet: "members",
    stream: "entries",
    other: "",
  };
  const numberFormat = new Intl.NumberFormat();

  function expiresIn(ms: number | null): string {
    if (ms === null) return "no expiry";
    const seconds = Math.ceil(ms / 1000);
    return seconds < 120 ? `expires in ${seconds} s` : `expires in ${numberFormat.format(Math.round(seconds / 60))} min`;
  }
</script>

<div class="pb-bar pb-keybar">
  <span class="pb-mono pb-key-name">{name}</span>
  <Badge size="sm" outline>{detail.kind === "sortedSet" ? "zset" : detail.kind}</Badge>
  {#if UNITS[detail.kind]}<Count value="{numberFormat.format(detail.length)} {UNITS[detail.kind]}" />{/if}
  <Count value={expiresIn(detail.ttlMs)} />
  <Spacer />
  <Button variant="ghost" size="sm" icon="refresh" disabled={busy} tip="Read the key again" onclick={onReload}>Reload</Button>
  <Button variant="ghost" size="sm" icon="copy" label="Copy the key name" tip="Copy the key name" tipAt="end" onclick={onCopy} />
</div>
