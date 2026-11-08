import XCTest
@testable import CodexUsageBar

final class UsageModelsTests: XCTestCase {

    func testDecodingChatGPTPlusRealPayload() throws {
        let json = """
        {
          "id": 3,
          "result": {
            "rateLimits": {
              "limitId": "codex",
              "limitName": null,
              "primary": {
                "usedPercent": 18,
                "windowDurationMins": 300,
                "resetsAt": 1794086814
              },
              "secondary": {
                "usedPercent": 36,
                "windowDurationMins": 10080,
                "resetsAt": 1794350644
              },
              "credits": {
                "hasCredits": false,
                "unlimited": false,
                "balance": "0"
              },
              "planType": "plus",
              "rateLimitReachedType": null
            },
            "rateLimitsByLimitId": {
              "codex": {
                "limitId": "codex",
                "limitName": null,
                "primary": {
                  "usedPercent": 18,
                  "windowDurationMins": 300,
                  "resetsAt": 1794086814
                },
                "secondary": {
                  "usedPercent": 36,
                  "windowDurationMins": 10080,
                  "resetsAt": 1794350644
                },
                "credits": {
                  "hasCredits": true,
                  "unlimited": false,
                  "balance": "2"
                },
                "planType": "plus",
                "rateLimitReachedType": null
              }
            }
          }
        }
        """.data(using: .utf8)!

        let response = try JSONDecoder().decode(JSONRPCResponse<GetAccountRateLimitsResponse>.self, from: json)
        XCTAssertNotNil(response.result)

        let codexSnapshot = response.result?.rateLimitsByLimitId?["codex"]
        XCTAssertNotNil(codexSnapshot)
        XCTAssertEqual(codexSnapshot?.primary?.usedPercent, 18)
        XCTAssertEqual(codexSnapshot?.primary?.windowDurationMins, 300)
        XCTAssertEqual(codexSnapshot?.secondary?.usedPercent, 36)
        XCTAssertEqual(codexSnapshot?.secondary?.windowDurationMins, 10080)
        XCTAssertEqual(codexSnapshot?.credits?.balance, "2")
        XCTAssertEqual(codexSnapshot?.credits?.hasCredits, true)
    }

    func testDecodingAccountResponse() throws {
        let chatgptJson = """
        {
          "id": 2,
          "result": {
            "account": {
              "type": "chatgpt",
              "email": "user@example.com",
              "planType": "pro"
            },
            "requiresOpenaiAuth": true
          }
        }
        """.data(using: .utf8)!

        let chatgptResp = try JSONDecoder().decode(JSONRPCResponse<GetAccountResponse>.self, from: chatgptJson)
        XCTAssertEqual(chatgptResp.result?.account?.email, "user@example.com")
        XCTAssertEqual(chatgptResp.result?.account?.planType, "pro")
        XCTAssertEqual(chatgptResp.result?.account?.type, "chatgpt")

        let apiKeyJson = """
        {
          "id": 2,
          "result": {
            "account": {
              "type": "apiKey"
            },
            "requiresOpenaiAuth": false
          }
        }
        """.data(using: .utf8)!

        let apiKeyResp = try JSONDecoder().decode(JSONRPCResponse<GetAccountResponse>.self, from: apiKeyJson)
        XCTAssertEqual(apiKeyResp.result?.account?.type, "apiKey")
        XCTAssertNil(apiKeyResp.result?.account?.email)
    }

    func testDynamicWindowNames() {
        XCTAssertEqual(QuotaWindow.dynamicWindowName(durationMins: 300), "5 小时额度")
        XCTAssertEqual(QuotaWindow.dynamicWindowName(durationMins: 1440), "24 小时额度")
        XCTAssertEqual(QuotaWindow.dynamicWindowName(durationMins: 10080), "每周额度")
        XCTAssertEqual(QuotaWindow.dynamicWindowName(durationMins: 20160), "2 周额度")
        XCTAssertEqual(QuotaWindow.dynamicWindowName(durationMins: 2880), "2 天额度")
        XCTAssertEqual(QuotaWindow.dynamicWindowName(durationMins: 120), "2 小时额度")
        XCTAssertEqual(QuotaWindow.dynamicWindowName(durationMins: 45), "45 分钟额度")
        XCTAssertEqual(QuotaWindow.dynamicWindowName(durationMins: nil), "额度窗口")
    }

    func testShortWindowLabels() {
        XCTAssertEqual(QuotaWindow.shortWindowLabel(durationMins: 300), "5h")
        XCTAssertEqual(QuotaWindow.shortWindowLabel(durationMins: 1440), "24h")
        XCTAssertEqual(QuotaWindow.shortWindowLabel(durationMins: 10080), "7d")
        XCTAssertEqual(QuotaWindow.shortWindowLabel(durationMins: 2880), "2d")
        XCTAssertEqual(QuotaWindow.shortWindowLabel(durationMins: 120), "2h")
        XCTAssertEqual(QuotaWindow.shortWindowLabel(durationMins: nil), "Quota")
    }

    func testRemainingPercentCalculationAndBounds() {
        let normal = QuotaWindow(id: "1", name: "5h", windowDurationMins: 300, usedPercent: 18, resetsAt: nil)
        XCTAssertEqual(normal.remainingPercent, 82)
        XCTAssertEqual(normal.statusLevel, .normal)

        let zeroRemaining = QuotaWindow(id: "2", name: "5h", windowDurationMins: 300, usedPercent: 100, resetsAt: nil)
        XCTAssertEqual(zeroRemaining.remainingPercent, 0)
        XCTAssertEqual(zeroRemaining.statusLevel, .critical)

        let overflowUsed = QuotaWindow(id: "3", name: "5h", windowDurationMins: 300, usedPercent: 120, resetsAt: nil)
        XCTAssertEqual(overflowUsed.remainingPercent, 0)
        XCTAssertEqual(overflowUsed.statusLevel, .critical)

        let negativeUsed = QuotaWindow(id: "4", name: "5h", windowDurationMins: 300, usedPercent: -10, resetsAt: nil)
        XCTAssertEqual(negativeUsed.remainingPercent, 100)
        XCTAssertEqual(negativeUsed.statusLevel, .normal)

        let warning = QuotaWindow(id: "5", name: "5h", windowDurationMins: 300, usedPercent: 85, resetsAt: nil)
        XCTAssertEqual(warning.remainingPercent, 15)
        XCTAssertEqual(warning.statusLevel, .warning)

        let critical = QuotaWindow(id: "6", name: "5h", windowDurationMins: 300, usedPercent: 95, resetsAt: nil)
        XCTAssertEqual(critical.remainingPercent, 5)
        XCTAssertEqual(critical.statusLevel, .critical)
    }

    func testCountdownFormatting() {
        let now = Date()
        let reset1 = now.addingTimeInterval(3600 * 2 + 60 * 15) // 2h 15m
        let w1 = QuotaWindow(id: "1", name: "5h", windowDurationMins: 300, usedPercent: 50, resetsAt: reset1)
        XCTAssertEqual(w1.countdownString(from: now), "还有 2 小时 15 分钟")

        let reset2 = now.addingTimeInterval(86400 * 3 + 3600 * 5) // 3d 5h
        let w2 = QuotaWindow(id: "2", name: "7d", windowDurationMins: 10080, usedPercent: 50, resetsAt: reset2)
        XCTAssertEqual(w2.countdownString(from: now), "还有 3 天 5 小时")

        let resetPast = now.addingTimeInterval(-60)
        let w3 = QuotaWindow(id: "3", name: "5h", windowDurationMins: 300, usedPercent: 50, resetsAt: resetPast)
        XCTAssertEqual(w3.countdownString(from: now), "已重置")

        let w4 = QuotaWindow(id: "4", name: "5h", windowDurationMins: 300, usedPercent: 50, resetsAt: nil)
        XCTAssertEqual(w4.countdownString(from: now), "重置时间未知")
    }

    func testAccountInfoDisplayNames() {
        let plus = AccountInfo(email: "a@b.com", planType: "plus", accountType: "chatgpt")
        XCTAssertEqual(plus.displayPlanName, "ChatGPT Plus")

        let pro = AccountInfo(email: "a@b.com", planType: "pro", accountType: "chatgpt")
        XCTAssertEqual(pro.displayPlanName, "ChatGPT Pro")

        let team = AccountInfo(email: "a@b.com", planType: "team", accountType: "chatgpt")
        XCTAssertEqual(team.displayPlanName, "ChatGPT Team")

        let custom = AccountInfo(email: "a@b.com", planType: "custom_tier", accountType: "chatgpt")
        XCTAssertEqual(custom.displayPlanName, "Custom_Tier")
    }
}
