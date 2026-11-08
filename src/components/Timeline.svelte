<script lang="ts">
  // Seven days of resets on one axis: each tag is a limit coming back, underlined in the colour
  // of how depleted it is now. Answers "when do I get quota back?" across every tool at once.
  import { levelOf, MONOGRAM, resetClock, windowName } from "../lib/format";
  import { t } from "../lib/i18n";
  import type { Lang, ProviderReport, Thresholds } from "../lib/types";

  let { reports, thresholds, lang, now }: { reports: ProviderReport[]; thresholds: Thresholds; lang: Lang; now: number } = $props();

  const RANGE = 7 * 86_400_000;
  const LEVEL_RANK = { normal: 0, warning: 1, critical: 2 } as const;

  const days = $derived.by(() => {
    const out: { x: number; label: string }[] = [];
    const d = new Date(now);
    d.setHours(24, 0, 0, 0);
    const fmt = new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", { weekday: "short" });
    for (; d.getTime() - now < RANGE; d.setDate(d.getDate() + 1)) {
      out.push({ x: ((d.getTime() - now) / RANGE) * 100, label: fmt.format(d) });
    }
    return out;
  });

  const marks = $derived.by(() => {
    type Raw = { key: string; x: number; level: keyof typeof LEVEL_RANK; later: boolean; mono: string; titles: string[] };
    const byKey = new Map<string, Raw>();
    for (const r of reports) {
      for (const w of r.snapshot?.windows ?? []) {
        if (!w.resets_at) continue;
        const dt = Date.parse(w.resets_at) - now;
        if (dt <= 0) continue;
        // One tag per provider per reset moment; it takes the most depleted window's colour.
        const key = `${r.id}:${Math.round(dt / 600_000)}`;
        const level = levelOf(w, thresholds);
        const title = `${r.name} ${windowName(w, lang)}${w.scope ? ` ${w.scope}` : ""}: ${resetClock(w.resets_at, now, lang)}`;
        const prev = byKey.get(key);
        if (prev) {
          prev.titles.push(title);
          if (LEVEL_RANK[level] > LEVEL_RANK[prev.level]) prev.level = level;
          continue;
        }
        byKey.set(key, { key, x: Math.min(dt / RANGE, 1) * 100, level, later: dt > RANGE, mono: MONOGRAM[r.id], titles: [title] });
      }
    }
    const raw = [...byKey.values()].sort((a, b) => a.x - b.x);
    // Tags are ~8% of the axis wide; neighbours closer than that drop to the next row.
    const lastX: number[] = [];
    return raw.map((m) => {
      let lane = lastX.findIndex((x) => m.x - x >= 8.5);
      if (lane < 0) lane = lastX.length;
      lastX[lane] = m.x;
      return { ...m, lane };
    });
  });
  const lanes = $derived(Math.max(1, ...marks.map((m) => m.lane + 1)));
</script>

{#if marks.length}
  <figure class="timeline" style:--lanes={lanes}>
    <figcaption class="sr-only">{$t("timeline")}</figcaption>
    <div class="plot">
      <div class="axis"></div>
      {#each days as d (d.x)}
        <span class="tick" style:left="{d.x}%"></span>
      {/each}
      {#each marks as m (m.key)}
        <span
          class="tag"
          data-level={m.level}
          class:later={m.later}
          style:left="{m.x}%"
          style:--lane={m.lane}
          title={m.titles.join("\n")}
        >{m.mono}</span>
      {/each}
    </div>
    <div class="days" aria-hidden="true">
      <span class="now">{$t("now")}</span>
      {#each days as d (d.x)}
        {#if d.x > 9 && d.x < 95}<span class="day" style:left="{d.x}%">{d.label}</span>{/if}
      {/each}
    </div>
  </figure>
{/if}

<style>
  .timeline {
    margin: 0;
    padding: 0 12px;
  }
  .plot {
    position: relative;
    height: calc(var(--lanes) * 22px + 4px);
  }
  .axis {
    position: absolute;
    left: 0;
    right: 0;
    top: 10px;
    height: 1px;
    background: var(--line-strong);
  }
  .tick {
    position: absolute;
    top: 6px;
    width: 1px;
    height: 9px;
    background: var(--line-strong);
  }
  .tag {
    position: absolute;
    top: calc(var(--lane) * 22px + 2px);
    transform: translateX(-50%);
    padding: 0 5px;
    border-radius: var(--r-sm);
    background: var(--surface);
    border: 1px solid var(--line-strong);
    font-size: 10px;
    font-weight: 600;
    line-height: 15px;
    color: var(--ink);
    box-shadow: inset 0 -2px 0 var(--tone);
    --tone: var(--ok-fill);
  }
  .tag[data-level="warning"] { --tone: var(--warn-fill); }
  .tag[data-level="critical"] { --tone: var(--crit-fill); }
  .tag.later {
    border-style: dashed;
  }
  .days {
    position: relative;
    height: 16px;
    font-size: var(--fs-xs);
    color: var(--ink-3);
  }
  .now {
    position: absolute;
    left: 0;
    transform: translateX(-4px);
    color: var(--ink-2);
    font-weight: 500;
  }
  .day {
    position: absolute;
    transform: translateX(-50%);
    white-space: nowrap;
  }
</style>
