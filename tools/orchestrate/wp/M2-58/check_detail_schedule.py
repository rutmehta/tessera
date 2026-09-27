"""Run the real value-only admission state and XCTest suite without an app launch."""
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[4]
source = (root / 'apps/mac/Sources/Tessera/App/DevelopTools.swift').read_text()
schedule = source[source.index('struct DetailPreviewSchedule {'):source.index('/// State and actions')]
tests = (root / 'apps/mac/Tests/TesseraCoreTests/DetailPreviewSchedulingTests.swift').read_text()
# Model the original callback's unconditional invalidate before the dedup API exists.
if 'func observeSettings(' not in schedule:
    schedule += '\nextension DetailPreviewSchedule { mutating func observeSettings(revision: UInt64) { invalidate() } }\n'
if 'engineCurrent:' not in schedule:
    schedule += '\nextension DetailPreviewSchedule { mutating func complete(_ request: UInt64, engineCurrent: Bool) -> Bool { complete(request) } }\n'
folder = pathlib.Path(__file__).resolve().parent / 'detail-check'
(folder / 'Tests').mkdir(parents=True, exist_ok=True)
(folder / 'Package.swift').write_text('''// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "DetailCheck", targets: [
    .testTarget(name: "DetailCheckTests", path: "Tests", swiftSettings: [.define("DETAIL_SCHEDULER_STANDALONE")])
])
''')
(folder / 'Tests/DetailTests.swift').write_text(schedule + tests)
subprocess.run(['swift', 'test', '--package-path', str(folder)], check=True)
