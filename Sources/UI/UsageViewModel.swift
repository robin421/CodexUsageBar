import Foundation
import SwiftUI
import AppKit

public enum MenuBarStyle: String, CaseIterable, Identifiable {
    case full = "full"       // Codex 5h 82% · 7d 64%
    case compact = "compact" // 5h 82%
    case iconOnly = "iconOnly" // Icon only

    public var id: String { rawValue }

    public var displayName: String {
        switch self {
        case .full: return "完整展示 (Codex 5h 82% · 7d 64%)"
        case .compact: return "紧凑展示 (5h 82%)"
        case .iconOnly: return "仅展示图标"
        }
    }
}

@MainActor
public final class UsageViewModel: ObservableObject {
    public static let shared = UsageViewModel()

    public static let refreshIntervalKey = "refreshIntervalSeconds"
    public static let menuBarStyleKey = "menuBarStyle"

    @Published public var status: AppStatus = .loading
    @Published public var isRefreshing: Bool = false
    @Published public var menuBarTitle: String = "Codex"
    @Published public var menuBarStatusLevel: QuotaStatusLevel = .normal
    @Published public var menuBarStyle: MenuBarStyle = .full

    private let repository: UsageRepository
    private let cacheManager: UsageCacheManager
    private nonisolated(unsafe) var timer: Timer?
    private nonisolated(unsafe) var wakeObserver: (any NSObjectProtocol)?

    public init(
        repository: UsageRepository = UsageRepository(),
        cacheManager: UsageCacheManager = .shared
    ) {
        self.repository = repository
        self.cacheManager = cacheManager
        registerDefaults()
        loadPreferences()
        setupSleepWakeObserver()

        // 1. Immediately present cached state if available
        if let cached = cacheManager.load() {
            self.status = .ready(cached)
            updateMenuBarPresentation(with: cached)
        }

        // 2. Perform initial live refresh
        Task { [weak self] in
            await self?.refresh()
            self?.startScheduledTimer()
        }
    }

    deinit {
        timer?.invalidate()
        if let obs = wakeObserver {
            NotificationCenter.default.removeObserver(obs)
        }
    }

    private func registerDefaults() {
        UserDefaults.standard.register(defaults: [
            Self.refreshIntervalKey: 300, // 5 minutes
            Self.menuBarStyleKey: MenuBarStyle.iconOnly.rawValue
        ])
    }

    public func loadPreferences() {
        let styleStr = UserDefaults.standard.string(forKey: Self.menuBarStyleKey) ?? MenuBarStyle.iconOnly.rawValue
        self.menuBarStyle = MenuBarStyle(rawValue: styleStr) ?? .iconOnly
    }

    public func setMenuBarStyle(_ style: MenuBarStyle) {
        self.menuBarStyle = style
        UserDefaults.standard.set(style.rawValue, forKey: Self.menuBarStyleKey)
        if let snapshot = status.currentSnapshot {
            updateMenuBarPresentation(with: snapshot)
        }
    }

    public func setRefreshInterval(_ seconds: Int) {
        UserDefaults.standard.set(seconds, forKey: Self.refreshIntervalKey)
        startScheduledTimer()
    }

    public func startScheduledTimer() {
        timer?.invalidate()
        let interval = Double(UserDefaults.standard.integer(forKey: Self.refreshIntervalKey))
        let effectiveInterval = max(60.0, interval > 0 ? interval : 300.0)

        timer = Timer.scheduledTimer(withTimeInterval: effectiveInterval, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                await self?.refresh()
            }
        }
    }

    private func setupSleepWakeObserver() {
        wakeObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor [weak self] in
                // Immediate refresh upon system wake
                await self?.refresh()
            }
        }
    }

    public func refresh() async {
        guard !isRefreshing else { return }
        isRefreshing = true
        defer { isRefreshing = false }

        let newStatus = await repository.fetchLatestStatus()
        self.status = newStatus

        switch newStatus {
        case .ready(let snapshot), .stale(let snapshot, _):
            updateMenuBarPresentation(with: snapshot)
        case .cliNotInstalled:
            self.menuBarTitle = "Codex (未安装)"
            self.menuBarStatusLevel = .warning
        case .notLoggedIn:
            self.menuBarTitle = "Codex (未登录)"
            self.menuBarStatusLevel = .warning
        case .apiKeyMode:
            self.menuBarTitle = "Codex (API Key)"
            self.menuBarStatusLevel = .warning
        case .authExpired:
            self.menuBarTitle = "Codex (需重新登录)"
            self.menuBarStatusLevel = .critical
        case .loading:
            if status.currentSnapshot == nil {
                self.menuBarTitle = "Codex ..."
            }
        case .error:
            if status.currentSnapshot == nil {
                self.menuBarTitle = "Codex (!)"
                self.menuBarStatusLevel = .critical
            }
        }
    }

    public func onPopoverOpened() {
        // If last updated more than 60s ago or stale, trigger background refresh
        if let snapshot = status.currentSnapshot {
            if Date().timeIntervalSince(snapshot.updatedAt) > 60 {
                Task { [weak self] in
                    await self?.refresh()
                }
            }
        } else {
            Task { [weak self] in
                await self?.refresh()
            }
        }
    }

    private func updateMenuBarPresentation(with snapshot: UsageSnapshot) {
        guard !snapshot.windows.isEmpty else {
            self.menuBarTitle = "Codex"
            self.menuBarStatusLevel = .normal
            return
        }

        // Determine highest severity
        let hasCritical = snapshot.windows.contains { $0.statusLevel == .critical }
        let hasWarning = snapshot.windows.contains { $0.statusLevel == .warning }
        if hasCritical {
            self.menuBarStatusLevel = .critical
        } else if hasWarning {
            self.menuBarStatusLevel = .warning
        } else {
            self.menuBarStatusLevel = .normal
        }

        switch menuBarStyle {
        case .iconOnly:
            self.menuBarTitle = ""
        case .compact:
            if let first = snapshot.windows.first {
                let label = QuotaWindow.shortWindowLabel(durationMins: first.windowDurationMins)
                self.menuBarTitle = "\(label) \(first.remainingPercent)%"
            } else {
                self.menuBarTitle = "Codex"
            }
        case .full:
            let windowParts = snapshot.windows.prefix(2).map { window in
                let label = QuotaWindow.shortWindowLabel(durationMins: window.windowDurationMins)
                return "\(label) \(window.remainingPercent)%"
            }
            let windowsStr = windowParts.joined(separator: " · ")
            self.menuBarTitle = "Codex \(windowsStr)"
        }
    }
}
