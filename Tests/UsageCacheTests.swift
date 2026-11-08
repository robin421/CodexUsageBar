import XCTest
@testable import CodexUsageBar

final class UsageCacheTests: XCTestCase {

    func testSaveAndLoadCache() throws {
        let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let cacheManager = UsageCacheManager(fileManager: .default)

        let account = AccountInfo(email: "test@openai.com", planType: "plus", accountType: "chatgpt")
        let window1 = QuotaWindow(id: "1", name: "5 小时额度", windowDurationMins: 300, usedPercent: 25, resetsAt: Date())
        let window2 = QuotaWindow(id: "2", name: "每周额度", windowDurationMins: 10080, usedPercent: 40, resetsAt: Date().addingTimeInterval(86400 * 3))
        let credits = CreditsSnapshot(hasCredits: true, unlimited: false, balance: "3")

        let snapshot = UsageSnapshot(account: account, windows: [window1, window2], credits: credits, updatedAt: Date())

        cacheManager.save(snapshot)

        let loaded = cacheManager.load()
        XCTAssertNotNil(loaded)
        XCTAssertEqual(loaded?.account.email, "test@openai.com")
        XCTAssertEqual(loaded?.account.planType, "plus")
        XCTAssertEqual(loaded?.windows.count, 2)
        XCTAssertEqual(loaded?.windows.first?.remainingPercent, 75)
        XCTAssertEqual(loaded?.credits?.balance, "3")

        cacheManager.clear()
        XCTAssertNil(cacheManager.load())
    }
}
