<script lang="ts">
  import { onMount } from "svelte";
  import BrandMark from "./BrandMark.svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import Monogram from "./Monogram.svelte";
  import Switch from "./Switch.svelte";
  import { api } from "../lib/api";
  import { ENV_KEYS } from "../lib/format";
  import { t, type Key } from "../lib/i18n";
  import type { AppSnapshot, DiagnosticEntry, DisplayMode, ProviderId, ProviderReport, SettingsPatch } from "../lib/types";

  let { snap }: { snap: AppSnapshot } = $props();
  const s = $derived(snap.settings);

  type Tab = "general" | "providers" | "alerts" | "advanced";
  const tabs: { id: Tab; label: Key; icon: IconName }[] = [
    { id: "general", label: "tabGeneral", icon: "sliders" },
    { id: "providers", label: "tabProviders", icon: "plug" },
    { id: "alerts", label: "tabAlerts", icon: "bell" },
    { id: "advanced", label: "tabAdvanced", icon: "wrench" },
  ];
  let tab: Tab = $state((location.hash.split("/")[2] as Tab) || "general");

  let autostart = $state(false);
  let diag: DiagnosticEntry[] = $state([]);
  let drafts: Partial<Record<ProviderId, string>> = $state({});
  let saving: ProviderId | null = $state(null);

  onMount(() => {
    api.autostart().then((v) => (autostart = v)).catch(() => {});
    api.diagnostics().then((d) => (diag = d));
  });

  const update = (patch: SettingsPatch) => api.updateSettings(patch);

  const local = $derived(snap.reports.filter((r) => r.auth === "local"));
  const keyed = $derived(snap.reports.filter((r) => r.auth === "api_key"));

  function toggleProvider(id: ProviderId, on: boolean) {
    const set = new Set(s.disabled);
    if (on) set.delete(id);
    else set.add(id);
    update({ disabled: [...set] });
  }

  async function saveKey(id: ProviderId, key: string | null) {
    saving = id;
    try {
      await api.setApiKey(id, key);
      drafts[id] = "";
    } finally {
      saving = null;
    }
  }

  function status(r: ProviderReport): { text: string; tone: "ok" | "muted" | "warn" } {
    if (s.disabled.includes(r.id)) return { text: $t("disabled"), tone: "muted" };
    if (r.status === "ok" || r.status === "stale") return { text: r.snapshot?.plan ? $t("detectedPlan", { plan: r.snapshot.plan }) : $t("detected"), tone: "ok" };
    if (r.status === "loading") return { text: "…", tone: "muted" };
    switch (r.issue?.kind) {
      case "not_detected": return { text: $t("notDetected"), tone: "muted" };
      case "missing_key": return { text: $t("missingKey"), tone: "muted" };
      case "unsupported": return { text: r.issue.reason === "no_subscription" ? $t("noSubTitle") : $t("apiKeyModeTitle"), tone: "muted" };
      case "error": return { text: r.issue.message, tone: "warn" };
      default: return { text: $t("needsAttention"), tone: "warn" };
    }
  }

  const intervals = [
    { v: 60, k: "min1" }, { v: 300, k: "min5" }, { v: 900, k: "min15" }, { v: 1800, k: "min30" },
  ] as const;
  const displays: { v: DisplayMode; k: Key; hint: Key }[] = [
    { v: "full", k: "displayFull", hint: "displayFullHint" },
    { v: "compact", k: "displayCompact", hint: "displayCompactHint" },
    { v: "icon_only", k: "displayIcon", hint: "displayIconHint" },
  ];
</script>

<div class="settings">
  <nav aria-label={$t("settingsTitle")}>
    <div class="brand"><BrandMark size={20} /> CodexUsageBar</div>
    {#each tabs as item (item.id)}
      <button class="nav-item" class:active={tab === item.id} aria-current={tab === item.id ? "page" : undefined} onclick={() => (tab = item.id)}>
        <Icon name={item.icon} size={15} />{$t(item.label)}
      </button>
    {/each}
  </nav>

  <main>
    {#if tab === "general"}
      <h1>{$t("tabGeneral")}</h1>
      <div class="group">
        <label class="field">
          <span class="label">{$t("refreshInterval")}</span>
          <select value={s.refresh_secs} onchange={(e) => update({ refresh_secs: +e.currentTarget.value })}>
            {#each intervals as o (o.v)}<option value={o.v}>{$t(o.k)}</option>{/each}
          </select>
        </label>
        <div class="field">
          <span class="label">{$t("launchAtLogin")}</span>
          <Switch checked={autostart} label={$t("launchAtLogin")} onchange={async (v) => (autostart = await api.autostart(v))} />
        </div>
      </div>

      <fieldset class="group">
        <legend class="group-title">{$t("displayMode")}</legend>
        {#each displays as d (d.v)}
          <label class="choice">
            <input type="radio" name="display" value={d.v} checked={s.display === d.v} onchange={() => update({ display: d.v })} />
            <span class="choice-text">
              <span>{$t(d.k)}</span>
              <span class="hint">{$t(d.hint)}</span>
            </span>
          </label>
        {/each}
        <p class="note">{$t("displayNote")}</p>
      </fieldset>

      <div class="group">
        <label class="field">
          <span class="label">{$t("language")}</span>
          <select value={s.language} onchange={(e) => update({ language: e.currentTarget.value as SettingsPatch["language"] })}>
            <option value="auto">{$t("langAuto")}</option>
            <option value="zh">{$t("langZh")}</option>
            <option value="en">{$t("langEn")}</option>
          </select>
        </label>
        <div class="field">
          <span class="label" id="theme-label">{$t("theme")}</span>
          <div class="segmented" role="radiogroup" aria-labelledby="theme-label">
            {#each [["system", "themeSystem"], ["light", "themeLight"], ["dark", "themeDark"]] as [v, k] (v)}
              <button role="radio" aria-checked={s.theme === v} class:on={s.theme === v} onclick={() => update({ theme: v as SettingsPatch["theme"] })}>{$t(k as Key)}</button>
            {/each}
          </div>
        </div>
      </div>
    {:else if tab === "providers"}
      <h1>{$t("tabProviders")}</h1>
      <p class="intro">{$t("providersIntro")}</p>

      <h2 class="group-title">{$t("localGroup")}</h2>
      <ul class="group list">
        {#each local as r (r.id)}
          {@const st = status(r)}
          <li class="provider-row">
            <Monogram id={r.id} size={26} />
            <div class="pr-text">
              <span class="pr-name">{r.name}{#if r.beta}<span class="beta">{$t("beta")}</span>{/if}</span>
              <span class="pr-status" data-tone={st.tone} title={st.text}>{st.text}</span>
            </div>
            <Switch checked={!s.disabled.includes(r.id)} label="{$t('showInPanel')}: {r.name}" onchange={(v) => toggleProvider(r.id, v)} />
          </li>
        {/each}
      </ul>

      <h2 class="group-title">{$t("keyGroup")}</h2>
      <ul class="group list">
        {#each keyed as r (r.id)}
          {@const st = status(r)}
          {@const masked = s.api_keys[r.id]}
          <li class="provider-row keyed">
            <Monogram id={r.id} size={26} />
            <div class="pr-text">
              <span class="pr-name">{r.name}</span>
              <span class="pr-status" data-tone={st.tone} title={st.text}>{masked ? `${masked}  ` : ""}{st.text}</span>
            </div>
            <Switch checked={!s.disabled.includes(r.id)} label="{$t('showInPanel')}: {r.name}" onchange={(v) => toggleProvider(r.id, v)} />
            <form class="key-form" onsubmit={(e) => { e.preventDefault(); if (drafts[r.id]) saveKey(r.id, drafts[r.id]!); }}>
              <input
                type="password"
                autocomplete="off"
                spellcheck="false"
                placeholder={$t("keyPlaceholder")}
                aria-label="{r.name} API Key"
                bind:value={drafts[r.id]}
              />
              <button class="btn" type="submit" disabled={!drafts[r.id] || saving === r.id}>{$t("save")}</button>
              {#if masked}<button class="btn quiet" type="button" onclick={() => saveKey(r.id, null)}>{$t("remove")}</button>{/if}
            </form>
            {#if ENV_KEYS[r.id]}<p class="hint key-hint">{$t("keyEnvHint", { env: ENV_KEYS[r.id]! })}</p>{/if}
          </li>
        {/each}
      </ul>
    {:else if tab === "alerts"}
      <h1>{$t("tabAlerts")}</h1>
      <p class="intro">{$t("alertsIntro")}</p>
      <div class="group">
        <div class="field">
          <span class="label">{$t("enableNotify")}</span>
          <Switch checked={s.notify} label={$t("enableNotify")} onchange={(v) => update({ notify: v })} />
        </div>
        <div class="field">
          <span class="label">{$t("notifyRestored")}</span>
          <Switch checked={s.notify_restored} disabled={!s.notify} label={$t("notifyRestored")} onchange={(v) => update({ notify_restored: v })} />
        </div>
        <label class="field">
          <span class="label"><i class="swatch warn"></i>{$t("warnThreshold")}</span>
          <select value={s.thresholds.warning} onchange={(e) => update({ thresholds: { ...s.thresholds, warning: +e.currentTarget.value } })}>
            {#each [30, 20, 15] as n (n)}<option value={n}>{$t("belowPct", { n })}{n === 20 ? $t("recommended") : ""}</option>{/each}
          </select>
        </label>
        <label class="field">
          <span class="label"><i class="swatch crit"></i>{$t("critThreshold")}</span>
          <select value={s.thresholds.critical} onchange={(e) => update({ thresholds: { ...s.thresholds, critical: +e.currentTarget.value } })}>
            {#each [10, 5] as n (n)}<option value={n}>{$t("belowPct", { n })}{n === 10 ? $t("recommended") : ""}</option>{/each}
          </select>
        </label>
      </div>
      <button class="btn" onclick={() => api.testNotification()}><Icon name="bell" size={14} />{$t("testNotify")}</button>
    {:else}
      <h1>{$t("tabAdvanced")}</h1>
      <p class="intro">{$t("diagIntro")}</p>
      <dl class="group diag">
        {#each diag as d (d.label)}
          <div class="field">
            <dt class="label">{d.label === "config" ? $t("configDir") : d.label === "cache" ? $t("cacheDir") : d.label}</dt>
            <dd class:missing={!d.value}><bdi>{d.value ?? $t("none")}</bdi></dd>
          </div>
        {/each}
        <div class="field">
          <dt class="label">{$t("configFile")}</dt>
          <dd><bdi>{s.config_path}</bdi></dd>
        </div>
      </dl>
      <p class="note privacy">{$t("privacy")}</p>
    {/if}
  </main>
</div>

<style>
  .settings {
    display: grid;
    grid-template-columns: 200px 1fr;
    height: 100vh;
    background: var(--paper);
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--s-4) var(--s-3);
    border-right: 1px solid var(--line);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: 0 var(--s-2) var(--s-4);
    font-weight: 600;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    height: 32px;
    padding: 0 var(--s-2);
    border: 0;
    border-radius: var(--r-md);
    background: transparent;
    color: var(--ink-2);
    text-align: left;
  }
  .nav-item:hover { background: var(--inset); color: var(--ink); }
  .nav-item.active {
    background: var(--surface);
    color: var(--ink);
    font-weight: 500;
    box-shadow: 0 0 0 1px var(--line);
  }
  main {
    overflow-y: auto;
    padding: var(--s-6) var(--s-8) var(--s-8);
  }
  main > * { max-width: 560px; }
  h1 {
    margin: 0 0 var(--s-4);
    font-size: var(--fs-xl);
    font-weight: 600;
  }
  .intro {
    margin: calc(var(--s-2) * -1) 0 var(--s-5);
    color: var(--ink-2);
    font-size: var(--fs-sm);
  }
  .group {
    margin: 0 0 var(--s-5);
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    background: var(--surface);
    list-style: none;
  }
  .group-title {
    margin: 0 0 var(--s-2);
    padding: 0;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--ink-2);
  }
  fieldset.group { padding-bottom: var(--s-2); }
  fieldset.group legend { float: left; width: 100%; padding: var(--s-3) var(--s-4) var(--s-1); }
  .field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-4);
    min-height: 48px;
    padding: var(--s-2) var(--s-4);
  }
  .field + .field { border-top: 1px solid var(--line); }
  .label {
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  select,
  input[type="password"] {
    height: 30px;
    padding: 0 var(--s-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-md);
    background: var(--surface);
    font-size: var(--fs-sm);
  }
  select { min-width: 180px; cursor: pointer; }
  .choice {
    display: flex;
    align-items: flex-start;
    gap: var(--s-3);
    padding: var(--s-2) var(--s-4);
    cursor: pointer;
  }
  .choice input { margin-top: 3px; accent-color: var(--ink); }
  .choice-text { display: flex; flex-direction: column; }
  .hint { font-size: var(--fs-xs); color: var(--ink-3); }
  .note {
    clear: both;
    margin: var(--s-1) var(--s-4) var(--s-2);
    font-size: var(--fs-xs);
    color: var(--ink-3);
  }
  .segmented {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--r-md);
    background: var(--inset);
  }
  .segmented button {
    height: 26px;
    padding: 0 var(--s-3);
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--ink-2);
    font-size: var(--fs-sm);
  }
  .segmented button.on {
    background: var(--surface);
    color: var(--ink);
    font-weight: 500;
    box-shadow: 0 1px 2px rgb(0 0 0 / 12%);
  }
  .provider-row {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 0 var(--s-3);
    padding: var(--s-3) var(--s-4);
  }
  .provider-row + .provider-row { border-top: 1px solid var(--line); }
  .pr-text { display: flex; flex-direction: column; min-width: 0; }
  .pr-name { display: flex; align-items: center; gap: 6px; font-weight: 500; }
  .pr-status {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-xs);
    color: var(--ink-3);
  }
  .pr-status[data-tone="ok"] { color: var(--ok); }
  .pr-status[data-tone="warn"] { color: var(--warn); }
  .beta {
    padding: 0 4px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm);
    font-size: 10px;
    font-weight: 500;
    line-height: 14px;
    color: var(--ink-3);
  }
  .key-form {
    grid-column: 2 / 4;
    display: flex;
    gap: var(--s-2);
    margin-top: var(--s-2);
  }
  .key-form input { flex: 1; min-width: 0; }
  .key-hint { grid-column: 2 / 4; margin: var(--s-1) 0 0; }
  .swatch {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .swatch.warn { background: var(--warn-fill); }
  .swatch.crit { background: var(--crit-fill); }
  .diag dd {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 62%;
    font-size: var(--fs-sm);
    color: var(--ink-2);
    user-select: text;
    direction: rtl;
    text-align: left;
  }
  .diag dd.missing { color: var(--ink-3); direction: ltr; }
  .privacy { margin: 0; max-width: 60ch; line-height: var(--lh); }
</style>
