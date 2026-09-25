import Foundation

/// Stub burst / near-duplicate grouping: consecutive frames (in capture order) whose capture times
/// are within `maxGap` seconds of the previous frame share a group. The real grouper (embeddings +
/// time, crates/cull) replaces this; the UI only depends on `groupID` and the group ranges.
public enum CaptureGrouper {
    public static let defaultMaxGap: TimeInterval = 2.0

    /// `dates` must be sorted ascending. Returns one range per group covering `0..<dates.count`.
    public static func groups(forSortedDates dates: [Date], maxGap: TimeInterval = defaultMaxGap) -> [Range<Int>] {
        guard !dates.isEmpty else { return [] }
        var result: [Range<Int>] = []
        var start = 0
        for i in 1..<dates.count where dates[i].timeIntervalSince(dates[i - 1]) > maxGap {
            result.append(start..<i)
            start = i
        }
        result.append(start..<dates.count)
        return result
    }
}
