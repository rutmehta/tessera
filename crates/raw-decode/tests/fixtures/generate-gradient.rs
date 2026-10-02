extern crate jpeg_encoder;
use jpeg_encoder::{Encoder,ColorType,SamplingFactor};
fn main() {
 let mut bytes=Vec::new();
 let data:Vec<u8>=(0..16).flat_map(|y| (0..16).flat_map(move |x| [32+x*8,48+y*7,64+(x+y)*4])).collect();
 let mut encoder=Encoder::new(&mut bytes,100);
 encoder.set_sampling_factor(SamplingFactor::F_1_1);
 // Preserve component values. These are camera channels, despite the encoder's Ycbcr label.
 encoder.encode(&data,16,16,ColorType::Ycbcr).unwrap();
 bytes.splice(2..2, [255,238,0,14,b'A',b'd',b'o',b'b',b'e',0,100,0,0,0,0,0]);
 std::fs::write(std::env::args().nth(1).unwrap(),bytes).unwrap();
}
