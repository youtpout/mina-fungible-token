import SwiftUI

extension View {
    /// A field holding an address, a key, an amount or a URL — none of which
    /// wants autocorrection, and none of which survives an autocapitalised
    /// first letter. A `B62q…` address silently becomes invalid, which on iOS is
    /// the default behaviour and on Android is not: the layout sets
    /// `textNoSuggestions` there.
    ///
    /// `keyboard` is ignored on macOS, which has one keyboard.
    func plainInput(_ keyboard: PlainInputKeyboard = .default) -> some View {
        let plain = autocorrectionDisabled()
        #if os(iOS)
            return plain
                .textInputAutocapitalization(.never)
                .keyboardType(keyboard.uiKeyboardType)
        #else
            return plain
        #endif
    }

    /// Wraps onto as many lines as the text needs instead of being truncated to
    /// one. Long node error messages and transaction hashes both land here.
    func wrapping() -> some View {
        frame(maxWidth: .infinity, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// The keyboards the form asks for, named without importing UIKit into the
/// views — the macOS build has no `UIKeyboardType`.
enum PlainInputKeyboard {
    case `default`
    case decimalPad
    case URL

    #if os(iOS)
        var uiKeyboardType: UIKeyboardType {
            switch self {
            case .default: return .default
            case .decimalPad: return .decimalPad
            case .URL: return .URL
            }
        }
    #endif
}
