# Classical technical quality v1

`analyze(&RgbImage)` returns finite [0,1] scores. Input is display RGB8, not
linear scene-referred RAW. Downsample with a triangle filter to a longest edge
of 1024 pixels, preserving aspect ratio and never upscaling. All measurements,
including clipping, refer to this preview. Resampling can remove isolated clipped
pixels; these are not sensor clipping estimates. Empty images are errors; tiny
images without a 3x3 interior have zero sharpness, anisotropy and noise.

Formulas (C = channel/255, Y = .2126 R + .7152 G + .0722 B):

- Sharpness: L = Y(left)+Y(right)+Y(up)+Y(down)-4Y(center).
  V = max(0, mean(L²)-mean(L)²), score = V/(V+.01).
- Motion-blur proxy: gx=(Y(right)-Y(left))/2, gy=(Y(down)-Y(up))/2.
  A=sum(gx²), B=sum(gy²), C=sum(gx gy).
  score = sqrt((A-B)²+4C²)/(A+B), clamped to [0,1], or zero with no gradients.
  This is the structure-tensor eigenvalue anisotropy, not a calibrated probability
  of blur. Sharp stripes also score high. Orientation-independent.
- Exposure per channel: mean(C). Shadow/highlight clipping fractions count
  samples exactly equal to 0/255. These are not aesthetic exposure ratings.
- Noise: flat pixels have a 3x3 luminance range <= .1. At these pixels use
  residual L/sqrt(20), normalizing the Laplacian kernel energy. Estimate sigma
  as 1.4826 median(abs(residual-median(residual))); score=clamp(sigma/.1,0,1).
  No flat pixels yields zero, meaning no estimate/evidence rather than a guarantee
  of noiselessness. Texture, JPEG compression and downsampling affect this proxy.
- Ranking aggregate: sharpness * (1-.25 motion_blur) * (1-.5 noise) * (1-clipping),
  where clipping is the sum of the six channel clipping fractions divided by 3.
  Clamped to [0,1]. Heuristic constants are versioned, not learned/calibrated.

`analyze_and_store(index,id,image)` writes 13 image-level signals with model
`classical-quality-v1`: sharpness, motion_blur, noise, exposure_{r,g,b},
shadow_clipping_{r,g,b}, highlight_clipping_{r,g,b}, quality. It does not alter
selection.
Quality writes use individual score upserts, not a multi-row transaction: a
database error may leave a partial update. Callers should serialize analysis per
image and retry the whole operation on failure. The aggregate is written last.

`QualityScorer::from_index(index,ids)` creates a Send+Sync snapshot
for `CullSession::set_scorer`; missing values score zero. Recreate snapshots after
rescoring. Without an explicit scorer, cull reads live persisted signals itself.

Synthetic tests cover blur, clipping, independent channels, noise, direction,
small/empty/large previews, SQLite roundtrips, and an undoable culling operation.
