# Native data finalization

Updated 2026-11-09. Scoped component handoff, not complete-native acceptance.

## Owner and preserved state

- Worktree/branch: `native-data-finalization`; base `fd8783f0d744b586051ce62fdf89a686e2e4700b`.
- Continuing session: `ses_e435e03a8ffeSY0CH5UjHkrjlM`. Inherited seven modified
  chat/store/files/jobs/SessionIo/test paths were preserved, not reset.
- Architecture consulted first in `../rust-p0-backend-bench/graphify-out`;
  coordination is that primary's `native-parallel-plan.md` and
  `native-client-handoff.md`. The original user project is not the native primary.
- Own code: `chat.rs`, `chat_store.rs`, `files.rs`, `file_jobs.rs`, their tests,
  and only `shell/session_io.rs`/`shell/session_io_tests.rs` in shell.
  `chat_transport.rs` was reviewed but remains unchanged. Other shell/UI/media/
  RC/import/updater/lib/Cargo/config/reference files are excluded.

## Checked changes and original ownership

- Download registration/final completion/failure use owner/conversation/message/
  retention-qualified expected-file updates; completion cannot overwrite a
  concurrently deleted or replaced file. Saved-path update changes only that field
  for the original available stored file, not a stale entire metadata snapshot.
- Read/ack receipts update only delivery and cannot downgrade read state or persist
  an in-memory transfer-progress overlay over the registered/completed file.
- `replace_friends` removes authorization/keys/pending downloads and detaches the
  actual original incoming partials without closing writers or performing SQLite
  IO. `finish_friend_revocations` performs original cleanup outside that admission
  boundary; it rejects a different ChatEngine even with the same owner. Late cleanup
  cannot delete or mark failed a replacement peer/message/transfer. Dropping the
  cleanup token removes only its detached original partials.
- Existing `set_friends` stays compatible and synchronous. File-root replacement
  is rejected; failed initialization does not install a partially recovered broker.
- File jobs retain per-operation cancellation and original JoinSet entries. Peer/
  message reuse does not reactivate cancelled results; save supersession cancels
  only older saves. `finish` cooperatively cancels and observes the actual original
  joins, including blocking workers, even after an earlier drain was cancelled.
- Final verified save rechecks cancellation after sync and after its acceptance
  callback; rejected saves preserve existing destinations and remove only their
  own temporary file. Recovery skips registered active partials/unrelated files;
  Windows partial handles reject sharing, and recovery fails closed on locked files.
- SessionIo retains original JoinHandles in its registry while awaiting by mutable
  reference. Cancelled/concurrent drains cannot detach them; completed/panicked joins
  are observed before pruning. Session save/clear use the original IntentTicket and
  existing profile journal. Leave preserves Session; Logout revokes it. P256/AESGCM,
  owner-isolated SQLite and 14-day retention remain unchanged.

## Original delivery verification, saved source and jobs

- Checked source commit: `bc84383f26032c51c7fd25d7907eddc638965e18`, exactly the
  seven inherited owned source/test paths; no excluded source/config change.
- Original check `sh_1bca8dc40001WcMI384E6oJqrL` completed with exit 1;
  actual receipt `msg_1bca8dc740014HfNkBVcpFNIZw` records fixture Clippy exit 101.
  Its private SessionIo test-access failure was repaired in the checked source.
- Original Graphify `sh_1bcac0d07001FVX3mCHGFEtoIK` completed with exit 0;
  actual receipt `msg_1bcac0d2e0011kipH1HJlRPBZT`. Neither original job was polled
  or duplicated; subsequent affected checks followed their delivered results.
- Original grouped job `sh_1bccf2a0a001HRTrc45JLdwEMv` automatically passed v5.
  Actual receipt `msg_1bccf2a26001EXFEFX0cSldDcp` records exit 0 for every check.
  No pending verification/file/graph job or source WIP remains in this worktree.
- Evidence: `migration-artifacts/native-data/data-finalization-2026-11-08/`;
  final `checked-v5/verification.json`, `sources-before.json`, logs and
  `save-verification.json` establish tests, unchanged fourteen hashes and scoped save.
  A read-only delivery check reconfirmed all fourteen SHA-256 values against
  `sources-before.json`; the source commit and clean worktree still match.
- 42 unique tests: chat 1/store 2/transport 3/chat-files 9/critical-intents 5/
  SessionIo 10 in v5, plus unchanged crypto 2/files 4/FileJobs 6 from v4.
  Failed v4 chat-files receipts are not counted; changed affected suites were rerun.
  Scoped fmt/diff, both fixture and normal all-targets Clippy `-D warnings`, normal
  native-media build passed with `--locked --offline -j 2`; no neural/visual matrix.
- Normal binary SHA-256:
  `85add2fccd4044d12fffd71f1efdb9d2527504c9285bbd08d234be0db9be16d1`.
  Owned target/approved-temp fixtures and all check/save runners were removed only
  after original jobs completed; primary pinned SDK inputs stayed read-only.
- Final own Graphify: 7553 nodes/19240 edges/308 communities, zero token cost.
  Seven baseline zero-node warnings/HTML5000 limit remain. Post-build diagnostic has
  no missing/dangling/self-loop/duplicate/same-endpoint edges; producer-suppressed
  edges cannot be recovered from that output. Primary graph was not changed.
  Post-save update receipt `msg_1bcdcc108003KlZYGyb6948F9n` records exit 0 and no
  topology changes for `sh_1bcdcc1030011WnSi8r0noE2VT`; no update was repeated.
- Prior v1/v2 runner failures preceded compilation; v3 private-test access failure
  had zero passed tests. V4 partial results exposed the receipt-overlay regression
  and re-add fixture rekey setup; both were repaired without assertion suppression,
  API visibility widening or crypto changes. All prior evidence is preserved.
- Own OpenCode V2 session.compact request was admitted with delivery=steer;
  admission receipt `msg_1bcddffa4001QjhkdaXolekxCs` is not completed compaction
  proof. No request was repeated. No other session/job/fixture is a cleanup or
  polling target.

## Checked saved-path P2 followup

- Source: `be6476f6a04a47141af4cd7182d35479a4b1c174`, direct parent
  `d25d6c241cadddc147e9d21604f7eab672b20d1a`; original `bc84383f` and both prior
  handoff commits are preserved. Only `chat.rs` and `tests/chat_files.rs` changed.
- Review `ses_e43154780ffeQj01ftTNBrv1EM` P2 is resolved: forgetting a saved path
  conditionally updates the exact original registered file, then synchronizes that
  download's registered and overlay metadata snapshots only after SQLite success.
  Progress, transfer ID, touched time and original owner/peer remain unchanged.
  Failed CAS/storage cannot update snapshots; replacement/deletion stay rejected.
  The existing mutable shell caller compiles unchanged; ChatStore/schema are unchanged.
- Three new deterministic regressions first failed on the unchanged predecessor
  chat source: valid completion returned Storage, failure stayed transferring, and
  forget changed replacement metadata. All three now pass; the guard test covers
  file replacement, peer replacement and message deletion for completion and failure.
- Current proof: 15 unique passed tests, 12 chat-files/2 store/1 chat, zero failed.
  The prior 42 remain predecessor evidence, not 42 newly executed tests. The 27
  unchanged-module receipts (crypto 2/files 4/jobs 6/critical-intents 5/SessionIo 10)
  are retained, not rerun; transport's 3 are historical after its chat dependency changed.
- Scoped fmt/diff and fixture/normal all-targets Clippy `-D warnings` passed with
  `--locked --offline -j 2`. No normal binary rebuild or unchanged full matrix.
  Own Graphify update/diagnostic passed: 7553 nodes/19240 edges/308 communities;
  prior zero-node/HTML-limit warnings and post-build diagnostic limitations remain.
- Evidence: `migration-artifacts/native-data/saved-path-p2-2026-11-08/`; `red-v1/`
  preserves failing regressions, `checked-v1/verification.json`, `sources-before.json`
  and `save-verification.json` record the current tests, source/Git hashes and save.
  All 14 frozen hashes survived checks/save; 12 match v5, only chat/test differ.
  Built working bytes and Git blobs have identical SHA-256:
  `chat.rs`: `8b8915663a56c96d23498434e3ef083194ed3b43170704ae7b643d4301505589`;
  `tests/chat_files.rs`: `df035c61eea578182e398151d11eebc8e4fcd8f59b0e9aff1c859ad74b3fff58`.
- Own marker-bound target, synthetic approved-temp fixtures and runner were removed.
  Primary import `sh_1bcecd5c1001Vnl2jBTT6qZbUx`, its separate target, shared shell/
  main/Cargo/assets/import paths and other owners were not polled, duplicated or changed.
  Original primary gates below remain open; whole-native/full readiness is false.

## Remaining primary gates

- Wire split chat/transport revocation and storage work outside shell acceptance
  locks, preserving the original worker/Session/peer authority; shell.rs is unchanged.
- Call and observe original FileJobs drains at Logout/app teardown before releasing
  chat/cache owners. Drop/abort/cancellation alone is not observed completion.
- Coordinate exact final filesystem-publication authorization with the original
  worker, not a freshly minted ticket or only a post-save UI check. These component
  checks do not prove arbitrary concurrent-volume/power-loss atomic publication.
- Actual app responsiveness, full-client regression, physical/long-session tests,
  authenticated helper/installer/legal/trust and release acceptance remain false.
  No production/user profiles/physical input, npm install/junction changes, legacy
  deletion, push, release, merge or shutdown. Next engineering step belongs to
  primary: integrate checked source/handoff, then close the original shell gates.
