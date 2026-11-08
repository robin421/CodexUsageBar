// Browser-only stand-in for the Tauri backend so every UI state can be previewed and screenshotted.
// Pick a scenario with ?mock=rich|calm|critical|empty|loading|failed, theme with ?theme=light|dark.

import type { AppSnapshot, DiagnosticEntry, ProviderId, ProviderReport, QuotaWindow, SettingsView, Snapshot } from "./types";

const H = 3600_000;
const at = (ms: number) => new Date(Date.now() + ms).toISOString();

function w(id: string, mins: number | null, used: number, resetIn: number | null, extra: Partial<QuotaWindow> = {}): QuotaWindow {
  const kind = mins == null ? "monthly" : mins <= 720 ? "session" : mins <= 2160 ? "daily" : mins <= 11520 ? "weekly" : "monthly";
  return { id, kind, window_mins: mins, scope: null, used_percent: used, resets_at: resetIn == null ? null : at(resetIn), used: null, limit: null, unit: null, ...extra };
}

const NAMES: Record<ProviderId, string> = {
  codex: "Codex", claude: "Claude Code", agy: "Antigravity", gemini: "Antigravity", grok: "Grok", copilot: "GitHub Copilot", opencode: "OpenCode Go",
  cursor: "Cursor", jetbrains: "JetBrains AI", codebuff: "Codebuff", openrouter: "OpenRouter", deepseek: "DeepSeek",
  moonshot: "Moonshot / Kimi", zai: "Z.ai / GLM", minimax: "MiniMax",
};
const KEYED: ProviderId[] = ["openrouter", "deepseek", "moonshot", "zai", "minimax"];

function report(id: ProviderId, status: ProviderReport["status"], snap: Partial<Snapshot> | null, issue: ProviderReport["issue"] = null): ProviderReport {
  return {
    id, name: NAMES[id], auth: KEYED.includes(id) ? "api_key" : "local", beta: ["cursor", "jetbrains", "codebuff", "opencode"].includes(id),
    status, issue, dashboard_url: "https://example.com", checked_at: new Date().toISOString(),
    snapshot: snap && { provider: id, account: null, plan: null, windows: [], balances: [], source: "oauth", updated_at: new Date().toISOString(), ...snap },
  };
}

function scenario(name: string): ProviderReport[] {
  const hidden = (ids: ProviderId[], kind: "not_detected" | "missing_key") => ids.map((id) => report(id, "issue", null, { kind }));
  switch (name) {
    case "empty":
      return [...hidden(["codex", "claude", "gemini", "copilot", "opencode", "cursor", "jetbrains", "codebuff"], "not_detected"), ...hidden(KEYED, "missing_key")];
    case "loading":
      return [];
    case "failed":
      return [
        report("codex", "issue", null, { kind: "error", message: "could not connect" }),
        report("claude", "issue", null, { kind: "auth_expired", command: "claude" }),
        report("gemini", "issue", null, { kind: "not_logged_in", command: "gemini" }),
        ...hidden(["copilot", "opencode", "cursor", "jetbrains", "codebuff"], "not_detected"),
        ...hidden(KEYED, "missing_key"),
      ];
    case "calm":
      return [
        report("claude", "ok", { plan: "Claude Max", account: "dev@studio.io", windows: [w("five_hour", 300, 22, 3.4 * H), w("seven_day", 10080, 41, 76 * H)] }),
        report("codex", "ok", { plan: "ChatGPT Pro", account: "dev@studio.io", windows: [w("primary", 300, 8, 4.1 * H), w("secondary", 10080, 27, 120 * H)] }),
        ...hidden(["gemini", "copilot", "opencode", "cursor", "jetbrains", "codebuff"], "not_detected"),
        ...hidden(KEYED, "missing_key"),
      ];
    case "critical":
      return [
        report("claude", "ok", { plan: "Claude Pro", account: "dev@studio.io", windows: [w("five_hour", 300, 96, 0.6 * H), w("seven_day", 10080, 88, 30 * H)] }),
        report("codex", "ok", { plan: "ChatGPT Plus", windows: [w("primary", 300, 71, 2.2 * H), w("secondary", 10080, 64, 98 * H)] }),
        ...hidden(["gemini", "copilot", "opencode", "cursor", "jetbrains", "codebuff"], "not_detected"),
        ...hidden(KEYED, "missing_key"),
      ];
    default:
      return [
        report("codex", "ok", {
          plan: "ChatGPT Pro", account: "maya.chen@studio.io",
          windows: [w("primary", 300, 18, 3.2 * H), w("secondary", 10080, 36, 101 * H)],
          balances: [{ id: "reset_credits", amount: 3, currency: "credits", total: null, unlimited: false }],
        }),
        report("claude", "ok", {
          plan: "Claude Max", account: "maya.chen@studio.io",
          windows: [w("five_hour", 300, 47, 1.6 * H), w("seven_day", 10080, 68, 39 * H), w("seven_day_opus", 10080, 84, 39 * H, { scope: "Opus" })],
        }),
        report("gemini", "ok", {
          plan: "Google AI Pro", account: "maya.chen@gmail.com",
          windows: [
            w("model:gemini-2.5-pro", 1440, 62, 9.5 * H, { scope: "gemini-2.5-pro" }),
            w("model:gemini-2.5-flash", 1440, 12, 9.5 * H, { scope: "gemini-2.5-flash" }),
          ],
        }),
        report("copilot", "ok", {
          plan: "Copilot Pro", account: "mayachen",
          windows: [w("premium_interactions", null, 74, 23 * 24 * H, { scope: "Premium requests", used: 222, limit: 300, unit: "requests" })],
        }),
        report("cursor", "stale", {
          plan: "Cursor Pro", windows: [w("plan", null, 58, 12 * 24 * H, { used: 11.6, limit: 20, unit: "USD" })],
        }, { kind: "error", message: "request timed out" }),
        report("opencode", "issue", null, { kind: "auth_expired", command: "opencode auth login" }),
        report("deepseek", "ok", { windows: [], balances: [{ id: "balance_cny", amount: 86.4, currency: "CNY", total: null, unlimited: false }] }),
        ...hidden(["jetbrains", "codebuff"], "not_detected"),
        ...hidden(["openrouter", "moonshot", "zai", "minimax"], "missing_key"),
      ];
  }
}

const params = new URLSearchParams(typeof location !== "undefined" ? location.search : "");
const lang = (params.get("lang") as "zh" | "en") ?? "zh";

let settings: SettingsView = {
  refresh_secs: 300, display: "icon_only", thresholds: { warning: 20, critical: 10 }, notify: true, notify_restored: true,
  disabled: [], api_keys: { deepseek: "sk-••••9f2c" }, language: lang, resolved_language: lang,
  theme: (params.get("theme") as SettingsView["theme"]) ?? "system", config_path: "~/.config/codexusagebar/config.json",
};

function build(reports: ProviderReport[], refreshing = false): AppSnapshot {
  const visible = reports.filter((r) => r.status !== "disabled" && !(r.issue && ["not_detected", "missing_key"].includes(r.issue.kind)));
  let best: { p: ProviderId; w: QuotaWindow } | null = null;
  for (const r of visible) for (const x of r.snapshot?.windows ?? []) if (!best || x.used_percent > best.w.used_percent) best = { p: r.id, w: x };
  const rem = best ? 100 - best.w.used_percent : null;
  const t = settings.thresholds;
  return {
    reports, refreshing, settings, platform: "linux", last_refresh: new Date(Date.now() - 4 * 60_000).toISOString(),
    overview: {
      level: rem == null ? null : rem < t.critical ? "critical" : rem < t.warning ? "warning" : "normal",
      min_remaining: rem, tightest_provider: best?.p ?? null, tightest_window: best?.w ?? null,
      has_issue: visible.some((r) => r.status === "stale" || r.status === "issue"),
      any_loading: reports.length === 0, active_count: visible.length,
    },
  };
}

const name = params.get("mock") ?? "rich";
let current = build(scenario(name), name === "loading");
const subs = new Set<(s: AppSnapshot) => void>();
let autostartOn = true;

export const mockBackend = {
  async invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    switch (cmd) {
      case "get_snapshot":
        return current as T;
      case "refresh":
        current = { ...current, refreshing: true };
        subs.forEach((f) => f(current));
        await new Promise((r) => setTimeout(r, 900));
        current = build(scenario(name));
        subs.forEach((f) => f(current));
        return current as T;
      case "update_settings":
        settings = { ...settings, ...(args?.patch as object) };
        if (settings.language !== "auto") settings.resolved_language = settings.language;
        current = build(current.reports);
        subs.forEach((f) => f(current));
        return current as T;
      case "set_api_key": {
        const { provider, key } = args as { provider: ProviderId; key: string | null };
        const keys = { ...settings.api_keys };
        if (key) keys[provider] = key.slice(0, 3) + "••••" + key.slice(-4);
        else delete keys[provider];
        settings = { ...settings, api_keys: keys };
        current = build(current.reports);
        subs.forEach((f) => f(current));
        return current as T;
      }
      case "diagnostics":
        return [
          { label: "codex CLI", value: "/opt/homebrew/bin/codex" },
          { label: "claude CLI", value: "/Users/maya/.local/bin/claude" },
          { label: "gemini CLI", value: "/opt/homebrew/bin/gemini" },
          { label: "gh CLI", value: "/opt/homebrew/bin/gh" },
          { label: "opencode CLI", value: null },
          { label: "codebuff CLI", value: null },
          { label: "config", value: "/Users/maya/Library/Application Support/codexusagebar" },
          { label: "cache", value: "/Users/maya/Library/Caches/codexusagebar" },
        ] satisfies DiagnosticEntry[] as T;
      default:
        return undefined as T;
    }
  },
  subscribe(cb: (s: AppSnapshot) => void) {
    subs.add(cb);
    return () => subs.delete(cb);
  },
  async autostart(enable?: boolean) {
    if (enable !== undefined) autostartOn = enable;
    return autostartOn;
  },
};
