import SwiftUI
import AppKit

public struct UsagePopoverView: View {
    @ObservedObject var viewModel: UsageViewModel
    public var onOpenSettings: () -> Void

    @MainActor
    public init(viewModel: UsageViewModel = .shared, onOpenSettings: @escaping () -> Void = {}) {
        self.viewModel = viewModel
        self.onOpenSettings = onOpenSettings
    }

    private var dateFormatter: DateFormatter {
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss"
        return f
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Header Section
            headerView
                .padding(.horizontal, 16)
                .padding(.top, 14)
                .padding(.bottom, 12)

            Divider()

            // Main Body Content
            ScrollView {
                VStack(spacing: 12) {
                    switch viewModel.status {
                    case .ready(let snapshot):
                        snapshotContentView(snapshot, isStale: false, error: nil)
                    case .stale(let snapshot, let error):
                        snapshotContentView(snapshot, isStale: true, error: error)
                    case .cliNotInstalled(let message):
                        statusNoticeView(
                            icon: "wrench.and.screwdriver",
                            iconColor: .orange,
                            title: "未找到 Codex CLI",
                            message: message,
                            actionTitle: "查看安装指引",
                            action: {
                                if let url = URL(string: "https://learn.chatgpt.com/docs/codex") {
                                    NSWorkspace.shared.open(url)
                                }
                            }
                        )
                    case .notLoggedIn:
                        statusNoticeView(
                            icon: "person.crop.circle.badge.exclamationmark",
                            iconColor: .blue,
                            title: "Codex 未登录",
                            message: "请在终端中运行 'codex login' 登录您的 ChatGPT 账号。",
                            actionTitle: "复制登录命令",
                            action: {
                                let pb = NSPasteboard.general
                                pb.clearContents()
                                pb.setString("codex login", forType: .string)
                            }
                        )
                    case .apiKeyMode:
                        statusNoticeView(
                            icon: "key.fill",
                            iconColor: .purple,
                            title: "当前为 API Key 模式",
                            message: "ChatGPT 套餐额度和 OpenAI API 余额是两套不同的计费体系。请切换为 ChatGPT 账号登录以查看套餐额度。",
                            actionTitle: nil,
                            action: nil
                        )
                    case .authExpired:
                        statusNoticeView(
                            icon: "exclamationmark.triangle",
                            iconColor: .red,
                            title: "登录凭据已过期",
                            message: "您的 Codex 登录凭据已过期，请在终端重新执行 'codex login'。",
                            actionTitle: "复制登录命令",
                            action: {
                                let pb = NSPasteboard.general
                                pb.clearContents()
                                pb.setString("codex login", forType: .string)
                            }
                        )
                    case .loading:
                        if let snapshot = viewModel.status.currentSnapshot {
                            snapshotContentView(snapshot, isStale: false, error: nil)
                        } else {
                            VStack(spacing: 12) {
                                ProgressView()
                                    .scaleEffect(0.9)
                                Text("正在连接 Codex App Server...")
                                    .font(.system(size: 12))
                                    .foregroundColor(.secondary)
                            }
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 30)
                        }
                    case .error(let message):
                        if let snapshot = viewModel.status.currentSnapshot {
                            snapshotContentView(snapshot, isStale: true, error: message)
                        } else {
                            statusNoticeView(
                                icon: "xmark.circle",
                                iconColor: .red,
                                title: "获取额度失败",
                                message: message,
                                actionTitle: "重试",
                                action: {
                                    Task { await viewModel.refresh() }
                                }
                            )
                        }
                    }
                }
                .padding(14)
            }
            .frame(maxHeight: 420)

            Divider()

            // Footer Section
            footerView
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
        }
        .frame(width: 340)
        .background(Color(NSColor.windowBackgroundColor))
        .onAppear {
            viewModel.onPopoverOpened()
        }
    }

    // MARK: - Header View

    private var headerView: some View {
        HStack(alignment: .center) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Image(nsImage: NSApp.applicationIconImage)
                        .resizable()
                        .frame(width: 18, height: 18)
                        .cornerRadius(4)

                    Text("Codex Usage")
                        .font(.system(size: 14, weight: .bold))

                    if let snapshot = viewModel.status.currentSnapshot {
                        Text(snapshot.account.displayPlanName)
                            .font(.system(size: 10, weight: .semibold))
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Color.accentColor.opacity(0.12))
                            .foregroundColor(.accentColor)
                            .cornerRadius(4)
                    }
                }

                if let snapshot = viewModel.status.currentSnapshot {
                    HStack(spacing: 4) {
                        if let email = snapshot.account.email {
                            Text(email)
                                .font(.system(size: 11))
                                .foregroundColor(.secondary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                        Text("·")
                            .font(.system(size: 11))
                            .foregroundColor(.secondary)
                        Text("更新于 \(dateFormatter.string(from: snapshot.updatedAt))")
                            .font(.system(size: 10))
                            .foregroundColor(.secondary)
                    }
                } else {
                    Text("实时套餐额度监控")
                        .font(.system(size: 11))
                        .foregroundColor(.secondary)
                }
            }

            Spacer()

            // Refresh Button
            Button(action: {
                Task { await viewModel.refresh() }
            }) {
                Image(systemName: "arrow.clockwise")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundColor(viewModel.isRefreshing ? .accentColor : .secondary)
                    .rotationEffect(.degrees(viewModel.isRefreshing ? 360 : 0))
                    .animation(
                        viewModel.isRefreshing
                            ? Animation.linear(duration: 1.0).repeatForever(autoreverses: false)
                            : .default,
                        value: viewModel.isRefreshing
                    )
            }
            .buttonStyle(.plain)
            .disabled(viewModel.isRefreshing)
            .help("立即刷新额度")
        }
    }

    // MARK: - Snapshot Content

    @ViewBuilder
    private func snapshotContentView(_ snapshot: UsageSnapshot, isStale: Bool, error: String?) -> some View {
        VStack(spacing: 10) {
            if isStale {
                HStack(spacing: 6) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundColor(.orange)
                        .font(.system(size: 11))
                    Text("数据已过期: \(error ?? "刷新失败")")
                        .font(.system(size: 11))
                        .foregroundColor(.secondary)
                        .lineLimit(1)
                    Spacer()
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.orange.opacity(0.1))
                .cornerRadius(6)
            }

            if snapshot.windows.isEmpty {
                VStack(spacing: 6) {
                    Image(systemName: "questionmark.circle")
                        .font(.system(size: 24))
                        .foregroundColor(.secondary)
                    Text("当前套餐暂无可识别的额度窗口")
                        .font(.system(size: 12))
                        .foregroundColor(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 20)
            } else {
                ForEach(snapshot.windows) { window in
                    UsageCardView(window: window)
                }
            }

            // Credits info card
            if let credits = snapshot.credits, credits.hasCredits || (credits.balance != nil && credits.balance != "0") {
                HStack {
                    HStack(spacing: 6) {
                        Image(systemName: "ticket")
                            .font(.system(size: 12))
                            .foregroundColor(.blue)
                        Text("可用重置次数")
                            .font(.system(size: 12, weight: .medium))
                    }
                    Spacer()
                    Text(credits.unlimited ? "无限次" : (credits.balance ?? "0"))
                        .font(.system(size: 13, weight: .bold, design: .rounded))
                        .foregroundColor(.blue)
                }
                .padding(10)
                .background(Color.blue.opacity(0.08))
                .cornerRadius(8)
            }
        }
    }

    // MARK: - Notice View

    private func statusNoticeView(
        icon: String,
        iconColor: Color,
        title: String,
        message: String,
        actionTitle: String?,
        action: (() -> Void)?
    ) -> some View {
        VStack(spacing: 10) {
            Image(systemName: icon)
                .font(.system(size: 28))
                .foregroundColor(iconColor)

            Text(title)
                .font(.system(size: 13, weight: .semibold))

            Text(message)
                .font(.system(size: 11))
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)

            if let actionTitle = actionTitle, let action = action {
                Button(action: action) {
                    Text(actionTitle)
                        .font(.system(size: 12, weight: .medium))
                        .padding(.horizontal, 12)
                        .padding(.vertical, 4)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 24)
        .padding(.horizontal, 8)
    }

    // MARK: - Footer View

    private var footerView: some View {
        HStack {
            Button(action: {
                onOpenSettings()
            }) {
                HStack(spacing: 4) {
                    Image(systemName: "gearshape")
                        .font(.system(size: 11))
                    Text("设置...")
                        .font(.system(size: 11))
                }
            }
            .buttonStyle(.plain)
            .foregroundColor(.secondary)

            Spacer()

            Button(action: {
                NSApplication.shared.terminate(nil)
            }) {
                HStack(spacing: 4) {
                    Image(systemName: "power")
                        .font(.system(size: 11))
                    Text("退出")
                        .font(.system(size: 11))
                }
            }
            .buttonStyle(.plain)
            .foregroundColor(.secondary)
        }
    }
}
