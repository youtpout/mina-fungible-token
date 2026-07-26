import Foundation

/// The Rust prover, behind the three `extern "C"` calls of `ffi.rs`.
///
/// Every call blocks for as long as the proof takes — seconds at best — so
/// they all run on `queue`, the single serial queue that stands in for the
/// Android app's `Executors.newSingleThreadExecutor()`. The compile is a
/// per-process `OnceLock` on the Rust side, so keeping one queue also keeps
/// the second transfer of a session free of it.
enum MinaBackend {
    private static let queue = DispatchQueue(label: "com.lumina.mina.prover")

    /// Calls one of the C entry points and copies the JSON out before Rust
    /// takes its allocation back.
    private static func call(
        _ function: (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?,
        _ request: String
    ) -> String {
        guard let raw = request.withCString({ function($0) }) else {
            return #"{"status":"error","message":"the native backend returned nothing"}"#
        }
        defer { mina_string_free(raw) }
        return String(cString: raw)
    }

    /// Runs `work` off the main thread and delivers its result back on it.
    private static func async(_ work: @escaping () -> String, then done: @escaping (String) -> Void) {
        queue.async {
            let response = work()
            DispatchQueue.main.async { done(response) }
        }
    }

    static func backendInfo(_ done: @escaping (String) -> Void) {
        async({ call({ _ in mina_backend_info() }, "") }, then: done)
    }

    static func transfer(_ request: TransferRequest, _ done: @escaping (String) -> Void) {
        let json = request.json
        async({ call(mina_transfer, json) }, then: done)
    }

    static func tokenBalance(_ request: BalanceRequest, _ done: @escaping (String) -> Void) {
        let json = request.json
        async({ call(mina_token_balance, json) }, then: done)
    }
}

/// The request `nativeTransfer` reads, field for field.
struct TransferRequest {
    var senderPrivateKey: String
    var receiver: String
    var amount: String
    var tokenAddress: String
    var graphqlUrl: String
    var fundReceiver: Bool

    var json: String {
        jsonObject([
            "senderPrivateKey": senderPrivateKey,
            "receiver": receiver,
            "amount": amount,
            "tokenAddress": tokenAddress,
            "graphqlUrl": graphqlUrl,
            "fundReceiver": fundReceiver,
        ])
    }
}

/// The request `nativeTokenBalance` reads.
struct BalanceRequest {
    var address: String
    var tokenAddress: String
    var graphqlUrl: String

    var json: String {
        jsonObject([
            "address": address,
            "tokenAddress": tokenAddress,
            "graphqlUrl": graphqlUrl,
        ])
    }
}

/// Serialises a flat object. `JSONSerialization` would reorder the keys, which
/// costs nothing here, but it also refuses non-`NSObject` values, so booleans
/// are wrapped explicitly.
private func jsonObject(_ fields: [String: Any]) -> String {
    let object = fields.mapValues { value -> Any in
        if let flag = value as? Bool { return NSNumber(value: flag) }
        return value
    }
    guard let data = try? JSONSerialization.data(withJSONObject: object),
          let json = String(data: data, encoding: .utf8)
    else {
        return "{}"
    }
    return json
}

/// The pieces of a response the UI reads. Anything unparseable falls back to
/// showing the raw payload, as on Android.
struct TransferResponse {
    var status: String?
    var message: String?
    var transactionHash: String?
    var balance: String?
    var timings: [String: Int]

    init(_ raw: String) {
        let object = (try? JSONSerialization.jsonObject(with: Data(raw.utf8))) as? [String: Any] ?? [:]
        status = object["status"] as? String
        message = object["message"] as? String
        transactionHash = object["transactionHash"] as? String
        balance = object["balance"] as? String
        let milliseconds = object["timingsMs"] as? [String: Any] ?? [:]
        timings = milliseconds.compactMapValues { ($0 as? NSNumber)?.intValue }
    }

    /// `Transfer submitted: <hash>` or the failure, as the app's status line.
    var transferStatus: String {
        if status == "sent" {
            return "Transfer submitted: \(transactionHash ?? "—")"
        }
        guard let message else { return "Operation completed" }
        return "Transfer failed: \(String(message.prefix(120)))"
    }

    /// The five-line timing block under the result.
    var timingLines: String {
        func value(_ name: String) -> String {
            guard let milliseconds = timings[name] else { return "—" }
            return "\(milliseconds) ms"
        }
        return """
        Compile: \(value("compile"))
        Proving: \(value("proving"))
        Signature: \(value("signature"))
        Submit: \(value("submit"))
        Total: \(value("total"))
        """
    }

    /// The balance line, in the token's smallest unit.
    var balanceLine: String {
        if status == "ok" {
            return "Token balance: \(balance ?? "—") smallest units"
        }
        return "Token balance unavailable: \(message ?? "native backend error")"
    }
}
