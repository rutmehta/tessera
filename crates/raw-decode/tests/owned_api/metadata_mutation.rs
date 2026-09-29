use raw_decode::capture::OwnedCapturedCfa;
fn mutate(x: &OwnedCapturedCfa) {
    x.metadata().make.clear();
}
fn main() {}
