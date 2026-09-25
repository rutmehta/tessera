"""Generate tiny Conv+Relu ONNX fixtures, using only Python's standard library.

The protobuf field numbers follow onnx.proto3. Fixed fixture is 1x3x64x64;
dynamic fixture permits the whole-image/tiled equivalence test.
"""
from pathlib import Path
import struct
import hashlib

ROOT = Path(__file__).resolve().parents[1] / 'crates/ml-runtime'


def varint(n):
    out = bytearray()
    while n > 127:
        out.append((n & 127) | 128)
        n >>= 7
    out.append(n)
    return bytes(out)


def integer(field, value):
    return varint(field << 3) + varint(value)


def blob(field, value):
    if isinstance(value, str):
        value = value.encode()
    return varint((field << 3) | 2) + varint(len(value)) + value


def value_info(name, dynamic, dtype=1):
    dims = [1, 3, 'height' if dynamic else 64, 'width' if dynamic else 64]
    shape = b''.join(blob(1, blob(2, d) if isinstance(d, str) else integer(1, d)) for d in dims)
    return blob(1, name) + blob(2, blob(1, integer(1, dtype) + blob(2, shape)))


def model(dynamic, fp16=False):
    weights = [((i % 7) - 3) / 64 for i in range(81)]
    dtype = 10 if fp16 else 1
    tensor = b''.join(integer(1, d) for d in [3, 3, 3, 3]) + integer(2, dtype) + blob(8, 'weights') + blob(9, struct.pack('<81e' if fp16 else '<81f', *weights))
    pads = blob(1, 'pads') + blob(8, bytes([1, 1, 1, 1])) + integer(20, 7)
    conv = blob(1, 'input') + blob(1, 'weights') + blob(2, 'conv') + blob(3, 'conv3x3') + blob(4, 'Conv') + blob(5, pads)
    relu = blob(1, 'conv') + blob(2, 'output') + blob(3, 'relu') + blob(4, 'Relu')
    graph = blob(1, conv) + blob(1, relu) + blob(2, 'tessera-test') + blob(5, tensor) + blob(11, value_info('input', dynamic, dtype)) + blob(12, value_info('output', dynamic, dtype))
    return integer(1, 8) + blob(2, 'tessera') + blob(7, graph) + blob(8, integer(2, 13))


if __name__ == '__main__':
    (ROOT / 'tests/data').mkdir(parents=True, exist_ok=True)
    for name, dynamic, fp16 in [('conv', False, False), ('conv-dynamic', True, False), ('conv-fp16', False, True)]:
        data = model(dynamic, fp16)
        (ROOT / f'tests/data/{name}.onnx').write_bytes(data)
        print(name, len(data), hashlib.sha256(data).hexdigest())
