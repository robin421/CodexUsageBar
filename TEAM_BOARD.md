# 项目协作看板 (TEAM_BOARD.md)

## 1. 业务目标与项目概览
- **项目名称**：CodexUsageBar (tokeusage)
- **定位**：常驻系统托盘的轻量化 AI 额度表（macOS / Windows / Linux），实时监控本机主流 AI 编程工具与 API 配额（Codex, Claude, Gemini/Antigravity, Copilot, Cursor, DeepSeek, OpenRouter 等）。
- **优化目标**：全面排查构建、后端 Provider 探测、数据安全、前端交互与死代码，提升系统稳定性、探测覆盖率与用户体验。

---

## 2. WBS 任务编排与 DAG 状态矩阵

| 任务 ID | 任务名称 | 负责人 | 前置依赖 | 状态 | 交付物路径 | 验收标准 / 结论 |
|---|---|---|---|---|---|---|
| **TASK-01** | 环境构建与工程配置优化 | Developer | 无 | DONE | `.cargo/config.toml`, `.gitignore` | 已配置 target-dir 重定向到 /tmp 并优化 dev/test 缓存；已配置 .gitignore 全局过滤 AppleDouble/DS_Store；dot_clean 已执行 |
| **TASK-02** | Provider 探测与数据安全修复 | Developer | TASK-01 | DONE | `crates/quota-core/src/providers/`, `src-tauri/src/lib.rs` | Copilot macOS 探测路径补全与去重；Claude 兼容无点凭据；agy 切换账号安全保护 old 历史并去重；新增 6 项单测全绿 |
| **TASK-03** | 前端架构与组件精简优化 | Developer | TASK-01 | DONE | `src/main.ts`, `src/components/` | 字体引入全面切换为 latin 字体子集（包体积与文件数显著下降）；已定位 Hero/Gauge 死代码并报请显式删除确认 |
| **TASK-04** | 测试用例补全与回归验证 | QA | TASK-02, TASK-03 | DONE | `docs/qa/`, `tests/run_qa_suite.sh`, `crates/quota-core/tests/` | 51 项测试全绿 (100% 通过)；cargo check 0 错 0 警；npm run check 0 错 0 警；产物与字体体积优化达标；出具 GO 准入报告 |
| **TASK-05** | 项目总体验收与文档同步 | AI-Manager | TASK-04 | DONE | `TEAM_BOARD.md`, 全量自动化套件 | 物理核验所有改动通过，各项质量门禁全部达标，出具全面优化简报 |
| **TASK-06** | 卡片视觉与用量逻辑重构（对齐截图） | Developer | TASK-05 | DONE | `src/components/`, `src/lib/format.ts`, `src/lib/i18n.ts`, `src/styles/tokens.css` | 1. 切换为“已用 X%”视角与细条紫蓝指示点进度条（Meter.svelte 带 Thumb 圆点）；2. 账号下移独立单行展示；3. 底部实现“重置于 · 相对倒计时 · 配速 ±X%”三段式排版与优雅回退；4. 自动化测试套件通过率 100% |
| **TASK-07** | 新视觉与配速逻辑回归与门禁验收 | QA / Developer | TASK-06 | DONE | `tests/format_spec.mjs`, `docs/qa/test_report.md` | 7 项门禁套件全部 100% 通过（51 项 Rust + 12 项前端测试）；2 项 P2 视觉排版细节（DEF-001/002）已彻底修复，零警告零缺陷，终审 PASS 准予发布 |
| **TASK-08** | AI 官方品牌图标升级与模型 Emoji 移除 | Developer | TASK-07 | DONE | `src/lib/brand_icons.ts`, `src/components/Monogram.svelte`, `src/components/ProviderSection.svelte` | 1. 替换 Monogram 字母方块为 15 家 AI 官方矢量 SVG Logo（xAI Grok 双斜杠 X, Claude 星芒, Codex 花瓣, Gemini 4角星, Copilot 头盔, Cursor 立方体, DeepSeek 跃鲸等）；2. 彻底删除模型行 emoji；3. 门禁全绿（npm run check 0 错 0 警，15 项新单测全通） |
| **TASK-09** | 官方图标与去 Emoji 回归验收与本地打包更新 | QA / Manager | TASK-08 | DONE | `/Applications/CodexUsageBar.app`, DMG | 门禁套件全部 100% 通过（含 15 项品牌图标单测）；已打包 Release 并安装更新至 /Applications 启动验证通过 |
| **TASK-10** | 双向导流顶层战略矩阵与产品交互规约 | Product | 无 | DONE | `docs/growth/mutual_traffic_spec.md` | 已输出完整 PRD 规约：商业闭环与飞轮模型、双向 8 大触点系统、14 家 Provider 精准 URL 路由与 UTM 字典契约、中英双语文案、Gherkin 验收准则与研发交接清单 |
| **TASK-11** | CodexUsageBar 客户端导流触点实现 | Developer | TASK-10 | DONE | `src/lib/affiliate.ts`, `src/components/`, `src/lib/i18n.ts`, `tests/affiliate_spec.mjs` | 1. 新建 `src/lib/affiliate.ts`，支持全量 14 家 Provider 精确比价路由、中英双语前缀与标准 UTM 归因参数装配；2. `Icon.svelte` 补齐 `tag` 与 `sparkles` 矢量图标；3. `i18n.ts` 补充全部 7 项中英文字典；4. `ProviderSection.svelte` 实现告急情境（≥80%）微型 deal 链接与无订阅引导按钮；5. `Panel.svelte` 实现 footer 常驻比价入口；6. `Settings.svelte` 实现精美生态推荐卡片；7. 新增 8 项自动化单测全绿，svelte-check 与全量 QA 套件 100% 通过（PASS）。 |
| **TASK-12** | AISubDeal Web 端额度监控回流触点实现 | Developer | TASK-10 | DONE | `/Volumes/T7 Shield/MyDocument/AI_map` (`components/QuotaCompanionCard.tsx`, `components/product-pricing/ProductPricingView.tsx`, `lib/subscription-tools.ts`, `components/Nav.tsx`, `components/Footer.tsx`) | 1. 新增 `QuotaCompanionCard.tsx`（自适应暗黑模式、中英双语、3 大核心特性、Windows/macOS 自动嗅探 CTA、UTM 跟踪直达 Release）；2. 在 `ProductPricingView.tsx` 支付指导紧邻高转化区集成伴侣卡片；3. `subscription-tools.ts` 新增 `usagebar` 并贯穿 Nav 桌面/移动抽屉及 Footer 列；4. 双端通过 tsc/build 门禁，新增 3 项自动化测试全绿 |
| **TASK-13** | 双端构建门禁、功能实测与体验回归 | QA | TASK-11, TASK-12 | DONE | `docs/qa/mutual_traffic_test_report.md`, `tests/` | 1. 客户端 7 项质量门禁与 12 项 affiliate 自动化用例 100% 通过（PASS）；2. Web 端 7 项 Vitest 用例 100% 通过；3. 双端 TS/Svelte 诊断 0 错 0 警；4. 20 条全量落地 URL 真实探测全部 HTTP 200 OK，无死链；5. 双向 UTM 参数互斥无污染，外链安全属性合规；出具发布准入终审结论：GO。 |
| **TASK-14** | 总体验收、物理核验与生产发布请示 | AI-Manager | TASK-13 | DONE | `TEAM_BOARD.md`, 最终交付成果 | 物理核验产物与双端链路全部通过（DMG 4.4MB、已热更至 /Applications；Web 端全量构建全绿）；向用户出具完整验收简报并报请生产环境正式发布审批 |

---

## 3. 风险与架构裁决日志 (ADR)
- **决策 01 [关于构建兼容性]**：macOS 在 exFAT 分区上会自动生成 `._*` AppleDouble 二进制文件，导致 `tauri-build` 误读引发 UTF-8 解析 panic。通过在 `.gitignore` 过滤 `._*` 并在 `.cargo/config.toml` 中支持可配置 target 目录规避此问题。
- **决策 02 [关于多账号切换数据持久性]**：`src-tauri/src/lib.rs` 的 `switch_agy_account` 原逻辑将 `"old": []` 硬编码写入，导致用户已关联的其他账号被意外抹掉。裁决改为保留已有账号列表，平滑更新 active 账号。
- **决策 03 [关于前端死代码清理]**：`Hero.svelte` (589 行) 与 `Gauge.svelte` 是早前重复卡片界面的遗留产物，在最新 Streamlined UI 下已完全无用且严重增加包体积与认知负担，执行安全清理与解耦。
- **决策 04 [关于配额卡片呈现“已用视角”与“配速计算”]**：采纳用户截图的直观视觉方案。AI 编程工具用户更关注“已消耗了多少（已用 X%）”以及“消耗节奏是否超过时间流逝节奏（配速 = 已用% - 时间流逝%）”。卡片头部独立展示账号邮箱，进度条增加高亮指示 Thumb，底部采用三段式“重置于 周日 13:08 · 4 天后 · 配速 -39%”紧凑排版。
- **决策 05 [关于官方品牌图标与去除模型 Emoji]**：抛弃易混淆的两位字母缩写方块，全面替换为各家 AI 官方高清晰度矢量 SVG 图标，增强辨识度与专业质感；同时根据用户指示，去除模型名称前的装饰性 Emoji（⚡、🧪、🧠），保持排版清爽专注。
- **决策 06 [关于双向导流互斥归因与防骚扰契约]**：桌面端（CodexUsageBar）只在用户进入真实痛点情境（额度 ≥80% 告急、无订阅、主动进入设置或面板底部）被动触发轻量 Deal 标签，严禁侵入式弹窗与系统通知商业推送；Web 端（AISubDeal）在比价页紧随支付攻略后挂载伴侣卡片，并通过 `useEffect` 在客户端无侵入嗅探操作系统避免 SSR 水合不匹配；双端 UTM 归因维度严格正交（桌面端固定 `utm_source=codexusagebar&utm_medium=desktop_app`，Web 端固定 `utm_source=aisubdeal&utm_medium=web`），彻底防范双向流量重定向自循环污染。
