# Native delivery prerequisite artifacts

Isolated branch `native/delivery-completion`, based on committed primary
`1f8c4f7d0c2c691e6ff1afcea3d0c40644d2012f`. Primary's Graphify report was read
first. The worktree tool initially selected old `c5d0734`; only the clean new
worktree was switched to the requested source checkpoint. No primary changes,
Cargo/dependency/model pin changes, runtime/UI edits or legacy deletion occurred.

## Actual upstream legal texts

`licenses/native/supplements.json` supplies all 24 previously missing components:
23 exact crate versions listed in `native-packaging.md`, plus the retained Silero
model. Its 42 component-text entries reference 26 physical upstream files. Files
are unchanged, with checkout EOL conversion disabled. Registry archive checksums,
published `.cargo_vcs_info.json`, immutable official revisions, Git blobs, byte
lengths and SHA-256 are recorded and checked against real cached `.crate` archives.
The renamed official gl-rs repository is explicitly recorded, not silently changed.
skia-bindings' published `dirty: true` is preserved.

The four Slint helper copyright headers are separately retained in
`licenses/native/attributions.json`, including exact header bytes, source paths,
byte ranges and full upstream source hashes. They match the checksum-verified
published crate sources. Generic upstream REUSE templates are not rewritten.
Silero's official model blob equals the existing local model at revision
`bfdc0193023f121ea5b3cc7b176dbed570a68a59`; no model was replaced.

Real Windows normal/build inventory: **559 components, 1,058 unique listed text
payloads, zero missing upstream texts**. `licenses/native/inventory-check.json`
records the checked current Cargo.lock and supplement hashes. SDK/runtime/model
caches were read-only and exact existing pins passed. Temporary inventory bundles
were removed after collection. The collector now includes the checked supplement
by default and retains GPL, trademark/privacy notices and both original agreements.
Explicit custom supplement manifests retain the previous schema and collector path.
Text collection is not legal approval, trademark permission, signing or installation
approval. Corresponding Source and the product's existing obligations still apply.

## Test-reference closure, not native runtime dependencies

`tests/native-delivery/closure.json` is derived from historical source `c8a622f`
and the exact lock blob already pinned by the independent reference owner.
It covers all 14 direct reference packages: **122 transitive Windows x64 package
placements, 12,628 files**, including normal/peer/platform-optional edges, nested
resolution, relative JS/WASM files and the existing esbuild Windows executable.
Every official npm archive is checked by original SHA-512 SRI, package identity,
version, complete file inventory and hashes. No npm install/postinstall/rebuild ran.

The required Electron 44.4.3 reference ZIP is separately pinned in
`electron-reference.json`, with official SHASUMS and all 73 runtime/notice files.
Archive SHA-256: `790a355b684d5c7cc8dc3cdd8c4cca7c4b2d054685427c7554a956879a82e70b`.
It is neither installed nor launched, and is explicitly forbidden native payload.
Both archive kinds remain necessary inputs in this worktree's ignored
`rust/target-reference-archives/`, never in primary's junction or user project.
There is no alternate checkout/node_modules fallback in these checkers.

`tailwind-sources.json.gz` preserves all 65 historical `./src/**/*.{js,jsx,ts,tsx,html}`
candidates plus original CSS/config, not just updater TSX. `capture-sources.json.gz`
retains all 69 exact primary working-byte inputs, including RU/EN, after independent
Git/EOL validation. Only these named source files were read from primary.
`capture-inputs.json` pins compiled `reference.css` and `reference.js.gz`, generated
by the original compiler options/entry in an owned temporary reference fixture.
This fixture assembly ran no installation/lifecycle scripts and was removed only
after the compiler exited. It never used the original node_modules junction.

The independent reproduction check rebuilds CSS/JS from frozen files without the
active renderer. These artifacts are prerequisites only: the primary reference
owner must integrate their admission/root/package/capture hooks. Its
`scripts/native/reference-contract.cjs`, manifest and consumers are not edited here.
No screenshots, visual review, physical tests or complete-native acceptance are claimed.

## Bounded local checks

```text
cmd.exe /c python -B scripts/native/packaging/check_supplements.py --cargo-cache C:\Users\porad\.cargo\registry\cache\index.crates.io-1949cf8c6b5b557f --inventory-cache C:\Users\porad\.local\share\opencode\worktree\b1b3f5\rust-p0-backend-bench\rust\target
cmd.exe /c python -B scripts/native/packaging/test_supplements.py -v
cmd.exe /c python -B scripts/native/packaging/test_package.py -v
cmd.exe /c python -B scripts/native/test_reference_closure.py -v
cmd.exe /c python -B scripts/native/test_electron_reference.py -v
cmd.exe /c python -B scripts/native/reference-closure.py check
cmd.exe /c python -B scripts/native/prepare-reference-assets.py check
cmd.exe /c python -B scripts/native/reproduce-reference-assets.py
```

Exact checked source/artifact hashes and completed results are in
`tests/native-delivery/verification.json`; the final scoped commit is returned in
the session handoff. No installer/build/sign/publish/real-profile operation ran.

Final affected suites passed **50 unique tests** (28 packaging, 8 supplements,
8 dependency closure, 6 Electron archive checks). Syntax and staged whitespace
checks passed; frozen CSS/JS reproduction matched exactly. Own `graphify update .`
completed: 9,849 nodes, 23,267 edges, 457 communities; exported duplicate IDs,
dangling endpoints and self-loops are zero. Fourteen inputs yielded no AST nodes,
HTML was skipped above 5,000 nodes, and documentation semantics were not refreshed.
This is an isolated code graph, not replacement of primary's curated graph.
