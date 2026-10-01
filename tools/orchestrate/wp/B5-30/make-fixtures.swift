#!/usr/bin/env swift
// B5-30 on-screen check fixtures: two 512 × 256 PNGs with IDENTICAL pixel bytes (left half a
// saturated red patch 255,0,0, right half a mid grey 128,128,128), one embedding Display P3 and one
// embedding sRGB. A second pair adds 1px stripes and an odd-aligned black/white edge: at
// 50% zoom, the stripes and the boundary pixel should be about 187/255, never 128/255.
// Usage: swift make-fixtures.swift [out-dir]   (default: current directory)
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let dir = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ".")
let (w, h) = (512, 256)
var px = [UInt8](repeating: 255, count: w * h * 4)
for y in 0..<h { for x in 0..<w {
    let i = (y * w + x) * 4
    let v: (UInt8, UInt8, UInt8) = x < w / 2 ? (255, 0, 0) : (128, 128, 128)
    (px[i], px[i + 1], px[i + 2]) = v
} }
var step = [UInt8](repeating: 255, count: w * h * 4)
for y in 0..<h { for x in 0..<w {
    let value: UInt8 = (y < h / 2 ? x % 2 == 0 : x < 255) ? 0 : 255
    let i = (y * w + x) * 4
    step[i] = value; step[i + 1] = value; step[i + 2] = value
} }
for (fixture, pixels) in [("red", px), ("half-step", step)] {
for (suffix, space) in [("p3", CGColorSpace.displayP3), ("srgb", CGColorSpace.sRGB)] {
    let name = "b5-30-\(fixture)-\(suffix).png"
    let cs = CGColorSpace(name: space)!
    let provider = CGDataProvider(data: Data(pixels) as CFData)!
    let image = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4, space: cs,
                        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                        provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
    let url = dir.appendingPathComponent(name)
    let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    guard CGImageDestinationFinalize(dest) else { fatalError("write \(url.path)") }
    print(url.path)
}
}
