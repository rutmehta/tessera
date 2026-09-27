#!/usr/bin/env python3
"""Structural oracle, paired with the release runtime benchmarks.

This does not time syscalls: it verifies all cache-directory enumeration sites
are in startup, and the reader has no maintenance-mutex acquisition.
"""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[4]
source = (root / "crates/previews/src/disk.rs").read_text()
startup = source.split("pub fn open(", 1)[1].split("pub fn get(", 1)[0]
reader = source.split("pub fn get(", 1)[1].split("pub fn put(", 1)[0]
writer = source.split("pub fn put(", 1)[1].split("#[cfg(test)]", 1)[0]
assert source.count("fs::read_dir(") == startup.count("fs::read_dir(") == 2
assert "read_dir" not in reader + writer
assert ".lock(" not in reader
assert "const BATCH: usize = 64;" in source
assert "sync_channel(1024)" in source
print(json.dumps({
    "oracle": "source structure, not syscall timing",
    "enumeration_sites": 2,
    "enumeration_sites_outside_startup": 0,
    "reader_maintenance_mutex_acquisitions": 0,
    "eviction_batch_max_entries": 64,
    "touch_queue_max_entries": 1024,
}, indent=2))
