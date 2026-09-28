"""EXP-45 scratch-only common->explicit ISO rational representation copy.

Requires exact unmodified Google reference SHA-256. Changes the auxiliary ISO APP2
representation and MPF auxiliary byte length, leaving primary/gain JPEG image data
and all other marker bytes identical. Does not modify Tessera or upstream source.
"""
import hashlib
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "denominator-explicit"
URN = b"urn:iso:std:iso:ts:21496:-1\0"
EXPECTED = {
    4: "a4564ba16f3febd8330bc751e181209bf9a8bfd1b4118d7ec004db9a724315ce",
    16: "6c8c2bad6287de3c51ea6b1a5d4ce203b0edd66d046958acd7cb7631109567a4",
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def inspect(data):
    assert data[:2] == b"\xff\xd8"
    mpf = data.find(b"MPF\0")
    assert mpf > 0 and data[mpf - 4 : mpf - 2] == b"\xff\xe2"
    tiff = mpf + 4
    assert data[tiff : tiff + 2] == b"MM"
    assert struct.unpack_from(">H", data, tiff + 2)[0] == 42
    ifd = tiff + struct.unpack_from(">I", data, tiff + 4)[0]
    count = struct.unpack_from(">H", data, ifd)[0]
    tags = {}
    for i in range(count):
        pos = ifd + 2 + i * 12
        tag, kind, n, value = struct.unpack_from(">HHII", data, pos)
        tags[tag] = (kind, n, value)
    assert tags[0xB001] == (4, 1, 2)
    assert tags[0xB002][:2] == (7, 32)
    entries = tiff + tags[0xB002][2]
    attr0, size0, offset0 = struct.unpack_from(">III", data, entries)
    attr1, size1, offset1 = struct.unpack_from(">III", data, entries + 16)
    assert attr0 == 0x00030000 and attr1 == 0 and offset0 == 0
    aux = tiff + offset1
    assert size0 == aux and aux + size1 == len(data)
    assert data[aux : aux + 2] == b"\xff\xd8" and data[-2:] == b"\xff\xd9"
    marker = aux + 2
    assert data[marker : marker + 2] == b"\xff\xe2"
    segment_len = struct.unpack_from(">H", data, marker + 2)[0]
    payload = data[marker + 4 : marker + 2 + segment_len]
    assert payload.startswith(URN)
    prefix = len(URN)
    assert payload[prefix : prefix + 4] == b"\0\0\0\0"
    flag = payload[prefix + 4]
    assert flag == 0x48, hex(flag)
    vals = struct.unpack_from(">8I", payload, prefix + 5)
    assert len(payload) == prefix + 5 + 32
    return {
        "mpf_marker": mpf, "tiff_origin": tiff, "mpentry_array": entries,
        "mpentry_second_size_offset": entries + 16 + 4,
        "primary_size": size0, "aux_offset": aux, "aux_size": size1,
        "aux_iso_segment_offset": marker, "aux_iso_segment_total_bytes": segment_len + 2,
        "aux_iso_flag": flag, "common_values": vals,
    }


for cap, expected in EXPECTED.items():
    path = ROOT / "fixtures" / f"reference-{cap}.jpg"
    original = path.read_bytes()
    assert sha(original) == expected, (cap, sha(original))
    info = inspect(original)
    denom, base, alternate, low, high, gamma, base_offset, alt_offset = info["common_values"]
    assert (denom, base, alternate, low, high, gamma, base_offset, alt_offset) == (
        1, 0, 2 if cap == 4 else 4, 0, 2 if cap == 4 else 4, 1, 0, 0
    )
    explicit = (base, denom, alternate, denom, low, denom, high, denom,
                gamma, denom, base_offset, denom, alt_offset, denom)
    payload = URN + bytes(4) + b"\x40" + struct.pack(">14I", *explicit)
    newseg = b"\xff\xe2" + struct.pack(">H", len(payload) + 2) + payload
    marker = info["aux_iso_segment_offset"]
    old_end = marker + info["aux_iso_segment_total_bytes"]
    changed_size = len(newseg) - (old_end - marker)
    assert changed_size == 24
    result = bytearray(original[:marker] + newseg + original[old_end:])
    sizefield = info["mpentry_second_size_offset"]
    before_size = bytes(result[sizefield:sizefield+4])
    assert struct.unpack(">I", before_size)[0] == info["aux_size"]
    struct.pack_into(">I", result, sizefield, info["aux_size"] + changed_size)
    assert len(result) == len(original) + changed_size
    assert result[:sizefield] == original[:sizefield]
    assert result[sizefield + 4:marker] == original[sizefield + 4:marker]
    assert result[marker + len(newseg):] == original[old_end:]
    # Reparse MPF with the new auxiliary size, then parse the new ISO form directly.
    tiff = info["tiff_origin"]
    assert struct.unpack_from(">I", result, sizefield)[0] == info["aux_size"] + 24
    assert result[info["aux_offset"]:info["aux_offset"]+2] == b"\xff\xd8"
    assert result[-2:] == b"\xff\xd9"
    assert result[marker:marker+2] == b"\xff\xe2"
    assert struct.unpack_from(">H", result, marker+2)[0] == len(payload)+2
    assert result[marker+4:marker+4+len(URN)] == URN
    assert result[marker+4+len(URN):marker+4+len(URN)+4] == bytes(4)
    assert result[marker+4+len(URN)+4] == 0x40
    assert struct.unpack_from(">14I", result, marker+4+len(URN)+5) == explicit
    output = OUT / f"reference-{cap}-explicit.jpg"
    output.write_bytes(result)
    log = {
        "source": str(path), "source_sha256": expected, "source_bytes": len(original),
        "output": str(output), "output_sha256": sha(result), "output_bytes": len(result),
        "source_mpf_primary_attr": "00030000", "output_mpf_primary_attr": "00030000",
        "source_aux_iso_flags": "48", "output_aux_iso_flags": "40",
        "source_common_values": list(info["common_values"]),
        "output_explicit_rationals": [list(explicit[i:i+2]) for i in range(0,14,2)],
        "mpf_aux_size_changed_range": [sizefield, sizefield+4],
        "mpf_aux_size_before_hex": before_size.hex(),
        "mpf_aux_size_after_hex": result[sizefield:sizefield+4].hex(),
        "aux_iso_replaced_source_range": [marker, old_end],
        "aux_iso_replacement_output_range": [marker, marker+len(newseg)],
        "aux_iso_segment_length_before": info["aux_iso_segment_total_bytes"],
        "aux_iso_segment_length_after": len(newseg),
        "aux_soi_offset_unchanged": info["aux_offset"],
        "prefix_suffix_unchanged_outside_these_ranges": True,
        "compressed_base_and_gain_data_unchanged": True,
    }
    (OUT / f"make-explicit-{cap}.json").write_text(json.dumps(log, indent=2) + "\n")
    print(cap, output, sha(result))
