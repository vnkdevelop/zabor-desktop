# RNNoise reference revision selection

Updated: 2026-11-07.

The unmodified rnnoise2-sys 0.1.2 crate was extracted from the crates.io archive
with SHA-256 `4d79d98cd6d8a69636f767af2c573b46049108a931816626df86acc6cfe0ab2c`.
Its original Rust bindings, C sources, comments and licences remain present.

The existing desktop uses `@timephy/rnnoise-wasm` 1.0.0, npm gitHead
`90ec0f0a1ba465f75df9983357ced828e3520748`. Its RNNoise submodule is
`xiph/rnnoise` at `2e3c812c62c32b3ac486c3cd4f4894e6f57d45fd`. That revision
specifies model SHA-256
`4ac81c5c0884ec4bd5907026aaae16209b7b76cd9d7f71af582094a2f98f4b43`.

The crate's later `rnn.c` concatenates convolution and three GRU states before
the output layers. The existing model/revision uses the third GRU state only.
The later `denoise.c` also changes gain-history energy compensation. Using the
correct old weights with those later algorithms failed the existing-WASM PCM
test (mean 0.04653851915744112, max 0.13982199).

The only original crate file changed is `build.rs`: its source-file selection
uses the two untouched reference-revision files below instead of the later
versions under `rnnoise/src`. The remaining compiled C sources and headers
match the reference after CRLF normalization, apart from `void` prototypes in
the public header for two parameterless functions. All 23 remaining compiled
sources/headers were compared. No model, tolerance or algorithm
was edited to obtain a passing test.

| Reference file | Git blob SHA-1 | File SHA-256 |
| --- | --- | --- |
| `reference/denoise.c` | `c2dff7ea3e513ecc6af99521edc2ac3c8675a29c` | `a7759a51af88381d135ed90f6e73919a6f9540845e437749758f6f0dc608e4c4` |
| `reference/rnn.c` | `44301edd8696e86852b820d1cab304a07da1ec2c` | `1a72e1d568c7930e89bdfd072291930775ac842bf5373e26adb6350c3fbb0454` |

Sources: <https://github.com/timephy/rnnoise-wasm/tree/90ec0f0a1ba465f75df9983357ced828e3520748>,
<https://github.com/xiph/rnnoise/tree/2e3c812c62c32b3ac486c3cd4f4894e6f57d45fd>.
Each reference file was checked against the pinned Git blob before writing.

The crate remains MIT; RNNoise remains BSD-3-Clause with its upstream notices.
This build selection is only for native reference compatibility. It does not
modify the React/WASM client, establish full-client acceptance, or repair the
diagnostic `runtime-model` feature's missing `init_rnnoise` link.
