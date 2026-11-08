import Foundation

// MARK: - JSON-RPC Base Types

public struct JSONRPCRequest<T: Encodable>: Encodable {
    public let jsonrpc: String
    public let id: Int
    public let method: String
    public let params: T

    public init(id: Int, method: String, params: T) {
        self.jsonrpc = "2.0"
        self.id = id
        self.method = method
        self.params = params
    }
}

public struct JSONRPCNotification<T: Encodable>: Encodable {
    public let jsonrpc: String
    public let method: String
    public let params: T?

    public init(method: String, params: T? = nil) {
        self.jsonrpc = "2.0"
        self.method = method
        self.params = params
    }
}

public struct EmptyParams: Codable {
    public init() {}
}

public struct InitializeParams: Codable {
    public struct ClientInfo: Codable {
        public let name: String
        public let version: String

        public init(name: String, version: String) {
            self.name = name
            self.version = version
        }
    }

    public let clientInfo: ClientInfo

    public init(clientInfo: ClientInfo) {
        self.clientInfo = clientInfo
    }
}

public struct AccountReadParams: Codable {
    public let refreshToken: Bool

    public init(refreshToken: Bool = false) {
        self.refreshToken = refreshToken
    }
}

public struct JSONRPCResponse<T: Decodable>: Decodable {
    public let id: Int?
    public let result: T?
    public let error: JSONRPCError?
}

public struct JSONRPCError: Decodable, Error {
    public let code: Int
    public let message: String
    public let data: AnyCodableValue?
}

public struct InitializeResult: Codable, Sendable {
    public let userAgent: String?
    public let codexHome: String?
    public let platformFamily: String?
    public let platformOs: String?
}

public struct AnyCodableValue: Codable, @unchecked Sendable {
    public let value: Any

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let string = try? container.decode(String.self) {
            value = string
        } else if let int = try? container.decode(Int.self) {
            value = int
        } else if let double = try? container.decode(Double.self) {
            value = double
        } else if let bool = try? container.decode(Bool.self) {
            value = bool
        } else {
            value = ""
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        if let string = value as? String {
            try container.encode(string)
        } else if let int = value as? Int {
            try container.encode(int)
        } else if let double = value as? Double {
            try container.encode(double)
        } else if let bool = value as? Bool {
            try container.encode(bool)
        }
    }
}

// MARK: - Server Protocol Models

public struct GetAccountResponse: Codable, Sendable {
    public let account: RawAccount?
    public let requiresOpenaiAuth: Bool?
}

public struct RawAccount: Codable, Sendable {
    public let type: String
    public let email: String?
    public let planType: String?
}

public struct GetAccountRateLimitsResponse: Codable, Sendable {
    public let rateLimits: RateLimitSnapshot?
    public let rateLimitsByLimitId: [String: RateLimitSnapshot]?
}

public struct RateLimitSnapshot: Codable, Sendable {
    public let limitId: String?
    public let limitName: String?
    public let planType: String?
    public let primary: RateLimitWindow?
    public let secondary: RateLimitWindow?
    public let credits: CreditsSnapshot?
    public let rateLimitReachedType: String?
}

public struct RateLimitWindow: Codable, Sendable, Equatable {
    public let usedPercent: Int
    public let windowDurationMins: Int?
    public let resetsAt: Int64?
}

public struct CreditsSnapshot: Codable, Sendable, Equatable {
    public let hasCredits: Bool
    public let unlimited: Bool
    public let balance: String?
}

// MARK: - Domain Models for App & UI

public enum QuotaStatusLevel: String, Sendable, Codable {
    case normal   // >= 20%
    case warning  // 10% - 19%
    case critical // < 10%
}

public struct QuotaWindow: Identifiable, Sendable, Codable, Equatable {
    public let id: String
    public let name: String
    public let windowDurationMins: Int?
    public let usedPercent: Int
    public let remainingPercent: Int
    public let resetsAt: Date?

    public init(
        id: String,
        name: String,
        windowDurationMins: Int?,
        usedPercent: Int,
        resetsAt: Date?
    ) {
        self.id = id
        self.name = name
        self.windowDurationMins = windowDurationMins
        self.usedPercent = max(0, min(100, usedPercent))
        self.remainingPercent = max(0, min(100, 100 - self.usedPercent))
        self.resetsAt = resetsAt
    }

    public var statusLevel: QuotaStatusLevel {
        if remainingPercent < 10 {
            return .critical
        } else if remainingPercent < 20 {
            return .warning
        } else {
            return .normal
        }
    }

    public func countdownString(from now: Date = Date()) -> String {
        guard let resetsAt = resetsAt else {
            return "重置时间未知"
        }

        let diff = resetsAt.timeIntervalSince(now)
        if diff <= 0 {
            return "已重置"
        }

        let totalSeconds = Int(diff)
        let days = totalSeconds / 86400
        let hours = (totalSeconds % 86400) / 3600
        let minutes = (totalSeconds % 3600) / 60

        if days > 0 {
            if hours > 0 {
                return "还有 \(days) 天 \(hours) 小时"
            } else {
                return "还有 \(days) 天"
            }
        } else if hours > 0 {
            if minutes > 0 {
                return "还有 \(hours) 小时 \(minutes) 分钟"
            } else {
                return "还有 \(hours) 小时"
            }
        } else if minutes > 0 {
            return "还有 \(minutes) 分钟"
        } else {
            return "即将重置"
        }
    }

    public func formattedResetTime(timeZone: TimeZone = .current) -> String {
        guard let resetsAt = resetsAt else {
            return "未知"
        }

        let formatter = DateFormatter()
        formatter.timeZone = timeZone
        formatter.locale = Locale(identifier: "zh_CN")

        let calendar = Calendar.current
        if calendar.isDateInToday(resetsAt) {
            formatter.dateFormat = "HH:mm"
            return "今天 " + formatter.string(from: resetsAt)
        } else if calendar.isDateInTomorrow(resetsAt) {
            formatter.dateFormat = "HH:mm"
            return "明天 " + formatter.string(from: resetsAt)
        } else {
            formatter.dateFormat = "M月d日 HH:mm"
            return formatter.string(from: resetsAt)
        }
    }

    public static func dynamicWindowName(durationMins: Int?) -> String {
        guard let mins = durationMins, mins > 0 else {
            return "额度窗口"
        }

        if mins == 300 {
            return "5 小时额度"
        } else if mins == 1440 {
            return "24 小时额度"
        } else if mins == 10080 {
            return "每周额度"
        } else if mins % 10080 == 0 {
            return "\(mins / 10080) 周额度"
        } else if mins % 1440 == 0 {
            return "\(mins / 1440) 天额度"
        } else if mins % 60 == 0 {
            return "\(mins / 60) 小时额度"
        } else {
            return "\(mins) 分钟额度"
        }
    }

    public static func shortWindowLabel(durationMins: Int?) -> String {
        guard let mins = durationMins, mins > 0 else {
            return "Quota"
        }
        if mins == 300 {
            return "5h"
        } else if mins == 1440 {
            return "24h"
        } else if mins == 10080 {
            return "7d"
        } else if mins % 1440 == 0 {
            return "\(mins / 1440)d"
        } else if mins % 60 == 0 {
            return "\(mins / 60)h"
        } else {
            return "\(mins)m"
        }
    }
}

public struct AccountInfo: Sendable, Codable, Equatable {
    public let email: String?
    public let planType: String
    public let accountType: String // "chatgpt", "apiKey", "amazonBedrock", "unknown"

    public init(email: String?, planType: String, accountType: String) {
        self.email = email
        self.planType = planType
        self.accountType = accountType
    }

    public var displayPlanName: String {
        switch planType.lowercased() {
        case "plus": return "ChatGPT Plus"
        case "pro": return "ChatGPT Pro"
        case "prolite": return "ChatGPT Pro Lite"
        case "team": return "ChatGPT Team"
        case "free": return "Free"
        case "business": return "Business"
        case "enterprise": return "Enterprise"
        case "edu": return "Edu"
        default: return planType.capitalized
        }
    }
}

public struct UsageSnapshot: Sendable, Codable, Equatable {
    public let account: AccountInfo
    public let windows: [QuotaWindow]
    public let credits: CreditsSnapshot?
    public let updatedAt: Date

    public init(
        account: AccountInfo,
        windows: [QuotaWindow],
        credits: CreditsSnapshot?,
        updatedAt: Date = Date()
    ) {
        self.account = account
        self.windows = windows
        self.credits = credits
        self.updatedAt = updatedAt
    }
}
