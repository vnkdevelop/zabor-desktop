# Tract compatibility patch

Upstream: `tract-core` 0.21.4 from crates.io, MIT OR Apache-2.0.
Canonical archive SHA-256: `98ef3e1f2d94e88d007811e78a3dcc9e5734bfd841849416fab09a407488cb7d`.

The source is the verified upstream archive, with one localized change in
`src/model/typed.rs`: `SpecialOps::add_const` assigns a deterministic unique
name when a different constant already has the requested name. Identical
tensor deduplication, tensor values, wiring and computation are unchanged.
This fixes the DeepFilterNet3 decoder's `/convt3/Conv.bias` optimizer collision
without disabling graph integrity assertions or changing the model.

The Tract family stays at 0.21.4 because the pinned DeepFilterNet implementation
requires its `symbol_table` API and ndarray 0.15 types. Tract 0.21.6 changes
the symbol API, and later releases also change ndarray types. The upstream
license files and comments are preserved. The three `.rej` files are also
present in the verified upstream crate archive; they are not local failed patches.

The patch is not an upstream release. Native waveform comparisons and a
constant-name collision regression test cover its use in ZABOR. Build and
runtime dependencies remain recorded in `rust/Cargo.lock`.
