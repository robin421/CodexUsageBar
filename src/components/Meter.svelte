<script lang="ts">
  // Fuel-style meter: the fill is what is left. The tick marks where the fill would be if usage
  // were spread evenly across the window, so a fill short of the tick means burning fast.
  import type { Level } from "../lib/types";

  let { remaining, level, expected, label }: { remaining: number; level: Level; expected: number | null; label: string } = $props();
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
  {#if expected != null}
    <div class="tick" style:left="{expected * 100}%"></div>
  {/if}
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
  .tick {
    position: absolute;
    top: -3px;
    bottom: -3px;
    width: 2px;
    margin-left: -1px;
    border-radius: 1px;
    background: var(--ink);
    opacity: 0.55;
  }
</style>
