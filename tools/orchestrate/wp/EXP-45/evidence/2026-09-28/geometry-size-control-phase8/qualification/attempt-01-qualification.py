# Preserved malformed qualification attempt: the split check compares the rest
# of the image with one row. The fixture's first row itself is correct.
from pathlib import Path
out=Path(__file__).resolve().parents[1]
w,h=640,128
for kind in ('uniform','split'):
    b=(out/f'extracted-gain-{kind}-640x128.pgm').read_bytes()
    p=b.split(b'\n',3); assert p[:3]==[b'P5',b'640 128',b'255']
    samples=p[3]; assert len(samples)==w*h
    if kind=='split':
        assert samples[:320]==bytes(320)
        # BUG: this tail includes rows 1..127, not just the right half of row 0.
        assert samples[320:]==bytes([255])*320
