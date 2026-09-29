use raw_decode::capture::OwnedCapturedCfa;
fn mutate(x: &OwnedCapturedCfa) {
    let _ = x.samples();
}
fn main() {}
