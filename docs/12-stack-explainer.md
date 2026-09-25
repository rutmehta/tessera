# The Stack, Explained From Zero

Companion to [11-execution-plan.md](11-execution-plan.md). No prior knowledge of GPUs, Rust, or image codecs assumed.

## 1. What a photo editor actually has to do

A 45-megapixel raw file is 45 million pixels. Every slider you drag (exposure, contrast, colour) is a small maths formula that has to run on all 45 million of them, and you expect the screen to update in under a sixtieth of a second. That is the whole engineering problem: do a simple thing an enormous number of times, fast, without corrupting anything.

Two kinds of processor can do this:

- **CPU**: the general-purpose brain. An M4 has about 10 cores. Each is very smart but there are few of them.
- **GPU**: the graphics chip. Thousands of small, dumb cores that all do the same instruction on different data at once. Exactly what "the same formula on 45 million pixels" wants. Every serious photo editor pushes the pixel maths to the GPU and keeps the CPU for file handling, databases, and UI.

So the engine is two halves: **CPU code** (reading files, the catalog, Tessera's UI) and **GPU kernels** (tiny programs that run per pixel). Everything below is about choosing tools for those two halves.

## 2. The language for the CPU half: Rust

**What Rust is.** A programming language in the same performance class as C and C++ (the languages Lightroom, Photoshop, and darktable are written in) but with a compiler that refuses to build code with the most common memory bugs.

**What "memory bugs" means.** In C++ you manually manage memory. Two classic mistakes: using a piece of memory after it has been freed, and two threads writing to the same memory at once. Both compile fine and then crash or silently corrupt pixels later, often only in production. Rust's compiler tracks who owns each piece of memory and refuses to compile those mistakes. This is not a small thing for us.

**Why it matters more for AI-written code.** The plan has four to six cheap model instances writing code in parallel, with a script deciding pass or fail by running the build and the tests. The stronger the compiler, the more mistakes get caught inside that cheap loop instead of at integration time when a human (or an expensive model) has to debug them. In C++ the same loop would let far more bugs through.

**What "crates" are.** Rust's word for libraries or packages, like npm packages in JavaScript or pip packages in Python. `cargo` is the tool that builds your project and downloads crates from crates.io. A "Cargo workspace" is one repo containing many crates that build together. Our engine is one workspace with one crate per module (raw decoding, catalog index, pipeline, and so on), which maps neatly onto one work package per crate.

**The toolchain issue found in research.** "Rust 1.87" is the compiler version installed. Some crates we need require 1.88 or 1.89. It's a one-line fix (`rust-toolchain.toml` pins the version and cargo installs it) but it would have made the very first build fail.

## 3. Talking to the GPU: wgpu and Metal

**Metal** is Apple's own API for programming the GPU on Mac and iPhone. Kernels for it are written in a language called MSL. It's the fastest path on Apple hardware and has every Apple-specific feature. Its downside: it only exists on Apple platforms. Windows has DirectX 12, Linux has Vulkan, browsers have WebGPU.

**wgpu** is a Rust crate that gives you one API and one kernel language (WGSL) and translates to Metal, DirectX, Vulkan, or WebGPU underneath. Write a kernel once, run it on a Mac now and on Windows or in a browser later. The cost: a translation layer, so it can be a bit slower than hand-written Metal, and it can't reach Apple-only features.

**Why wgpu, with Metal held in reserve.** The spec wants Mac now, Windows and web later. Writing every kernel twice would double the GPU work. So the plan is wgpu by default, plus a Milestone 0 benchmark (currently running) that measures three typical kernels in wgpu versus hand-written Metal on your M4. If wgpu is within a reasonable margin, we keep it. If a specific kernel is too slow, wgpu lets us drop in raw Metal code for just that kernel.

**"Bit-identical" and why I dropped it.** The spec asked that the same edit give the exact same pixels on any GPU or CPU. Research showed Metal compiles kernels with "fast math" enabled: it may reorder floating-point operations for speed, which changes results in the last decimal place. So we cannot promise identical bits. We can promise that the GPU result is within a tolerance of a slow, exact CPU reference implementation, and that the same machine gives the same result every time. For photos, "within a tolerance no eye can see" is what matters.

**Compute versus presentation.** Two different jobs: doing the maths (compute) and getting pixels onto the screen with the right colours and HDR brightness (presentation). Presentation on a Mac involves Apple-specific things: which colour profile the monitor has, how much HDR headroom it currently allows. The plan gives presentation to Swift, Apple's language, using Apple's own APIs, and gives compute to Rust and wgpu. They share GPU memory directly (an "IOSurface") so pixels never get copied between the two.

**The 24 GB memory issue.** A 45-megapixel image held as four 32-bit floating-point numbers per pixel is about 720 MB. The spec wants to cache the expensive intermediate stages so that moving a colour slider doesn't redo the demosaic. Cache a few of those per image, plus the neighbours pre-loaded for fast browsing, and 24 GB is gone. The fix: do the maths in 32-bit precision but store cached results in 16-bit half-floats, which halves memory with no visible loss.

## 4. Tessera: Swift, AppKit and SwiftUI

**Swift** is Apple's language for apps. **AppKit** is the older, mature Mac UI framework. **SwiftUI** is the newer, more pleasant one. SwiftUI is great for panels, settings, and inspectors, but its grid view rebuilds items rather than recycling them, which stutters with tens of thousands of thumbnails. AppKit's `NSCollectionView` recycles, which is why the grid uses it. Same for the image view, which needs an Apple Metal layer for HDR, and the sliders, which need to feel instant. Pixelmator Pro, a well-regarded Mac editor, uses exactly this mix.

**Why not a web-style app** (Electron, Tauri). Fastest to build, but a web view cannot host an HDR Metal viewport cleanly, and it would never feel like a real Mac app. Since design taste is a stated goal, native it is.

**Why not a Rust-only UI toolkit.** They exist (gpui from the Zed editor, Slint, iced) and would make Windows easier later. Research found each is either immature, has no supported release, or carries a licence problem. The engine is built UI-agnostic so this can be revisited.

**How Rust and Swift talk.** A tool called UniFFI generates Swift wrappers around Rust functions. Only commands and metadata cross that bridge ("set exposure to +0.5", "here are the 200 files in this folder"). Pixels stay in shared GPU memory.

## 5. Reading and writing image files: decoders, encoders and licences

**Raw files** (CR3, ARW, NEF, RAF, DNG) are the sensor's unprocessed data in each manufacturer's proprietary container. Reading them is a reverse-engineering effort maintained by a few open-source projects. The standard one is **LibRaw**, written in C++. The Rust crate that wrapped it, `libraw-sys`, was last updated in 2015, so we write our own thin wrapper over the current LibRaw. That is one of the running work packages.

**Previews and thumbnails.** JPEG is old, universal and decoded by dedicated hardware on your Mac, which matters when scrolling a million thumbnails. JPEG XL is newer, smaller and better quality, but not hardware-decoded. So: JPEG for the thumbnail tier, JPEG XL for larger previews and export.

**What GPL is and why it was a problem.** Open-source code comes with a licence saying what you may do with it. Two families:

- **Permissive** (MIT, Apache, BSD): use it in anything, including closed-source products, just keep the copyright notice.
- **Copyleft** (GPL, AGPL): if you ship a product containing this code, your whole product must also be released under the GPL with source code. LGPL is a milder version: you may use the library in closed software if you link it in a way that lets users swap it out.

If we ever want to sell this app, or even keep the source private, GPL code inside the engine is a legal blocker. The research found that the obvious Rust crate for writing JPEG XL files is GPL. The underlying C library, libjxl, is permissive (BSD), so we bind to it directly. The `cargo-deny` tool in CI now checks every crate's licence on every build and fails if a GPL one sneaks in. LibRaw itself offers a choice of licences; we pick the permissive one (CDDL).

**Other pieces, one line each.** `rusqlite` is SQLite (a small, single-file database engine) for the catalog index. `lcms2` handles ICC colour profiles, which describe how a camera, monitor or printer sees colour. `ort` runs ONNX machine-learning models, using Apple's CoreML acceleration where it can. `zune-jpeg` decodes JPEGs fast in pure Rust.

## 6. How the models are used, in one paragraph

Fable (this session) breaks the spec into work packages with a written brief and a test command, and never writes leaf code. Opus takes the packages that need judgement: the shared engine contracts, the Mac app's design. GPT-6 Luna takes any package where the build and tests can decide pass or fail; it runs in its own copy of the repo, the script runs the tests, and on failure it retries with the error log, up to three times. GPT-6 Sol only verifies: it builds the app, drives it with computer use following a numbered acceptance script, and reports pass or fail with screenshots. Fable merges what passes.
