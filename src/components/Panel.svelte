<script lang="ts">
  import BrandMark from "./BrandMark.svelte";
  import Hero from "./Hero.svelte";
  import Icon from "./Icon.svelte";
  import ProviderSection from "./ProviderSection.svelte";
  import { api } from "../lib/api";
  import { now } from "../lib/clock";
  import { hhmm } from "../lib/format";
  import { t } from "../lib/i18n";
  import type { AppSnapshot, ProviderReport } from "../lib/types";

  let { snap }: { snap: AppSnapshot } = $props();

  const lang = $derived(snap.settings.resolved_language);
  const thresholds = $derived(snap.settings.thresholds);

  const visible = $derived(
    snap.reports.filter(
      (r) => r.status !== "disabled" && !(r.issue && (r.issue.kind === "not_detected" || r.issue.kind === "missing_key" || (r.issue.kind === "unsupported" && r.issue.reason === "no_subscription"))),
    ),
  );
  // Providers with numbers first, in a stable order; ones needing attention after.
  const ordered = $derived([...visible.filter((r) => r.snapshot), ...visible.filter((r) => !r.snapshot)]);
  const firstLoad = $derived(snap.reports.length === 0 || (snap.overview.any_loading && visible.every((r) => !r.snapshot) && !snap.last_refresh));

  let selectedFilter = $state<string>("all");
  $effect(() => {
    if (snap.active_filter) {
      selectedFilter = snap.active_filter;
    }
  });

  const filteredOrdered = $derived.by(() => {
    if (selectedFilter === "all") return ordered;
    if (selectedFilter === "codex") return ordered.filter((r) => r.id === "codex");
    if (selectedFilter === "grok") return ordered.filter((r) => r.id === "grok");
    if (
      selectedFilter === "agy" ||
      selectedFilter.startsWith("account:") ||
      ["gemini-flash", "argon-4", "claude-gpt"].includes(selectedFilter)
    ) {
      return ordered.filter((r) => r.id === "agy");
    }
    return ordered.filter((r) => r.id === selectedFilter);
  });

  function handleSelectFilter(id: string) {
    selectedFilter = id;
    api.setActiveFilter(id);
  }

  function handleSwitchAccount(email: string) {
    api.switchAgyAccount(email);
  }

  let body: HTMLElement | undefined = $state();
  $effect(() => {
    // Shrink the panel to its content where it hangs from the top of the screen (not Windows,
    // where it grows up from the taskbar and keeps a fixed height).
    if (!body || snap.platform === "windows") return;
    const ro = new ResizeObserver(() => api.fitPanelHeight((body?.offsetHeight ?? 0) + 52 + 48));
    ro.observe(body);
    return () => ro.disconnect();
  });

  function refresh() {
    if (!snap.refreshing) api.refresh();
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") api.hidePanel();
    if ((e.metaKey || e.ctrlKey) && e.key === "r") {
      e.preventDefault();
      refresh();
    }
    if ((e.metaKey || e.ctrlKey) && e.key === ",") api.openSettings();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="panel">
  <header class="top">
    <span class="brand"><BrandMark /> {$t("appTitle")}</span>
    <span class="updated" aria-live="polite">
      {#if snap.refreshing}{$t("refreshing")}{:else if snap.last_refresh}{$t("updatedAt", { time: hhmm(snap.last_refresh, lang) })}{/if}
    </span>
    <button class="btn quiet icon" class:spinning={snap.refreshing} onclick={refresh} disabled={snap.refreshing} title={$t("refreshNow")} aria-label={$t("refreshNow")}>
      <Icon name="refresh" />
    </button>
  </header>

  <main>
    <div bind:this={body}>
    {#if firstLoad}
      <div class="state">
        <span class="pulse"><BrandMark size={28} /></span>
        <p>{$t("loadingTitle")}</p>
      </div>
    {:else if ordered.length === 0}
      <div class="state">
        <BrandMark size={28} />
        <p class="state-title">{$t("emptyTitle")}</p>
        <p class="state-body">{$t("emptyBody")}</p>
        <button class="btn primary" onclick={() => api.openSettings()}><Icon name="key" size={14} />{$t("addKey")}</button>
      </div>
    {:else}
      <div class="hero-wrap">
        <Hero
          overview={snap.overview}
          reports={visible}
          {thresholds}
          {lang}
          now={$now}
          {selectedFilter}
          activeAgyAccount={snap.active_agy_account}
          onSelectFilter={handleSelectFilter}
          onSwitchAccount={handleSwitchAccount}
        />
      </div>
      <div class="list">
        {#each filteredOrdered as report (report.id)}
          <ProviderSection {report} {thresholds} {lang} now={$now} onRetry={refresh} />
        {/each}
      </div>
    {/if}
    </div>
  </main>

  <footer>
    <button class="btn quiet" onclick={() => api.openSettings()}><Icon name="settings" size={14} />{$t("settings")}</button>
    <button class="btn quiet" onclick={() => api.quit()}><Icon name="power" size={14} />{$t("quit")}</button>
  </footer>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--paper);
  }
  .top {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--s-2);
    height: 52px;
    padding: 0 var(--s-2) 0 var(--s-4);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    font-size: var(--fs-md);
    font-weight: 600;
  }
  .updated {
    flex: 1;
    text-align: right;
    font-size: var(--fs-xs);
    color: var(--ink-3);
  }
  .spinning :global(svg) {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
  main {
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .hero-wrap {
    padding-bottom: var(--s-3);
  }
  .list {
    background: var(--surface);
    border-bottom: 1px solid var(--line);
  }
  footer {
    flex: none;
    display: flex;
    justify-content: space-between;
    height: 48px;
    align-items: center;
    padding: 0 var(--s-2);
    border-top: 1px solid var(--line);
    background: var(--paper);
  }
  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-8) var(--s-6);
    text-align: center;
    color: var(--ink-2);
  }
  .state p {
    margin: 0;
    max-width: 30ch;
  }
  .state-title {
    color: var(--ink);
    font-size: var(--fs-lg);
    font-weight: 600;
  }
  .state-body {
    margin-bottom: var(--s-2) !important;
    font-size: var(--fs-sm);
  }
  .pulse {
    animation: pulse 1.4s var(--ease) infinite alternate;
  }
  @keyframes pulse {
    from { opacity: 0.35; }
    to { opacity: 1; }
  }
  main::-webkit-scrollbar { width: 8px; }
  main::-webkit-scrollbar-thumb {
    border-radius: 8px;
    background: var(--line-strong);
    border: 2px solid var(--paper);
  }
</style>
