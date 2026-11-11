<script lang="ts">
  import Icon from "./Icon.svelte";
  import Meter from "./Meter.svelte";
  import Monogram from "./Monogram.svelte";
  import { amounts, formatSubLine, levelOf, money, windowName } from "../lib/format";
  import { t } from "../lib/i18n";
  import { api } from "../lib/api";
  import { getDealUrl } from "../lib/affiliate";
  import type { Issue, Lang, ProviderReport, Thresholds } from "../lib/types";

  let {
    report,
    thresholds,
    lang,
    now,
    accounts = [],
    selectedAccount = null,
    activeAgyAccount = null,
    onSelectAccount,
    onSwitchAccount,
    onRetry,
  }: {
    report: ProviderReport;
    thresholds: Thresholds;
    lang: Lang;
    now: number;
    accounts?: string[];
    selectedAccount?: string | null;
    activeAgyAccount?: string | null;
    onSelectAccount?: (email: string) => void;
    onSwitchAccount?: (email: string) => void;
    onRetry: () => void;
  } = $props();

  const snap = $derived(report.snapshot);
  let copied = $state(false);

  const displayWindows = $derived.by(() => {
    if (!snap) return [];
    let list = snap.windows;
    if (report.id === "agy" && selectedAccount && selectedAccount !== "all") {
      const short = selectedAccount.split("@")[0];
      list = list.filter((w) => w.account === selectedAccount || w.scope?.includes(short));
    }
    return list;
  });

  const currentAccount = $derived(
    report.id === "agy" && selectedAccount && selectedAccount !== "all"
      ? selectedAccount
      : (snap?.account || null)
  );

  function cleanScope(scope: string | null | undefined, acc: string | null): string {
    if (!scope) return "";
    if (!acc) return scope;
    const short = acc.split("@")[0];
    return scope.replace(` (${short})`, "").trim();
  }

  async function copy(cmd: string) {
    await api.copy(cmd);
    copied = true;
    setTimeout(() => (copied = false), 1600);
  }

  function issueText(issue: Issue): { title: string; body: string; cmd?: string } {
    const name = report.name;
    switch (issue.kind) {
      case "not_installed":
        return { title: $t("notInstalledTitle", { name }), body: $t("notInstalledBody", { name }) };
      case "not_logged_in":
        return { title: $t("notLoggedInTitle", { name }), body: $t("notLoggedInBody", { cmd: issue.command ?? "" }), cmd: issue.command ?? undefined };
      case "auth_expired":
        return {
          title: $t("expiredTitle"),
          body: report.id === "claude" ? $t("expiredBodyClaude") : $t("expiredBody", { name, cmd: issue.command ?? "" }),
          cmd: issue.command ?? undefined,
        };
      case "unsupported":
        if (issue.reason === "api_key_mode") return { title: $t("apiKeyModeTitle"), body: $t("apiKeyModeBody") };
        if (issue.reason === "no_subscription") return { title: $t("noSubTitle"), body: "" };
        return { title: $t("errorTitle"), body: issue.reason };
      case "error":
        return { title: $t("errorTitle"), body: issue.message };
      default:
        return { title: $t("errorTitle"), body: "" };
    }
  }
</script>

<section class="provider" aria-labelledby="p-{report.id}">
  <header>
    <div class="header-content">
      <div class="service-row">
        <Monogram id={report.id} />
        <h3 id="p-{report.id}">
          {report.name}
          {#if snap?.plan}<span class="plan-badge">{snap.plan}</span>{/if}
          {#if report.beta}<span class="beta">{$t("beta")}</span>{/if}
        </h3>

        {#if report.id === "agy" && accounts.length > 1}
          <div class="account-switcher" role="tablist">
            {#each accounts as acc (acc)}
              {@const isSelected = selectedAccount === acc}
              <button
                type="button"
                class="acc-chip"
                class:selected={isSelected}
                role="tab"
                aria-selected={isSelected}
                onclick={() => onSelectAccount?.(acc)}
                title={acc}
              >
                {acc.split('@')[0]}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      {#if currentAccount}
        <div class="account-row">
          <span class="account" title={currentAccount}>{currentAccount}</span>
        </div>
      {/if}
    </div>
  </header>

  {#if report.status === "stale" && report.issue}
    <p class="stale" role="status">
      <Icon name="alert" size={13} />
      <span>{$t("stale", { error: report.issue.kind === "error" ? report.issue.message : "" })}</span>
    </p>
  {/if}

  {#if snap}
    {#if displayWindows.length === 0 && snap.balances.length === 0}
      <p class="muted">{$t("noWindows")}</p>
    {/if}
    {#each displayWindows as w (w.id)}
      {@const level = levelOf(w, thresholds)}
      {@const amt = amounts(w, lang)}
      {@const cleanedScope = cleanScope(w.scope, report.id === "agy" ? selectedAccount : null)}
      <div class="window" data-level={level}>
        <div class="row">
          <span class="name">
            {#if cleanedScope}
              <span class="model-title">{cleanedScope}</span>
            {/if}
            <span class="window-kind">{windowName(w, lang)}</span>
          </span>
          <span class="pct">{$t("usedPct", { pct: Math.round(w.used_percent) })}</span>
        </div>
        <Meter used={w.used_percent} {level} label={cleanedScope || windowName(w, lang)} />
        <div class="row sub">
          <span class="when">{formatSubLine(w, now, lang)}</span>
          {#if w.used_percent >= 80}
            <button
              type="button"
              class="deal-alert-link"
              onclick={() => api.openUrl(getDealUrl(report.id, 'quota_alert', lang, { usedPercent: w.used_percent, scope: w.scope ?? undefined }))}
              title={$t("dealAlert")}
            >
              {$t("dealAlert")}
            </button>
          {/if}
          {#if amt}
            <span class="right">{amt}</span>
          {/if}
        </div>
      </div>
    {/each}
    {#each snap.balances as b (b.id)}
      <div class="balance">
        <span>{b.id === "reset_credits" ? $t("resetCredits") : $t("balance")}</span>
        <span class="value">{b.id === "reset_credits" && !b.unlimited ? b.amount : money(b, lang)}{#if b.total != null}<span class="of"> / {money({ ...b, amount: b.total }, lang)}</span>{/if}</span>
      </div>
    {/each}
  {:else if report.issue}
    {@const text = issueText(report.issue)}
    <div class="issue">
      <p class="issue-title">{text.title}</p>
      {#if text.body}<p class="issue-body">{text.body}</p>{/if}
      <div class="actions">
        {#if text.cmd}
          <button class="btn" onclick={() => copy(text.cmd!)}>
            <Icon name={copied ? "check" : "copy"} size={13} />{copied ? $t("copied") : $t("copyLogin")}
          </button>
        {/if}
        {#if report.issue.kind === "error"}
          <button class="btn" onclick={onRetry}><Icon name="refresh" size={13} />{$t("retry")}</button>
        {/if}
        {#if report.issue.kind === "not_installed"}
          <button class="btn" onclick={() => api.openUrl(report.dashboard_url)}><Icon name="external" size={13} />{$t("installGuide")}</button>
        {:else}
          <button class="btn quiet" onclick={() => api.openUrl(report.dashboard_url)}>{$t("openDashboard")}</button>
        {/if}
        {#if report.issue.kind === "unsupported" && report.issue.reason === "no_subscription"}
          <button class="btn quiet" onclick={() => api.openUrl(getDealUrl(report.id, 'no_subscription', lang))}>
            <Icon name="tag" size={13} />{$t("dealNoSub")}
          </button>
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .provider {
    padding: var(--s-3) var(--s-4) var(--s-4);
  }
  :global(.provider + .provider) {
    border-top: 1px solid var(--line);
  }
  header {
    margin-bottom: var(--s-3);
  }
  .header-content {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .service-row {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    min-width: 0;
  }
  .account-row {
    display: flex;
    align-items: center;
    padding-left: 0;
    min-width: 0;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: var(--fs-md);
    font-weight: 600;
    line-height: var(--lh-tight);
  }
  .beta {
    padding: 0 4px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm);
    font-size: 10px;
    font-weight: 500;
    line-height: 14px;
    color: var(--ink-3);
  }
  .plan-badge {
    padding: 0 5px;
    border-radius: var(--r-sm);
    background: var(--inset);
    font-size: 10px;
    font-weight: 600;
    line-height: 16px;
    color: var(--ink-2);
  }
  .account-switcher {
    display: inline-flex;
    align-items: center;
    background: var(--inset);
    padding: 2px;
    border-radius: var(--r-md);
    gap: 2px;
    margin-left: auto;
  }
  .acc-chip {
    padding: 2px 8px;
    border-radius: var(--r-sm);
    border: none;
    background: transparent;
    color: var(--ink-2);
    font-size: var(--fs-xs);
    font-weight: 500;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease, box-shadow 0.12s ease;
  }
  .acc-chip:hover {
    color: var(--ink);
  }
  .acc-chip.selected {
    background: var(--paper);
    color: var(--ink);
    font-weight: 600;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
  }
  .account {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--ink-2);
    line-height: var(--lh-tight);
  }
  .stale {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 var(--s-3);
    padding: 5px var(--s-2);
    border-radius: var(--r-md);
    background: var(--warn-wash);
    color: var(--warn);
    font-size: var(--fs-xs);
  }
  .stale span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .window + .window {
    margin-top: var(--s-3);
  }
  .window + .balance {
    margin-top: var(--s-3);
  }
  .balance + .balance {
    margin-top: var(--s-1);
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    margin-bottom: 5px;
  }
  .name {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-size: var(--fs-sm);
    color: var(--ink-2);
  }
  .model-title {
    font-weight: 600;
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .window-kind {
    color: var(--ink-3);
    font-size: var(--fs-xs);
    white-space: nowrap;
  }
  .pct {
    font-size: var(--fs-md);
    font-weight: 600;
  }
  [data-level="warning"] .pct { color: var(--warn); }
  [data-level="critical"] .pct { color: var(--crit); }
  .sub {
    margin: 5px 0 0;
    font-size: var(--fs-xs);
  }
  .when {
    flex: 1;
    color: var(--ink-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .deal-alert-link {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0;
    border: none;
    background: transparent;
    font-family: inherit;
    font-size: var(--fs-xs);
    color: var(--warn);
    white-space: nowrap;
    cursor: pointer;
    line-height: inherit;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .deal-alert-link:hover {
    text-decoration: underline;
    opacity: 0.85;
  }
  [data-level="critical"] .deal-alert-link {
    color: var(--crit);
  }
  .right {
    color: var(--ink-3);
  }
  .balance {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    font-size: var(--fs-sm);
    color: var(--ink-2);
  }
  .balance .value {
    font-weight: 600;
    color: var(--ink);
  }
  .balance .of {
    font-weight: 400;
    color: var(--ink-3);
  }
  .muted {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--ink-3);
  }
  .issue p {
    margin: 0;
  }
  .issue-title {
    font-weight: 500;
  }
  .issue-body {
    margin-top: 2px !important;
    font-size: var(--fs-sm);
    color: var(--ink-2);
    user-select: text;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
    margin-top: var(--s-3);
  }
</style>
