#!/usr/bin/env swift
// B5-30 on-screen check fixtures: two 512 × 256 PNGs with IDENTICAL pixel bytes (left half a
// saturated red patch 255,0,0, right half a mid grey 128,128,128), one embedding Display P3 and one
// embedding sRGB. Usage: swift make-fixtures.swift [out-dir]   (default: current directory)
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
for (name, space) in [("b5-30-red-p3.png", CGColorSpace.displayP3), ("b5-30-red-srgb.png", CGColorSpace.sRGB)] {
    let cs = CGColorSpace(name: space)!
    let provider = CGDataProvider(data: Data(px) as CFData)!
    let image = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4, space: cs,
                        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                        provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
    let url = dir.appendingPathComponent(name)
    let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    guard CGImageDestinationFinalize(dest) else { fatalError("write \(url.path)") }
    print(url.path)
}
