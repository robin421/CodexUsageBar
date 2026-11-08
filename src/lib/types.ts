// Mirrors the serde types in crates/quota-core/src/model.rs and src-tauri/src/{state,settings}.rs.

export type ProviderId =
  | "codex" | "claude" | "agy" | "gemini" | "grok" | "copilot" | "opencode" | "cursor" | "jetbrains"
  | "codebuff" | "openrouter" | "deepseek" | "moonshot" | "zai" | "minimax";

export type WindowKind = "session" | "daily" | "weekly" | "monthly" | "other";
export type Level = "normal" | "warning" | "critical";
export type Status = "ok" | "stale" | "issue" | "loading" | "disabled";
export type AuthKind = "local" | "api_key";

export interface QuotaWindow {
  id: string;
  kind: WindowKind;
  window_mins: number | null;
  scope: string | null;
  used_percent: number;
  resets_at: string | null;
  used: number | null;
  limit: number | null;
  unit: string | null;
  account?: string | null;
}

export interface Balance {
  id: string;
  amount: number;
  currency: string;
  total: number | null;
  unlimited: boolean;
}

export interface Snapshot {
  provider: ProviderId;
  account: string | null;
  plan: string | null;
  windows: QuotaWindow[];
  balances: Balance[];
  source: string;
  updated_at: string;
}

export type Issue =
  | { kind: "not_detected" }
  | { kind: "not_installed" }
  | { kind: "not_logged_in"; command: string | null }
  | { kind: "auth_expired"; command: string | null }
  | { kind: "missing_key" }
  | { kind: "unsupported"; reason: string }
  | { kind: "error"; message: string };

export interface ProviderReport {
  id: ProviderId;
  name: string;
  auth: AuthKind;
  beta: boolean;
  status: Status;
  snapshot: Snapshot | null;
  issue: Issue | null;
  dashboard_url: string;
  checked_at: string;
}

export interface Overview {
  level: Level | null;
  min_remaining: number | null;
  tightest_provider: ProviderId | null;
  tightest_window: QuotaWindow | null;
  has_issue: boolean;
  any_loading: boolean;
  active_count: number;
}

export type DisplayMode = "full" | "compact" | "icon_only";
export type Lang = "zh" | "en";

export interface Thresholds {
  warning: number;
  critical: number;
}

export interface SettingsView {
  refresh_secs: number;
  display: DisplayMode;
  thresholds: Thresholds;
  notify: boolean;
  notify_restored: boolean;
  disabled: ProviderId[];
  api_keys: Partial<Record<ProviderId, string>>;
  language: "auto" | Lang;
  resolved_language: Lang;
  theme: "system" | "light" | "dark";
  config_path: string;
}

export interface AppSnapshot {
  reports: ProviderReport[];
  overview: Overview;
  refreshing: boolean;
  last_refresh: string | null;
  settings: SettingsView;
  platform: string;
  active_filter?: string;
  active_agy_account?: string | null;
}

export type SettingsPatch = Partial<
  Pick<SettingsView, "refresh_secs" | "display" | "thresholds" | "notify" | "notify_restored" | "disabled" | "language" | "theme">
>;

export interface DiagnosticEntry {
  label: string;
  value: string | null;
}
