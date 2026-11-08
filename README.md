# Codex Usage Bar (macOS 原生菜单栏额度工具)

一款轻量级、常驻 macOS 顶部菜单栏的原生应用，专为 ChatGPT / Codex 套餐用户打造。通过与本地官方 `codex app-server` 的 STDIO JSON-RPC 2.0 协议直连，实时精准展示账号套餐额度、重置倒计时与预警通知。

---

## 核心特性

- **100% 原生 Swift + SwiftUI 开发**：无 Electron、无 WebView、无额外外部服务端，安装包仅约 600KB，运行时极低资源占用。
- **官方 App Server JSON-RPC 直连**：
  - 通过 `initialize` -> `account/read` -> `account/rateLimits/read` 获取真实额度。
  - 严格区分 ChatGPT 套餐额度与 OpenAI API Token 余额，坚决不通过 Token 消耗推算套餐额度。
- **多窗口动态识别**：
  - 不预设窗口数量，动态解析服务端返回的配额窗口。
  - 根据 `windowDurationMins` 智能分类（5小时、24小时、每周等）。
- **重置时间与精确倒计时**：
  - 毫秒级重置时间戳转换为本地时区展示。
  - 实时动态计算“还有 X 小时 Y 分钟”倒计时。
- **原生额度预警与通知**：
  - 额度低于 20% 黄色预警，低于 10% 红色告急。
  - 周期内自动去重，避免重复打扰；重置后自动恢复提醒。
- **自动化生命周期**：
  - 支持定时自动刷新（默认 5 分钟，支持自定义 1m/5m/15m/30m）。
  - 支持从 macOS 睡眠恢复（Wake from sleep）时立即自动刷新。
  - 启动时优先展示本地缓存快照，网络失败自动降级标记数据过期。
- **开机自动启动**：
  - 采用现代 macOS `SMAppService.mainApp` API，无侵入式登录项管理。

---

## 系统与环境要求

- **macOS**：macOS 14.0 (Sonoma) 或更高版本
- **架构**：Apple Silicon (M1/M2/M3/M4) 及 Intel Mac
- **依赖**：本机已安装 [Codex CLI](https://learn.chatgpt.com/docs/codex) 并在终端中完成 `codex login`

---

## 快速构建与运行

### 方式一：直接运行构建脚本生成 .app 与 .dmg

在终端执行：
```bash
./scripts/build_release.sh
```
构建产物将保存在 `dist/` 目录：
- `dist/CodexUsageBar.app`：原生可执行应用
- `dist/CodexUsageBar.dmg`：可直接拖拽安装的 DMG 镜像包
- `dist/CodexUsageBar-macOS.zip`：便携压缩包

### 方式二：使用 Xcode 打开工程开发调试

1. 确保已安装 [XcodeGen](https://github.com/yonaskolb/XcodeGen)（已通过 Homebrew 安装）：
   ```bash
   brew install xcodegen
   ```
2. 生成 Xcode 工程：
   ```bash
   xcodegen generate
   ```
3. 打开生成的 `CodexUsageBar.xcodeproj`：
   ```bash
   open CodexUsageBar.xcodeproj
   ```
4. 在 Xcode 中直接选择 `CodexUsageBar` Scheme 进行编译或运行单元测试（`Cmd + U`）。

### 方式三：运行单元测试

```bash
xcodebuild test -project CodexUsageBar.xcodeproj -scheme CodexUsageBar -destination 'platform=macOS'
```

---

## 工程目录结构

```
CodexUsageBar/
├── project.yml                     # XcodeGen 规范配置文件
├── CodexUsageBar.xcodeproj         # 生成的 Xcode 工程
├── Sources/
│   ├── App/
│   │   └── CodexUsageBarApp.swift   # 应用入口与 MenuBarExtra 配置
│   ├── Core/
│   │   ├── CLIPathFinder.swift      # Codex 可执行文件动态发现
│   │   ├── CodexAppServerClient.swift # App Server 进程与 JSON-RPC 客户端
│   │   └── JSONRPCTransport.swift   # STDIO JSONL 异步管道通信
│   ├── Models/
│   │   ├── AppStatus.swift          # 应用与认证连接状态机
│   │   └── UsageModels.swift        # 协议与领域数据模型
│   ├── Services/
│   │   ├── LaunchAtLoginManager.swift # macOS 13+ 开机自启服务
│   │   ├── NotificationManager.swift # 原生系统通知与去重
│   │   ├── UsageCacheManager.swift  # 本地快照缓存
│   │   └── UsageRepository.swift    # 状态聚合与并发去重管理
│   └── UI/
│       ├── MenuBarStyle.swift       # 菜单栏显示模式枚举
│       ├── SettingsView.swift       # 原生设置面板（常规/预警/诊断）
│       ├── SettingsWindowController.swift # 设置窗口控制器
│       ├── UsageCardView.swift      # 额度进度卡片组件
│       └── UsagePopoverView.swift   # 菜单栏主详情 Popover
├── Tests/
│   ├── NotificationManagerTests.swift # 通知触发与去重单测
│   ├── UsageCacheTests.swift        # 缓存读写与清空单测
│   └── UsageModelsTests.swift       # 协议解析、窗口计算与倒计时单测
├── scripts/
│   └── build_release.sh             # 自动化 Release 构建与打包脚本
└── dist/                            # 构建就绪产物
    ├── CodexUsageBar.app
    ├── CodexUsageBar.dmg
    └── CodexUsageBar-macOS.zip
```

---

## 菜单栏展示模式说明

在“设置 -> 常规”中可自由切换菜单栏展示模式：
1. **完整展示**（默认）：`Codex 5h 82% · 7d 64%`（动态展示前两个主要额度窗口剩余百分比）
2. **紧凑展示**：`5h 82%`（仅展示最短周期主窗口）
3. **仅展示图标**：使用 SF Symbol 图标，通过颜色（绿/橙/红）反映额度健康度，点击展开查看完整详情。

---

## 安全与隐私承诺

- **无凭证盗取**：本软件不读取、不解析、不存储任何 `~/.codex/auth.json` 中的 OAuth Token 或敏感私钥。
- **让 CLI 自主认证**：所有的认证刷新与权限检验均由系统中的官方 `codex` 原生进程维护。
- **纯本地运行**：无云端代理、无第三方上报接口、无分析跟踪代码。
