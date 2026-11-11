import http from "node:https";

const PATHS = [
  "/pricing/chatgpt/",
  "/pricing/claude/",
  "/pricing/gemini/",
  "/pricing/grok/",
  "/pricing/copilot/",
  "/pricing/",
  "/pricing/deepseek/",
  "/pricing/kimi/",
  "/pricing/glm/",
  "/pricing/hailuo/",
  "/zh/pricing/chatgpt/",
  "/zh/pricing/claude/",
  "/zh/pricing/gemini/",
  "/zh/pricing/grok/",
  "/zh/pricing/copilot/",
  "/zh/pricing/",
  "/zh/pricing/deepseek/",
  "/zh/pricing/kimi/",
  "/zh/pricing/glm/",
  "/zh/pricing/hailuo/",
];

async function checkUrl(path) {
  const url = `https://www.aisubdeal.com${path}`;
  return new Promise((resolve) => {
    http.get(url, (res) => {
      resolve({ path, status: res.statusCode });
    }).on("error", (err) => {
      resolve({ path, status: "ERR", error: err.message });
    });
  });
}

async function run() {
  console.log("Checking all 20 landing paths against https://www.aisubdeal.com ...");
  const results = await Promise.all(PATHS.map(checkUrl));
  let failed = 0;
  for (const r of results) {
    if (r.status >= 200 && r.status < 400) {
      console.log(`[PASS] ${r.path} -> ${r.status}`);
    } else {
      console.log(`[FAIL] ${r.path} -> ${r.status}`);
      failed++;
    }
  }
  if (failed > 0) {
    console.error(`Verification FAILED: ${failed} paths returned non-2xx/3xx status.`);
    process.exit(1);
  } else {
    console.log("Verification SUCCESS: All 20 landing paths are alive and return 200/3xx!");
  }
}

run();
