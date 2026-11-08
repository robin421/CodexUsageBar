import Foundation

public struct CLIPathFinder: Sendable {
    public static let userDefaultsKey = "customCodexCLIPath"

    public static func findCodexPath() -> String? {
        // 1. Check custom path in UserDefaults
        if let customPath = UserDefaults.standard.string(forKey: userDefaultsKey),
           !customPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let expanded = (customPath as NSString).expandingTildeInPath
            if FileManager.default.isExecutableFile(atPath: expanded) {
                return expanded
            }
        }

        // 2. Check standard installation locations
        let home = NSHomeDirectory()
        let standardCandidates = [
            "/opt/homebrew/bin/codex",
            "/usr/local/bin/codex",
            "\(home)/.cargo/bin/codex",
            "\(home)/.local/bin/codex",
            "/usr/bin/codex"
        ]

        for path in standardCandidates {
            if FileManager.default.isExecutableFile(atPath: path) {
                return path
            }
        }

        // 3. Fallback: query user's interactive login shell
        if let shellPath = resolveFromLoginShell() {
            return shellPath
        }

        return nil
    }

    private static func resolveFromLoginShell() -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-l", "-c", "which codex"]

        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = Pipe()

        do {
            try process.run()
            process.waitUntilExit()

            if process.terminationStatus == 0 {
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                if let output = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
                   !output.isEmpty,
                   FileManager.default.isExecutableFile(atPath: output) {
                    return output
                }
            }
        } catch {
            return nil
        }

        return nil
    }
}
