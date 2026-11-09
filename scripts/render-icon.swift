// Renders an SVG into the ten PNG files of a macOS .iconset (icon_16x16.png to
// icon_512x512@2x.png) with AppKit's own SVG support (macOS 14 and later), for
// scripts/bundle-macos.sh, which turns the set into AppIcon.icns with iconutil.
//
// Usage: swift scripts/render-icon.swift SVG ICONSET_DIR
import AppKit

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("render-icon: \(message)\n".utf8))
    exit(1)
}

let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    FileHandle.standardError.write(Data("usage: render-icon.swift SVG ICONSET_DIR\n".utf8))
    exit(2)
}
guard let image = NSImage(contentsOf: URL(fileURLWithPath: arguments[1])), image.isValid else {
    fail("cannot read \(arguments[1]) as an image")
}
let iconset = URL(fileURLWithPath: arguments[2], isDirectory: true)
do {
    try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
} catch {
    fail("cannot create \(iconset.path): \(error.localizedDescription)")
}

for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = points * scale
        guard
            let bitmap = NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
                bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
        else {
            fail("cannot make a \(pixels) px bitmap")
        }
        // Drawn in points on a bitmap of `scale` pixels per point, so the vectors are
        // rasterised at the final resolution rather than scaled from another size. The
        // context takes its scale from the size, so the size comes first.
        bitmap.size = NSSize(width: points, height: points)
        guard let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
            fail("cannot draw into a \(pixels) px bitmap")
        }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        context.imageInterpolation = .high
        image.draw(in: NSRect(x: 0, y: 0, width: points, height: points))
        NSGraphicsContext.restoreGraphicsState()
        let name = "icon_\(points)x\(points)\(scale == 2 ? "@2x" : "").png"
        guard let png = bitmap.representation(using: .png, properties: [:]) else {
            fail("cannot encode \(name)")
        }
        do {
            try png.write(to: iconset.appendingPathComponent(name))
        } catch {
            fail("cannot write \(name): \(error.localizedDescription)")
        }
    }
}
