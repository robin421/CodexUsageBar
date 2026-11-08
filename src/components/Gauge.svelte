<script lang="ts">
  // The brand ring: arc = remaining share, clockwise from 12 o'clock, with the end dot.
  import type { Level } from "../lib/types";

  let {
    remaining,
    level,
    size = 84,
    stroke = 8,
    children,
  }: { remaining: number | null; level: Level | null; size?: number; stroke?: number; children?: import("svelte").Snippet } = $props();

  const r = $derived(size / 2 - stroke / 2 - 2);
  const c = $derived(2 * Math.PI * r);
  const frac = $derived(remaining == null ? 0 : Math.max(0, Math.min(1, remaining / 100)));
  const angle = $derived(-Math.PI / 2 + frac * 2 * Math.PI);
  const dot = $derived({ x: size / 2 + r * Math.cos(angle), y: size / 2 + r * Math.sin(angle) });
</script>

<div class="gauge" data-level={level ?? "none"} style:width="{size}px" style:height="{size}px">
  <svg width={size} height={size} viewBox="0 0 {size} {size}" aria-hidden="true">
    <circle class="track" cx={size / 2} cy={size / 2} {r} stroke-width={stroke} fill="none" />
    {#if frac > 0.003}
      <circle
        class="arc"
        cx={size / 2}
        cy={size / 2}
        {r}
        stroke-width={stroke}
        fill="none"
        stroke-linecap="round"
        stroke-dasharray="{c * frac} {c}"
        transform="rotate(-90 {size / 2} {size / 2})"
      />
      <circle class="dot" cx={dot.x} cy={dot.y} r={stroke * 0.85} />
    {/if}
  </svg>
  <div class="center">{@render children?.()}</div>
</div>

<style>
  .gauge {
    position: relative;
    flex: none;
    --tone: var(--ink-3);
  }
  .gauge[data-level="normal"] { --tone: var(--ok-fill); }
  .gauge[data-level="warning"] { --tone: var(--warn-fill); }
  .gauge[data-level="critical"] { --tone: var(--crit-fill); }
  .track { stroke: var(--inset); }
  .arc {
    stroke: var(--tone);
    transition: stroke-dasharray 600ms var(--ease);
  }
  .dot {
    fill: var(--tone);
    stroke: var(--surface);
    stroke-width: 2;
  }
  svg { display: block; }
  .center {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    text-align: center;
  }
</style>
