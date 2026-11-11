export type DealPlacement = "quota_alert" | "no_subscription" | "footer" | "settings" | string;

export interface DealDetail {
  scope?: string;
  usedPercent?: number;
}

const PROVIDER_PATHS: Record<string, string> = {
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

export function getContent(
  providerId: string,
  placement: DealPlacement,
  detail?: DealDetail
): string {
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

export function getDealUrl(
  providerId: string,
  placement: DealPlacement,
  lang: string,
  detail?: DealDetail
): string {
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

export const buildDealUrl = getDealUrl;
