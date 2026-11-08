import Foundation

public actor UsageRepository {
    private let client: CodexAppServerClient
    private let cacheManager: UsageCacheManager
    private var inFlightRefreshTask: Task<AppStatus, Never>?

    public init(
        client: CodexAppServerClient = CodexAppServerClient(),
        cacheManager: UsageCacheManager = .shared
    ) {
        self.client = client
        self.cacheManager = cacheManager
    }

    public func fetchLatestStatus() async -> AppStatus {
        if let existing = inFlightRefreshTask {
            return await existing.value
        }

        let task = Task<AppStatus, Never> { [weak self] () -> AppStatus in
            guard let self = self else { return .loading }
            return await self.performFetch()
        }

        self.inFlightRefreshTask = task
        let result = await task.value
        self.inFlightRefreshTask = nil
        return result
    }

    private func performFetch() async -> AppStatus {
        do {
            let accountResp = try await client.readAccount()
            guard let rawAcc = accountResp.account else {
                return .notLoggedIn
            }

            if rawAcc.type == "apiKey" {
                return .apiKeyMode
            }

            let limitsResp = try await client.readRateLimits()

            // Extract limit snapshot: prioritize byLimitId["codex"] then fallback to rateLimits
            let snapshot = limitsResp.rateLimitsByLimitId?["codex"] ?? limitsResp.rateLimits

            var windows: [QuotaWindow] = []

            if let primary = snapshot?.primary {
                let name = QuotaWindow.dynamicWindowName(durationMins: primary.windowDurationMins)
                let date = primary.resetsAt.map { Date(timeIntervalSince1970: TimeInterval($0)) }
                let window = QuotaWindow(
                    id: "primary_\(primary.windowDurationMins ?? 0)",
                    name: name,
                    windowDurationMins: primary.windowDurationMins,
                    usedPercent: primary.usedPercent,
                    resetsAt: date
                )
                windows.append(window)
            }

            if let secondary = snapshot?.secondary {
                let name = QuotaWindow.dynamicWindowName(durationMins: secondary.windowDurationMins)
                let date = secondary.resetsAt.map { Date(timeIntervalSince1970: TimeInterval($0)) }
                let window = QuotaWindow(
                    id: "secondary_\(secondary.windowDurationMins ?? 0)",
                    name: name,
                    windowDurationMins: secondary.windowDurationMins,
                    usedPercent: secondary.usedPercent,
                    resetsAt: date
                )
                // Avoid duplicating if primary & secondary have identical durations
                if !windows.contains(where: { $0.windowDurationMins == secondary.windowDurationMins }) {
                    windows.append(window)
                }
            }

            // Sort windows by duration ascending (e.g. 5 hours before 7 days)
            windows.sort { ($0.windowDurationMins ?? 0) < ($1.windowDurationMins ?? 0) }

            let accountInfo = AccountInfo(
                email: rawAcc.email,
                planType: rawAcc.planType ?? snapshot?.planType ?? "unknown",
                accountType: rawAcc.type
            )

            let usageSnapshot = UsageSnapshot(
                account: accountInfo,
                windows: windows,
                credits: snapshot?.credits,
                updatedAt: Date()
            )

            // Persist to cache
            cacheManager.save(usageSnapshot)

            // Evaluate notifications
            NotificationManager.shared.checkAndNotify(for: windows)

            return .ready(usageSnapshot)
        } catch let clientError as ClientError {
            switch clientError {
            case .cliNotFound:
                return .cliNotInstalled(message: clientError.localizedDescription)
            case .notLoggedIn:
                return .notLoggedIn
            case .apiKeyModeNotSupported:
                return .apiKeyMode
            case .authExpired:
                return .authExpired
            default:
                return fallbackOrError(clientError.localizedDescription)
            }
        } catch {
            return fallbackOrError(error.localizedDescription)
        }
    }

    private func fallbackOrError(_ message: String) -> AppStatus {
        if let cached = cacheManager.load() {
            return .stale(cached, error: message)
        } else {
            return .error(message: message)
        }
    }

    public func getCachedSnapshot() -> UsageSnapshot? {
        return cacheManager.load()
    }
}
