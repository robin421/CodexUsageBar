import test from "node:test";
import assert from "node:assert/strict";
import { BRAND_ICONS } from "../src/lib/brand_icons.ts";

const EXPECTED_PROVIDERS = [
  "grok",
  "claude",
  "codex",
  "agy",
  "gemini",
  "copilot",
  "cursor",
  "deepseek",
  "jetbrains",
  "openrouter",
  "moonshot",
  "zai",
  "minimax",
  "opencode",
  "codebuff",
];

test("brand_icons: all 15 official AI providers have vector logo definitions", () => {
  assert.equal(Object.keys(BRAND_ICONS).length, 15);
  for (const id of EXPECTED_PROVIDERS) {
    const icon = BRAND_ICONS[id];
    assert.ok(icon, `Provider ${id} should have a brand icon definition`);
    assert.ok(icon.viewBox && icon.viewBox.length > 0, `Provider ${id} should have a non-empty viewBox`);
    assert.ok(Array.isArray(icon.paths) && icon.paths.length > 0, `Provider ${id} should have at least 1 path`);
    for (const p of icon.paths) {
      assert.ok(p.length > 10, `Provider ${id} path should have valid path data`);
      assert.ok(!p.includes("NaN"), `Provider ${id} path should not contain NaN`);
    }
  }
});

test("brand_icons: grok icon matches official xAI double-slash X geometry", () => {
  const grok = BRAND_ICONS.grok;
  assert.equal(grok.viewBox, "0 0 24 24");
  assert.equal(grok.paths.length, 1);
  assert.ok(grok.paths[0].startsWith("M6.469 8.776L16.512 23"), "grok path must match official xAI vector mark");
});

test("brand_icons: claude icon matches Anthropic sunburst sparkle geometry", () => {
  const claude = BRAND_ICONS.claude;
  assert.equal(claude.viewBox, "0 0 24 24");
  assert.equal(claude.paths.length, 1);
  assert.ok(claude.paths[0].includes("15.955"), "claude path must match official Anthropic sparkle");
});

test("brand_icons: codex icon matches OpenAI 6-petal rosette geometry", () => {
  const codex = BRAND_ICONS.codex;
  assert.equal(codex.viewBox, "0 0 24 24");
  assert.equal(codex.paths.length, 1);
  assert.ok(codex.paths[0].includes("15.128"), "codex path must match official OpenAI rosette");
});

test("brand_icons: agy and gemini match Google Gemini 4-pointed sparkle", () => {
  assert.equal(BRAND_ICONS.agy.viewBox, "0 0 24 24");
  assert.equal(BRAND_ICONS.gemini.viewBox, "0 0 24 24");
  assert.deepEqual(BRAND_ICONS.agy.paths, BRAND_ICONS.gemini.paths);
});

test("brand_icons: copilot matches GitHub Copilot helmet geometry", () => {
  const copilot = BRAND_ICONS.copilot;
  assert.equal(copilot.viewBox, "0 0 24 24");
  assert.equal(copilot.paths.length, 1);
  assert.ok(copilot.paths[0].includes("19.245 5.364"), "copilot path must match official helmet mark");
});

test("brand_icons: cursor matches isometric 3D cube geometry", () => {
  const cursor = BRAND_ICONS.cursor;
  assert.equal(cursor.viewBox, "0 0 24 24");
  assert.equal(cursor.paths.length, 1);
  assert.ok(cursor.paths[0].includes("22.106 5.68L12.5.135"), "cursor path must match isometric 3D cube");
});

test("brand_icons: deepseek matches leaping whale geometry", () => {
  const ds = BRAND_ICONS.deepseek;
  assert.equal(ds.viewBox, "0 0 24 24");
  assert.equal(ds.paths.length, 1);
  assert.ok(ds.paths[0].includes("23.748 4.482"), "deepseek path must match leaping whale");
});

test("brand_icons: jetbrains matches JetBrains frame with code prompt geometry", () => {
  const jb = BRAND_ICONS.jetbrains;
  assert.equal(jb.viewBox, "0 0 24 24");
  assert.equal(jb.paths.length, 1);
  assert.ok(jb.paths[0].includes("23.997A2.347"), "jetbrains path must match official JetBrains mark");
});

test("brand_icons: openrouter matches network router node geometry", () => {
  const or = BRAND_ICONS.openrouter;
  assert.equal(or.viewBox, "0 0 24 24");
  assert.equal(or.paths.length, 1);
  assert.ok(or.paths[0].includes("18.654 3.87a5.087"), "openrouter path must match official node");
});

test("brand_icons: moonshot matches Kimi stylized K geometry", () => {
  const ms = BRAND_ICONS.moonshot;
  assert.equal(ms.viewBox, "0 0 24 24");
  assert.equal(ms.paths.length, 2);
  assert.ok(ms.paths[0].includes("21.846 0a1.923"), "moonshot path must match Kimi dot");
  assert.ok(ms.paths[1].includes("11.065 11.199l7.257"), "moonshot path must match Kimi K body");
});

test("brand_icons: zai matches Zhipu GLM interlocking loops geometry", () => {
  const zai = BRAND_ICONS.zai;
  assert.equal(zai.viewBox, "0 0 24 24");
  assert.equal(zai.paths.length, 2, "zai should have 2 interlocking loop paths");
  assert.ok(zai.paths[0].includes("6.075 10.494C7.6"), "zai path must match first interlocking loop");
  assert.ok(zai.paths[1].includes("3.747 11.494c-.62"), "zai path must match second interlocking loop");
});

test("brand_icons: minimax matches official 4-pillar waveform geometry", () => {
  const mm = BRAND_ICONS.minimax;
  assert.equal(mm.viewBox, "0 0 24 24");
  assert.equal(mm.paths.length, 1);
  assert.ok(mm.paths[0].includes("16.278 2c1.156"), "minimax path must match 4-pillar waveform");
});

test("brand_icons: opencode matches official code interpreter box mark", () => {
  const oc = BRAND_ICONS.opencode;
  assert.equal(oc.viewBox, "0 0 24 24");
  assert.equal(oc.paths.length, 1);
  assert.ok(oc.paths[0].includes("M16 6H8v12h8V6zm4 16H4V2h16v20z"), "opencode path must match official mark");
});

test("brand_icons: codebuff matches official terminal mark geometry", () => {
  const codebuff = BRAND_ICONS.codebuff;
  assert.equal(codebuff.viewBox, "0 0 514 514");
  assert.equal(codebuff.paths.length, 2);
});
