use raw_decode::capture::{DecodedCapturedCfa, OwnedCapturedCfa};
fn forge(x: DecodedCapturedCfa) -> OwnedCapturedCfa {
    OwnedCapturedCfa {
        image: x.image,
        metadata: x.metadata,
        identity: x.identity,
        route: x.route,
    }
}
fn main() {}
