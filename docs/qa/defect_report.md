# CodexUsageBar 缺陷与问题报告 (Defect Report)

- **缺陷报告编号**：QA-DEFECT-20261111-001
- **所属任务**：TASK-07（新视觉与配速逻辑回归与门禁验收）
- **报告日期**：2026-11-11
- **报告人**：资深 QA 测试工程师 (QA)

---

## 缺陷清单总览

| 缺陷 ID | 严重级别 | 模块 | 缺陷标题 | 状态 | 修复责任人 |
|---|---|---|---|---|---|
| **DEF-001** | **P2 (Normal)** | `ProviderSection.svelte` | 服务头部套餐名称重复双重渲染 (`snap.plan`) | OPEN | Developer |
| **DEF-002** | **P2 (Normal)** | `ProviderSection.svelte` | 账号行左内边距导致未与设计截图保持齐平 (Flush-left) | OPEN | Developer |

---

## 缺陷详述

### DEF-001: 服务头部套餐名称重复双重渲染 (`snap.plan`)

- **严重级**：P2 (Normal)
- **影响范围**：带有套餐计划字段的 Provider 卡片（例如 Claude "Pro", Codex "ChatGPT Pro", Copilot "Copilot Pro", Cursor "Cursor Pro", Grok "SuperGrok"）
- **源码定位**：`src/components/ProviderSection.svelte` 第 106 行 与 第 127-129 行
- **前置条件**：检测到有效的 Provider 且返回 `snap.plan`（非 agy 多账号状态）
- **复现步骤**：
  1. 启动应用并登录包含 plan 属性的 Provider（例如 Claude Pro 账号）；
  2. 观察服务卡片顶部 `service-row`。
- **实际结果 (Actual Result)**：
  - 第 106 行在 `<h3>` 内部渲染了 `<span class="plan-badge">{snap.plan}</span>`；
  - 第 128 行在 `service-row` 右侧再次渲染了 `<span class="plan">{snap.plan}</span>`；
  - 界面呈现：`Claude [Pro] Pro`（同一行出现两次 "Pro"），造成视觉冗余与混乱。
- **预期结果 (Expected Result)**：
  - 套餐仅应展示一次。既然标题旁已设计了精美的胶囊式徽章 `<span class="plan-badge">`，则应移除第 127-129 行的冗余文本 `<span class="plan">{snap.plan}</span>`。
- **建议修复方案**：
  在 `src/components/ProviderSection.svelte` 中，清理第 127-129 行无用的 `{:else if snap?.plan}<span class="plan">{snap.plan}</span>{/if}`，保留 `h3` 内部的 `plan-badge`。

---

### DEF-002: 账号行左内边距导致未与设计截图保持齐平 (Flush-left)

- **严重级**：P2 (Normal)
- **影响范围**：所有展示独立账号邮箱的 Provider 卡片
- **源码定位**：`src/components/ProviderSection.svelte` 第 228-233 行 `.account-row` CSS
- **前置条件**：卡片拥有账号邮箱信息（例如 `robin421@126.com`）
- **复现步骤**：
  1. 打开卡片查看用户账号邮箱；
  2. 比对用户提供的基准设计截图（`uploaded_media_1794360574955.png`）。
- **实际结果 (Actual Result)**：
  - 当前 CSS 代码包含 `padding-left: calc(22px + var(--s-2));` (30px)；
  - 导致账号邮箱向右缩进了 30px（与标题文字而不是图标左边缘齐平），在服务图标下方留出了 30px 的空白区域。
- **预期结果 (Expected Result)**：
  - 在用户设计截图中，卡片整体采用全平齐（Flush-left）网格：
    1. 服务图标（Monogram）左边缘；
    2. 账号邮箱（`robin421@126.com`）首字符；
    3. 周期标签（`7 天`）首字符；
    4. 进度条轨道（Track）左端点；
    5. 底部三段式（`重置于 周日 13:08 ...`）首字符；
    以上所有元素的左边距严格一致对齐，无缩进偏移。
- **建议修复方案**：
  将 `.account-row` 的 `padding-left: calc(22px + var(--s-2));` 调整为 `padding-left: 0;`，并调整文字字号与颜色（建议 `font-size: var(--fs-sm); color: var(--ink-2);`），达到 100% 像素级贴合设计原稿。
