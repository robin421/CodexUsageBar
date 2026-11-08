import Foundation

public final class UsageCacheManager: Sendable {
    public static let shared = UsageCacheManager()

    private let cacheURL: URL

    public init(fileManager: FileManager = .default) {
        let appSupport = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: "\(NSHomeDirectory())/Library/Application Support")
        let dir = appSupport.appendingPathComponent("CodexUsageBar", isDirectory: true)

        if !fileManager.fileExists(atPath: dir.path) {
            try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
        }

        self.cacheURL = dir.appendingPathComponent("usage_cache.json")
    }

    public func save(_ snapshot: UsageSnapshot) {
        do {
            let data = try JSONEncoder().encode(snapshot)
            try data.write(to: cacheURL, options: .atomic)
        } catch {
            // Non-critical, ignore write errors
        }
    }

    public func load() -> UsageSnapshot? {
        guard let data = try? Data(contentsOf: cacheURL) else {
            return nil
        }
        return try? JSONDecoder().decode(UsageSnapshot.self, from: data)
    }

    public func clear() {
        try? FileManager.default.removeItem(at: cacheURL)
    }
}
