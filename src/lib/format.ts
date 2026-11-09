import type { Balance, Lang, Level, ProviderId, QuotaWindow, Thresholds } from "./types";
import { translate } from "./i18n";

export const remaining = (w: QuotaWindow) => Math.max(0, Math.min(100, 100 - w.used_percent));

export function levelOf(w: QuotaWindow, t: Thresholds): Level {
  const r = remaining(w);
  return r < t.critical ? "critical" : r < t.warning ? "warning" : "normal";
}

/** "3 小时 25 分钟" / "3h 25m" — the span only, callers add "后重置". */
export function span(ms: number, lang: Lang): string {
  const mins = Math.floor(ms / 60_000);
  const d = Math.floor(mins / 1440), h = Math.floor((mins % 1440) / 60), m = mins % 60;
  if (lang === "zh") {
    if (d > 0) return h > 0 ? `${d} 天 ${h} 小时` : `${d} 天`;
    if (h > 0) return m > 0 ? `${h} 小时 ${m} 分钟` : `${h} 小时`;
    return `${m} 分钟`;
  }
  if (d > 0) return h > 0 ? `${d}d ${h}h` : `${d}d`;
  if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
  return `${m}m`;
}

export function countdown(
  resetsAt: string | null,
  now: number,
  lang: Lang,
  rem?: number | null,
  w?: QuotaWindow | null,
): string {
  if (!resetsAt) {
    if (rem != null && Math.round(rem) <= 0) return translate(lang, "noQuotaAssigned");
    if (rem != null && Math.round(rem) >= 100) {
      if (w?.window_mins === 300 || w?.kind === "session") {
        return translate(lang, "fullQuotaStartsOnUse");
      }
      return translate(lang, "fullQuota");
    }
    if (w?.kind === "monthly") {
      return translate(lang, "activeSubscription");
    }
    return translate(lang, "resetsUnknown");
  }
  const ms = Date.parse(resetsAt) - now;
  if (ms <= 0) return translate(lang, "resetDone");
  if (ms < 60_000) return translate(lang, "resetSoon");
  return translate(lang, "resetsIn", { span: span(ms, lang) });
}

/** Absolute reset time: "今天 18:00", "明天 05:12", "11月20日 09:00". */
export function resetClock(resetsAt: string | null, now: number, lang: Lang): string {
  if (!resetsAt) return "";
  const d = new Date(resetsAt);
  const today = new Date(now);
  const dayDiff = Math.round((new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() - new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime()) / 86_400_000);
  const hm = d.toLocaleTimeString(lang === "zh" ? "zh-CN" : "en-GB", { hour: "2-digit", minute: "2-digit", hour12: false });
  if (lang === "zh") {
    if (dayDiff === 0) return `今天 ${hm}`;
    if (dayDiff === 1) return `明天 ${hm}`;
    return `${d.getMonth() + 1}月${d.getDate()}日 ${hm}`;
  }
  if (dayDiff === 0) return `Today ${hm}`;
  if (dayDiff === 1) return `Tomorrow ${hm}`;
  return `${d.toLocaleDateString("en-US", { month: "short", day: "numeric" })}, ${hm}`;
}

/** "5 小时额度", "每周额度", with the model/feature scope shown separately by the caller. */
export function windowName(w: QuotaWindow, lang: Lang): string {
  const zh = lang === "zh";
  if (w.window_mins === 300) return zh ? "5 小时额度" : "5-hour limit";
  switch (w.kind) {
    case "weekly": return zh ? "每周额度" : "Weekly limit";
    case "daily": return zh ? "每日额度" : "Daily limit";
    case "monthly": return zh ? "每月额度" : "Monthly limit";
  }
  if (w.window_mins && w.window_mins % 60 === 0) return zh ? `${w.window_mins / 60} 小时额度` : `${w.window_mins / 60}-hour limit`;
  return zh ? "额度窗口" : "Usage limit";
}

const nf = (lang: Lang, max = 0) => new Intl.NumberFormat(lang === "zh" ? "zh-CN" : "en-US", { maximumFractionDigits: max });

/** "222 / 300 次", "$11.60 / $20.00". */
export function amounts(w: QuotaWindow, lang: Lang): string | null {
  if (w.used == null || w.limit == null) return null;
  if (w.unit === "USD") return `$${w.used.toFixed(2)} / $${w.limit.toFixed(2)}`;
  const unit = w.unit && ["requests", "credits", "prompts", "calls"].includes(w.unit) ? translate(lang, w.unit as "requests") : w.unit ?? "";
  return `${nf(lang).format(w.used)} / ${nf(lang).format(w.limit)}${lang === "zh" ? " " : " "}${unit}`.trim();
}

export function money(b: Balance, lang: Lang): string {
  if (b.unlimited) return translate(lang, "unlimited");
  const sym: Record<string, string> = { USD: "$", CNY: "¥" };
  if (sym[b.currency]) return `${sym[b.currency]}${nf(lang, 2).format(b.amount)}`;
  if (b.currency === "credits") return `${nf(lang).format(b.amount)} ${translate(lang, "credits")}`;
  return `${nf(lang, 2).format(b.amount)} ${b.currency}`;
}

/**
 * Share of the window still ahead of us (0..1): where the fuel needle "should" be if usage
 * were spread evenly. Null when the window length or reset time is unknown.
 */
export function expectedRemaining(w: QuotaWindow, now: number): number | null {
  if (!w.resets_at) return null;
  const mins = w.window_mins ?? (w.kind === "monthly" ? 30 * 1440 : null);
  if (!mins) return null;
  const left = (Date.parse(w.resets_at) - now) / 60_000;
  if (left <= 0 || left > mins) return null;
  return left / mins;
}

export function hhmm(iso: string | null, lang: Lang): string {
  if (!iso) return "";
  return new Date(iso).toLocaleTimeString(lang === "zh" ? "zh-CN" : "en-GB", { hour: "2-digit", minute: "2-digit", hour12: false });
}

export const MONOGRAM: Record<ProviderId, string> = {
  codex: "Cx", claude: "Cl", agy: "Ag", gemini: "Gm", grok: "Gk", copilot: "Gh", opencode: "Oc", cursor: "Cu", jetbrains: "Jb",
  codebuff: "Cb", openrouter: "Or", deepseek: "Ds", moonshot: "Ki", zai: "Z", minimax: "Mm",
};

export const ENV_KEYS: Partial<Record<ProviderId, string>> = {
  agy: "ANTIGRAVITY_API_KEY", gemini: "GEMINI_API_KEY", grok: "GROK_API_KEY",
  openrouter: "OPENROUTER_API_KEY", deepseek: "DEEPSEEK_API_KEY", moonshot: "MOONSHOT_API_KEY",
  zai: "Z_AI_API_KEY", minimax: "MINIMAX_API_KEY", codebuff: "CODEBUFF_API_KEY", opencode: "OPENCODE_API_KEY",
};
