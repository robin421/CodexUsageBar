import test from "node:test";
import assert from "node:assert/strict";

// Mirror pure logic from src/lib/affiliate.ts
const PROVIDER_PATHS = {
  codex: "/pricing/chatgpt/",
  claude: "/pricing/claude/",
  gemini: "/pricing/gemini/",
  agy: "/pricing/gemini/",
  grok: "/pricing/grok/",
  copilot: "/pricing/copilot/",
  cursor: "/pricing/",
  codebuff: "/pricing/",
  opencode: "/pricing/",
  jetbrains: "/pricing/",
  openrouter: "/pricing/",
  deepseek: "/pricing/deepseek/",
  moonshot: "/pricing/kimi/",
  zai: "/pricing/glm/",
  minimax: "/pricing/hailuo/",
};

const DEFAULT_PATH = "/pricing/";

function getContent(providerId, placement, detail) {
  if (placement === "quota_alert") {
    if (detail?.scope) {
      const cleanScope = detail.scope
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "");
      if (cleanScope) {
        return `${providerId}_${cleanScope}`;
      }
    }
    return `${providerId}_alert`;
  }

  if (placement === "no_subscription") {
    return `${providerId}_no_sub`;
  }

  if (placement === "footer") {
    return providerId === "global" ? "global_footer" : `${providerId}_footer`;
  }

  if (placement === "settings") {
    return providerId === "global" ? "settings_ecosystem" : `${providerId}_settings`;
  }

  return `${providerId}_${placement}`;
}

function getDealUrl(providerId, placement, lang, detail) {
  const path = PROVIDER_PATHS[providerId] ?? DEFAULT_PATH;
  const baseUrl = lang === "zh" ? "https://aisubdeal.com/zh" : "https://aisubdeal.com";
  const content = getContent(providerId, placement, detail);
  const langParam = lang === "zh" ? "zh" : "en";

  const params = new URLSearchParams({
    utm_source: "codexusagebar",
    utm_medium: "desktop_app",
    utm_campaign: placement,
    utm_content: content,
    utm_term: "v2.0.0_desktop",
    lang: langParam,
  });

  return `${baseUrl}${path}?${params.toString()}`;
}

test("affiliate: all 14 provider path mappings exist and end with slash", () => {
  const expected = {
    codex: "/pricing/chatgpt/",
    claude: "/pricing/claude/",
    gemini: "/pricing/gemini/",
    agy: "/pricing/gemini/",
    grok: "/pricing/grok/",
    copilot: "/pricing/copilot/",
    cursor: "/pricing/",
    codebuff: "/pricing/",
    opencode: "/pricing/",
    jetbrains: "/pricing/",
    openrouter: "/pricing/",
    deepseek: "/pricing/deepseek/",
    moonshot: "/pricing/kimi/",
    zai: "/pricing/glm/",
    minimax: "/pricing/hailuo/",
  };

  for (const [id, path] of Object.entries(expected)) {
    assert.equal(PROVIDER_PATHS[id], path, `Provider ${id} mapped path mismatch`);
    assert.ok(path.endsWith("/"), `Path for ${id} must end with /`);
  }
});

test("affiliate: language prefix routing (zh vs en)", () => {
  const zhUrl = getDealUrl("claude", "quota_alert", "zh", { scope: "5h", usedPercent: 85 });
  const enUrl = getDealUrl("claude", "quota_alert", "en", { scope: "5h", usedPercent: 85 });

  assert.ok(zhUrl.startsWith("https://aisubdeal.com/zh/pricing/claude/"));
  assert.ok(enUrl.startsWith("https://aisubdeal.com/pricing/claude/"));
  assert.ok(!enUrl.includes("/zh/"));
});

test("affiliate: quota_alert UTM parameters", () => {
  const url = getDealUrl("claude", "quota_alert", "zh", { scope: "5h", usedPercent: 85 });
  const parsed = new URL(url);

  assert.equal(parsed.searchParams.get("utm_source"), "codexusagebar");
  assert.equal(parsed.searchParams.get("utm_medium"), "desktop_app");
  assert.equal(parsed.searchParams.get("utm_campaign"), "quota_alert");
  assert.equal(parsed.searchParams.get("utm_content"), "claude_5h");
  assert.equal(parsed.searchParams.get("utm_term"), "v2.0.0_desktop");
  assert.equal(parsed.searchParams.get("lang"), "zh");
});

test("affiliate: quota_alert scope sanitization", () => {
  const complexUrl = getDealUrl("agy", "quota_alert", "zh", { scope: "Gemini 2.5 Pro (Workspace)" });
  const parsed = new URL(complexUrl);
  assert.equal(parsed.searchParams.get("utm_content"), "agy_gemini_2_5_pro_workspace");

  const noScopeUrl = getDealUrl("codex", "quota_alert", "en");
  const parsedNoScope = new URL(noScopeUrl);
  assert.equal(parsedNoScope.searchParams.get("utm_content"), "codex_alert");
});

test("affiliate: no_subscription UTM parameters", () => {
  const url = getDealUrl("copilot", "no_subscription", "en");
  const parsed = new URL(url);

  assert.equal(parsed.pathname, "/pricing/copilot/");
  assert.equal(parsed.searchParams.get("utm_source"), "codexusagebar");
  assert.equal(parsed.searchParams.get("utm_medium"), "desktop_app");
  assert.equal(parsed.searchParams.get("utm_campaign"), "no_subscription");
  assert.equal(parsed.searchParams.get("utm_content"), "copilot_no_sub");
  assert.equal(parsed.searchParams.get("utm_term"), "v2.0.0_desktop");
  assert.equal(parsed.searchParams.get("lang"), "en");
});

test("affiliate: footer persistent entry UTM parameters", () => {
  const url = getDealUrl("global", "footer", "zh");
  const parsed = new URL(url);

  assert.equal(parsed.pathname, "/zh/pricing/");
  assert.equal(parsed.searchParams.get("utm_source"), "codexusagebar");
  assert.equal(parsed.searchParams.get("utm_medium"), "desktop_app");
  assert.equal(parsed.searchParams.get("utm_campaign"), "footer");
  assert.equal(parsed.searchParams.get("utm_content"), "global_footer");
  assert.equal(parsed.searchParams.get("utm_term"), "v2.0.0_desktop");
  assert.equal(parsed.searchParams.get("lang"), "zh");
});

test("affiliate: settings ecosystem recommendation UTM parameters", () => {
  const url = getDealUrl("global", "settings", "en");
  const parsed = new URL(url);

  assert.equal(parsed.pathname, "/pricing/");
  assert.equal(parsed.searchParams.get("utm_source"), "codexusagebar");
  assert.equal(parsed.searchParams.get("utm_medium"), "desktop_app");
  assert.equal(parsed.searchParams.get("utm_campaign"), "settings");
  assert.equal(parsed.searchParams.get("utm_content"), "settings_ecosystem");
  assert.equal(parsed.searchParams.get("utm_term"), "v2.0.0_desktop");
  assert.equal(parsed.searchParams.get("lang"), "en");
});

test("affiliate: fallback for unknown provider id", () => {
  const url = getDealUrl("unknown_ai", "quota_alert", "zh");
  const parsed = new URL(url);
  assert.equal(parsed.pathname, "/zh/pricing/");
  assert.equal(parsed.searchParams.get("utm_content"), "unknown_ai_alert");
});

test("affiliate: all 14 providers URL generation matrix across zh and en", () => {
  const providers = [
    "codex", "claude", "gemini", "agy", "grok", "copilot", "cursor",
    "codebuff", "opencode", "jetbrains", "openrouter", "deepseek", "moonshot", "zai", "minimax"
  ];

  for (const pid of providers) {
    for (const lang of ["zh", "en"]) {
      const alertUrl = getDealUrl(pid, "quota_alert", lang, { scope: "default_window", usedPercent: 88 });
      const noSubUrl = getDealUrl(pid, "no_subscription", lang);

      const parsedAlert = new URL(alertUrl);
      const parsedNoSub = new URL(noSubUrl);

      assert.equal(parsedAlert.protocol, "https:");
      assert.equal(parsedAlert.hostname, "aisubdeal.com");
      assert.equal(parsedAlert.searchParams.get("utm_source"), "codexusagebar");
      assert.equal(parsedAlert.searchParams.get("utm_medium"), "desktop_app");
      assert.equal(parsedAlert.searchParams.get("utm_campaign"), "quota_alert");
      assert.equal(parsedAlert.searchParams.get("utm_content"), `${pid}_default_window`);

      assert.equal(parsedNoSub.protocol, "https:");
      assert.equal(parsedNoSub.hostname, "aisubdeal.com");
      assert.equal(parsedNoSub.searchParams.get("utm_campaign"), "no_subscription");
      assert.equal(parsedNoSub.searchParams.get("utm_content"), `${pid}_no_sub`);

      if (lang === "zh") {
        assert.ok(parsedAlert.pathname.startsWith("/zh/"), `${pid} zh alert path should start with /zh/`);
        assert.ok(parsedNoSub.pathname.startsWith("/zh/"), `${pid} zh no_sub path should start with /zh/`);
      } else {
        assert.ok(!parsedAlert.pathname.startsWith("/zh/"), `${pid} en alert path should NOT start with /zh/`);
        assert.ok(!parsedNoSub.pathname.startsWith("/zh/"), `${pid} en no_sub path should NOT start with /zh/`);
      }
    }
  }
});

test("affiliate: extreme scope edge cases sanitization", () => {
  // All special characters & symbols
  const symUrl = getDealUrl("agy", "quota_alert", "zh", { scope: "!!!@@@###$$$%%%^^^&&&***()" });
  const parsedSym = new URL(symUrl);
  // When all characters are stripped, cleanScope is empty -> fallback to {pid}_alert
  assert.equal(parsedSym.searchParams.get("utm_content"), "agy_alert");

  // Scope with Chinese and brackets
  const cnUrl = getDealUrl("claude", "quota_alert", "zh", { scope: "5小时窗口 (重度使用)" });
  const parsedCn = new URL(cnUrl);
  assert.equal(parsedCn.searchParams.get("utm_content"), "claude_5");

  // Leading and trailing spaces and underscores
  const trimUrl = getDealUrl("cursor", "quota_alert", "en", { scope: "  ___fast_requests___  " });
  const parsedTrim = new URL(trimUrl);
  assert.equal(parsedTrim.searchParams.get("utm_content"), "cursor_fast_requests");
});

test("affiliate: bidirectional UTM attribution isolation and exclusivity", () => {
  // Client outbound:
  const clientUrl = getDealUrl("claude", "quota_alert", "zh");
  const parsedClient = new URL(clientUrl);
  const clientSource = parsedClient.searchParams.get("utm_source");
  const clientMedium = parsedClient.searchParams.get("utm_medium");

  // Web companion card outbound:
  const webDownloadUrl = new URL("https://github.com/robin421/CodexUsageBar/releases/latest?utm_source=aisubdeal&utm_medium=web&utm_campaign=companion_card&utm_content=claude");
  const webSource = webDownloadUrl.searchParams.get("utm_source");
  const webMedium = webDownloadUrl.searchParams.get("utm_medium");

  // Mutual exclusion: source & medium must never collide
  assert.notEqual(clientSource, webSource, "Client and Web utm_source must be strictly distinct");
  assert.notEqual(clientMedium, webMedium, "Client and Web utm_medium must be strictly distinct");
  assert.equal(clientSource, "codexusagebar");
  assert.equal(clientMedium, "desktop_app");
  assert.equal(webSource, "aisubdeal");
  assert.equal(webMedium, "web");
});

test("affiliate: quota alert threshold trigger condition (used_percent >= 80)", () => {
  const shouldShowDealAlert = (usedPercent) => usedPercent >= 80;

  assert.equal(shouldShowDealAlert(0), false);
  assert.equal(shouldShowDealAlert(50), false);
  assert.equal(shouldShowDealAlert(79), false);
  assert.equal(shouldShowDealAlert(79.9), false);
  assert.equal(shouldShowDealAlert(80), true, "80% must trigger deal alert");
  assert.equal(shouldShowDealAlert(80.1), true);
  assert.equal(shouldShowDealAlert(90), true);
  assert.equal(shouldShowDealAlert(100), true);
});
