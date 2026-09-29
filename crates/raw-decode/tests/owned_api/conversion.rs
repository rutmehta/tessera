use raw_decode::capture::{DecodedCapturedCfa, OwnedCapturedCfa};
fn forge(x: DecodedCapturedCfa) -> OwnedCapturedCfa {
    x.into()
}
fn main() {}
