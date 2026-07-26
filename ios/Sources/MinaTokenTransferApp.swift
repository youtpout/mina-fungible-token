import SwiftUI

/// One SwiftUI screen over the Rust prover, for macOS and iOS alike: the
/// backend is `shared/native` built for the host, linked as a static archive.
@main
struct MinaTokenTransferApp: App {
    var body: some Scene {
        WindowGroup("Mina token transfer") {
            // A minimum size is a window hint on macOS but a hard constraint on
            // the view itself on iOS, where 520 pt is wider than an iPhone 13
            // mini's 375 — every control then hangs off the screen.
            #if os(macOS)
                TransferView().frame(minWidth: 520, minHeight: 620)
            #else
                TransferView()
            #endif
        }
        #if os(macOS)
            .defaultSize(width: 620, height: 860)
        #endif
    }
}
