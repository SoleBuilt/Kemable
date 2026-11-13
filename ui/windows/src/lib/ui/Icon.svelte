<script lang="ts" module>
  /**
   * Lucide's shapes (24px grid, 1.75 stroke), kept here as path data so no icon
   * package ships with the app (docs/decisions/0012-design-system.md, D3).
   */
  const PATHS = {
    play: ["M6 4.5v15a1 1 0 0 0 1.5.86l12-7.5a1 1 0 0 0 0-1.72l-12-7.5A1 1 0 0 0 6 4.5z"],
    stop: ["M7 5h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2z"],
    "chev-down": ["m6 9 6 6 6-6"],
    "chev-right": ["m9 18 6-6-6-6"],
    "chev-left": ["m15 18-6-6 6-6"],
    "chevs-left": ["m11 17-5-5 5-5", "M18 17l-5-5 5-5"],
    "chevs-right": ["m6 17 5-5-5-5", "M13 17l5-5-5-5"],
    database: ["M3 5a9 3 0 1 0 18 0a9 3 0 1 0-18 0", "M3 5v14a9 3 0 0 0 18 0V5", "M3 12a9 3 0 0 0 18 0"],
    "list-tree": ["M21 12h-8M21 6H8M21 18h-8", "M3 6v4c0 1.1.9 2 2 2h3", "M3 10v6c0 1.1.9 2 2 2h3"],
    folder: ["M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z"],
    table: ["M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M3 9h18M3 15h18M12 9v12"],
    view: ["M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0", "M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0"],
    fn: ["M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M9 17c2 0 2.8-1 2.8-2.8V10c0-2 1-3.3 3.2-3M9 11.2h5.7"],
    plus: ["M5 12h14M12 5v14"],
    radio: ["M4.9 19.1C1 15.2 1 8.8 4.9 4.9", "M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5", "M10 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0", "M16.2 7.8c2.3 2.3 2.3 6.1 0 8.5", "M19.1 4.9C23 8.8 23 15.1 19.1 19"],
    "to-end": ["M12 17V3", "m6 11 6 6 6-6", "M19 21H5"],
    refresh: ["M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5", "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5"],
    filter: ["M3 6h18M7 12h10M10 18h4"],
    search: ["M4 11a7 7 0 1 0 14 0a7 7 0 1 0-14 0", "m20 20-3.5-3.5"],
    x: ["M18 6 6 18M6 6l12 12"],
    error: ["M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0", "M12 8v4M12 16h.01"],
    warning: ["m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3", "M12 9v4M12 17h.01"],
    "check-circle": ["M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0", "m9 12 2 2 4-4"],
    check: ["M20 6 9 17l-5-5"],
    info: ["M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0", "M12 16v-4M12 8h.01"],
    save: ["M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7M7 3v4a1 1 0 0 0 1 1h7"],
    pane: ["M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M15 3v18"],
    "arrow-up": ["m5 12 7-7 7 7M12 19V5"],
    "arrow-down": ["m19 12-7 7-7-7M12 5v14"],
    file: ["M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z", "M14 2v4a2 2 0 0 0 2 2h4"],
    lock: ["M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z", "M7 11V7a5 5 0 0 1 10 0v4"],
    loader: ["M21 12a9 9 0 1 1-6.22-8.56"],
    unplug: ["m19 5 3-3M2 22l3-3", "M6.3 20.3a2.4 2.4 0 0 0 3.4 0L12 18l-6-6-2.3 2.3a2.4 2.4 0 0 0 0 3.4z", "M7.5 13.5 10 11M10.5 16.5 13 14", "m12 6 6 6 2.3-2.3a2.4 2.4 0 0 0 0-3.4l-2.6-2.6a2.4 2.4 0 0 0-3.4 0z"],
    plug: ["M12 22v-5M9 8V2M15 8V2", "M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"],
    key: ["M3 15.5a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0-9 0", "m21 2-9.6 9.6M15.5 7.5l3 3L22 7l-3-3"],
    history: ["M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0", "M12 7v5l3 2"],
    bookmark: ["m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z"],
    settings: ["M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4"],
    copy: ["M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z", "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"],
    trash: ["M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"],
  } satisfies Record<string, string[]>;

  export type IconName = keyof typeof PATHS;
</script>

<script lang="ts">
  let { name, size = "", spin = false, class: extra = "" }: { name: IconName; size?: "" | "sm"; spin?: boolean; class?: string } =
    $props();
</script>

<svg
  class="ic ic-{name} {size === 'sm' ? 'ic-sm' : ''} {spin ? 'ic-spin' : ''} {extra}"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.75"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#each PATHS[name] as d (d)}
    <path {d} />
  {/each}
</svg>
