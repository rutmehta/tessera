// Writes N small JPEGs with EXIF capture times shot in bursts, for trying the shell when
// fixtures/raw has not been fetched yet (the grouping stub needs realistic capture times).
//
//   swift Support/make-sample-folder.swift <output-folder> [count=60]
import AppKit
import ImageIO
import UniformTypeIdentifiers

let args = CommandLine.arguments
guard args.count >= 2 else { print("usage: make-sample-folder.swift <folder> [count]"); exit(1) }
let out = URL(fileURLWithPath: args[1], isDirectory: true)
let count = args.count > 2 ? Int(args[2]) ?? 60 : 60
try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

let fmt = DateFormatter()
fmt.dateFormat = "yyyy:MM:dd HH:mm:ss"
var t = Date(timeIntervalSince1970: 1_750_000_000)
var burst = 0
var group = 0
var rng = SystemRandomNumberGenerator()
for i in 0..<count {
    if burst == 0 { burst = Int.random(in: 1...6, using: &rng); t += Double.random(in: 5...90, using: &rng); group += 1 }
    else { t += Double.random(in: 0.3...1.2, using: &rng) }
    burst -= 1
    let w = 1800, h = 1200
    let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    let hue = CGFloat((group * 53) % 360) / 360
    let c1 = NSColor(hue: hue, saturation: 0.5, brightness: 0.8, alpha: 1).cgColor
    let c2 = NSColor(hue: hue, saturation: 0.7, brightness: 0.25, alpha: 1).cgColor
    let g = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!, colors: [c1, c2] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(g, start: CGPoint(x: 0, y: h), end: CGPoint(x: CGFloat(i % 7) * 200, y: 0), options: [])
    ctx.setFillColor(NSColor.white.withAlphaComponent(0.8).cgColor)
    ctx.fillEllipse(in: CGRect(x: 300 + (i % 5) * 180, y: 600, width: 260, height: 260))
    let img = ctx.makeImage()!
    let url = out.appendingPathComponent(String(format: "SAMPLE_%04d.jpg", i + 1))
    let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil)!
    let props: [CFString: Any] = [
        kCGImageDestinationLossyCompressionQuality: 0.8,
        kCGImagePropertyExifDictionary: [kCGImagePropertyExifDateTimeOriginal: fmt.string(from: t)],
    ]
    CGImageDestinationAddImage(dest, img, props as CFDictionary)
    CGImageDestinationFinalize(dest)
}
print("Wrote \(count) JPEGs in \(group) bursts to \(out.path)")
