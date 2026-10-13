import Foundation

/// A mask's alpha at thumbnail resolution (`DevelopController.maskThumbnail`), used by the dial
/// (UX-13) to tell which AI mask is under a click and to draw that mask's ring. Points are
/// mask-space fractions (the engine's normalised, uncropped, sensor-orientation frame).
public struct MaskRaster: Sendable {
    public let width: Int
    public let height: Int
    public let alpha: [UInt8]

    public init?(width: Int, height: Int, alpha: [UInt8]) {
        guard width > 0, height > 0, alpha.count == width * height else { return nil }
        self.width = width
        self.height = height
        self.alpha = alpha
    }

    /// Alpha (0…255) of the pixel under a mask-space point; 0 outside the frame.
    public func alpha(atX x: Double, y: Double) -> UInt8 {
        guard x >= 0, y >= 0, x < 1, y < 1 else { return 0 }
        let px = min(Int(x * Double(width)), width - 1), py = min(Int(y * Double(height)), height - 1)
        return alpha[py * width + px]
    }

    public func contains(x: Double, y: Double, threshold: UInt8 = 128) -> Bool {
        alpha(atX: x, y: y) >= threshold
    }

    /// Fraction of the frame the mask covers (at `threshold`).
    public func coverage(threshold: UInt8 = 128) -> Double {
        Double(alpha.lazy.filter { $0 >= threshold }.count) / Double(width * height)
    }

    /// Closed outlines of the mask at `threshold` (marching squares between pixel centres, the
    /// frame edge counted as outside), as mask-space polylines, longest first, at most `limit`.
    public func outlines(threshold: UInt8 = 128, limit: Int = 4) -> [[(x: Double, y: Double)]] {
        // Padded binary grid: index (i, j) is pixel (i - 1, j - 1); the border is empty.
        let gw = width + 2, gh = height + 2
        var grid = [Bool](repeating: false, count: gw * gh)
        for y in 0..<height {
            for x in 0..<width where alpha[y * width + x] >= threshold {
                grid[(y + 1) * gw + x + 1] = true
            }
        }
        // Segment endpoints are edge midpoints, keyed at twice the grid resolution so they are
        // integers: top (2i+1, 2j), right (2i+2, 2j+1), bottom (2i+1, 2j+2), left (2i, 2j+1).
        struct Key: Hashable { let x: Int, y: Int }
        var segments: [(Key, Key)] = []
        for j in 0..<(gh - 1) {
            for i in 0..<(gw - 1) {
                let tl = grid[j * gw + i], tr = grid[j * gw + i + 1]
                let bl = grid[(j + 1) * gw + i], br = grid[(j + 1) * gw + i + 1]
                let index = (tl ? 8 : 0) | (tr ? 4 : 0) | (br ? 2 : 0) | (bl ? 1 : 0)
                if index == 0 || index == 15 { continue }
                let t = Key(x: 2 * i + 1, y: 2 * j), r = Key(x: 2 * i + 2, y: 2 * j + 1)
                let b = Key(x: 2 * i + 1, y: 2 * j + 2), l = Key(x: 2 * i, y: 2 * j + 1)
                switch index {
                case 1, 14: segments.append((l, b))
                case 2, 13: segments.append((b, r))
                case 3, 12: segments.append((l, r))
                case 4, 11: segments.append((t, r))
                case 6, 9: segments.append((t, b))
                case 7, 8: segments.append((t, l))
                case 5: segments.append((t, r)); segments.append((b, l))   // saddles: a fixed resolution
                case 10: segments.append((t, l)); segments.append((b, r))
                default: break
                }
            }
        }
        guard !segments.isEmpty else { return [] }
        var at: [Key: [Int]] = [:]
        for (n, s) in segments.enumerated() {
            at[s.0, default: []].append(n)
            at[s.1, default: []].append(n)
        }
        // Chain segments end to end (undirected: every endpoint joins exactly two segments).
        var used = [Bool](repeating: false, count: segments.count)
        var loops: [[Key]] = []
        for start in 0..<segments.count where !used[start] {
            var loop: [Key] = [segments[start].0]
            var point = segments[start].1
            var current = start
            used[start] = true
            while true {
                loop.append(point)
                guard let next = at[point]?.first(where: { $0 != current && !used[$0] }) else { break }
                used[next] = true
                current = next
                point = segments[next].0 == point ? segments[next].1 : segments[next].0
            }
            loops.append(loop)
        }
        let scale = { (k: Key) -> (x: Double, y: Double) in
            // Half-steps of the padded grid back to pixel centres, then to fractions.
            ((Double(k.x) / 2 - 1 + 0.5) / Double(self.width), (Double(k.y) / 2 - 1 + 0.5) / Double(self.height))
        }
        return loops.sorted { $0.count > $1.count }.prefix(limit).map { $0.map(scale) }
    }
}
