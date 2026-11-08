<script lang="ts">
  import Gauge from "./Gauge.svelte";
  import { countdown, levelOf, remaining, resetClock, windowName } from "../lib/format";
  import { t } from "../lib/i18n";
  import type { Lang, Overview, ProviderReport, Thresholds } from "../lib/types";

  let {
    overview,
    reports,
    thresholds,
    lang,
    now,
    selectedFilter = "all",
    onSelectFilter,
  }: {
    overview: Overview;
    reports: ProviderReport[];
    thresholds: Thresholds;
    lang: Lang;
    now: number;
    selectedFilter?: string;
    onSelectFilter?: (id: string) => void;
  } = $props();

  // Selected tool report if filter is not "all"
  const selectedReport = $derived(
    selectedFilter !== "all" ? reports.find((r) => r.id === selectedFilter) : null
  );

  // Derive target window, provider, remaining, and level based on filter
  const current = $derived.by(() => {
    if (selectedReport && selectedReport.snapshot && selectedReport.snapshot.windows.length > 0) {
      const snap = selectedReport.snapshot;
      const usable = snap.windows.filter((win) => remaining(win) > 0);
      const win = usable.length > 0
        ? usable.reduce((min, cur) => remaining(cur) < remaining(min) ? cur : min)
        : snap.windows.reduce((min, cur) => remaining(cur) < remaining(min) ? cur : min);
      const rem = remaining(win);
      const lvl = levelOf(win, thresholds);
      return {
        window: win,
        provider: selectedReport,
        remaining: rem,
        level: lvl,
      };
    }

    const w = overview.tightest_window;
    const provider = reports.find((r) => r.id === overview.tightest_provider);
    return {
      window: w,
      provider: provider,
      remaining: overview.min_remaining,
      level: overview.level,
    };
  });

  const w = $derived(current.window);
  const provider = $derived(current.provider);

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

  function toggleFilter(id: string) {
    if (!onSelectFilter) return;
    if (selectedFilter === id) {
      onSelectFilter("all");
    } else {
      onSelectFilter(id);
    }
  }
</script>

{#if w && provider}
  <section class="hero" data-level={current.level} aria-label={$t("lowestLabel")}>
    <div class="lead">
      <Gauge remaining={current.remaining} level={current.level}>
        <div class="readout">
          <span class="pct">{Math.round(current.remaining ?? 0)}<small>%</small></span>
          <span class="unit">{$t("remaining")}</span>
        </div>
      </Gauge>
      <div class="detail">
        <span class="caption">
          {#if selectedFilter !== "all"}
            {provider.name} {lang === "zh" ? "额度" : "Quota"}
          {:else if current.level === "normal"}
            {lang === "zh" ? "当前可用额度" : "Available Headroom"}
          {:else}
            {$t("lowestLabel")}
          {/if}
        </span>
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
        <button
          type="button"
          class="tool-chip"
          class:active={selectedFilter === "all"}
          onclick={() => toggleFilter("all")}
        >
          <span class="tool-name">{lang === "zh" ? "全部" : "All"}</span>
        </button>

        {#each activeTools as tool (tool.id)}
          <button
            type="button"
            class="tool-chip"
            class:active={selectedFilter === tool.id}
            data-level={tool.level}
            onclick={() => toggleFilter(tool.id)}
          >
            <span class="dot"></span>
            <span class="tool-name">{tool.name}</span>
            <span class="tool-pct">{tool.pct}%</span>
          </button>
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
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: var(--r-sm);
    background: var(--surface-2);
    border: 1px solid var(--line);
    font-size: var(--fs-xs);
    font-family: inherit;
    line-height: 1.2;
    transition: all 0.15s ease;
  }
  .tool-chip:hover {
    background: var(--surface);
    border-color: var(--ink-3);
  }
  .tool-chip.active {
    background: var(--surface);
    border-color: var(--ink);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
  }
  .tool-chip.active .tool-name {
    color: var(--ink);
    font-weight: 600;
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
