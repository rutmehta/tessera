use raw_decode::capture::OwnedCapturedCfa;
fn duplicate(x: &OwnedCapturedCfa) -> OwnedCapturedCfa {
    OwnedCapturedCfa::clone(x)
}
fn main() {}
