import XCTest
@testable import TesseraCore

/// M2-22: which ring the loupe allocates and which headroom the engine tone-maps for, from a
/// mocked screen (no display needed).
final class EDRPresentationTests: XCTestCase {
    private struct MockScreen: EDRScreen {
        var currentEDRHeadroom: Double
        var potentialEDRHeadroom: Double
    }

    private let sdrScreen = MockScreen(currentEDRHeadroom: 1, potentialEDRHeadroom: 1)
    /// A Liquid Retina XDR at mid brightness: 16× potential, 8× available now.
    private let xdr = MockScreen(currentEDRHeadroom: 8, potentialEDRHeadroom: 16)

    func testSDRScreenAlwaysKeepsTheRGBA8Path() {
        for hdr in [false, true] {
            let p = EDRPresentation.resolve(screen: sdrScreen, hdrEnabled: hdr)
            XCTAssertFalse(p.floatSurfaces)
            XCTAssertFalse(p.isEDRCapable)
            XCTAssertEqual(p.displayHeadroom, 1)
            XCTAssertEqual(p.maxStops, 0)
            XCTAssertEqual(p.effectiveHeadroom(stops: 3), 0, "0 = SDR RGBA8 contract")
            XCTAssertEqual(p.readout, "SDR display")
        }
        XCTAssertEqual(EDRPresentation.resolve(screen: nil, hdrEnabled: true), .sdr, "no window yet")
    }

    func testEDRScreenUsesFloatSurfacesOnlyWithHDROn() {
        let off = EDRPresentation.resolve(screen: xdr, hdrEnabled: false)
        XCTAssertFalse(off.floatSurfaces, "HDR off: today's SDR path")
        XCTAssertTrue(off.isEDRCapable)
        let on = EDRPresentation.resolve(screen: xdr, hdrEnabled: true)
        XCTAssertTrue(on.floatSurfaces)
        XCTAssertEqual(on.displayHeadroom, 8)
        XCTAssertEqual(on.potentialHeadroom, 16)
        XCTAssertEqual(on.maxStops, 4, "slider reaches the display's potential")
        XCTAssertEqual(on.defaultStops, 4)
        XCTAssertEqual(on.readout, "EDR 8.0× now, 16.0× max")
    }

    func testHeadroomIsCappedByTheCurrentDisplayHeadroom() {
        let on = EDRPresentation.resolve(screen: xdr, hdrEnabled: true)
        XCTAssertEqual(on.effectiveHeadroom(stops: 0), 1, "0 EV = SDR tone-mapped")
        XCTAssertEqual(on.effectiveHeadroom(stops: 2), 4)
        XCTAssertEqual(on.effectiveHeadroom(stops: 4), 8, "current 8× caps the 16× request")
        XCTAssertEqual(on.effectiveHeadroom(stops: .nan), 1)
        XCTAssertEqual(on.effectiveHeadroom(stops: -3), 1)
        // A brighter backlight leaves less headroom: 2.5× now.
        let dim = EDRPresentation.resolve(screen: MockScreen(currentEDRHeadroom: 2.5, potentialEDRHeadroom: 16),
                                          hdrEnabled: true)
        XCTAssertEqual(dim.effectiveHeadroom(stops: 4), 2.5)
        XCTAssertEqual(dim.maxStops, 4)
    }

    func testOddScreenValuesAreSanitized() {
        // EDR not engaged yet: capable, float ring, SDR white until the headroom rises.
        let idle = EDRPresentation.resolve(screen: MockScreen(currentEDRHeadroom: 1, potentialEDRHeadroom: 5),
                                           hdrEnabled: true)
        XCTAssertTrue(idle.floatSurfaces)
        XCTAssertEqual(idle.displayHeadroom, 1)
        XCTAssertEqual(idle.maxStops, 2.3, accuracy: 1e-9, "log2(5) rounded down to 0.1 EV")
        // Current above potential, NaN and sub-1 values.
        let odd = EDRPresentation.resolve(screen: MockScreen(currentEDRHeadroom: 30, potentialEDRHeadroom: 4),
                                          hdrEnabled: true)
        XCTAssertEqual(odd.displayHeadroom, 4)
        let broken = EDRPresentation.resolve(screen: MockScreen(currentEDRHeadroom: .nan, potentialEDRHeadroom: 0.5),
                                             hdrEnabled: true)
        XCTAssertEqual(broken, .sdr)
        // 1.005× potential is not EDR.
        XCTAssertFalse(EDRPresentation.resolve(screen: MockScreen(currentEDRHeadroom: 1, potentialEDRHeadroom: 1.005),
                                               hdrEnabled: true).floatSurfaces)
    }

    func testHeadroomChangesAreDebounced() {
        XCTAssertFalse(EDRPresentation.headroomChanged(8, 8.2))
        XCTAssertTrue(EDRPresentation.headroomChanged(8, 9))
        XCTAssertTrue(EDRPresentation.headroomChanged(1, 1.2))
        XCTAssertEqual(EDRScreenValues(xdr), EDRScreenValues(current: 8, potential: 16))
    }

    func testHeadroomControlPatchesTheRecipe() {
        let c = HDRControls.headroom(maxStops: 4)
        XCTAssertEqual(c.range, 0...4)
        XCTAssertEqual(DevelopController.encode(c.patch(2.5)), #"{"output":{"hdr_headroom_stops":2.5}}"#)
        XCTAssertEqual(DevelopController.encode(c.patch(9)), #"{"output":{"hdr_headroom_stops":4}}"#)
        XCTAssertEqual(HDRControls.headroom(maxStops: 0).range.upperBound, 0.1, "never an empty range")
    }
}
