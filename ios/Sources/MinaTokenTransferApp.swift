import SwiftUI

/// One SwiftUI screen over the Rust prover, for macOS and iOS alike: the
/// backend is `shared/native` built for the host, linked as a static archive.
@main
struct MinaTokenTransferApp: App {
    var body: some Scene {
        WindowGroup("Mina token transfer") {
            TransferView()
                .frame(minWidth: 520, minHeight: 620)
        }
        .defaultSize(width: 620, height: 860)
    }
}
