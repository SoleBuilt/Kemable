<script lang="ts">
  import Button from "./Button.svelte";

  /** A question asked in place, before something that cannot be undone: "Delete it? Delete · Keep". */
  interface Props {
    question: string;
    action: string;
    busy?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
    size?: "md" | "sm";
  }

  let { question, action, busy = false, onConfirm, onCancel, size = "sm" }: Props = $props();
</script>

<span class="pb-meta">{question}</span>
<Button variant={size === "sm" ? "ghost" : "danger"} {size} class={size === "sm" ? "is-danger" : ""} disabled={busy} onclick={onConfirm}>{action}</Button>
<Button variant="ghost" {size} onclick={onCancel}>Keep</Button>
