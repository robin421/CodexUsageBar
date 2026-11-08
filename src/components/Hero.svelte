<script lang="ts">
  import Gauge from "./Gauge.svelte";
  import { countdown, levelOf, remaining, resetClock, windowName } from "../lib/format";
  import { t } from "../lib/i18n";
  import type { Lang, Overview, ProviderReport, Thresholds } from "../lib/types";

  let { overview, reports, thresholds, lang, now }: { overview: Overview; reports: ProviderReport[]; thresholds: Thresholds; lang: Lang; now: number } = $props();

  const w = $derived(overview.tightest_window);
  const provider = $derived(reports.find((r) => r.id === overview.tightest_provider));

  // Clean summary of active tools
  const activeTools = $derived.by(() => {
    return reports
      .filter((r) => r.snapshot && r.snapshot.windows.length > 0)
      .map((r) => {
        const snap = r.snapshot!;
        const usable = snap.windows.filter((win) => remaining(win) > 0);
        const win = usable.length > 0
          ? usable.reduce((min, cur) => remaining(cur) < remaining(min) ? cur : min)
          : snap.windows.reduce((min, cur) => remaining(cur) < remaining(min) ? cur : min);
        const rem = remaining(win);
        const lvl = levelOf(win, thresholds);
        return {
          id: r.id,
          name: r.name,
          pct: Math.round(rem),
          level: lvl,
          scope: win.scope,
        };
      });
  });
</script>

{#if w && provider}
  <section class="hero" data-level={overview.level} aria-label={$t("lowestLabel")}>
    <div class="lead">
      <Gauge remaining={overview.min_remaining} level={overview.level}>
        <div class="readout">
          <span class="pct">{Math.round(overview.min_remaining ?? 0)}<small>%</small></span>
          <span class="unit">{$t("remaining")}</span>
        </div>
      </Gauge>
      <div class="detail">
        <span class="caption">{overview.level === "normal" ? (lang === "zh" ? "当前可用额度" : "Available Headroom") : $t("lowestLabel")}</span>
        <h2>{provider.name}</h2>
        <p class="window">
          {windowName(w, lang)}
          {#if w.scope}<span class="scope">{w.scope}</span>{/if}
        </p>
        <p class="when">{countdown(w.resets_at, now, lang)}</p>
        {#if w.resets_at}<p class="clock">{resetClock(w.resets_at, now, lang)}</p>{/if}
      </div>
    </div>

    {#if activeTools.length > 1}
      <div class="tool-strip">
        {#each activeTools as tool (tool.id)}
          <div class="tool-chip" data-level={tool.level}>
            <span class="dot"></span>
            <span class="tool-name">{tool.name}</span>
            <span class="tool-pct">{tool.pct}%</span>
          </div>
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  .hero {
    margin: 0 var(--s-3);
    padding: var(--s-4) var(--s-4) var(--s-3);
    border-radius: var(--r-lg);
    background: var(--surface);
    border: 1px solid var(--line);
  }
  .hero[data-level="warning"] { border-color: color-mix(in srgb, var(--warn-fill) 45%, var(--line)); }
  .hero[data-level="critical"] { border-color: color-mix(in srgb, var(--crit-fill) 55%, var(--line)); }
  .lead {
    display: flex;
    align-items: center;
    gap: var(--s-4);
  }
  .readout {
    display: flex;
    flex-direction: column;
    align-items: center;
    line-height: 1;
  }
  .pct {
    font-size: var(--fs-hero);
    font-weight: 600;
    letter-spacing: -0.03em;
  }
  .pct small {
    font-size: 14px;
    font-weight: 500;
    margin-left: 1px;
    color: var(--ink-2);
  }
  .unit {
    margin-top: 3px;
    font-size: var(--fs-xs);
    color: var(--ink-3);
  }
  .detail {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .caption {
    font-size: var(--fs-xs);
    color: var(--ink-3);
  }
  h2 {
    margin: 0;
    font-size: var(--fs-lg);
    font-weight: 600;
    line-height: var(--lh-tight);
  }
  p { margin: 0; }
  .window {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-sm);
    color: var(--ink-2);
  }
  .scope {
    padding: 0 5px;
    border-radius: var(--r-sm);
    background: var(--inset);
    font-size: var(--fs-xs);
    font-weight: 500;
    color: var(--ink);
  }
  .when {
    margin-top: 4px;
    font-weight: 500;
  }
  .hero[data-level="warning"] .when { color: var(--warn); }
  .hero[data-level="critical"] .when { color: var(--crit); }
  .clock {
    font-size: var(--fs-sm);
    color: var(--ink-3);
  }
  .tool-strip {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: var(--s-3);
    padding-top: var(--s-3);
    border-top: 1px solid var(--line);
  }
  .tool-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    border-radius: var(--r-sm);
    background: var(--surface-2);
    border: 1px solid var(--line);
    font-size: var(--fs-xs);
    line-height: 1.2;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--good);
  }
  .tool-chip[data-level="warning"] .dot {
    background: var(--warn);
  }
  .tool-chip[data-level="critical"] .dot {
    background: var(--crit);
  }
  .tool-name {
    font-weight: 500;
    color: var(--ink-2);
  }
  .tool-pct {
    font-weight: 600;
    color: var(--ink);
  }
  .tool-chip[data-level="warning"] .tool-pct {
    color: var(--warn);
  }
  .tool-chip[data-level="critical"] .tool-pct {
    color: var(--crit);
  }
</style>
