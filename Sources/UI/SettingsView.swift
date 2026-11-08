import SwiftUI
import AppKit

public struct SettingsView: View {
    @ObservedObject var viewModel: UsageViewModel

    @AppStorage(UsageViewModel.refreshIntervalKey) private var refreshIntervalSeconds: Int = 300
    @AppStorage(UsageViewModel.menuBarStyleKey) private var menuBarStyleRaw: String = MenuBarStyle.full.rawValue
    @AppStorage(NotificationManager.notificationsEnabledKey) private var notificationsEnabled: Bool = true
    @AppStorage(NotificationManager.warningThresholdKey) private var warningThreshold: Int = 20
    @AppStorage(NotificationManager.criticalThresholdKey) private var criticalThreshold: Int = 10
    @AppStorage(CLIPathFinder.userDefaultsKey) private var customCLIPath: String = ""

    @State private var launchAtLogin: Bool = LaunchAtLoginManager.shared.isEnabled
    @State private var detectedCLIPath: String = CLIPathFinder.findCodexPath() ?? "未检测到"
    @State private var testResult: String = ""
    @State private var isTesting: Bool = false

    @MainActor
    public init(viewModel: UsageViewModel = .shared) {
        self.viewModel = viewModel
    }

    public var body: some View {
        TabView {
            generalTab
                .tabItem {
                    Label("常规", systemImage: "gearshape")
                }

            notificationsTab
                .tabItem {
                    Label("额度预警", systemImage: "bell.badge")
                }

            advancedTab
                .tabItem {
                    Label("高级与诊断", systemImage: "terminal")
                }
        }
        .frame(width: 480, height: 320)
        .padding(20)
    }

    // MARK: - General Tab

    private var generalTab: some View {
        Form {
            Section {
                Toggle("开机自动启动", isOn: Binding(
                    get: { launchAtLogin },
                    set: { newValue in
                        launchAtLogin = newValue
                        try? LaunchAtLoginManager.shared.setEnabled(newValue)
                    }
                ))

                Picker("自动刷新频率", selection: Binding(
                    get: { refreshIntervalSeconds },
                    set: { newValue in
                        refreshIntervalSeconds = newValue
                        viewModel.setRefreshInterval(newValue)
                    }
                )) {
                    Text("1 分钟").tag(60)
                    Text("5 分钟 (推荐)").tag(300)
                    Text("15 分钟").tag(900)
                    Text("30 分钟").tag(1800)
                }

                Picker("菜单栏展示模式", selection: Binding(
                    get: { MenuBarStyle(rawValue: menuBarStyleRaw) ?? .full },
                    set: { newStyle in
                        menuBarStyleRaw = newStyle.rawValue
                        viewModel.setMenuBarStyle(newStyle)
                    }
                )) {
                    ForEach(MenuBarStyle.allCases) { style in
                        Text(style.displayName).tag(style)
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    // MARK: - Notifications Tab

    private var notificationsTab: some View {
        Form {
            Section {
                Toggle("启用额度不足系统通知", isOn: $notificationsEnabled)

                if notificationsEnabled {
                    Picker("黄色预警阈值", selection: $warningThreshold) {
                        Text("剩余额度低于 30%").tag(30)
                        Text("剩余额度低于 20% (推荐)").tag(20)
                        Text("剩余额度低于 15%").tag(15)
                    }

                    Picker("红色告急阈值", selection: $criticalThreshold) {
                        Text("剩余额度低于 10% (推荐)").tag(10)
                        Text("剩余额度低于 5%").tag(5)
                    }

                    Button("请求系统通知权限 / 发送测试通知") {
                        NotificationManager.shared.requestAuthorization()
                        NotificationManager.shared.clearHistory()
                        let testWindow = QuotaWindow(
                            id: "test",
                            name: "测试窗口",
                            windowDurationMins: 300,
                            usedPercent: 92,
                            resetsAt: Date().addingTimeInterval(3600)
                        )
                        NotificationManager.shared.checkAndNotify(for: [testWindow])
                    }
                    .controlSize(.small)
                }
            } footer: {
                Text("当额度低于设定阈值时发出 macOS 原生通知。同一重置周期内相同阈值仅提醒一次。")
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    // MARK: - Advanced Tab

    private var advancedTab: some View {
        Form {
            Section(header: Text("Codex CLI 配置")) {
                HStack {
                    TextField("自定义 CLI 路径 (留空自动检测)", text: $customCLIPath)
                        .textFieldStyle(.roundedBorder)

                    Button("浏览...") {
                        selectCustomCLI()
                    }
                }

                HStack {
                    Text("当前检测路径:")
                        .font(.system(size: 11))
                        .foregroundColor(.secondary)
                    Text(detectedCLIPath)
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundColor(.primary)
                }

                HStack {
                    Button(isTesting ? "正在测试..." : "测试 App Server 连接") {
                        testConnection()
                    }
                    .disabled(isTesting)

                    if !testResult.isEmpty {
                        Text(testResult)
                            .font(.system(size: 11))
                            .foregroundColor(testResult.contains("成功") ? .green : .red)
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    private func selectCustomCLI() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.prompt = "选择"

        if panel.runModal() == .OK, let url = panel.url {
            customCLIPath = url.path
            detectedCLIPath = CLIPathFinder.findCodexPath() ?? "未检测到"
        }
    }

    private func testConnection() {
        isTesting = true
        testResult = ""

        Task {
            let client = CodexAppServerClient()
            do {
                let acc = try await client.readAccount()
                let limits = try await client.readRateLimits()
                await client.stop()

                await MainActor.run {
                    self.isTesting = false
                    let email = acc.account?.email ?? "未登录"
                    let plan = acc.account?.planType ?? "未知"
                    let hasCodex = limits.rateLimitsByLimitId?["codex"] != nil || limits.rateLimits != nil
                    self.testResult = "连接成功! 账号: \(email) (\(plan)), 额度数据: \(hasCodex ? "就绪" : "无")"
                    self.detectedCLIPath = CLIPathFinder.findCodexPath() ?? "未检测到"
                }
            } catch {
                await client.stop()
                await MainActor.run {
                    self.isTesting = false
                    self.testResult = "连接失败: \(error.localizedDescription)"
                }
            }
        }
    }
}
