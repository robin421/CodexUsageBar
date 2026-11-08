import Foundation

public enum ClientError: LocalizedError, Sendable {
    case cliNotFound
    case failedToLaunchProcess(String)
    case handshakeFailed(String)
    case notLoggedIn
    case apiKeyModeNotSupported
    case authExpired

    public var errorDescription: String? {
        switch self {
        case .cliNotFound:
            return "未找到 Codex CLI，请先安装 Codex"
        case .failedToLaunchProcess(let detail):
            return "启动 Codex App Server 失败: \(detail)"
        case .handshakeFailed(let detail):
            return "初始化协议握手失败: \(detail)"
        case .notLoggedIn:
            return "当前未登录 Codex 账号，请在终端执行 'codex login'"
        case .apiKeyModeNotSupported:
            return "当前使用 API Key 登录，无法直接读取 ChatGPT 套餐额度"
        case .authExpired:
            return "Codex 认证已失效，请重新执行 'codex login'"
        }
    }
}

public actor CodexAppServerClient {
    private var process: Process?
    private let transport = JSONRPCTransport()
    private var isInitialized = false
    private var isStarting = false

    public var onRateLimitsUpdated: (@Sendable () -> Void)?

    public init() {}

    private func handleRateLimitsNotification() {
        onRateLimitsUpdated?()
    }

    public func isRunning() -> Bool {
        return process?.isRunning == true && isInitialized
    }

    public func startIfNeeded() async throws {
        if isRunning() {
            return
        }

        guard !isStarting else {
            // Wait briefly if already starting
            try? await Task.sleep(nanoseconds: 500_000_000)
            if isRunning() { return }
            throw ClientError.handshakeFailed("启动进行中，请稍候重试")
        }

        isStarting = true
        defer { isStarting = false }

        // 1. Locate executable
        guard let cliPath = CLIPathFinder.findCodexPath() else {
            throw ClientError.cliNotFound
        }

        // 2. Tear down previous process if any
        await stop()

        // 3. Prepare process and pipes
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: cliPath)
        proc.arguments = ["app-server"]

        let stdinPipe = Pipe()
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()

        proc.standardInput = stdinPipe
        proc.standardOutput = stdoutPipe
        proc.standardError = stderrPipe

        proc.terminationHandler = { [weak self] _ in
            Task { [weak self] in
                await self?.handleProcessTermination()
            }
        }

        do {
            try proc.run()
        } catch {
            throw ClientError.failedToLaunchProcess(error.localizedDescription)
        }

        self.process = proc
        await transport.attach(
            stdin: stdinPipe.fileHandleForWriting,
            stdout: stdoutPipe.fileHandleForReading,
            stderr: stderrPipe.fileHandleForReading,
            onNotification: { [weak self] method, _ in
                if method.contains("RateLimits") || method.contains("rateLimits") {
                    Task { [weak self] in
                        await self?.handleRateLimitsNotification()
                    }
                }
            }
        )

        // 4. Perform handshake
        do {
            let initParams = InitializeParams(
                clientInfo: .init(name: "CodexUsageBar", version: "1.0.0")
            )
            let _: InitializeResult = try await transport.sendRequest(
                method: "initialize",
                params: initParams,
                timeoutSeconds: 8.0
            )

            try await transport.sendNotification(method: "initialized", params: EmptyParams())
            self.isInitialized = true
        } catch {
            await stop()
            throw ClientError.handshakeFailed(error.localizedDescription)
        }
    }

    public func readAccount() async throws -> GetAccountResponse {
        try await startIfNeeded()
        let params = AccountReadParams(refreshToken: false)
        return try await transport.sendRequest(
            method: "account/read",
            params: params,
            timeoutSeconds: 10.0
        )
    }

    public func readRateLimits() async throws -> GetAccountRateLimitsResponse {
        try await startIfNeeded()
        return try await transport.sendRequest(
            method: "account/rateLimits/read",
            params: EmptyParams(),
            timeoutSeconds: 10.0
        )
    }

    public func stop() async {
        isInitialized = false
        await transport.close()

        if let proc = process {
            proc.terminationHandler = nil
            if proc.isRunning {
                proc.terminate()
            }
            process = nil
        }
    }

    private func handleProcessTermination() async {
        isInitialized = false
        await transport.close()
        process = nil
    }
}
