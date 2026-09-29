use raw_decode::capture::OwnedCapturedCfa;
fn mutate(x: &OwnedCapturedCfa) {
    x.metadata().opcode_lists[0].as_mut().unwrap().push(9);
}
fn main() {}
