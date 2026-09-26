# Upstream PSD fixtures

Source: https://github.com/chinedufn/psd
Pinned revision: `28357a29414f940b6272b71831b3273f57f267bd`.
The repository supplies `LICENSE-MIT` and `LICENSE-APACHE`; these fixtures are
used under the MIT option. The original MIT copyright and license are included
as `LICENSE-MIT`. No separate restrictive fixture license was present in the
source tree. Files are unmodified downloads, not synthesized examples.

Original paths:

- `tests/fixtures/green-1x1.psd`
- `tests/fixtures/green-chinese-layer-name-1x1.psd`
- `tests/fixtures/rle-3-layer-8x8.psd`
- `tests/fixtures/groups/green-1x1-one-group-inside-another.psd`
- `tests/fixtures/groups/rle-compressed-empty-channel.psd`

SHA-256:

```
7789f045e41991b8834704bd81e630cd3624f60684213fe9c07284b336dfa7ce  green-1x1.psd
097eda3d7e9f7efaf27aae7d83bc0f8618db2c576857e6e02ccd8526955ac218  green-chinese-layer-name-1x1.psd
c6eec3f2514416b332c28e64069b39138cda280df4eb6a21f9f1ecbcfb5efea2  rle-3-layer-8x8.psd
f17feefbe4b69f1a455d765bfffc309680d09fab79fdc336b3800d1beea26769  green-1x1-one-group-inside-another.psd
7c07c1d4628fbdfdb3faab015f9c19c3b163a768460a399938a47fd2363e6ea7  rle-compressed-empty-channel.psd
```

The integration test reads each fixture and re-reads its rewritten output,
comparing the entire intermediate representation, including opaque metadata.
The green fixture also independently asserts dimensions and merged RGB values.
