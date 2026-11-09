<script lang="ts">
  import type { Level } from "../lib/types";

  let { remaining, level, label }: { remaining: number; level: Level; label: string } = $props();
</script>

<div
  class="meter"
  data-level={level}
  role="meter"
  aria-label={label}
  aria-valuemin="0"
  aria-valuemax="100"
  aria-valuenow={Math.round(remaining)}
>
  <div class="fill" style:width="{remaining}%"></div>
</div>

<style>
  .meter {
    position: relative;
    height: var(--meter-h);
    border-radius: 99px;
    background: var(--inset);
  }
  .fill {
    height: 100%;
    min-width: 2px;
    border-radius: inherit;
    background: var(--ok-fill);
    transition: width 600ms var(--ease);
  }
  [data-level="warning"] .fill { background: var(--warn-fill); }
  [data-level="critical"] .fill { background: var(--crit-fill); }
</style>
