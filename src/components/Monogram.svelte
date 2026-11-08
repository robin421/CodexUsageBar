<script lang="ts">
  import { MONOGRAM } from "../lib/format";
  import type { ProviderId } from "../lib/types";

  let { id, size = 22 }: { id: ProviderId; size?: number } = $props();
</script>

<span class="mono" style:--size="{size}px" aria-hidden="true">{MONOGRAM[id]}</span>

<style>
  .mono {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: var(--size);
    height: var(--size);
    border-radius: calc(var(--size) * 0.3);
    background: var(--inset);
    box-shadow: inset 0 0 0 1px var(--line-strong);
    color: var(--ink);
    font-size: calc(var(--size) * 0.46);
    font-weight: 600;
    letter-spacing: -0.02em;
    line-height: 1;
  }
</style>
