// Writes N small JPEGs with EXIF capture times shot in bursts, for trying the app when
// fixtures/raw has not been fetched yet, and for exercising group review.
//
// Each burst is its own "scene" (a seeded block mosaic, so near-duplicate hashing keeps bursts
// apart); frames in a burst differ only by a small shift, brightness and grain. Grain varies,
// so file sizes (the default best-frame score) differ within a burst. Deterministic.
//
//   swift Support/make-sample-folder.swift <output-folder> [count=60] [--defects]
//
// --defects (assisted culling, M3-11): frame n (1-based) with n % 5 == 3 is heavily blurred
// (missed focus) and frame n with n % 7 == 4 has a blown-out white top third (clipped highlights),
// so the real quality analysis has frames to flag. Without the flag the output is unchanged.
import AppKit
import CoreImage
import ImageIO
import UniformTypeIdentifiers

let defects = CommandLine.arguments.contains("--defects")
let args = CommandLine.arguments.filter { $0 != "--defects" }
guard args.count >= 2 else { print("usage: make-sample-folder.swift <folder> [count] [--defects]"); exit(1) }
let out = URL(fileURLWithPath: args[1], isDirectory: true)
let count = args.count > 2 ? Int(args[2]) ?? 60 : 60
var blurred = 0, blown = 0
try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

struct Rng {
    var state: UInt64
    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }
    mutating func unit() -> Double { Double(next() % 10_000) / 10_000 }
}

let fmt = DateFormatter()
fmt.locale = Locale(identifier: "en_US_POSIX")
fmt.dateFormat = "yyyy:MM:dd HH:mm:ss"
var t = Date(timeIntervalSince1970: 1_750_000_000)
var burst = 0
var group = 0
var frame = 0
var rng = Rng(state: 0x5A11_7E5)
let w = 1200, h = 800, cols = 6, rows = 4
var blocks: [Double] = []
var hue: CGFloat = 0
for i in 0..<count {
    if burst == 0 {
        burst = 1 + Int(rng.next() % 5)
        t += 20 + Double(rng.next() % 70)
        group += 1
        frame = 0
        blocks = (0..<(cols * rows)).map { _ in 0.15 + 0.75 * rng.unit() }
        hue = CGFloat(rng.unit())
    } else {
        t += 1   // one second apart: EXIF has whole seconds, the burst gap is 2 s
    }
    burst -= 1
    frame += 1
    let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    let shift = CGFloat(frame) * 6
    let gain = 0.94 + 0.04 * CGFloat(frame % 3)
    let bw = CGFloat(w) / CGFloat(cols), bh = CGFloat(h) / CGFloat(rows)
    for r in 0..<rows {
        for c in 0..<cols {
            let v = CGFloat(blocks[r * cols + c]) * gain
            ctx.setFillColor(NSColor(hue: hue, saturation: 0.45, brightness: v, alpha: 1).cgColor)
            ctx.fill(CGRect(x: CGFloat(c) * bw + shift, y: CGFloat(r) * bh, width: bw + 1, height: bh + 1))
        }
    }
    ctx.setFillColor(NSColor.white.withAlphaComponent(0.85).cgColor)
    ctx.fillEllipse(in: CGRect(x: 420 + shift * 3, y: 380, width: 160, height: 160))
    // Grain: more on some frames, so the "largest file" best-frame score differs.
    let grain = Int(rng.next() % 4)
    if grain > 0 {
        for _ in 0..<(grain * 9000) {
            let x = CGFloat(rng.next() % UInt64(w)), y = CGFloat(rng.next() % UInt64(h))
            ctx.setFillColor(CGColor(gray: CGFloat(rng.unit()), alpha: 0.35))
            ctx.fill(CGRect(x: x, y: y, width: 2, height: 2))
        }
    }
    if defects, (i + 1) % 7 == 4 {
        ctx.setFillColor(CGColor(gray: 1, alpha: 1))
        ctx.fill(CGRect(x: 0, y: CGFloat(h) * 2 / 3, width: CGFloat(w), height: CGFloat(h) / 3))
        blown += 1
    }
    var img = ctx.makeImage()!
    if defects, (i + 1) % 5 == 3 {
        let ci = CIImage(cgImage: img).clampedToExtent().applyingGaussianBlur(sigma: 14).cropped(to: CGRect(x: 0, y: 0, width: w, height: h))
        img = CIContext().createCGImage(ci, from: ci.extent)!
        blurred += 1
    }
    let url = out.appendingPathComponent(String(format: "SAMPLE_%04d.jpg", i + 1))
    let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil)!
    let props: [CFString: Any] = [
        kCGImageDestinationLossyCompressionQuality: 0.8,
        kCGImagePropertyExifDictionary: [kCGImagePropertyExifDateTimeOriginal: fmt.string(from: t),
                                         kCGImagePropertyExifLensModel: group % 2 == 0 ? "Sim 35mm F1.4" : "Sim 85mm F1.8"],
        // Two simulated bodies (alternating bursts) give the filter bar's camera facet values.
        kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFMake: "Tessera",
                                         kCGImagePropertyTIFFModel: group % 3 == 0 ? "Sim B" : "Sim A"],
    ]
    CGImageDestinationAddImage(dest, img, props as CFDictionary)
    CGImageDestinationFinalize(dest)
}
print("Wrote \(count) JPEGs in \(group) bursts to \(out.path)"
      + (defects ? " (\(blurred) blurred, \(blown) with blown highlights)" : ""))
