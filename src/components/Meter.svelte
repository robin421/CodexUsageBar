<script lang="ts">
  import type { Level } from "../lib/types";

  let {
    used = 0,
    remaining,
    level = "normal",
    label = "",
  }: {
    used?: number;
    remaining?: number;
    level?: Level;
    label?: string;
  } = $props();

  const pct = $derived(
    Math.max(0, Math.min(100, used ?? (remaining != null ? 100 - remaining : 0)))
  );
</script>

<div
  class="meter-wrap"
  data-level={level}
  role="meter"
  aria-label={label}
  aria-valuemin="0"
  aria-valuemax="100"
  aria-valuenow={Math.round(pct)}
>
  <div class="track">
    <div class="fill" style:width="{pct}%"></div>
    <div class="thumb" style:left="clamp(4px, {pct}%, calc(100% - 4px))"></div>
  </div>
</div>

<style>
  .meter-wrap {
    position: relative;
    width: 100%;
    padding: 3px 0;
  }
  .track {
    position: relative;
    height: var(--meter-h, 4px);
    border-radius: 99px;
    background: var(--inset);
  }
  .fill {
    height: 100%;
    min-width: 0;
    border-radius: inherit;
    background: var(--accent-fill);
    transition: width 400ms var(--ease);
  }
  .thumb {
    position: absolute;
    top: 50%;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent-fill);
    box-shadow: 0 0 0 1.5px var(--surface), 0 1px 2px rgba(0, 0, 0, 0.18);
    transform: translate(-50%, -50%);
    transition: left 400ms var(--ease);
    pointer-events: none;
  }

  [data-level="warning"] .fill,
  [data-level="warning"] .thumb {
    background: var(--warn-fill);
  }
  [data-level="critical"] .fill,
  [data-level="critical"] .thumb {
    background: var(--crit-fill);
  }
</style>
