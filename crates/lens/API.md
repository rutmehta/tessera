# tessera-lens API — M2-09

Standalone crate, no image-core/pipeline dependency. Public items re-exported from crate root except `opcodes` (separate worker). Coordinates: `Point = [f64;2]`, x right/y down, image corners (-1,-1)/(1,1), each axis independently normalized. No geometry method clamps output.

## Geometry and profile integration

- `BrownConrady { k1,k2,k3,p1,p2,cx,cy: f64 }`: `distort(Point)->Point` ideal→observed, `undistort(Point)->Option<Point>` Newton inverse. All-zero Default is identity.
- `CalibrationSample { focal,aperture,distance, distortion: BrownConrady, distortion_scale, radial_odd:[f64;2], coordinate_scale:[f64;2], ca_red:[f64;3],ca_blue:[f64;3],vignette:[f64;3] }`.
- **Use `CalibrationSample::distort`, not only its Brown coefficients**, to preserve Lensfun PTLens/poly3 constant/odd-radius terms and LCP focal coordinate metrics. It is an inverse-resampling source lookup: corrected destination coordinate → distorted input coordinate.
- `coordinate_scale` maps public centered coordinates into profile metric before distortion. LCP FocalLengthX/Y map to 0.5/focal; absent values default to 0.5. Optical center remains public-coordinate `cx/cy`. Lensfun defaults to unit metric; caller must adapt for physical aspect/crop geometry (see limitations).
- CA observed channel radius/green radius = `c[0] + c[1]*r² + c[2]*r⁴`.
- Vignette **illumination**, not gain: `1 + v[0]*r² + v[1]*r⁴ + v[2]*r⁶`; correction is reciprocal, with caller safety/noise caps.
- `Profile { maker, model, camera: Option<CameraIdentity>, samples }`; `sample(focal_mm,aperture_f_number,distance_m)->Option<CalibrationSample>` inverse-distance interpolation in range-normalized focal/aperture/diopter space, clamps query to sampled range, exact samples preserved. Camera identity has `maker` and `model`; old JSON without this field remains readable.
- `ProfileDatabase::from_lensfun(&str)` / `from_lcp(&str)` → `Result<ProfileDatabase>`, `profiles: Vec<Profile>`, `find(maker,model)->Option<&Profile>` searches camera-independent profiles. `find_for_camera(camera_maker,camera_model,lens_maker,lens_model)` additionally permits matching camera-specific profiles. Camera identity is punctuation/case normalized but not edit-distance matched, avoiding interchange of R5/R6-style model names. Lens model uses fuzzy edit distance; empty lens maker means unavailable. Camera-specific profiles win equal lens scores over generic profiles; equal-ranked remaining candidates return None instead of arbitrary selection. LCP Make/Model describe the camera, LensMake describes the lens and is not inferred from camera make.
- `save_user_profile(path,&Profile)` / `load_user_profile(path)` validated JSON roundtrip; does not create parent directories.

## Image calibration

`GrayImage::new(width,height,Vec<f64>)`, `RgbImage::new(width,height,Vec<[f64;3]>)` validate dimensions/finite values. Pixels must be linear-light. Gray exposes width/height/pixels getters.

- `detect_lines(&GrayImage, absolute_gradient_threshold, min_pixels)->Vec<LineSegment>`; segment has `start,end,points,strength`. Gradient-orientation connected regions retain observed edge point clouds, not just straight endpoints.
- `estimate_k1(&[Vec<Point>],[min,max])->Option<Estimate<f64>>`: observed curved traces of genuinely straight scene edges; fits Brown k1 via inverse-map TLS straightness. Requires ≥2 traces with ≥5 points and nondegenerate curvature evidence.
- `estimate_ca(&RgbImage,max_fractional_shift)->Option<Estimate<ChromaticAberration>>`: red/blue radial scale + r² edge correlation against green; result `.value.red/.blue` are [scale,k1,0].
- `estimate_vignette(&GrayImage)->Option<Estimate<[f64;3]>>`: robust smooth-pixel radial illumination regression.
- `Estimate<T> { value, residual, confidence }`; confidence is a heuristic quality indicator, not calibrated probability. None means insufficient/degenerate evidence.

## Upright

- `estimate_upright(&[LineSegment],UprightMode)->Option<UprightResult>`; Off, Level, Vertical, Full, Auto.
- `guided_upright(&[Guide])->Option<UprightResult>`; 2–4 guides with start/end and `GuideAxis::{Horizontal,Vertical}`. At least two in an orientation are needed to constrain its vanishing point.
- `UprightResult { homography: Homography, confidence, inliers }`.
- `Homography([[f64;3];3])`, `map(Point)->Option<Point>`, `inverse()->Option<Homography>`, `IDENTITY`. Upright homography maps observed→rectified; invert for source lookup. Compose with lens map before one final resample.
- Deterministic pair-consensus vanishing-point RANSAC supports infinity; rejects horizons crossing the source rectangle. Full corrects both axes, Vertical one axis, Level uses robust median roll. Auto uses available axis families. No crop or image resampling in this crate.

## Current limitations

Lensfun supports poly3, poly5, PTLens, Brown, linear TCA and PA vignette. Other models fail explicitly (not silently identity); no bundled database. Source Lensfun normalization depends on crop/aspect, not encoded automatically. Independent distortion, TCA and vignette grids are interpolated onto the union of measured focal lengths crossed with measured vignette aperture/distance pairs (or 4/10 when absent). This preserves each measured component at its original capture coordinates, rather than dropping distortion/TCA measurements absent from the vignette grid. Intermediate samples use the documented inverse-distance interpolation, not Lensfun's native interpolation algorithm. LCP supports rectilinear RDF Description attributes/element properties and camera Make/Model restrictions, rejects fisheye, and has no PSF or complete Adobe convention coverage. One camera/lens identity per LCP file is supported, not heterogeneous multi-camera collections. Lensfun mount compatibility and crop-factor matching remain unimplemented. LCP CA/vignette model-specific centers/focal metrics are not separately represented. JSON user profiles are this crate's schema, not LCP export.

Detector is LSD-style gradient region growing with TLS rejection, not the complete a-contrario LSD algorithm. Synthetic tests cover algorithms; no real-photo accuracy claim. Line k1 cannot distinguish actual curved objects, CA assumes shared edge structure, vignette cannot distinguish smooth scene illumination from optical falloff. Upright assumes predominantly horizontal/vertical families in current orientation, caps RANSAC candidate pairs at first 128 lines, and provides projective—not metric/camera-calibrated—rectification. No learned calibration, softness PSF, volume correction, defringe, constrain-crop or content-aware fill.
