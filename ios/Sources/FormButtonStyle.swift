import SwiftUI

/// Filled, full-width buttons, as on the Android form.
///
/// The macOS default bordered style draws its chrome in a near-black grey that
/// disappears against the form's `#101018` background — worse while the two
/// actions are still disabled, which is the app's first second. So both get an
/// explicit fill: accent for the send action, an outlined panel for the
/// balance check, and a dimmed-but-legible disabled state.
struct FormButtonStyle: ButtonStyle {
    private static let accent = Color(red: 0x8C / 255, green: 0x66 / 255, blue: 0xFF / 255)

    var fill: Color
    var stroke: Color
    var label: Color
    var height: CGFloat

    static let primary = FormButtonStyle(
        fill: accent,
        stroke: .clear,
        label: .white,
        height: 44
    )

    static let secondary = FormButtonStyle(
        fill: .white.opacity(0.07),
        stroke: accent.opacity(0.55),
        label: .white,
        height: 36
    )

    func makeBody(configuration: Configuration) -> some View {
        ButtonBody(style: self, configuration: configuration)
    }

    /// A nested view, because only a view can read `isEnabled` — a
    /// `ButtonStyle` cannot, and the disabled state is exactly what has to
    /// stay visible here.
    private struct ButtonBody: View {
        let style: FormButtonStyle
        let configuration: Configuration

        @Environment(\.isEnabled) private var isEnabled

        var body: some View {
            configuration.label
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(style.label)
                .frame(maxWidth: .infinity, minHeight: style.height)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(style.fill)
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .strokeBorder(style.stroke)
                        )
                )
                .opacity(isEnabled ? (configuration.isPressed ? 0.75 : 1) : 0.45)
        }
    }
}
