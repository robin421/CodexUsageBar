import Foundation
import UserNotifications

public final class NotificationManager: @unchecked Sendable {
    public static let shared = NotificationManager()

    public static let notificationsEnabledKey = "notificationsEnabled"
    public static let warningThresholdKey = "warningThreshold"
    public static let criticalThresholdKey = "criticalThreshold"

    private let center = UNUserNotificationCenter.current()
    private var triggeredAlerts: [String: Set<String>] = [:]
    private var lastResetsAtMap: [String: Date] = [:]
    private let lock = NSLock()

    public init() {
        registerDefaults()
    }

    private func registerDefaults() {
        UserDefaults.standard.register(defaults: [
            Self.notificationsEnabledKey: true,
            Self.warningThresholdKey: 20,
            Self.criticalThresholdKey: 10
        ])
    }

    public func requestAuthorization() {
        center.requestAuthorization(options: [.alert, .sound, .badge]) { granted, error in
            // System authorization status updated
        }
    }

    public func checkAndNotify(for windows: [QuotaWindow]) {
        guard UserDefaults.standard.bool(forKey: Self.notificationsEnabledKey) else {
            return
        }

        let warningThreshold = UserDefaults.standard.integer(forKey: Self.warningThresholdKey)
        let criticalThreshold = UserDefaults.standard.integer(forKey: Self.criticalThresholdKey)

        lock.lock()
        defer { lock.unlock() }

        for window in windows {
            let key = window.name

            // Check if window has been reset or reset time updated
            if let resetsAt = window.resetsAt {
                if let lastReset = lastResetsAtMap[key], lastReset != resetsAt {
                    triggeredAlerts[key]?.removeAll()
                }
                lastResetsAtMap[key] = resetsAt
            }

            // If quota recovered above warning threshold, allow alerting again in future
            if window.remainingPercent >= warningThreshold {
                triggeredAlerts[key]?.removeAll()
                continue
            }

            var triggered = triggeredAlerts[key] ?? Set<String>()

            // Check critical threshold (< 10%)
            if window.remainingPercent < criticalThreshold {
                if !triggered.contains("critical") {
                    sendNotification(
                        title: "⚠️ Codex 额度告急 (\(window.name))",
                        body: "剩余额度仅剩 \(window.remainingPercent)%，重置时间：\(window.formattedResetTime())（\(window.countdownString())）"
                    )
                    triggered.insert("critical")
                    triggered.insert("warning")
                    triggeredAlerts[key] = triggered
                }
            } else if window.remainingPercent < warningThreshold {
                // Check warning threshold (< 20%)
                if !triggered.contains("warning") {
                    sendNotification(
                        title: "⚡️ Codex 额度预警 (\(window.name))",
                        body: "剩余额度低于 \(warningThreshold)%（当前剩余 \(window.remainingPercent)%），重置时间：\(window.formattedResetTime())"
                    )
                    triggered.insert("warning")
                    triggeredAlerts[key] = triggered
                }
            }
        }
    }

    private func sendNotification(title: String, body: String) {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        content.sound = .default

        let request = UNNotificationRequest(
            identifier: UUID().uuidString,
            content: content,
            trigger: nil // Deliver immediately
        )

        center.add(request) { _ in }
    }

    public func clearHistory() {
        lock.lock()
        defer { lock.unlock() }
        triggeredAlerts.removeAll()
        lastResetsAtMap.removeAll()
    }
}
