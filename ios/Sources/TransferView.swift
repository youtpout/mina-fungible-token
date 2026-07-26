import SwiftUI

/// The Android `activity_main.xml` form, field for field, on SwiftUI.
///
/// Same order, same labels, same colours (`#101018` background, `#8C66FF`
/// accent), same two progress bars — the header one scrolls out of sight
/// during a long proving run, so it is mirrored under the send button — and
/// the same monospaced result and timing blocks. The one source of truth for
/// behaviour is `MainActivity.kt`; anything visible here should match it.
struct TransferView: View {
    @State private var privateKey = Defaults.senderPrivateKey
    @State private var receiver = Defaults.receiver
    @State private var amount = Defaults.amount
    @State private var tokenAddress = Defaults.tokenAddress
    @State private var graphqlUrl = Defaults.graphqlUrl
    @State private var fundReceiver = true

    /// The key is kept for repeat transfers rather than cleared, but it locks
    /// once used; replacing it is deliberate and starts from an empty field.
    @State private var keyLocked = false

    @State private var backendStatus = "Loading the native Rust backend…"
    @State private var loadingBackend = true
    @State private var busy = false
    @State private var ready = false
    @State private var sendStatus: String?
    @State private var result = ""
    @State private var tokenBalance = "Token balance: —"
    @State private var timings = """
        Compile: —
        Proving: —
        Signature: —
        Submit: —
        Total: —
        """

    private static let background = Color(red: 0x10 / 255, green: 0x10 / 255, blue: 0x18 / 255)
    private static let accent = Color(red: 0x8C / 255, green: 0x66 / 255, blue: 0xFF / 255)
    private static let secondaryText = Color(red: 0xB9 / 255, green: 0xB7 / 255, blue: 0xC7 / 255)
    private static let bodyText = Color(red: 0xD9 / 255, green: 0xD5 / 255, blue: 0xE8 / 255)
    private static let timingText = Color(red: 0xAF / 255, green: 0xA9 / 255, blue: 0xC8 / 255)
    private static let placeholderText = Color(red: 0x88 / 255, green: 0x85 / 255, blue: 0x96 / 255)

    var body: some View {
        ScrollView {
            form
        }
        .background(Self.background)
        .onAppear(perform: loadBackend)
    }

    /// The form itself, outside the scroll container so that `ImageRenderer`
    /// can draw it — a `ScrollView` renders empty there, which makes the
    /// snapshot tool useless unless the content can be reached on its own.
    @ViewBuilder
    var form: some View {
            VStack(alignment: .leading, spacing: 12) {
                Text("Mina token transfer")
                    .font(.system(size: 26, weight: .bold))
                    .foregroundStyle(.white)

                Text(backendStatus)
                    .font(.system(size: 14))
                    .foregroundStyle(Self.secondaryText)

                if loadingBackend || busy {
                    ProgressView().progressViewStyle(.linear).tint(Self.accent)
                }

                input("Sender private key (EK…)", text: $privateKey, secure: true)
                    .disabled(keyLocked)

                if keyLocked {
                    Button("Use a different key") {
                        privateKey = ""
                        keyLocked = false
                    }
                    .font(.system(size: 13))
                    .foregroundStyle(Self.accent)
                }

                input("Receiver address (B62…)", text: $receiver)
                input("Amount in the token's smallest unit", text: $amount, keyboard: .decimalPad)
                input("Token contract address (B62…)", text: $tokenAddress)
                input("Devnet GraphQL endpoint", text: $graphqlUrl, keyboard: .URL)

                Button("Check selected address token balance", action: checkBalance)
                    .buttonStyle(FormButtonStyle.secondary)
                    .disabled(!ready)

                Text(tokenBalance)
                    .font(.system(size: 14, design: .monospaced))
                    .foregroundStyle(Self.bodyText)
                    .textSelection(.enabled)
                    .wrapping()

                checkbox("Create the receiver token account when needed", isOn: $fundReceiver)

                Button("Prove and send", action: send)
                    .buttonStyle(FormButtonStyle.primary)
                    .disabled(!ready)
                    .padding(.top, 8)

                // The mirror of the header pair, so the state stays visible
                // while the form is scrolled down.
                if busy {
                    ProgressView().progressViewStyle(.linear).tint(Self.accent)
                }
                if let sendStatus {
                    Text(sendStatus)
                        .font(.system(size: 14))
                        .foregroundStyle(Self.secondaryText)
                        .textSelection(.enabled)
                        .wrapping()
                }

                // The response is JSON, and a JSON token has nowhere to break:
                // as a plain Text it demands its full width, the enclosing VStack
                // grows to match, and every `maxWidth: .infinity` control is
                // dragged off a phone screen with it. A horizontal ScrollView
                // takes the width it is given and scrolls the overflow instead.
                ScrollView(.horizontal, showsIndicators: false) {
                    Text(result)
                        .font(.system(size: 13, design: .monospaced))
                        .foregroundStyle(Self.bodyText)
                        .textSelection(.enabled)
                }
                .padding(.top, 8)

                Text(timings)
                    .font(.system(size: 14, design: .monospaced))
                    .foregroundStyle(Self.timingText)
                    .textSelection(.enabled)
                    .wrapping()
                    .padding(.top, 4)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(24)
    }

    /// One labelled input, styled after `values/transfer_input.xml`: no box, a
    /// `#8C66FF` underline, white 15pt text and a `#888596` placeholder. The
    /// standard rounded-border field would put a white slab on the dark form.
    ///
    /// The label above it is the one addition the phone form does not need —
    /// Android leans on `android:hint`, which vanishes as soon as a value is
    /// prefilled, and every one of these fields is prefilled.
    private func input(
        _ label: String,
        text: Binding<String>,
        secure: Bool = false,
        keyboard: PlainInputKeyboard = .default
    ) -> some View {
        let prompt = Text(label).foregroundStyle(Self.placeholderText)

        return VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.system(size: 12))
                .foregroundStyle(Self.secondaryText)

            Group {
                if secure {
                    SecureField(label, text: text, prompt: prompt)
                } else {
                    TextField(label, text: text, prompt: prompt)
                }
            }
            .textFieldStyle(.plain)
            .plainInput(keyboard)
            .font(.system(size: 15))
            .foregroundStyle(.white)
            .padding(.horizontal, 8)
            .frame(height: 40)
            .background(alignment: .bottom) {
                Rectangle()
                    .fill(Self.accent)
                    .frame(height: 2)
            }
        }
    }

    /// The `CheckBox` of the Android form. iOS has no checkbox style, so it
    /// falls back to the platform switch there.
    private func checkbox(_ label: String, isOn: Binding<Bool>) -> some View {
        let toggle = Toggle(label, isOn: isOn)
            .tint(Self.accent)
            .foregroundStyle(.white)
        #if os(macOS)
            return toggle.toggleStyle(.checkbox)
        #else
            return toggle
        #endif
    }

    private func showStatus(_ message: String, busy running: Bool) {
        backendStatus = message
        sendStatus = message
        busy = running
    }

    private func loadBackend() {
        MinaBackend.backendInfo { info in
            backendStatus = "Native Rust backend loaded"
            result = prettyJSON(info)
            loadingBackend = false
            ready = true
        }
    }

    private func checkBalance() {
        ready = false
        showStatus("Loading the selected address token balance…", busy: true)
        let request = BalanceRequest(
            address: receiver,
            tokenAddress: tokenAddress,
            graphqlUrl: graphqlUrl
        )
        MinaBackend.tokenBalance(request) { response in
            tokenBalance = TransferResponse(response).balanceLine
            showStatus("Balance check completed", busy: false)
            ready = true
        }
    }

    private func send() {
        ready = false
        showStatus("Building and proving with native Rust…", busy: true)
        result = "Preparing the transaction…"
        let request = TransferRequest(
            senderPrivateKey: privateKey,
            receiver: receiver,
            amount: amount,
            tokenAddress: tokenAddress,
            graphqlUrl: graphqlUrl,
            fundReceiver: fundReceiver
        )
        MinaBackend.transfer(request) { response in
            let parsed = TransferResponse(response)
            keyLocked = true
            result = prettyJSON(response)
            timings = parsed.timingLines
            showStatus(parsed.transferStatus, busy: false)
            ready = true
        }
    }
}
