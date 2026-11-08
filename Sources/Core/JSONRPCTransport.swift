import Foundation

public enum TransportError: LocalizedError, Sendable {
    case processNotRunning
    case requestTimedOut(id: Int)
    case channelClosed
    case invalidResponse(String)
    case serverError(code: Int, message: String)

    public var errorDescription: String? {
        switch self {
        case .processNotRunning:
            return "Codex App Server 进程未运行"
        case .requestTimedOut(let id):
            return "JSON-RPC 请求超时 (id: \(id))"
        case .channelClosed:
            return "通信通道已关闭"
        case .invalidResponse(let detail):
            return "无效的响应数据: \(detail)"
        case .serverError(let code, let message):
            return "服务端错误 (\(code)): \(message)"
        }
    }
}

public actor JSONRPCTransport {
    private var nextRequestId: Int = 1
    private var pendingRequests: [Int: CheckedContinuation<Data, Error>] = [:]
    private var stdinHandle: FileHandle?
    private var stdoutHandle: FileHandle?
    private var stderrHandle: FileHandle?
    private var readTask: Task<Void, Never>?
    private var stderrTask: Task<Void, Never>?
    private var isClosed = false
    private var onNotification: (@Sendable (String, Data) -> Void)?

    public init() {}

    public func attach(
        stdin: FileHandle,
        stdout: FileHandle,
        stderr: FileHandle,
        onNotification: (@Sendable (String, Data) -> Void)? = nil
    ) {
        self.stdinHandle = stdin
        self.stdoutHandle = stdout
        self.stderrHandle = stderr
        self.onNotification = onNotification
        self.isClosed = false

        startReadingStdout(stdout)
        startDrainingStderr(stderr)
    }

    public func setOnNotification(_ handler: (@Sendable (String, Data) -> Void)?) {
        self.onNotification = handler
    }

    public func close() {
        isClosed = true
        readTask?.cancel()
        readTask = nil
        stderrTask?.cancel()
        stderrTask = nil

        for (_, continuation) in pendingRequests {
            continuation.resume(throwing: TransportError.channelClosed)
        }
        pendingRequests.removeAll()

        try? stdinHandle?.close()
        try? stdoutHandle?.close()
        try? stderrHandle?.close()
        stdinHandle = nil
        stdoutHandle = nil
        stderrHandle = nil
    }

    public func sendNotification<T: Encodable>(method: String, params: T? = nil) throws {
        guard !isClosed, let stdin = stdinHandle else {
            throw TransportError.processNotRunning
        }

        let notification = JSONRPCNotification(method: method, params: params)
        let data = try JSONEncoder().encode(notification)
        var lineData = data
        lineData.append(0x0A) // '\n'
        try stdin.write(contentsOf: lineData)
    }

    public func sendRequest<P: Encodable, R: Decodable>(
        method: String,
        params: P,
        timeoutSeconds: TimeInterval = 15.0
    ) async throws -> R {
        guard !isClosed, let stdin = stdinHandle else {
            throw TransportError.processNotRunning
        }

        let reqId = nextRequestId
        nextRequestId += 1

        let request = JSONRPCRequest(id: reqId, method: method, params: params)
        let data = try JSONEncoder().encode(request)
        var lineData = data
        lineData.append(0x0A) // '\n'

        let responseData: Data = try await withCheckedThrowingContinuation { continuation in
            self.pendingRequests[reqId] = continuation

            do {
                try stdin.write(contentsOf: lineData)
            } catch {
                self.pendingRequests.removeValue(forKey: reqId)
                continuation.resume(throwing: error)
                return
            }

            // Timeout watchdog
            Task { [weak self] in
                try? await Task.sleep(nanoseconds: UInt64(timeoutSeconds * 1_000_000_000))
                await self?.handleTimeout(for: reqId)
            }
        }

        // Parse JSON-RPC response
        let decoder = JSONDecoder()
        if let jsonObject = try? JSONSerialization.jsonObject(with: responseData) as? [String: Any] {
            if let errorObj = jsonObject["error"] as? [String: Any] {
                let code = errorObj["code"] as? Int ?? -1
                let message = errorObj["message"] as? String ?? "Unknown error"
                throw TransportError.serverError(code: code, message: message)
            }
        }

        let rpcResponse = try decoder.decode(JSONRPCResponse<R>.self, from: responseData)
        if let result = rpcResponse.result {
            return result
        } else if let error = rpcResponse.error {
            throw TransportError.serverError(code: error.code, message: error.message)
        } else {
            throw TransportError.invalidResponse("缺少 result 或 error 字段")
        }
    }

    private func handleTimeout(for reqId: Int) {
        if let continuation = pendingRequests.removeValue(forKey: reqId) {
            continuation.resume(throwing: TransportError.requestTimedOut(id: reqId))
        }
    }

    private func startReadingStdout(_ stdout: FileHandle) {
        readTask = Task { [weak self] in
            var buffer = Data()
            while !Task.isCancelled {
                guard let chunk = try? stdout.read(upToCount: 4096), !chunk.isEmpty else {
                    break
                }
                buffer.append(chunk)

                while let newlineIndex = buffer.firstIndex(of: 0x0A) {
                    let lineData = buffer.subdata(in: 0..<newlineIndex)
                    buffer.removeSubrange(0...newlineIndex)

                    if !lineData.isEmpty {
                        await self?.processIncomingLine(lineData)
                    }
                }
            }
            await self?.close()
        }
    }

    private func startDrainingStderr(_ stderr: FileHandle) {
        stderrTask = Task {
            while !Task.isCancelled {
                guard let chunk = try? stderr.read(upToCount: 4096), !chunk.isEmpty else {
                    break
                }
                // Drain stderr to prevent pipe buffer from filling up
            }
        }
    }

    private func processIncomingLine(_ lineData: Data) {
        guard let json = try? JSONSerialization.jsonObject(with: lineData) as? [String: Any] else {
            return
        }

        if let reqId = json["id"] as? Int {
            if let continuation = pendingRequests.removeValue(forKey: reqId) {
                continuation.resume(returning: lineData)
            }
        } else if let method = json["method"] as? String {
            onNotification?(method, lineData)
        }
    }
}
