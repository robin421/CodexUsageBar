<script lang="ts">
  import BrandMark from "./BrandMark.svelte";
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

  const agyReport = $derived(snap.reports.find((r) => r.id === "agy"));
  const agyAccounts = $derived.by(() => {
    const accs = new Set<string>();
    for (const w of agyReport?.snapshot?.windows ?? []) {
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

  let selectedAccount = $state<string | null>(null);

  $effect(() => {
    if (!selectedAccount && agyAccounts.length > 0) {
      if (snap.active_agy_account && agyAccounts.includes(snap.active_agy_account)) {
        selectedAccount = snap.active_agy_account;
      } else {
        selectedAccount = agyAccounts[0];
      }
    }
  });

  $effect(() => {
    if (snap.active_filter && snap.active_filter.startsWith("account:")) {
      const email = snap.active_filter.replace("account:", "").split("#")[0];
      if (email && email !== selectedAccount) {
        selectedAccount = email;
      }
    }
  });

  function handleSelectAccount(email: string) {
    selectedAccount = email;
    api.setActiveFilter(`account:${email}`);
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
      <div class="list">
        {#each ordered as report (report.id)}
          <ProviderSection
            {report}
            {thresholds}
            {lang}
            now={$now}
            accounts={report.id === "agy" ? agyAccounts : []}
            {selectedAccount}
            activeAgyAccount={snap.active_agy_account}
            onSelectAccount={handleSelectAccount}
            onSwitchAccount={handleSwitchAccount}
            onRetry={refresh}
          />
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
  .list {
    background: var(--surface);
    border-top: 1px solid var(--line);
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
