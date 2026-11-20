import type { Balance, Lang, Level, ProviderId, QuotaWindow, Thresholds } from "./types";
import { translate } from "./i18n";

export const remaining = (w: QuotaWindow) => Math.max(0, Math.min(100, 100 - w.used_percent));

export function localeOf(lang: Lang): string {
  if (lang === "zh") return "zh-CN";
  if (lang === "ja") return "ja-JP";
  if (lang === "ko") return "ko-KR";
  return "en-GB";
}

const WEEKDAYS: Record<Lang, string[]> = {
  zh: ["周日", "周一", "周二", "周三", "周四", "周五", "周六"],
  ja: ["日曜", "月曜", "火曜", "水曜", "木曜", "金曜", "土曜"],
  ko: ["일", "월", "화", "수", "목", "금", "토"],
  en: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
};

function dayLabel(lang: Lang, dayDiff: number, hm: string): string | null {
  if (dayDiff === 0) {
    if (lang === "zh") return `今天 ${hm}`;
    if (lang === "ja") return `今日 ${hm}`;
    if (lang === "ko") return `오늘 ${hm}`;
    return `Today ${hm}`;
  }
  if (dayDiff === 1) {
    if (lang === "zh") return `明天 ${hm}`;
    if (lang === "ja") return `明日 ${hm}`;
    if (lang === "ko") return `내일 ${hm}`;
    return `Tomorrow ${hm}`;
  }
  return null;
}

function calendarDate(d: Date, lang: Lang, hm: string, withYear: boolean): string {
  const y = d.getFullYear();
  const m = d.getMonth() + 1;
  const day = d.getDate();
  if (lang === "zh" || lang === "ja") {
    return withYear ? `${y}年${m}月${day}日 ${hm}` : `${m}月${day}日 ${hm}`;
  }
  if (lang === "ko") {
    return withYear ? `${y}년 ${m}월 ${day}일 ${hm}` : `${m}월 ${day}일 ${hm}`;
  }
  const monthNames = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  return withYear
    ? `${monthNames[d.getMonth()]} ${day}, ${y} ${hm}`
    : `${monthNames[d.getMonth()]} ${day} ${hm}`;
}

/**
 * One flagship per model family. A shared pool stored as
 * "Claude Opus 5.5 · Sonnet 5.5 · GPT-OSS 120B" reads as "Claude Opus 5.5 · GPT-OSS 120B".
 */
export function representativeScope(scope: string): string {
  const parts = scope.split(" · ").map((s) => s.trim()).filter(Boolean);
  if (parts.length <= 1) return scope;

  const explicitFamily = (name: string): string | null => {
    const l = name.toLowerCase();
    if (l.includes("argon")) return "argon";
    if (l.includes("claude") || l.includes("opus") || l.includes("sonnet") || l.includes("haiku")) return "claude";
    if (l.includes("gpt")) return "gpt";
    if (l.includes("gemini") || l.includes("hydrogen") || l.includes("neon") || l.includes("orchestrator") || l.includes("flash") || l.includes("sonar")) return "gemini";
    return null;
  };

  const versionOf = (name: string): [number, number] => {
    for (const token of name.split(/[^0-9.]+/).filter(Boolean)) {
      if (token.includes(".")) {
        const [a, b] = token.split(".");
        const maj = Number(a);
        const min = Number(b);
        if (Number.isFinite(maj) && Number.isFinite(min) && b !== "") return [maj, min];
      } else {
        const n = Number(token);
        if (Number.isFinite(n) && n < 100) return [n, 0];
      }
    }
    return [0, 0];
  };

  const tierOf = (name: string): number => {
    const l = name.toLowerCase();
    let tier = 20;
    if (/ultra|opus|hydrogen|neon|sonar|orchestrator/.test(l)) tier = 40;
    else if (/pro|sonnet/.test(l)) tier = 30;
    else if (l.includes("gpt")) tier = 30;
    else if (/flash|haiku/.test(l)) tier = 10;
    if (l.includes("hydrogen") || l.includes("opus")) tier += 5;
    else if (l.includes("neon")) tier += 4;
    else if (l.includes("sonar")) tier += 3;
    return tier;
  };

  const better = (a: string, b: string): number => {
    const [aMaj, aMin] = versionOf(a);
    const [bMaj, bMin] = versionOf(b);
    if (aMaj !== bMaj) return aMaj - bMaj;
    if (aMin !== bMin) return aMin - bMin;
    return tierOf(a) - tierOf(b);
  };

  const branded = (part: string, family: string, donor?: string): string => {
    if (family === "claude" && !/claude/i.test(part) && donor && /claude/i.test(donor)) return `Claude ${part}`;
    if (family === "gemini" && !/gemini/i.test(part) && donor && /gemini/i.test(donor)) return `Gemini ${part}`;
    return part;
  };

  const order: string[] = [];
  const best = new Map<string, string>();
  let lastFamily = "";
  for (const part of parts) {
    const family = explicitFamily(part) ?? (lastFamily || (part.split(/\s+/)[0] ?? part).toLowerCase());
    lastFamily = family;
    const current = best.get(family);
    if (!current) {
      order.push(family);
      best.set(family, part);
    } else if (better(part, current) > 0) {
      best.set(family, branded(part, family, current));
    }
  }
  return order.map((family) => best.get(family) ?? "").filter(Boolean).join(" · ");
}

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
  if (lang === "ja") {
    if (d > 0) return h > 0 ? `${d}日${h}時間` : `${d}日`;
    if (h > 0) return m > 0 ? `${h}時間${m}分` : `${h}時間`;
    return `${m}分`;
  }
  if (lang === "ko") {
    if (d > 0) return h > 0 ? `${d}일 ${h}시간` : `${d}일`;
    if (h > 0) return m > 0 ? `${h}시간 ${m}분` : `${h}시간`;
    return `${m}분`;
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
  const hm = d.toLocaleTimeString(localeOf(lang), { hour: "2-digit", minute: "2-digit", hour12: false });
  const named = dayLabel(lang, dayDiff, hm);
  if (named) return named;
  return calendarDate(d, lang, hm, d.getFullYear() !== today.getFullYear());
}

/** "5 小时", "7 天", "24 小时", "月度" */
export function windowName(w: QuotaWindow, lang: Lang): string {
  const label = (zh: string, en: string, ja: string, ko: string) =>
    lang === "zh" ? zh : lang === "ja" ? ja : lang === "ko" ? ko : en;
  if (w.window_mins === 300) return label("5 小时", "5 hours", "5時間", "5시간");
  if (w.window_mins === 1440 || w.kind === "daily") return label("24 小时", "24 hours", "24時間", "24시간");
  if (w.window_mins === 10080 || w.kind === "weekly") return label("7 天", "7 days", "7日", "7일");
  if (w.kind === "monthly") return label("月度", "Monthly", "月間", "월간");
  if (w.window_mins) {
    if (w.window_mins >= 1440 && w.window_mins % 1440 === 0) {
      const days = w.window_mins / 1440;
      return label(
        `${days} 天`,
        `${days} ${days === 1 ? "day" : "days"}`,
        `${days}日`,
        `${days}일`,
      );
    }
    if (w.window_mins % 60 === 0) {
      const hrs = w.window_mins / 60;
      return label(
        `${hrs} 小时`,
        `${hrs} ${hrs === 1 ? "hour" : "hours"}`,
        `${hrs}時間`,
        `${hrs}시간`,
      );
    }
    return label(`${w.window_mins} 分钟`, `${w.window_mins} mins`, `${w.window_mins}分`, `${w.window_mins}분`);
  }
  if (w.kind === "session") return label("单次会话", "Session", "セッション", "세션");
  return label("周期额度", "Limit", "期間", "기간");
}

export function isResetSoon(resetsAt: string | null | undefined, now: number): boolean {
  if (!resetsAt) return false;
  const resetMs = Date.parse(resetsAt);
  if (Number.isNaN(resetMs)) return false;
  const diff = resetMs - now;
  return diff > 0 && diff <= 86_400_000;
}

/**
 * Absolute reset time formatted for subline or display:
 * "今天 13:08", "明天 13:08", "周日 13:08", "4月12日 13:08".
 */
export function formatResetAt(resetsAt: string | null, now: number, lang: Lang): string | null {
  if (!resetsAt) return null;
  const t = Date.parse(resetsAt);
  if (Number.isNaN(t)) return null;
  const d = new Date(t);
  const nowD = new Date(now);

  const targetMidnight = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const nowMidnight = new Date(nowD.getFullYear(), nowD.getMonth(), nowD.getDate()).getTime();
  const dayDiff = Math.round((targetMidnight - nowMidnight) / 86_400_000);

  const hm = d.toLocaleTimeString(localeOf(lang), {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });

  const named = dayLabel(lang, dayDiff, hm);
  if (named) return named;
  if (dayDiff >= 2 && dayDiff <= 6) {
    return `${WEEKDAYS[lang][d.getDay()]} ${hm}`;
  }

  return calendarDate(d, lang, hm, d.getFullYear() !== nowD.getFullYear());
}

/**
 * Time still left, as its own clause:
 * "还有 4 天", "还有 3 小时", "还有 25 分钟", "不到 1 分钟", "已重置".
 */
export function formatRelativeCountdown(resetsAt: string | null, now: number, lang: Lang): string | null {
  if (!resetsAt) return null;
  const t = Date.parse(resetsAt);
  if (Number.isNaN(t)) return null;
  const ms = t - now;

  if (ms <= 0) return translate(lang, "resetDone");
  if (ms < 60_000) return translate(lang, "underOneMin");

  const mins = Math.floor(ms / 60_000);
  const d = Math.floor(mins / 1440);
  const h = Math.floor((mins % 1440) / 60);
  const m = mins % 60;

  const left = (n: number, unit: "d" | "h" | "m") => {
    if (lang === "zh") return `还有 ${n} ${unit === "d" ? "天" : unit === "h" ? "小时" : "分钟"}`;
    if (lang === "ja") return `あと${n}${unit === "d" ? "日" : unit === "h" ? "時間" : "分"}`;
    if (lang === "ko") return `${n}${unit === "d" ? "일" : unit === "h" ? "시간" : "분"} 남음`;
    const word = unit === "d" ? (n === 1 ? "day" : "days") : unit === "h" ? (n === 1 ? "hour" : "hours") : (n === 1 ? "min" : "mins");
    return `${n} ${word} left`;
  };

  if (d > 0) return left(d, "d");
  if (h > 0) return left(h, "h");
  return left(m, "m");
}

/** Usage vs even burn: "比匀速快 15%" / "15% ahead of pace". */
function formatPace(pace: number, lang: Lang): string {
  if (pace > 0) return translate(lang, "paceAhead", { n: pace });
  if (pace < 0) return translate(lang, "paceBehind", { n: -pace });
  return translate(lang, "paceEven");
}

/**
 * Pace calculation:
 * timeElapsedPct = (1 - (resets_at - now) / window_duration) * 100
 * pace = Math.round(w.used_percent - timeElapsedPct)
 */
export function calcPace(w: QuotaWindow, now: number): number | null {
  if (!w.resets_at) return null;
  const resetMs = Date.parse(w.resets_at);
  if (Number.isNaN(resetMs)) return null;

  let windowMins = w.window_mins;
  if (!windowMins) {
    if (w.kind === "weekly") windowMins = 7 * 1440;
    else if (w.kind === "daily") windowMins = 1440;
    else if (w.kind === "monthly") windowMins = 30 * 1440;
  }
  if (!windowMins || windowMins <= 0) return null;

  const totalMs = windowMins * 60_000;
  const leftMs = resetMs - now;
  if (leftMs <= 0) return null;

  const effectiveLeft = Math.max(0, Math.min(totalMs, leftMs));
  const timeElapsedPct = (1 - effectiveLeft / totalMs) * 100;
  return Math.round(w.used_percent - timeElapsedPct);
}

/**
 * Three-segment subline:
 * "周日 13:08 重置 · 还有 4 天 · 比匀速慢 39%"
 * Falls back gracefully when resets_at or window duration is missing.
 */
export function formatSubLine(w: QuotaWindow, now: number, lang: Lang): string {
  if (!w.resets_at) {
    const rem = remaining(w);
    if (Math.round(rem) <= 0) return translate(lang, "noQuotaAssigned");
    if (Math.round(rem) >= 100) {
      if (w.window_mins === 300 || w.kind === "session") {
        return translate(lang, "fullQuotaStartsOnUse");
      }
      return translate(lang, "fullQuota");
    }
    if (w.kind === "monthly") {
      return translate(lang, "activeSubscription");
    }
    return translate(lang, "resetsUnknown");
  }

  const resetMs = Date.parse(w.resets_at);
  if (Number.isNaN(resetMs)) {
    return translate(lang, "resetsUnknown");
  }

  const ms = resetMs - now;
  if (ms <= 0) {
    return translate(lang, "resetDone");
  }

  const parts: string[] = [];

  const clock = formatResetAt(w.resets_at, now, lang);
  if (clock) {
    parts.push(translate(lang, "resetsAtPrefix", { time: clock }));
  }

  const rel = formatRelativeCountdown(w.resets_at, now, lang);
  if (rel) {
    parts.push(rel);
  }

  const pace = calcPace(w, now);
  if (pace !== null) {
    parts.push(formatPace(pace, lang));
  }

  return parts.length > 0 ? parts.join(" · ") : countdown(w.resets_at, now, lang, remaining(w), w);
}

const nf = (lang: Lang, max = 0) => new Intl.NumberFormat(localeOf(lang), { maximumFractionDigits: max });

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
  return new Date(iso).toLocaleTimeString(localeOf(lang), { hour: "2-digit", minute: "2-digit", hour12: false });
}

export const MONOGRAM: Record<ProviderId, string> = {
  codex: "Cx", claude: "Cl", agy: "Ag", gemini: "Gm", grok: "Gk", copilot: "Gh", opencode: "Oc", cursor: "Cu", jetbrains: "Jb",
  codebuff: "Cb", workbuddy: "Wb", kiro: "Kr", commandcode: "Cc", openrouter: "Or", deepseek: "Ds", moonshot: "Ki", zai: "Z", minimax: "Mm",
  volcengine: "Ve", bailian: "Bl", qianfan: "Qf", qoder: "Qd",
};

export const ENV_KEYS: Partial<Record<ProviderId, string>> = {
  agy: "ANTIGRAVITY_API_KEY", gemini: "GEMINI_API_KEY", grok: "GROK_API_KEY",
  openrouter: "OPENROUTER_API_KEY", deepseek: "DEEPSEEK_API_KEY", moonshot: "MOONSHOT_API_KEY",
  zai: "Z_AI_API_KEY", minimax: "MINIMAX_API_KEY", codebuff: "CODEBUFF_API_KEY", opencode: "OPENCODE_API_KEY",
  volcengine: "VOLC_ACCESSKEY", bailian: "ALIBABA_CLOUD_ACCESS_KEY_ID", qianfan: "QIANFAN_COOKIE", qoder: "QODER_TOKEN",
};
