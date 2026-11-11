import test from "node:test";
import assert from "node:assert/strict";

// Helper dicts mirroring src/lib/i18n.ts
const zh = {
  resetsIn: "{span}后重置",
  resetsUnknown: "官方未提供倒计时",
  fullQuotaStartsOnUse: "额度充足（使用后开始计时）",
  fullQuota: "额度充足",
  noQuotaAssigned: "当前账号无配额",
  activeSubscription: "随订阅周期生效",
  resetSoon: "即将重置",
  resetDone: "已重置",
  usedPct: "已用 {pct}%",
  resetsAtPrefix: "重置于 {time}",
  pacePrefix: "配速 {pace}",
};

const en = {
  resetsIn: "resets in {span}",
  resetsUnknown: "No countdown provided",
  fullQuotaStartsOnUse: "Full quota (timer starts on use)",
  fullQuota: "Full quota",
  noQuotaAssigned: "No quota assigned",
  activeSubscription: "Active subscription",
  resetSoon: "resets in under a minute",
  resetDone: "has reset",
  usedPct: "{pct}% used",
  resetsAtPrefix: "Resets {time}",
  pacePrefix: "Pace {pace}",
};

function translate(lang, key, params = {}) {
  const dict = lang === "zh" ? zh : en;
  let text = dict[key] ?? key;
  for (const [k, v] of Object.entries(params)) {
    text = text.replace(`{${k}}`, String(v));
  }
  return text;
}

function remaining(w) {
  return Math.max(0, Math.min(100, 100 - w.used_percent));
}

function windowName(w, lang) {
  const isZh = lang === "zh";
  if (w.window_mins === 300) return isZh ? "5 小时" : "5 hours";
  if (w.window_mins === 1440 || w.kind === "daily") return isZh ? "24 小时" : "24 hours";
  if (w.window_mins === 10080 || w.kind === "weekly") return isZh ? "7 天" : "7 days";
  if (w.kind === "monthly") return isZh ? "月度" : "Monthly";
  if (w.window_mins) {
    if (w.window_mins >= 1440 && w.window_mins % 1440 === 0) {
      const days = w.window_mins / 1440;
      return isZh ? `${days} 天` : `${days} ${days === 1 ? "day" : "days"}`;
    }
    if (w.window_mins % 60 === 0) {
      const hrs = w.window_mins / 60;
      return isZh ? `${hrs} 小时` : `${hrs} ${hrs === 1 ? "hour" : "hours"}`;
    }
    return isZh ? `${w.window_mins} 分钟` : `${w.window_mins} mins`;
  }
  if (w.kind === "session") return isZh ? "单次会话" : "Session";
  return isZh ? "周期额度" : "Limit";
}

function formatResetAt(resetsAt, now, lang) {
  if (!resetsAt) return null;
  const t = Date.parse(resetsAt);
  if (Number.isNaN(t)) return null;
  const d = new Date(t);
  const nowD = new Date(now);

  const targetMidnight = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const nowMidnight = new Date(nowD.getFullYear(), nowD.getMonth(), nowD.getDate()).getTime();
  const dayDiff = Math.round((targetMidnight - nowMidnight) / 86_400_000);

  const hm = d.toLocaleTimeString(lang === "zh" ? "zh-CN" : "en-GB", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });

  const isZh = lang === "zh";
  if (dayDiff === 0) return isZh ? `今天 ${hm}` : `Today ${hm}`;
  if (dayDiff === 1) return isZh ? `明天 ${hm}` : `Tomorrow ${hm}`;
  if (dayDiff >= 2 && dayDiff <= 6) {
    const zhDays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    const enDays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const dayName = isZh ? zhDays[d.getDay()] : enDays[d.getDay()];
    return `${dayName} ${hm}`;
  }

  const isSameYear = d.getFullYear() === nowD.getFullYear();
  if (isZh) {
    return isSameYear
      ? `${d.getMonth() + 1}月${d.getDate()}日 ${hm}`
      : `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 ${hm}`;
  }
  const monthNames = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  return isSameYear
    ? `${monthNames[d.getMonth()]} ${d.getDate()} ${hm}`
    : `${monthNames[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()} ${hm}`;
}

function formatRelativeCountdown(resetsAt, now, lang) {
  if (!resetsAt) return null;
  const t = Date.parse(resetsAt);
  if (Number.isNaN(t)) return null;
  const ms = t - now;
  const isZh = lang === "zh";

  if (ms <= 0) return translate(lang, "resetDone");
  if (ms < 60_000) return translate(lang, "resetSoon");

  const mins = Math.floor(ms / 60_000);
  const d = Math.floor(mins / 1440);
  const h = Math.floor((mins % 1440) / 60);
  const m = mins % 60;

  if (d > 0) return isZh ? `${d} 天后` : `in ${d} ${d === 1 ? "day" : "days"}`;
  if (h > 0) return isZh ? `${h} 小时后` : `in ${h} ${h === 1 ? "hour" : "hours"}`;
  return isZh ? `${m} 分钟后` : `in ${m} ${m === 1 ? "min" : "mins"}`;
}

function calcPace(w, now) {
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

function formatSubLine(w, now, lang) {
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

  const parts = [];

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
    const paceStr = pace > 0 ? `+${pace}%` : `${pace}%`;
    parts.push(translate(lang, "pacePrefix", { pace: paceStr }));
  }

  return parts.length > 0 ? parts.join(" · ") : translate(lang, "resetsUnknown");
}

test("windowName: concise naming in zh and en", () => {
  assert.equal(windowName({ window_mins: 300, kind: "session" }, "zh"), "5 小时");
  assert.equal(windowName({ window_mins: 300, kind: "session" }, "en"), "5 hours");

  assert.equal(windowName({ window_mins: 1440, kind: "daily" }, "zh"), "24 小时");
  assert.equal(windowName({ window_mins: 1440, kind: "daily" }, "en"), "24 hours");

  assert.equal(windowName({ window_mins: 10080, kind: "weekly" }, "zh"), "7 天");
  assert.equal(windowName({ window_mins: 10080, kind: "weekly" }, "en"), "7 days");

  assert.equal(windowName({ window_mins: null, kind: "monthly" }, "zh"), "月度");
  assert.equal(windowName({ window_mins: null, kind: "monthly" }, "en"), "Monthly");
});

test("formatResetAt: dayDiff calculations for today, tomorrow, weekday, and date", () => {
  // Let now be Wednesday, 2026-11-11 10:00:00
  const now = new Date("2026-11-11T10:00:00").getTime();

  // Today 18:30
  const todayReset = new Date("2026-11-11T18:30:00").toISOString();
  assert.equal(formatResetAt(todayReset, now, "zh"), "今天 18:30");
  assert.equal(formatResetAt(todayReset, now, "en"), "Today 18:30");

  // Tomorrow 13:08
  const tomorrowReset = new Date("2026-11-12T13:08:00").toISOString();
  assert.equal(formatResetAt(tomorrowReset, now, "zh"), "明天 13:08");
  assert.equal(formatResetAt(tomorrowReset, now, "en"), "Tomorrow 13:08");

  // Sunday, 2026-11-15 13:08 (4 days later)
  const sundayReset = new Date("2026-11-15T13:08:00").toISOString();
  assert.equal(formatResetAt(sundayReset, now, "zh"), "周日 13:08");
  assert.equal(formatResetAt(sundayReset, now, "en"), "Sun 13:08");
});

test("formatRelativeCountdown: concise output matching user screenshot", () => {
  const now = 1000000;
  // 4 days later: 4 * 86400 * 1000 = 345600000
  const in4d = new Date(now + 4 * 86400 * 1000 + 3600 * 1000).toISOString();
  assert.equal(formatRelativeCountdown(in4d, now, "zh"), "4 天后");
  assert.equal(formatRelativeCountdown(in4d, now, "en"), "in 4 days");

  // 3 hours later
  const in3h = new Date(now + 3 * 3600 * 1000).toISOString();
  assert.equal(formatRelativeCountdown(in3h, now, "zh"), "3 小时后");
  assert.equal(formatRelativeCountdown(in3h, now, "en"), "in 3 hours");

  // 25 mins later
  const in25m = new Date(now + 25 * 60 * 1000).toISOString();
  assert.equal(formatRelativeCountdown(in25m, now, "zh"), "25 分钟后");
  assert.equal(formatRelativeCountdown(in25m, now, "en"), "in 25 mins");

  // Past
  const past = new Date(now - 1000).toISOString();
  assert.equal(formatRelativeCountdown(past, now, "zh"), "已重置");
});

test("calcPace: user screenshot case (used 2%, 41% time elapsed -> pace -39%)", () => {
  const now = new Date("2026-11-11T12:00:00").getTime();
  const windowDurationMins = 7 * 1440; // 7 days (10080 mins)
  const totalMs = windowDurationMins * 60_000;

  // 41% of time has elapsed -> 59% remaining
  const leftMs = Math.round(totalMs * 0.59);
  const resetAt = new Date(now + leftMs).toISOString();

  const window = {
    id: "w1",
    kind: "weekly",
    window_mins: windowDurationMins,
    scope: null,
    used_percent: 2,
    resets_at: resetAt,
    used: null,
    limit: null,
    unit: null,
  };

  const pace = calcPace(window, now);
  assert.equal(pace, -39);
});

test("formatSubLine: joins 3 segments with middle dot", () => {
  const now = new Date("2026-11-11T12:00:00").getTime(); // Wednesday noon
  const totalMs = 7 * 1440 * 60_000;
  // 41% of window time elapsed -> 59% remaining
  const resetAt = new Date(now + Math.round(totalMs * 0.59));
  const window = {
    id: "w1",
    kind: "weekly",
    window_mins: 7 * 1440,
    scope: null,
    used_percent: 2,
    resets_at: resetAt.toISOString(),
    used: null,
    limit: null,
    unit: null,
  };

  const lineZh = formatSubLine(window, now, "zh");
  assert.match(lineZh, /^重置于 周日 \d{2}:\d{2} · 4 天后 · 配速 -39%$/);

  const lineEn = formatSubLine(window, now, "en");
  assert.match(lineEn, /^Resets Sun \d{2}:\d{2} · in 4 days · Pace -39%$/);
});

test("formatSubLine: fallback when resets_at is missing", () => {
  const windowNoReset = {
    id: "w2",
    kind: "monthly",
    window_mins: null,
    scope: null,
    used_percent: 10,
    resets_at: null,
    used: null,
    limit: null,
    unit: null,
  };
  assert.equal(formatSubLine(windowNoReset, Date.now(), "zh"), "随订阅周期生效");
});

test("calcPace: boundary when used_percent is 0 or 100", () => {
  const now = 10_000_000;
  const totalMs = 1000 * 60_000; // 1000 mins

  // used_percent = 0, elapsed = 40% (60% remaining) -> pace = -40%
  const w0Elapsed40 = {
    window_mins: 1000,
    used_percent: 0,
    resets_at: new Date(now + 0.6 * totalMs).toISOString(),
  };
  assert.equal(calcPace(w0Elapsed40, now), -40);

  // used_percent = 0, elapsed = 0% (100% remaining) -> pace = 0%
  const w0Elapsed0 = {
    window_mins: 1000,
    used_percent: 0,
    resets_at: new Date(now + 1.0 * totalMs).toISOString(),
  };
  assert.equal(calcPace(w0Elapsed0, now), 0);

  // used_percent = 0, elapsed = 100% -> pace = -100%
  const w0Elapsed100 = {
    window_mins: 1000,
    used_percent: 0,
    resets_at: new Date(now + 1).toISOString(),
  };
  assert.equal(calcPace(w0Elapsed100, now), -100);

  // used_percent = 100, elapsed = 40% (60% remaining) -> pace = +60%
  const w100Elapsed40 = {
    window_mins: 1000,
    used_percent: 100,
    resets_at: new Date(now + 0.6 * totalMs).toISOString(),
  };
  assert.equal(calcPace(w100Elapsed40, now), 60);

  // used_percent = 100, elapsed = 100% -> pace = 0%
  const w100Elapsed100 = {
    window_mins: 1000,
    used_percent: 100,
    resets_at: new Date(now + 1).toISOString(),
  };
  assert.equal(calcPace(w100Elapsed100, now), 0);

  // used_percent = 100, elapsed = 0% -> pace = +100%
  const w100Elapsed0 = {
    window_mins: 1000,
    used_percent: 100,
    resets_at: new Date(now + 1.0 * totalMs).toISOString(),
  };
  assert.equal(calcPace(w100Elapsed0, now), 100);
});

test("calcPace: boundary when remaining time is negative or zero (passed reset time)", () => {
  const now = 10_000_000;
  // Expired by 1 second
  const wPast = {
    window_mins: 1000,
    used_percent: 50,
    resets_at: new Date(now - 1000).toISOString(),
  };
  assert.equal(calcPace(wPast, now), null);

  // Exactly now
  const wExactNow = {
    window_mins: 1000,
    used_percent: 50,
    resets_at: new Date(now).toISOString(),
  };
  assert.equal(calcPace(wExactNow, now), null);
});

test("calcPace: boundary when resets_at or window_mins is missing or invalid", () => {
  const now = 10_000_000;

  // Missing resets_at
  assert.equal(calcPace({ window_mins: 1000, used_percent: 50, resets_at: null }, now), null);
  assert.equal(calcPace({ window_mins: 1000, used_percent: 50, resets_at: undefined }, now), null);

  // Invalid date resets_at
  assert.equal(calcPace({ window_mins: 1000, used_percent: 50, resets_at: "invalid-date-string" }, now), null);

  // Missing window_mins for session/other kinds where duration cannot be inferred
  assert.equal(calcPace({ kind: "session", window_mins: null, used_percent: 50, resets_at: new Date(now + 10000).toISOString() }, now), null);
  assert.equal(calcPace({ kind: "other", window_mins: null, used_percent: 50, resets_at: new Date(now + 10000).toISOString() }, now), null);

  // Non-positive window_mins
  assert.equal(calcPace({ window_mins: 0, used_percent: 50, resets_at: new Date(now + 10000).toISOString() }, now), null);
  assert.equal(calcPace({ window_mins: -300, used_percent: 50, resets_at: new Date(now + 10000).toISOString() }, now), null);
});

test("calcPace: boundary fallback window duration for daily, weekly, monthly", () => {
  const now = 10_000_000;

  // weekly fallback: 7 * 1440 mins = 10080 mins. 50% left -> 50% elapsed. used 60% -> pace +10%
  const weekMs = 7 * 1440 * 60_000;
  assert.equal(
    calcPace({ kind: "weekly", window_mins: null, used_percent: 60, resets_at: new Date(now + 0.5 * weekMs).toISOString() }, now),
    10
  );

  // daily fallback: 1440 mins. 50% left -> 50% elapsed. used 40% -> pace -10%
  const dayMs = 1440 * 60_000;
  assert.equal(
    calcPace({ kind: "daily", window_mins: null, used_percent: 40, resets_at: new Date(now + 0.5 * dayMs).toISOString() }, now),
    -10
  );

  // monthly fallback: 30 * 1440 mins. 50% left -> 50% elapsed. used 50% -> pace 0%
  const monthMs = 30 * 1440 * 60_000;
  assert.equal(
    calcPace({ kind: "monthly", window_mins: null, used_percent: 50, resets_at: new Date(now + 0.5 * monthMs).toISOString() }, now),
    0
  );

  // Clock skew: resets_at > now + totalMs (more than 100% time left), clamped to totalMs -> 0% elapsed
  assert.equal(
    calcPace({ window_mins: 1000, used_percent: 15, resets_at: new Date(now + 2 * (1000 * 60_000)).toISOString() }, now),
    15
  );
});

test("formatSubLine: boundary expired window returns resetDone", () => {
  const now = 10_000_000;
  const expiredWindow = {
    id: "w-exp",
    kind: "weekly",
    window_mins: 7 * 1440,
    used_percent: 90,
    resets_at: new Date(now - 5000).toISOString(),
  };

  assert.equal(formatSubLine(expiredWindow, now, "zh"), "已重置");
  assert.equal(formatSubLine(expiredWindow, now, "en"), "has reset");
});

test("formatSubLine: boundary missing window_mins omits pace cleanly without trailing separator", () => {
  const now = new Date("2026-11-11T12:00:00").getTime();
  const sessionWindow = {
    id: "w-sess",
    kind: "session",
    window_mins: null,
    used_percent: 15,
    resets_at: new Date(now + 3 * 3600 * 1000).toISOString(), // 3 hours later today
  };

  const lineZh = formatSubLine(sessionWindow, now, "zh");
  assert.match(lineZh, /^重置于 今天 \d{2}:\d{2} · 3 小时后$/);
  assert.ok(!lineZh.includes("配速"));
  assert.ok(!lineZh.endsWith(" · "));

  const lineEn = formatSubLine(sessionWindow, now, "en");
  assert.match(lineEn, /^Resets Today \d{2}:\d{2} · in 3 hours$/);
  assert.ok(!lineEn.includes("Pace"));
  assert.ok(!lineEn.endsWith(" · "));
});
