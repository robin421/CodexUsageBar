<script lang="ts">
  import Gauge from "./Gauge.svelte";
  import { countdown, levelOf, remaining, resetClock, windowName } from "../lib/format";
  import { t } from "../lib/i18n";
  import type { Lang, Overview, ProviderReport, QuotaWindow, Thresholds } from "../lib/types";

  let {
    overview,
    reports,
    thresholds,
    lang,
    now,
    selectedFilter = "all",
    selectedAccount = null,
    activeAgyAccount = null,
    onSelectFilter,
    onSelectAccount,
    onSwitchAccount,
  }: {
    overview: Overview;
    reports: ProviderReport[];
    thresholds: Thresholds;
    lang: Lang;
    now: number;
    selectedFilter?: string;
    selectedAccount?: string | null;
    activeAgyAccount?: string | null;
    onSelectFilter?: (id: string) => void;
    onSelectAccount?: (email: string) => void;
    onSwitchAccount?: (email: string) => void;
  } = $props();

  // Bottleneck window helper (strictly minimum remaining percentage)
  function bottleneck(windows: QuotaWindow[]): QuotaWindow | null {
    if (!windows || windows.length === 0) return null;
    return windows.reduce((min, cur) => remaining(cur) < remaining(min) ? cur : min);
  }

  function cleanScope(scope: string | null | undefined, acc: string | null): string {
    if (!scope) return "";
    if (!acc) return scope;
    const short = acc.split("@")[0];
    return scope.replace(` (${short})`, "").trim();
  }

  const agyReport = $derived(reports.find((r) => r.id === "agy"));
  const agyWindows = $derived(agyReport?.snapshot?.windows ?? []);

  // Distinct Antigravity accounts
  const agyAccounts = $derived.by(() => {
    const accs = new Set<string>();
    for (const w of agyWindows) {
      if (w.account) accs.add(w.account);
    }
    if (accs.size === 0 && agyReport?.snapshot?.account) {
      for (const a of agyReport.snapshot.account.split(",")) {
        const trimmed = a.trim();
        if (trimmed) accs.add(trimmed);
      }
    }
    return Array.from(accs);
  });

  // Current active/selected account
  const currentAccount = $derived(
    (selectedFilter.startsWith("account:") ? selectedFilter.replace("account:", "").split("#")[0] : null) ||
    selectedAccount ||
    (activeAgyAccount && agyAccounts.includes(activeAgyAccount) ? activeAgyAccount : null) ||
    agyAccounts[0] ||
    null
  );

  // Distinct Antigravity models/scopes for CURRENT selected account
  const agyModels = $derived.by(() => {
    const models = [
      { id: "gemini-flash", name: "Gemini 3.8 Flash", keyword: "3.8 Flash", icon: "⚡" },
      { id: "argon-4", name: "Argon 4", keyword: "Argon", icon: "🧪" },
      { id: "claude-gpt", name: "Claude & GPT", keyword: "Claude", icon: "🧠" },
    ];
    const accountWindows = currentAccount
      ? agyWindows.filter((w) => w.account === currentAccount || w.scope?.includes(currentAccount.split("@")[0]))
      : agyWindows;

    return models
      .map((m) => {
        const matching = accountWindows.filter((w) => w.scope?.includes(m.keyword));
        const win = bottleneck(matching);
        if (!win) return null;
        const rem = remaining(win);
        return {
          id: m.id,
          name: m.name,
          icon: m.icon,
          pct: Math.round(rem),
          level: levelOf(win, thresholds),
          window: win,
        };
      })
      .filter((m): m is NonNullable<typeof m> => m !== null);
  });

  // Top level active tools summary
  const activeTools = $derived.by(() => {
    return reports
      .filter((r) => r.snapshot && r.snapshot.windows.length > 0)
      .map((r) => {
        const win = bottleneck(r.snapshot!.windows);
        const rem = win ? remaining(win) : 0;
        const lvl = win ? levelOf(win, thresholds) : "normal";
        return {
          id: r.id,
          name: r.name,
          pct: Math.round(rem),
          level: lvl,
          window: win,
        };
      });
  });

  // Selected state derivation
  const current = $derived.by(() => {
    // 1. Antigravity model filter (e.g. gemini-flash, argon-4, claude-gpt, or account:xxx#model)
    let activeModelId = agyModels.find((m) => m.id === selectedFilter)?.id;
    if (!activeModelId && selectedFilter.includes("#")) {
      const modelPart = selectedFilter.split("#")[1];
      activeModelId = agyModels.find((m) => m.id === modelPart)?.id;
    }
    const model = agyModels.find((m) => m.id === activeModelId);
    if (model && agyReport) {
      const shortName = currentAccount ? currentAccount.split("@")[0] : null;
      return {
        title: shortName ? `${model.name} (${shortName})` : model.name,
        caption: lang === "zh" ? `${model.name} 额度` : `${model.name} Quota`,
        window: model.window,
        provider: agyReport,
        remaining: model.pct,
        level: model.level,
      };
    }

    // 2. Account filter (e.g. account:togxida@gmail.com)
    if (selectedFilter.startsWith("account:") && agyReport) {
      const email = selectedFilter.replace("account:", "").split("#")[0];
      const matching = agyWindows.filter((w) => w.account === email || w.scope?.includes(email.split("@")[0]));
      const win = bottleneck(matching);
      if (win) {
        const rem = remaining(win);
        const shortName = email.split("@")[0];
        return {
          title: `Antigravity (${shortName})`,
          caption: lang === "zh" ? `${shortName} 额度` : `${shortName} Quota`,
          window: win,
          provider: agyReport,
          remaining: rem,
          level: levelOf(win, thresholds),
        };
      }
    }

    // 3. Provider filter (e.g. codex, grok, agy)
    const provider = reports.find((r) => r.id === selectedFilter);
    if (provider && provider.snapshot && provider.snapshot.windows.length > 0) {
      let win: QuotaWindow | null = null;
      if (provider.id === "agy" && currentAccount) {
        const matching = provider.snapshot.windows.filter(
          (w) => w.account === currentAccount || w.scope?.includes(currentAccount.split("@")[0])
        );
        win = bottleneck(matching) || bottleneck(provider.snapshot.windows);
      } else {
        win = bottleneck(provider.snapshot.windows);
      }
      if (win) {
        const rem = remaining(win);
        return {
          title: provider.name,
          caption: lang === "zh" ? `${provider.name} 额度` : `${provider.name} Quota`,
          window: win,
          provider: provider,
          remaining: rem,
          level: levelOf(win, thresholds),
        };
      }
    }

    // 4. "all" or default overview
    const w = overview.tightest_window;
    const bestProvider = reports.find((r) => r.id === overview.tightest_provider);
    return {
      title: bestProvider?.name ?? (lang === "zh" ? "总体额度" : "Overview"),
      caption: overview.level === "normal"
        ? (lang === "zh" ? "当前可用额度" : "Available Headroom")
        : (lang === "zh" ? "余量最低" : "Lowest Headroom"),
      window: w,
      provider: bestProvider,
      remaining: overview.min_remaining,
      level: overview.level,
    };
  });

  const w = $derived(current.window);
  const showAgySubbar = $derived(
    selectedFilter === "agy" ||
    agyModels.some((m) => m.id === selectedFilter) ||
    selectedFilter.startsWith("account:") ||
    (agyAccounts.length > 1 && (selectedFilter === "all" || !selectedFilter))
  );

  function select(id: string) {
    if (!onSelectFilter) return;
    onSelectFilter(id);
  }

  function selectAccount(acc: string) {
    if (onSelectAccount) onSelectAccount(acc);
    if (onSelectFilter) onSelectFilter(`account:${acc}`);
  }

  function selectModel(modelId: string) {
    if (!onSelectFilter) return;
    const isCurModelActive = selectedFilter === modelId || selectedFilter === `account:${currentAccount}#${modelId}`;
    if (isCurModelActive) {
      if (currentAccount) {
        selectAccount(currentAccount);
      } else {
        onSelectFilter("agy");
      }
      return;
    }
    if (currentAccount) {
      onSelectFilter(`account:${currentAccount}#${modelId}`);
    } else {
      onSelectFilter(modelId);
    }
  }
</script>

{#if w}
  <section class="hero" data-level={current.level} aria-label={$t("lowestLabel")}>
    <div class="lead">
      <Gauge remaining={current.remaining} level={current.level}>
        <div class="readout">
          <span class="pct">{Math.round(current.remaining ?? 0)}<small>%</small></span>
          <span class="unit">{$t("remaining")}</span>
        </div>
      </Gauge>
      <div class="detail">
        <span class="caption">{current.caption}</span>
        <h2>{current.title}</h2>
        <p class="window">
          {windowName(w, lang)}
          {#if w.scope}<span class="scope">{cleanScope(w.scope, currentAccount)}</span>{/if}
        </p>
        <p class="when">{countdown(w.resets_at, now, lang, current.remaining)}</p>
        {#if w.resets_at}<p class="clock">{resetClock(w.resets_at, now, lang)}</p>{/if}
      </div>
    </div>

    {#if activeTools.length > 0}
      <div class="tool-strip">
        <button
          type="button"
          class="tool-chip"
          class:active={selectedFilter === "all"}
          onclick={() => select("all")}
        >
          <span class="tool-name">{lang === "zh" ? "全部" : "All"}</span>
        </button>

        {#each activeTools as tool (tool.id)}
          <button
            type="button"
            class="tool-chip"
            class:active={selectedFilter === tool.id}
            data-level={tool.level}
            onclick={() => select(tool.id)}
          >
            <span class="dot"></span>
            <span class="tool-name">{tool.name}</span>
            <span class="tool-pct">{tool.pct}%</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if showAgySubbar && (agyModels.length > 0 || agyAccounts.length > 1)}
      <div class="sub-strip">
        {#if agyAccounts.length > 1}
          <div class="account-group">
            <span class="group-label">{lang === "zh" ? "账号" : "Account"}:</span>
            {#each agyAccounts as acc (acc)}
              {@const isCurActive = activeAgyAccount === acc}
              {@const isSelected = currentAccount === acc}
              <button
                type="button"
                class="account-chip"
                class:active={isSelected}
                class:is-active-cli={isCurActive}
                onclick={() => selectAccount(acc)}
                title="{acc} {isCurActive ? (lang === 'zh' ? '(当前CLI活动账号)' : '(Current CLI active)') : ''}"
              >
                <span class="acc-name">{acc.split('@')[0]}</span>
                {#if isCurActive}<span class="active-badge" title={lang === 'zh' ? '当前CLI账号' : 'Active CLI account'}>CLI</span>{/if}
              </button>
            {/each}
            {#if onSwitchAccount && currentAccount}
              {#if activeAgyAccount !== currentAccount}
                <button
                  type="button"
                  class="btn-switch-cli"
                  onclick={() => onSwitchAccount(currentAccount)}
                  title={lang === "zh" ? "将此账号设为 Antigravity CLI 活跃账号" : "Set as active Antigravity CLI account"}
                >
                  {lang === "zh" ? "设为活动账号" : "Set as Active"}
                </button>
              {/if}
            {/if}
          </div>
        {/if}

        {#if agyModels.length > 0}
          <div class="model-group">
            {#each agyModels as m (m.id)}
              {@const isSelected = selectedFilter === m.id || selectedFilter === `account:${currentAccount}#${m.id}`}
              <button
                type="button"
                class="model-chip"
                class:active={isSelected}
                data-level={m.level}
                onclick={() => selectModel(m.id)}
              >
                <span class="m-icon">{m.icon}</span>
                <span class="m-name">{m.name}</span>
                <span class="m-pct">{m.pct}%</span>
              </button>
            {/each}
          </div>
        {/if}
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

  .sub-strip {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px dashed var(--line);
  }
  .account-group, .model-group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .group-label {
    font-size: 11px;
    color: var(--ink-3);
    font-weight: 500;
  }
  .account-chip {
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--line);
    font-size: 11px;
    font-family: inherit;
    color: var(--ink-2);
    transition: all 0.15s ease;
  }
  .account-chip:hover {
    border-color: var(--ink-3);
  }
  .account-chip.active {
    background: var(--inset);
    border-color: var(--ink);
    color: var(--ink);
    font-weight: 600;
  }
  .active-badge {
    font-size: 9px;
    font-weight: 700;
    background: var(--good);
    color: white;
    padding: 1px 4px;
    border-radius: 6px;
  }
  .btn-switch-cli {
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--surface);
    border: 1px solid var(--good);
    font-size: 10px;
    color: var(--good);
    font-weight: 600;
    transition: all 0.15s;
  }
  .btn-switch-cli:hover {
    background: var(--good);
    color: white;
  }

  .model-chip {
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px;
    border-radius: var(--r-sm);
    background: var(--surface-2);
    border: 1px solid var(--line);
    font-size: 11px;
    font-family: inherit;
    color: var(--ink-2);
    transition: all 0.15s ease;
  }
  .model-chip:hover {
    border-color: var(--ink-3);
  }
  .model-chip.active {
    background: var(--surface);
    border-color: var(--ink);
    font-weight: 600;
    color: var(--ink);
  }
  .m-icon {
    font-size: 11px;
  }
  .m-pct {
    font-weight: 600;
  }
  .model-chip[data-level="warning"] .m-pct { color: var(--warn); }
  .model-chip[data-level="critical"] .m-pct { color: var(--crit); }
</style>
