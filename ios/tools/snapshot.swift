import AppKit
import SwiftUI

// Draws the transfer form to a PNG without a window, a display or a screen
// recording permission — which is how the layout gets reviewed on a headless
// machine, or from a shell that has no access to the screen.
//
// `ImageRenderer` renders a `ScrollView` as an empty rectangle, so this takes
// `TransferView.form` rather than the view itself; that is why the form sits
// outside its scroll container.
//
// AppKit-backed controls — text fields, the progress bar, the checkbox — come
// out as yellow placeholder blocks. That is the renderer, not the app: what
// this is good for is the layout around them, and whether a control is legible
// against the dark form at all.
//
//   ios/snapshot.sh [width] [height] [out.png]

// An explicit entry point rather than top-level code, which Swift only allows
// in a file called main.swift.
@main
enum Snapshot {
    static func main() {
        let arguments = CommandLine.arguments
        let width = Double(arguments.count > 1 ? arguments[1] : "620") ?? 620
        let height = Double(arguments.count > 2 ? arguments[2] : "900") ?? 900
        let path = arguments.count > 3 ? arguments[3] : "snapshot.png"
        MainActor.assumeIsolated { snapshot(width: width, height: height, to: path) }
    }
}

@MainActor
func snapshot(width: CGFloat, height: CGFloat, to path: String) {
    let renderer = ImageRenderer(
        content: TransferView().form
            .frame(width: width)
            .frame(minHeight: height, alignment: .top)
            .background(Color(red: 0x10 / 255, green: 0x10 / 255, blue: 0x18 / 255))
            .textFieldStyle(.roundedBorder)
    )
    renderer.scale = 2

    guard let image = renderer.nsImage,
          let tiff = image.tiffRepresentation,
          let bitmap = NSBitmapImageRep(data: tiff),
          let png = bitmap.representation(using: .png, properties: [:])
    else {
        print("render failed")
        return
    }

    do {
        try png.write(to: URL(fileURLWithPath: path))
        print("wrote \(path) at \(Int(width))×\(Int(height))")
    } catch {
        print("cannot write \(path): \(error.localizedDescription)")
    }
}
