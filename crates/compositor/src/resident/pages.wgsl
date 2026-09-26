// The resident page pool: up to eight slabs of fixed-size pages. A page
// holds one tile (at most 256², planar, in the tile's own extent) of a
// layer or mask at some pyramid level, in the document depth, laid out
// exactly as the engine-api Tile samples (8-bit: four samples per word,
// 16-bit: two, float: one).

struct Pool {
    depth: u32,       // 0 = 8-bit, 1 = 16-bit, 2 = float
    page_words: u32,
    inv16: f32,       // the CPU's 1/65535 (f32)
    _p: u32,
}

@group(0) @binding(0) var<storage, ACCESS> slab0: array<u32>;
@group(0) @binding(1) var<storage, ACCESS> slab1: array<u32>;
@group(0) @binding(2) var<storage, ACCESS> slab2: array<u32>;
@group(0) @binding(3) var<storage, ACCESS> slab3: array<u32>;
@group(0) @binding(4) var<storage, ACCESS> slab4: array<u32>;
@group(0) @binding(5) var<storage, ACCESS> slab5: array<u32>;
@group(0) @binding(6) var<storage, ACCESS> slab6: array<u32>;
@group(0) @binding(7) var<storage, ACCESS> slab7: array<u32>;

// ACCESS is `read` in the document shader (cacheable loads) and
// `read_write` in the mip shader.

const NONE: u32 = 0xffffffffu;

// Pages are addressed as (slab << 29) | page-in-slab; the CPU resolves the
// slab when it writes the page tables and mip jobs.
fn page_load(page: u32, word: u32) -> u32 {
    let w = (page & 0x1fffffffu) * pool.page_words + word;
    switch page >> 29u {
        case 0u: { return slab0[w]; }
        case 1u: { return slab1[w]; }
        case 2u: { return slab2[w]; }
        case 3u: { return slab3[w]; }
        case 4u: { return slab4[w]; }
        case 5u: { return slab5[w]; }
        case 6u: { return slab6[w]; }
        default: { return slab7[w]; }
    }
}

// Integer code (8/16-bit) or raw bits (float) of sample index k.
fn page_code(page: u32, k: u32) -> u32 {
    switch pool.depth {
        case 0u: { return (page_load(page, k >> 2u) >> ((k & 3u) * 8u)) & 255u; }
        case 1u: { return (page_load(page, k >> 1u) >> ((k & 1u) * 16u)) & 65535u; }
        default: { return page_load(page, k); }
    }
}
