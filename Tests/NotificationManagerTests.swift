import XCTest
@testable import CodexUsageBar

final class NotificationManagerTests: XCTestCase {

    override func setUp() {
        super.setUp()
        UserDefaults.standard.set(true, forKey: NotificationManager.notificationsEnabledKey)
        UserDefaults.standard.set(20, forKey: NotificationManager.warningThresholdKey)
        UserDefaults.standard.set(10, forKey: NotificationManager.criticalThresholdKey)
        NotificationManager.shared.clearHistory()
    }

    func testNotificationThresholdTriggers() {
        let mgr = NotificationManager.shared

        // 1. Normal quota (remaining 50%) -> should not trigger
        let normalWindow = QuotaWindow(
            id: "1",
            name: "5 小时额度",
            windowDurationMins: 300,
            usedPercent: 50,
            resetsAt: Date().addingTimeInterval(3600)
        )
        mgr.checkAndNotify(for: [normalWindow])

        // 2. Warning quota (remaining 15%) -> should trigger warning
        let warningWindow = QuotaWindow(
            id: "1",
            name: "5 小时额度",
            windowDurationMins: 300,
            usedPercent: 85,
            resetsAt: Date().addingTimeInterval(3600)
        )
        mgr.checkAndNotify(for: [warningWindow])

        // 3. Second check with same warning -> deduplication should suppress
        mgr.checkAndNotify(for: [warningWindow])

        // 4. Critical quota (remaining 5%) -> should trigger critical
        let criticalWindow = QuotaWindow(
            id: "1",
            name: "5 小时额度",
            windowDurationMins: 300,
            usedPercent: 95,
            resetsAt: Date().addingTimeInterval(3600)
        )
        mgr.checkAndNotify(for: [criticalWindow])

        // 5. Quota resets / recovers to 80% -> clears trigger history
        let recoveredWindow = QuotaWindow(
            id: "1",
            name: "5 小时额度",
            windowDurationMins: 300,
            usedPercent: 20,
            resetsAt: Date().addingTimeInterval(7200)
        )
        mgr.checkAndNotify(for: [recoveredWindow])
    }
}
