# M2-04 schema gaps

`engine-api/src/recipe/settings.rs` is unchanged.

- **B&W treatment and eight-band B&W mix:** DevelopSettings/ColorSettings has neither a treatment switch nor B&W mix coefficients. Skip this control. Saturation -100 remains available, but is not presented as the missing B&W mix.
- **User-configurable grain seed:** Grain exposes amount, size and roughness only. Use the fixed native seed `0x5445535345524132`; skip the seed control. Equal settings and image coordinates produce deterministic grain.
- **Explicit detail enable flags:** Sharpening defaults to amount 40 and color NR to 25, although the M1 renderer bypassed both. To meet the required unchanged default goldens, each unchanged sharpening/NR substructure is independently bypassed. Once a substructure differs, its absolute amounts apply. No hidden field or schema change was introduced. This compatibility discontinuity should be resolved by an explicit process-version/enable-field decision in a later work package.

Other unsupported controls already have fields and are not schema gaps: Point Color, LUT, AI denoise, lens blur, lens distortion/Upright/manual transforms, orientation and constrain-crop, HDR/proofing and other display transforms remain explicit errors when changed. Crop aspect is UI metadata; the supplied rectangle is authoritative.
