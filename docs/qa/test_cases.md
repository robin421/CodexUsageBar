# CodexUsageBar 测试用例与测试矩阵 (Test Cases Matrix)

## 1. 测试策略与设计原则
本测试套件旨在为 CodexUsageBar 提供全生命周期、全方位的质量守门保障。重点覆盖：
- **正向功能测试 (Happy Path)**：各 Provider 凭据探测、正常 JSON 解析、多账号平滑轮换。
- **边界与极端值测试 (Boundary Cases)**：多账号历史更新超大列表、重复账号去重、空白及特殊字符；配速计算边界（used=0/100、负倒计时、缺失窗口周期）。
- **负向异常与容错测试 (Negative & Resilience Cases)**：非法 JSON、凭据缺失、凭据过期、网络异常、过期重置时间回退。
- **视觉契约与交互设计核验 (Visual Contract & UI Fidelity)**：对齐产品截图视觉规范（已用视角、细条高亮 Thumb 进度条、三段式优雅排版、独立账号行对齐）。
- **构建与跨平台门禁 (Build & Compatibility Gates)**：Rust 编译 0 警告、Svelte/TypeScript 诊断 0 警告、Vite 生产构建资源与体积核验。
- **死代码与资产隔离 (Dead Code & Asset Auditing)**：已废弃组件解耦、字体资源按需子集打包。

---

## 2. 全景测试矩阵 (Test Matrix)

| 用例编号 | 模块 / 范围 | 测试类型 | 测试目标与前置条件 | 预期结果 | 实测结论 |
|---|---|---|---|---|---|
| **TC-CORE-001** | `copilot.rs` | 跨平台路径 | macOS 下探测 `~/Library/Application Support/github-copilot` 与 `~/.config/github-copilot` | 正确探测并自动去重，不重复扫描相同目录 | **PASS** |
| **TC-CORE-002** | `copilot.rs` | 凭据兼容 | 缺少 copilot 会话时探测 `gh` CLI 凭据或 hosts.yml | 正确降级并读取 gh 凭据 | **PASS** |
| **TC-CORE-003** | `claude.rs` | 凭据兼容器 | 当配置目录同时存在 `.credentials.json` 与 `credentials.json` 时 | 严格优先读取 `.credentials.json` | **PASS** |
| **TC-CORE-004** | `claude.rs` | 凭据兼容器 | 当仅存在无点前缀 `credentials.json` 时 | 成功回退并解析 `credentials.json` | **PASS** |
| **TC-CORE-005** | `claude.rs` | 异常处理 | 凭据存在但 token 已过期 (expired) | 正确返回 `FetchError::AuthExpired` | **PASS** |
| **TC-CORE-006** | `agy.rs` | 多账号管理 | 初次切换账号（现有配置为空或不存在） | 生成 `{"active": email, "old": []}` | **PASS** |
| **TC-CORE-007** | `agy.rs` | 数据持久性 | 切换至新账号，现有配置存在 active 及 old 列表 | 旧 active 账号安全移入 old 列表首位，不丢失历史 | **PASS** |
| **TC-CORE-008** | `agy.rs` | 多账号管理 | 切换至已经在 old 列表中的既有账号 | 既有账号成为 active，且从 old 列表中安全移除并保持其余历史 | **PASS** |
| **TC-CORE-009** | `agy.rs` | 幂等性 | 切换至当前已是 active 的相同账号 | 保持 active 不变，old 列表不重复插入 | **PASS** |
| **TC-CORE-010** | `agy.rs` | 边界安全 | 输入非法 JSON (非 JSON 字符串、截断格式、标量数值等) | 安全兜底，以新 active 账号重置，无 panic | **PASS** |
| **TC-CORE-011** | `agy.rs` | 边界安全 | 输入空字符串、空白符、全空白 old 项 | 空白无效项全部过滤，保留干净合规的账号数据 | **PASS** |
| **TC-CORE-012** | `agy.rs` | 压力测试 | 输入包含 5,000 个重复账号的大载荷 JSON | 500 个唯一账号正确去重，耗时 < 10ms，内存稳定 | **PASS** |
| **TC-CORE-013** | `agy.rs` | 字符编码 | 账号包含 Unicode 中文、Emoji、特殊符号 (+, #, %, ') | 正常解析与序列化，无字符截断或乱码 | **PASS** |
| **TC-CORE-014** | `agy.rs` | Argon 额度保护 | 账号缺失 Argon 4 配额字段 | 填充 0% 额度且不带误导性重置时间，防止虚假 100% 报警 | **PASS** |
| **TC-BUILD-001** | `workspace` | 编译构建 | 运行 `cargo check --workspace --all-targets --all-features` | 0 error, 0 warning | **PASS** |
| **TC-BUILD-002** | `quota-core` | 单元/集成测试 | 运行 `cargo test -p quota-core` (全量 51 项测试) | 51 passed, 0 failed | **PASS** |
| **TC-UI-001** | `frontend` | 类型与模板检查 | 运行 `npm run check` (Svelte 5 / TypeScript) | 0 error, 0 warning | **PASS** |
| **TC-UI-002** | `frontend` | 生产构建 | 运行 `npm run build` (Vite 打包) | 打包成功，耗时 < 1s | **PASS** |
| **TC-ASSET-001** | `font-subset` | 资源按需加载 | 检查 `dist/assets` 中字体文件 | 仅输出 latin-400, 500, 600 (共 6 个 woff/woff2 文件)，零冗余字体 | **PASS** |
| **TC-ASSET-002** | `typography` | 字体回退栈 | CSS `--font` 定义及 Latin 字符与 CJK 中文回退 | 英文/数字/标点命中 IBM Plex Sans Latin，中文字符平滑回退至系统苹方/微软雅黑 | **PASS** |
| **TC-DEAD-001** | `dead-code` | 代码隔离度 | 核验 `Hero.svelte` 和 `Gauge.svelte` 引用链 | 活跃组件 (`App`, `Panel`, `ProviderSection`, `Settings`) 零引用，Vite 树摇完全剥离 | **PASS** |
| **TC-PACE-001** | `calcPace` | 业务基准计算 | 输入 Grok 截图用例 (7天周期已过41%, 已用2%) | 计算得出 `-39%` 配速，格式化为 `配速 -39%` | **PASS** |
| **TC-PACE-002** | `calcPace` | 边界值 (used=0%) | `used_percent` 为 0，时间已过 40% / 0% / 100% | 对应计算为 `-40%`, `0%`, `-100%`，无除零与 NaN 异常 | **PASS** |
| **TC-PACE-003** | `calcPace` | 边界值 (used=100%) | `used_percent` 为 100，时间已过 40% / 100% / 0% | 对应计算为 `+60%`, `0%`, `+100%`，无溢出与 NaN 异常 | **PASS** |
| **TC-PACE-004** | `calcPace` | 负倒计时 (已过期) | 当前时间 `now` 超过 `resets_at` (`leftMs <= 0`) | `calcPace` 返回 `null`；`formatSubLine` 返回 `已重置` / `has reset` | **PASS** |
| **TC-PACE-005** | `calcPace` | 缺失窗口时长 | `window_mins` 为空，依赖 `kind` (weekly/daily/monthly) | 正确 fallback 至 7天/24h/30天基准；非周期类型安全返回 null | **PASS** |
| **TC-PACE-006** | `calcPace` | 非法数据边界 | `resets_at` 为非法日期字符串或 null；`window_mins <= 0` | 安全拦截返回 `null`，无未捕获异常 | **PASS** |
| **TC-PACE-007** | `formatSubLine` | 多段拼接与回退 | 完整三段式、缺失配速两段式、缺失倒计时回退 | `·` 间隔符拼接规范，无尾部游离分隔符，无 undefined/NaN 字符 | **PASS** |
| **TC-UI-003** | `Meter.svelte` | 视觉元素核验 | 细条轨道 + 实体 Thumb 指示点 + 紫蓝渐变/强调色 | 轨道 4px，Thumb 8px 圆点带白色轮廓光，clamp(4px, pct, 100%-4px) 无溢出 | **PASS** |
| **TC-UI-004** | `ProviderSection` | 视觉文本与结构 | 卡片左侧周期、右侧“已用 X%” | 中文字符精准匹配，粗细层级分明 | **PASS** |
| **TC-UI-005** | `ProviderSection` | 视觉对齐审查 | 账号行 `account-row` 与服务图标/卡片左边距对齐 | 实测带有 `padding-left: calc(22px + 8px)`，未与截图完全 flush-left | **DEFECT (P2)** |
| **TC-UI-006** | `ProviderSection` | Plan Badge 审查 | 检查包含套餐计划的服务卡片 (Claude/Copilot/Codex) | `<h3>` 内部和外部均渲染 `snap.plan`，导致重复文案 | **DEFECT (P2)** |

---

## 3. 自动化测试脚本结构
- 单元与集成测试套件：
  - `crates/quota-core/src/lib.rs` (45 单元测试)
  - `crates/quota-core/tests/boundary_cases.rs` (6 边界与集成测试)
  - `tests/format_spec.mjs` (12 前端格式化、配速边界与排版断言单元测试)
- 一键自动化回归与门禁脚本：
  - `tests/run_qa_suite.sh` (集成执行 7 项核心套件：Cargo 测试、Cargo Check、Svelte Check、Vite Build、字体资产核验、死代码隔离性检查、前端单元测试)
