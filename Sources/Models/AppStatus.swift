import Foundation

public enum AppStatus: Sendable, Equatable {
    case loading
    case ready(UsageSnapshot)
    case stale(UsageSnapshot, error: String)
    case cliNotInstalled(message: String)
    case notLoggedIn
    case apiKeyMode
    case authExpired
    case error(message: String)

    public var isSuccess: Bool {
        switch self {
        case .ready:
            return true
        default:
            return false
        }
    }

    public var currentSnapshot: UsageSnapshot? {
        switch self {
        case .ready(let snapshot):
            return snapshot
        case .stale(let snapshot, _):
            return snapshot
        default:
            return nil
        }
    }
}
