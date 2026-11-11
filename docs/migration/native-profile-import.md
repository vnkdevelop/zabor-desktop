# Native beta: copy-import and explicit rollback

Scope: independent profile owner on base `3ba1ad8` (native RC source `2108f86`).
This is not a complete-client, real-profile, visual, physical-device or release acceptance report.

## Confirmed Electron contracts

The importer does not treat Electron history as SQLite or assume its safeStorage
ciphertext is native DPAPI ciphertext:

- `src/main/index.ts:925–984`: `session.enc` and `chat-identity.enc` use Electron
  `safeStorage.encryptString/decryptString`. `session.json` is the legacy fallback.
  The copy reader never calls the reference `load-session` handler: it would mutate
  or remove the legacy source file.
- `src/renderer/src/App.tsx:880–908`: session JSON is
  `{login,password,userId,settings}`. The native session remains
  `{username,password,userId}`, current-user DPAPI, `ZNS1`, entropy
  `zabor-native-v1`. Copied secrets are decrypted in helper memory and reprotected
  through `Profile`; no imported plaintext credential file is published.
- `src/renderer/src/chat/chatCrypto.ts`: global `{publicKey,privateKey}` P-256
  ECDH JWK. This file has **no owner binding**. A session's asserted `userId` does
  not independently prove that this JWK belongs to that account.
- `src/renderer/src/chat/chatDb.ts`: IndexedDB `zabor-chat-v1`, version 1,
  `messages`, key path `[ownerId,id]`, 14-day retention.
- `src/renderer/src/services/chatPeer.ts:541–545,1031–1079`: TOFU fingerprint
  JSON fields `kty/crv/x/y`; Local Storage prefixes `zabor_chat_key_<owner>_`,
  `zabor_chat_deletions_<owner>_`, `zabor_chat_deleted_incoming_<owner>_`.
  Tombstones are imported before history so deleted contents do not reappear.
  These source queues have no timestamps; native file-deletion retention begins
  at the explicit import time, without inventing an original deletion date.
- `src/main/index.ts:388–419`: `app-settings.json` contains `openAtLogin` and
  `minimizeToTray`. Importing the JSON does not change OS startup registration.
- `src/renderer/src/chat/types.ts`, `src/main/chatFiles.ts`: attachment metadata
  references `chat-files`. External `savedPath` is never read or preserved.

## API and trust boundary

Module: `zabor_native::profile::import`, wired by a path declaration in `profile.rs`;
no `lib.rs`, manifest, dependency, authentication, UI or shell changes are needed
to compile this independent block.

1. Obtain `ImportOwner::verified(user_id, username, public_key)` **after independent
   account authentication and explicit import consent**. Do not derive verification
   from the exported session, a manifest, or the copied identity. If no independent
   P-256 public-key proof is available, a present source identity is rejected.
   `None` is accepted only when there is no copied identity; native can subsequently
   generate a fresh identity through its existing account flow.
2. `Snapshot::copy(source, approved_scratch)` copies only `Local State`, session
   files, chat identity, app settings, IndexedDB, Local Storage and chat-files.
   It never opens the source for writing, changes its names, or removes its files.
   Windows file handles deny sharing with writers/deleters and remain held for
   the entire copy; directory pins deny deletion, not child-file writes. The source
   must be closed. Both source inventories must agree. The source root remains
   pinned until snapshot closure. All traversed entries reject symlinks/reparse points, non-files,
   ancestor escapes, UNC/device namespaces. Maximum 100,000 entries/2 GiB copy.
   The scratch directory must already exist and may not overlap the source.
3. Run `scripts/migration/native-profile-export.cjs` using a trusted Electron
   executable **on this snapshot only**, never on the source. The helper relocates
   appData/userData/sessionData/logs/crash storage to the snapshot, disables
   hardware acceleration, denies permissions, network requests and new windows,
   opens a local inert file-origin renderer, and uses a readonly IndexedDB cursor.
   It does not start the reference app or call its mutating IPC handlers.
4. Capture the helper's output in a bounded, private pipe as `Zeroizing<Vec<u8>>`
   (maximum 64 MiB), and observe successful exporter/native child completion.
   **Never redirect the output to a terminal, log or plaintext file.** The helper
   outputs credential/JWK material. Failure, unknown schema or partial output
   must not be converted into an empty successful export.
5. `PreparedImport::prepare(snapshot,parent,owner,export,now)` validates owner,
   login, key pair and independent public key, writes a journal and sibling stage,
   and builds the existing native SQLite/DPAPI/JSON contracts. Foreign-owner rows
   and keys are excluded. Expired rows are not copied. Attachments are bounded,
   SHA-256 verified and copied to safe digest filenames; transfer state is restored
   from copied availability, not from an old external path. SQLite is checkpointed
   and closed before publication. Receipt hashes cover the staged native files.
   The snapshot retains the canonical source path privately, so a destination
   parent inside the original, or a beta destination enclosing it, is rejected
   before creating any gate, journal or staging entry in that source.
6. `PreparedImport::publish(accept)` revalidates stage hashes and journals, and
   renames the sibling stage to `parent/zabor-desktop-native-beta` only if absent.
   The parent gate and profile locks exclude cooperating native readers/writers.
   Admission is checked before and after the atomic rename. A revoked post-rename
   result is journaled as an abort and quarantined while the parent gate still
   prevents normal readers; it cannot be recovered as an accepted publication.
   A rejected import preserves its data in the fixed aborted directory.
   Successful publication is reported only after journal removal succeeds.

`PreparedImport::copy(..., extract)` combines snapshot, caller-owned extraction
and stage construction. The extraction callback receives only the temporary
snapshot. The combined API explicitly closes the snapshot and reports cleanup
failure before returning a prepared transaction. With the separate APIs, call
`Snapshot::close()` after observed exporter completion and before publication;
RAII remains a fallback, not a successful cleanup acknowledgement.
The stage/journal intentionally survive failed or dropped preparations for explicit
recovery. They are not silently cleaned, promoted or accepted on next startup.

## Publication, recovery and rollback

All paths are fixed beneath the explicitly selected parent; journals never supply
filesystem paths. Profile access rejects unsafe paths. Opening the beta also holds
the parent import gate and refuses an unresolved journal or staging directory,
including if publication happened before a crash but journal cleanup did not.

| Path | Meaning |
| --- | --- |
| `zabor-desktop-native-beta` | Only target profile; existing beta is never overwritten |
| `.zabor-desktop-native-beta.import-stage` | Unpublished native conversion |
| `.zabor-desktop-native-beta.import.pending.json` | Versioned operation/token/owner journal; no credentials |
| `.zabor-desktop-native-beta.import.lock` | Persistent, nonsecret coordination lock inode |
| `.zabor-desktop-native-beta.import-aborted` | Preserved unpublished/aborted stage; no replacement of an existing quarantine |
| `.zabor-desktop-native-beta.import-rollback` | Preserved complete beta after explicit rollback |

`recover(parent, action)` is explicit, never automatic:

- `PublishVerifiedStage`: only an intact staged receipt/owner/token/hash inventory,
  no beta, no unresolved session or SQLite WAL/SHM/journal.
- `AbortUnpublishedStage`: no beta; safely quarantine a partial/intact stage.
- `ConfirmPublished`: only intact receipt hashes after a completed stage rename,
  absent stage. No normal beta access is admitted before confirmation.
- `FinishRollback`: complete the journaled rename or confirm its preserved rollback
  directory. Unknown/malformed state or sharing failures remain errors and blocked.
- `FinishAbortedPublication`: finish a journaled rejected/revoked publication;
  preserve it in aborted quarantine, never promote it to a usable beta.

`rollback(parent, owner)` is allowed only for the receipt-bound imported owner and
an unopened beta without a pending session journal. It atomically renames the
**entire** beta into rollback quarantine, preserving later native settings/history/
credentials too. It does not restore, write, remove or relaunch Electron. Returning
to Electron means using the original unchanged source. The quarantine is not
automatically restored or deleted, and cannot be overwritten by another rollback.

Receipts and journals detect corruption and incomplete transactions; they are not
a security boundary against a malicious process already running as the same user.
Windows rename is same-volume atomic namespace publication, not a proof of directory
metadata durability under abrupt power loss. Any surviving journal blocks startup.

## Main-agent wiring and remaining gates

Development CLI integration on `1f8c4f7` is now implemented by `migration.rs` and
an early `main.rs` dispatch; see [native-import-command.md](native-import-command.md).
Fresh independent account login, explicit server/action/key/bundle approval, original
intent/helper-registry retirement, authorized recovery and profile reopen are wired.
The historical checklist below records the original module handoff, not a claim
that the separate CLI remains missing. UI, real Chromium/profile and full-client
acceptance remain outside this integration.

The bounded resume retains actual helper/registry retirement receipts and offers
`MigrationOperation::execute_for_reuse` plus `MigrationPublication::install` to
transfer the reopened Profile under its original ticket without a path-only reopen.
Existing importer/recovery/rollback APIs remain unchanged; application readers still
require their own completed FileIo/ChatIo/SessionIo drains before rollback.

- Provide an explicit migration/recovery/rollback command or UI using existing
  original Session/intent ownership, without creating a normal beta first.
- Independently authenticate selected account and establish a trusted public-key
  proof (or define an explicitly approved new-identity migration path). Global
  Electron JWK cannot safely be assigned to a user merely by inspecting files.
- Own the helper process and all children, bounded pipe, cancellation and observed
  teardown in the main integration's registered native-resource owner. This module
  deliberately does not implement an unowned `Command::output`, process-tree kill,
  or pretend a cancelled wait proves Chromium cleanup. Clear `NODE_OPTIONS`,
  `NODE_PATH`, `ELECTRON_RUN_AS_NODE`; use trusted exact executable/helper paths.
- On startup, expose explicit recovery when `Profile::open` reports storage failure
  due to the import journal; do not swallow it and open a new empty profile.
- After `publish`, reopen the beta through `Profile::open`; only then install normal
  chat/auth state under the original ticket. Native launch/updater must share this
  profile gate. Rollback requires draining chat/SQLite/Profile owners first.
- Test actual Electron file-origin Local Storage/IndexedDB extraction with a fully
  synthetic, closed Electron profile and controlled runtime. Mocked helper tests
  are not evidence that Chromium recovery/origin behavior is lossless.
  The script is a one-off migration utility using an independently available
  trusted Electron runtime, not a dependency of the final native executable.
- No existing beta merge, automatic quarantine deletion, foreign identity adoption,
  plaintext credential export, source migration in place or OS startup mutation.
- Full-client, real user profile, power-loss durability, visual, physical and release
  acceptance remain outside this independent functional block.

## Verification

Targeted Rust command uses `--no-default-features --lib profile --offline --locked`
and this worktree's isolated `rust/target-profile-import`; no neural/default/media
matrix, dependency upgrade, real AppData, network or physical device tests.
Fixtures use RAII `tempfile` directories only, including Windows junction and sharing
failure tests. Helper tests use mocked Electron/IndexedDB and approved Temp fixtures;
they remove every created fixture directory in `finally`.

Verified functional results on this worktree:

- 30 profile tests passed (20 import/recovery/rollback tests and all 10 existing
  session/DPAPI tests), plus 2 unchanged P-256/AES-GCM tests and 1 unchanged native
  owner-isolation/retention test. Repeated profile runs are not double-counted.
- 6 mocked Electron-export tests passed: copied storage paths, owner filtering,
  readonly history, denied permissions/network, decrypt/session failure, unknown
  schema, cyclic-record abort, missing copied IndexedDB rejection and failed stdout
  pipe acknowledgement.
- Targeted `rustfmt --edition 2021 --check` passed for the three owned Rust files.
- Offline/locked no-default all-targets Clippy with `-D clippy::all` passed, including
  library and test type checking. No new Clippy warnings remain.
- The stricter `-D warnings` command is **not accepted**: it exposed a corrected
  needless borrow in the new module and five pre-existing no-default dead-code
  diagnostics in `mic_test.rs`. That out-of-scope file and lint policy were not
  changed. Existing Slint `viewport-y` deprecation at `ui/app.slint:453` remains.
- Early profile runs failed on the new drive-prefix validation, directory-sharing
  mask and hashing an open attachment writer. Those implementation defects were
   corrected; the earlier complete 29-test profile run passed after explicit snapshot
  cleanup was added. The initial interrupted Clippy attempt is not a passed check.

Total: 39 distinct passing tests, not full-client acceptance. No real profile,
Electron process, browser network, physical device or production service was used.
Rust fixtures are owned RAII temp directories; helper fixtures were explicitly
deleted and their absence checked. The isolated Cargo build target is cleaned
after verification, before final graph synchronization and scoped save.

The final destination/source-boundary regression passed without any source tree or
byte changes; the snapshot retains its source-directory pin until closure. Final
profile tests and all-targets Clippy/type checks passed in the fresh isolated target
`rust/target/profile-import` (ignored). Debug information alone was disabled for
that repeat build, without disabling test assertions or changing pins. Both owned
Cargo targets are cleaned after their last checks, not shared with another worktree.

The first own-worktree `graphify update .` attempted a fresh code-only graph and
timed out after AST extraction; it is not a completed graph sync. Its warning count
included generated artifacts in the then-present isolated Cargo target. The final
code-only sync completed after cleanup and source save: 13,716 nodes, 29,741 edges,
532 communities. It updated this worktree's `graphify-out/graph.json` and
`GRAPH_REPORT.md`; 8 zero-node warnings remain, and HTML was skipped because the
graph exceeds the 5,000-node visualization limit. The primary worktree graph was
only queried read-only at task entry and was never updated here.

Verified source commit: `dfb735a6fdcb49441c848c3a9b3292b142f55da6`. This final
documentation update does not change the tested Rust or helper sources. No pending
verification jobs, real-profile access, production, push, merge, release or shutdown.
