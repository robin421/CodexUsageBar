# CodexUsageBar 视觉复刻与配速逻辑终审质量验收报告

- **报告编号**：QA-REP-20261111-002
- **报告版本**：v2.1.0-visual-gate
- **测试执行人**：资深 QA 测试工程师 (QA)
- **验收环境**：macOS (Apple Silicon aarch64), Rust 1.86+, Node.js v24.22.2, Svelte 5.57.3
- **准入结论**：**CONDITIONAL GO (功能与核心算法门禁全部通过；附 2 项 P2 视觉排版缺陷交由开发快速修正)**

---

## 1. 质量裁决概要 (Executive Summary & Verdict)

| 验收维度 | 门禁标准 | 实测结果 | 状态 |
|---|---|---|:---:|
| **全量自动化测试套件** | 7 项套件 100% 通过，0 失败 | **7 项套件全部通过 (100% 成功率)** | **PASS** |
| **Rust 单元与边界测试** | `cargo test -p quota-core` 100% | **51 / 51 测试用例通过** (耗时 0.09s) | **PASS** |
| **Rust 编译与代码检查** | `cargo check --workspace` 0 错 0 警 | **0 Errors, 0 Warnings** (耗时 2.66s) | **PASS** |
| **Svelte / TypeScript 诊断** | `svelte-check` 0 错 0 警 | **0 Errors, 0 Warnings** | **PASS** |
| **Vite 生产打包** | `npm run build` 构建成功且体积受控 | **构建成功，耗时 911ms，产物优化良好** | **PASS** |
| **字体子集按需加载** | 仅保留 Latin 子集，汉字平滑回退系统栈 | **仅 6 个 Latin woff/woff2 字体文件** | **PASS** |
| **死代码隔离度** | `Hero.svelte` / `Gauge.svelte` 完全解耦 | **活跃组件零引用，已完全树摇隔离** | **PASS** |
| **前端算法与格式化断言** | `node --test tests/format_spec.mjs` | **12 / 12 测试用例全部通过** (耗时 84ms) | **PASS** |
| **配速 (Pace) 算法边界健壮性** | used=0/100, 负倒计时, 缺失窗口 | **各极端边界均有防护与平滑回退，零崩溃** | **PASS** |
| **视觉契约对齐核验** | 严格比对用户设计截图 | **主体视觉高度还原；检出 2 项 P2 排版缺陷** | **P2 DEFECTS** |

---

## 2. 自动化回归测试执行证据链 (Execution Proofs)

执行一键自动化脚本：`bash tests/run_qa_suite.sh`

```text
================================================================
         CodexUsageBar Automated QA Regression & Gate Suite    
================================================================
[1/7] Running cargo test -p quota-core (all unit & boundary tests)...
     Running unittests src/lib.rs (/tmp/tauri-build-target/debug/deps/quota_core-10e7e1434abfb763)
running 45 tests ... 45 passed; 0 failed; 0 ignored; finished in 0.09s

     Running tests/boundary_cases.rs (/tmp/tauri-build-target/debug/deps/boundary_cases-64dd247429c46eef)
running 6 tests ... 6 passed; 0 failed; 0 ignored; finished in 0.06s
  -> Cargo tests passed.

[2/7] Running cargo check --workspace --all-targets --all-features...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.66s
  -> Cargo check passed (0 errors, 0 warnings).

[3/7] Running npm run check (Svelte 5 / TypeScript diagnostics)...
> svelte-check --tsconfig ./tsconfig.json
svelte-check found 0 errors and 0 warnings
  -> Svelte check passed (0 errors, 0 warnings).

[4/7] Running npm run build (Vite production bundle)...
dist/index.html                                             0.49 kB │ gzip:  0.31 kB
dist/assets/ibm-plex-sans-latin-400-normal-CYLoc0-x.woff   22.10 kB
dist/assets/ibm-plex-sans-latin-400-normal-CDDApCn2.woff2  22.59 kB
dist/assets/ibm-plex-sans-latin-600-normal-Cu4Hd6ag.woff   23.88 kB
dist/assets/ibm-plex-sans-latin-500-normal-BgVn5rGT.woff   23.92 kB
dist/assets/ibm-plex-sans-latin-500-normal-6ng42L7E.woff2  24.18 kB
dist/assets/ibm-plex-sans-latin-600-normal-CuJfVYMP.woff2  24.25 kB
dist/assets/index-DkpY53jL.css                             15.46 kB │ gzip:  3.74 kB
dist/assets/index-Cd_jBKRh.js                              97.31 kB │ gzip: 35.85 kB
✓ built in 911ms
  -> Vite build passed.

[5/7] Verifying font assets & bundle size in dist/...
  -> Font assets verified: exactly 6 latin font files (400, 500, 600 woff & woff2).

[6/7] Verifying dead code isolation (Hero.svelte & Gauge.svelte)...
  -> Dead code isolation verified: neither Hero nor Gauge is used by active UI.

[7/7] Running frontend format, pace & subline unit tests...
✔ windowName: concise naming in zh and en (1.188459ms)
✔ formatResetAt: dayDiff calculations for today, tomorrow, weekday, and date (20.467791ms)
✔ formatRelativeCountdown: concise output matching user screenshot (0.410959ms)
✔ calcPace: user screenshot case (used 2%, 41% time elapsed -> pace -39%) (0.150875ms)
✔ formatSubLine: joins 3 segments with middle dot (0.564625ms)
✔ formatSubLine: fallback when resets_at is missing (0.076125ms)
✔ calcPace: boundary when used_percent is 0 or 100 (0.093167ms)
✔ calcPace: boundary when remaining time is negative or zero (passed reset time) (0.073125ms)
✔ calcPace: boundary when resets_at or window_mins is missing or invalid (0.207833ms)
✔ calcPace: boundary fallback window duration for daily, weekly, monthly (0.238958ms)
✔ formatSubLine: boundary expired window returns resetDone (0.262792ms)
✔ formatSubLine: boundary missing window_mins omits pace cleanly without trailing separator (0.456417ms)
ℹ tests 12
ℹ suites 0
ℹ pass 12
ℹ fail 0
ℹ duration_ms 84.210792
  -> Frontend format & pace unit tests passed.

================================================================
  ALL QUALITY GATES PASSED (100% SUCCESS)                       
================================================================
```

---

## 3. 配速 (`calcPace`) 与格式化边界审查报告

### 3.1 核心算法解析
```ts
timeElapsedPct = (1 - (resets_at - now) / window_duration) * 100
pace = Math.round(w.used_percent - timeElapsedPct)
```

### 3.2 极端边界深度审查结论

1. **当 `used_percent` 为 0 时**：
   - 时间刚开始（0% 已过）：`pace = 0 - 0 = 0`，格式化为 `配速 0%`；
   - 时间已过 40%：`pace = 0 - 40 = -40`，格式化为 `配速 -40%`（代表用得慢，额度充裕）；
   - 周期即将结束（100% 已过）：`pace = 0 - 100 = -100`，格式化为 `配速 -100%`；
   - **结论**：计算稳定，无负零或 NaN。

2. **当 `used_percent` 为 100 时**：
   - 时间已过 40%：`pace = 100 - 40 = +60`，格式化为 `配速 +60%`（大幅超速耗尽）；
   - 周期即将结束（100% 已过）：`pace = 100 - 100 = 0`，格式化为 `配速 0%`；
   - 刚开周期即耗尽（0% 已过）：`pace = 100 - 0 = +100`，格式化为 `配速 +100%`；
   - **结论**：带有显式 `+` 符号，层级鲜明。

3. **当剩余时间为负或零（`leftMs <= 0`，已过重置时刻）时**：
   - `calcPace` 在第 178 行有直接保护：`if (leftMs <= 0) return null;`，返回 `null`；
   - `formatSubLine` 在第 212 行检测到 `ms <= 0` 时，直接短路返回 `translate(lang, "resetDone")`（中文为 `"已重置"`，英文为 `"has reset"`）；
   - **结论**：不会出现过期的荒谬负倒计时或无效配速，符合预期。

4. **当缺少 `resets_at` 或 `window_mins` 时**：
   - 缺少 `resets_at`：`calcPace` 返回 `null`；`formatSubLine` 优雅降级返回“随订阅周期生效”或“额度充足（使用后开始计时）”；
   - 缺少 `window_mins`：算法内置了周期类型推导（`kind === "weekly"` 补 10080 分钟，`daily` 补 1440 分钟，`monthly` 补 43200 分钟）；若类型为 `session` 或不可推导，`calcPace` 返回 `null`，`formatSubLine` 输出两段式（`重置于 周日 13:08 · 4 天后`），末尾无多余点号 `·`；
   - **结论**：降级链路严密完整。

5. **时钟偏差异常（`now < resetMs - totalMs`）**：
   - 存在 `effectiveLeft = Math.max(0, Math.min(totalMs, leftMs))` 限制，将已过比例截断在 [0%, 100%] 之间，避免因本机时钟快慢导致配速计算溢出。

---

## 4. 视觉契约审查与设计截图像素级核对

比对基准文件：`/Users/xiaowen/.gemini/antigravity-cli/brain/2f8fa27f-2677-4203-94a7-5345507771cd/.user_uploaded/uploaded_media_1794360574955.png`

| 界面元素 | 基准设计截图特征 | 代码实现现状 (`src/components/`) | QA 审核结论 |
|---|---|---|:---:|
| **进度条形态 (Meter)** | 极细轨道 (4px)，微灰背景，紫蓝主色 (#6366f1) | `tokens.css`: `--meter-h: 4px; --accent-fill: #6366f1;` | **100% 吻合** |
| **指示点 (Thumb)** | 实心紫蓝圆形实体，处于已用进度条顶端，带白色微光外圈 | `.thumb`: 8px 圆点，`style:left="clamp(4px, {pct}%, calc(100% - 4px))"` | **100% 吻合** |
| **视角转换** | 明确展示“已用 2%”，而非“剩余 98%” | 统一切换为 `$t("usedPct", { pct })`，“已用 2%” | **100% 吻合** |
| **底部三段式排版** | `重置于 周日 13:08 · 4 天后 · 配速 -39%` | `formatSubLine`: 中文前缀 + 间隔符 `·` 拼接 | **100% 吻合** |
| **账号独立展示** | 账号独立占一行，单行溢出省略 | `currentAccount`: `<div class="account-row">` | **符合方向** |
| **账号行边距对齐** | **与服务图标、卡片内容全齐平 (Flush-left)** | **代码中存在 `padding-left: calc(22px + var(--s-2));` (30px 缩进)** | **DEFECT (DEF-002)** |
| **套餐标签展示** | 仅在有套餐时展示一次徽章 | **`ProviderSection.svelte` 同时渲染 `plan-badge` 与 `.plan` (重复输出)** | **DEFECT (DEF-001)** |

---

## 5. 发现的缺陷列表 (Defect Backlog)

1. **DEF-001 [P2] `src/components/ProviderSection.svelte` 头部套餐名称重复渲染**
   - **现象**：对于存在套餐计划的服务（如 Claude Pro、Copilot Pro、Codex ChatGPT Pro），卡片第一行同时渲染了胶囊徽章与右侧普通文本（呈现形如 `Claude [Pro] Pro`）。
   - **建议**：清理第 127-129 行冗余的 `{:else if snap?.plan}<span class="plan">{snap.plan}</span>{/if}`。

2. **DEF-002 [P2] `src/components/ProviderSection.svelte` 账号行左内边距未与设计图齐平**
   - **现象**：原设计截图中 `robin421@126.com` 首字符与 Grok 图标左边线是完全垂直齐平的（Flush-left），但当前代码设置了 30px 的左内边距，导致在图标下方形成明显的留白空洞。
   - **建议**：移除 `.account-row` 的 `padding-left`，使其保持 flush-left 自然对齐，并将字号设置为 `var(--fs-sm)`、颜色为 `var(--ink-2)`。

---

## 6. 质量准入结论 (Sign-Off Verdict)

- **核心门禁判定**：
  - P0 / P1 级致命与严重阻断性缺陷：**0 个**
  - 自动化回归套件通过率：**100%**（Rust 51 项全绿，Node 12 项全绿，Svelte 检查 0 错 0 警）
  - 核心算法稳定性与极端边界：**验证全部通过**
- **终审裁决**：
  **CONDITIONAL GO（附修复建议交由 Developer 执行微调优化）**。上述 2 项 P2 缺陷不影响功能核心逻辑，但属于高可见度视觉细节，建议 Developer 立即按照建议完成快速打磨后即可正式交付。
