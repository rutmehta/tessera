# EXP45 ImageIO HDR Stats option diagnostic (phase 9)

This isolated option experiment compared the documented nested `kCGComputeHDRStats=YES` request option against default decoding on the same three retained 80×16 fixtures: independent uniform, independent split-gain, and the Tessera control. It is not a producer/file change and does not alter the prior geometry phase.

## Outcome

- `uniform80`: HDR headroom 16 → 16; RGB max 15.951576232910156 → 15.951576232910156; provider, rendered-float, ICC, and properties hashes identical: True.
- `split80`: HDR headroom 8 → 8; RGB max 7.983762741088867 → 7.983762741088867; provider, rendered-float, ICC, and properties hashes identical: True.
- `A80`: HDR headroom 8 → 8; RGB max 7.983762264251709 → 7.983762264251709; provider, rendered-float, ICC, and properties hashes identical: True.

The option was confirmed false on every SDR request and true only on the enabled HDR requests. Default-process results matched the phase8 baseline for each input before the paired enabled run. Ordinary source-property plists remained byte-identical; returned ICC bytes, provider bytes, rendered RGBAf buffers, headroom, samples, and warning prefixes were also identical between default and enabled for every input. No observable decode, metadata, profile, or pixel change occurred in this bounded test.

The original ImageIO acceptance remains unresolved; this diagnostic does not change its failed case or acceptance status. No tolerances or product code were changed.

All six per-input/per-mode run directories include actual provider and pixel payloads, ICCs, source-property plists, exact JSON, warnings, commands, stdout/stderr, and direct exits. Inputs, source/header, compiled executable, runner, phase8 control manifest, compile transcript, and runner transcript are retained. See `stats-comparison-manifest.json` for exact hashes and measurements.
