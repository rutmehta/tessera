// Draws the Tessera app icon and writes AppIcon.icns next to this script.
//
//   swift Support/make-icon.swift            # from apps/mac
//
// The mark: a 3 × 3 mosaic of quiet graphite tiles with one warm tessera lifted out of the
// grid, slightly turned. It is the app's identity in one picture: the photo is the loudest
// thing on screen, the interface is the frame around it. Colours are the design tokens
// (DESIGN.md §2): canvas #0A0A0A, raised #1C1C1C, accent #F0A52A. macOS icon grid: the
// squircle fills 824 of 1024 px with a 100 px margin on each side.
import AppKit

let canvas: CGFloat = 1024
let margin: CGFloat = 100
let plate = CGRect(x: margin, y: margin, width: canvas - 2 * margin, height: canvas - 2 * margin)

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(red: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255, alpha: alpha)
}

func draw(into ctx: CGContext) {
    ctx.clear(CGRect(x: 0, y: 0, width: canvas, height: canvas))

    // Plate: the macOS squircle, near-black with a hint of lift at the top.
    let plateRadius = plate.width * 0.2237
    let platePath = CGPath(roundedRect: plate, cornerWidth: plateRadius, cornerHeight: plateRadius, transform: nil)
    ctx.saveGState()
    ctx.addPath(platePath)
    ctx.clip()
    let plateGradient = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
                                   colors: [color(0x161514), color(0x0A0A0A)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(plateGradient, start: CGPoint(x: 0, y: plate.maxY), end: CGPoint(x: 0, y: plate.minY), options: [])
    ctx.restoreGState()
    // Hairline edge so the plate reads on a black desktop.
    ctx.saveGState()
    ctx.addPath(platePath)
    ctx.setStrokeColor(color(0xFFFFFF, 0.08))
    ctx.setLineWidth(3)
    ctx.strokePath()
    ctx.restoreGState()

    // The mosaic: 3 × 3 tiles, 20 px gaps, inset so the grid breathes inside the plate.
    let inset: CGFloat = 164
    let gap: CGFloat = 26
    let grid = plate.insetBy(dx: inset, dy: inset)
    let tile = (grid.width - 2 * gap) / 3
    let tileRadius = tile * 0.16
    var centre = CGRect.zero
    for row in 0..<3 {
        for col in 0..<3 {
            let rect = CGRect(x: grid.minX + CGFloat(col) * (tile + gap),
                              y: grid.minY + CGFloat(2 - row) * (tile + gap), width: tile, height: tile)
            if row == 1 && col == 1 { centre = rect; continue }
            let path = CGPath(roundedRect: rect, cornerWidth: tileRadius, cornerHeight: tileRadius, transform: nil)
            ctx.addPath(path)
            ctx.setFillColor(color(0x2C2B28))
            ctx.fillPath()
            // A soft top edge so each tile has a little depth.
            ctx.saveGState()
            ctx.addPath(path)
            ctx.clip()
            ctx.setFillColor(color(0xFFFFFF, 0.05))
            ctx.fill(CGRect(x: rect.minX, y: rect.maxY - 6, width: rect.width, height: 6))
            ctx.restoreGState()
        }
    }

    // The tessera: the centre tile, lifted (scaled 1.16), turned 7°, warm amber, with a shadow.
    let lift = centre.insetBy(dx: -centre.width * 0.08, dy: -centre.height * 0.08)
    ctx.saveGState()
    ctx.translateBy(x: lift.midX, y: lift.midY)
    ctx.rotate(by: 7 * .pi / 180)
    let local = CGRect(x: -lift.width / 2, y: -lift.height / 2, width: lift.width, height: lift.height)
    let tesseraPath = CGPath(roundedRect: local, cornerWidth: tileRadius * 1.16, cornerHeight: tileRadius * 1.16, transform: nil)
    ctx.setShadow(offset: CGSize(width: 0, height: -18), blur: 40, color: color(0x000000, 0.6))
    ctx.addPath(tesseraPath)
    ctx.setFillColor(color(0xF0A52A))
    ctx.fillPath()
    ctx.setShadow(offset: .zero, blur: 0, color: nil)
    // Warm gradient and a glossless highlight band: amber reads as a lit tile, not a button.
    ctx.addPath(tesseraPath)
    ctx.clip()
    let amber = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
                           colors: [color(0xF7B948), color(0xE8951F)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(amber, start: CGPoint(x: local.minX, y: local.maxY), end: CGPoint(x: local.maxX, y: local.minY), options: [])
    ctx.setFillColor(color(0xFFFFFF, 0.18))
    ctx.fill(CGRect(x: local.minX, y: local.maxY - 10, width: local.width, height: 10))
    ctx.restoreGState()
}

func render(size: Int) -> Data {
    let scale = CGFloat(size) / canvas
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                        space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.scaleBy(x: scale, y: scale)
    ctx.setAllowsAntialiasing(true)
    ctx.interpolationQuality = .high
    draw(into: ctx)
    let image = ctx.makeImage()!
    let rep = NSBitmapImageRep(cgImage: image)
    return rep.representation(using: .png, properties: [:])!
}

let scriptDir = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent()
let iconset = scriptDir.appendingPathComponent("AppIcon.iconset")
try? FileManager.default.removeItem(at: iconset)
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
for (name, size) in [("icon_16x16", 16), ("icon_16x16@2x", 32), ("icon_32x32", 32), ("icon_32x32@2x", 64),
                     ("icon_128x128", 128), ("icon_128x128@2x", 256), ("icon_256x256", 256), ("icon_256x256@2x", 512),
                     ("icon_512x512", 512), ("icon_512x512@2x", 1024)] {
    try render(size: size).write(to: iconset.appendingPathComponent("\(name).png"))
}
try render(size: 1024).write(to: scriptDir.appendingPathComponent("AppIcon-1024.png"))
let icns = scriptDir.appendingPathComponent("AppIcon.icns")
let task = Process()
task.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
task.arguments = ["-c", "icns", iconset.path, "-o", icns.path]
try task.run()
task.waitUntilExit()
guard task.terminationStatus == 0 else { fatalError("iconutil failed") }
try? FileManager.default.removeItem(at: iconset)
print("wrote \(icns.path)")
