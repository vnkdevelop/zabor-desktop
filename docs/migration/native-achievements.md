# Native achievements and RU/EN presence: scoped source handoff

Base: `3c873d45ac326f2a50fc68bd1e3477dd48403a34`, continuing the primary
`5d4a540e2132465ac1501627aafb415d1e39a57e` profile subset. This is a source/module
milestone, not an app-ready, physical, visual or whole-client acceptance claim.

## Owned files and reference

- `rust/crates/zabor-native/src/achievements.rs` and `src/achievements/tests.rs`.
- `rust/crates/zabor-native/src/presence.rs` and `src/presence/tests.rs`.
- `rust/crates/zabor-native/ui/achievements.slint`.
- This document. Shared Cargo manifests/lock, lib/auth/shell/avatar/main/UI wiring,
  reference TypeScript, locales, assets and release pins are unchanged.

Reference: `App.tsx:2526-2539,4972-5050,5143-5238`,
`services/signalr.ts:1462-1505`, `achievements.ts`, `utils/presence.ts:3-42` and
`docs/migration/native-profile-actions.md` in primary. Primary Graphify was
consulted before source reads; there was no initial child graph.

## Achievement API and acceptance contract

`AchievementsView::begin(intents, original_ticket, owner_id, target)` returns an
immutable `AchievementQuery` carrying owner, own/peer target, checked monotonic
view generation and the **original** Session ticket. It clears data to `None`
for loading. Repeated pending requests for the same owner/target are rejected as
`Busy`; selecting a different view replaces the acceptance identity.

Primary must register and call `load(hub, intents, shutdown, &query)` on its
existing Session worker, through its `SessionWork`/`run_session!` mechanism. It
must not create an unobserved second Session task or substitute a fresh ticket
after completion. The module invokes only:

- own: `GetMyAchievements`, arguments `[]`;
- peer: `GetUserAchievements`, arguments `[original_user_id]`.

Timeout is the reference safeInvoke default, 20 seconds. The pinned operation
survives Leave and observes Logout/shutdown. There is no retry, `ViewProfile`,
achievement grant, notification registration, friend prerequisite or voice exit.

At dispatch primary must still validate current authenticated owner, connection
and selected view. At UI receipt it must validate shutdown, owner and selected
modal/view, then call `view.accept(intents, &original_query, result)`. Acceptance
is Session-linearized and rejects replaced/closed views, duplicate replies and
Logout. `close()` must run on actual achievements close and owner/reset changes;
do not close it just because the underlying profile closes on open. Opening
achievements must close only profile and retain its member-list context.

Errors are typed, not successful unlocks: accepted failures retain the reference
empty/default presentation and an explicit `view.error()`. `data()` is `None`
while loading. `AchievementUnlocked` remains a separate primary-owned event.

### Wire limits and definitions

`parse_result(&Value)` requires a JSON **string** containing an object. Empty
string/null are `Missing`; direct object/array, malformed/nonobject JSON and
invalid fields are rejected. Whole-object parsing enforces serde's nesting limit
even inside unknown fields. Envelope and hidden definitions accept Pascal/camel
names. Missing/null envelope fields default to empty stats/arrays and 10 hidden.
Literal `totalHiddenCount: 0` is preserved.

- Inner JSON: 256 KiB maximum; the original Hub still owns its outer record cap.
- Each unlocked/visited input array: 2048 entries; IDs/stat keys: 256 bytes,
  nonempty and no control characters.
- Stats: 128 entries, finite nonnegative values no greater than JS MAX_SAFE_INTEGER.
- Hidden count/definitions/catalog: 256; each title/description: 4096 bytes;
  icon: 64 bytes; positive finite maximum no greater than MAX_SAFE_INTEGER.
- ID arrays and identical repeated hidden definitions deduplicate in original
  order. Duplicate relevant JSON/map keys, conflicting hidden definitions and
  hidden IDs colliding with standard IDs fail closed.

These are explicit malformed/resource hardening boundaries, not a claim to
emulate the renderer's permissive behavior on arbitrary malformed data.

`STANDARD` retains all 15 original IDs/icons/stat keys/maxima/categories/units.
Titles/descriptions and other labels resolve through primary's unchanged RU/EN
`Strings` catalog. `rows(language, |key| strings.text(key), own_catalog)` retains
hidden-unlocked first, unlocked standard first, raw progress/category stable
ordering, forced full progress for already unlocked rows, and minute-to-hour
flooring in the displayed text **and** bar. Hidden definitions always render as
hidden regardless of optional wire category. Unknown-hidden placeholders appear
only for own view; peer view never consults the own durable catalog.

Primary maps `AchievementRow` to Slint `AchievementItem`. Summary parameters are
`data.unlocked_ids.len()` and `STANDARD.len() + data.total_hidden_count`; use
`Strings::format("achievements.summary", ...)`, not locally granted IDs.

## Explicit primary-owned hidden persistence boundary

There is no filesystem write or implicit global hidden/i18n cache in fetch or
view acceptance. Peer responses cannot produce a persistence plan.

`HiddenCatalog::empty(owner)` / `from_durable(owner, json)` represent a bounded,
owner-scoped catalog. The JSON map preserves the reference storage identifier
`zabor_unlocked_hidden_achievements_v1`; that identifier is not a filesystem path
and is not authorization to read Electron profile storage.

After successful own-view acceptance, `catalog.prepare(&view)` can prepare
`HiddenPersistence`. Primary may prepare a native owner-scoped durable resource
using `owner()` / `prepared_json()`, but must perform final durable acceptance
through `plan.commit_if_current(intents, &view, primary_durable_accept)`. The
callback must be a short, synchronous final acceptance/atomic replacement of an
already prepared owner-bound resource, not async work or reentrant calls to
`CriticalIntents`. Its Session lock linearizes against Logout. Replaced/closed
views, peer views and revoked owners cannot execute the callback. A failed
durable callback returns its error and does not publish an updated catalog;
only a successful callback returns the replacement `HiddenCatalog`.

**Remaining gate:** wire this to primary's native durable preparation and
Session acceptance infrastructure, including cleanup/rollback of prepared files.
An in-memory acceptance test is not proof of a real persistent native store.
Clear/hydrate catalog at owner changes; never cross-share it with peer views.

## Slint presentation contract

`AchievementsPanel` exposes items/loading/own-view/peer-name/summary/language/
reduced-motion, translation callback, `close-requested`, and `focus-close()`.
It is intentionally not imported into `app.slint` in this child branch.

The component retains the existing dark surface, primary `#c81e70`, muted
`#949ba4`, typography/icon conventions, existing close SVG, hidden/unlocked
badges, scrollable rows, progress bars, loading/empty states and summary. It
adds no font/image assets or generated imagery. Main must size the panel within
the current window/modal safe area rather than assuming 620px always fits.

Close uses the existing `IconButton` pointer/Return/Space/accessibility contract
and visible keyboard focus. Escape bubbles to panel close. Main must focus close
when opening, manage modal focus trapping and restore focus to the originating
view when closing. Rows expose a combined accessible label including full
non-elided title/description, hidden/unlocked state and progress. Width animation
is 500ms normally and zero in reduced-motion mode.

**Remaining gates:** shared modal integration, accessibility-tree/screen-reader
inspection, exact font/emoji/blur/raster/motion parity and physical focus/input.
The synthetic test neither renders a screenshot nor exercises a real OS window.

## Presence: exact pure rules and honest local-time boundary

`format_last_online(ServerLastOnline, now: LocalInstant) -> PresenceText` consumes
a **trusted resolved local-time input**, not a reconstructed timestamp. Missing
and invalid/redacted server timestamps remain `Missing`/`Invalid` and render
offline. No `showLastOnline` permission inference or fallback to cached activity
is introduced.

Primary's OS/date boundary must parse the original server timestamp and resolve
each instant's current local calendar date and local midnight epoch, including
the applicable DST offset. `LocalDate` supports valid Gregorian years 1..9999;
`LocalInstant` validates JS epoch range and a bounded local-day offset. Do not
supply UTC days or one fixed timezone offset for both dates. Conversion outside
this typed boundary must fail honestly, not guess.

Rules match the reference order: diff under 60000ms (including future skew) is
justNow; floored minutes under 60 use minutes; rounded local-midnight day diff
at/below zero uses floored hours; previous local day is yesterday; otherwise a
localized long-month date, including year only when local year changed. The
midnight rounding retains the reference behavior across 23h/25h DST days.

`PresenceText::key(PresenceLanguage)` explicitly selects RU one/few/many or EN
one/other. `render(language, |key| strings.text(key))` interpolates unchanged
source locale templates. Pure month/date output matches RU and default EN
(en-US) reference Intl formatting for the supported calendar range, including
RU `г.` / EN comma when year is shown. Own/peer online dots remain primary's
existing connection/isOnline state; this module only formats last-online text.

**Remaining gate:** actual native ISO/local-time/clock binding and its OS/DST
verification. Typed synthetic instants are not physical timezone proof.

## Checked local subset

Temporary isolated Cargo harness under approved `Temp/opencode` references only
these new modules plus original Hub/protocol/CriticalIntents. Exact dependency
versions were checked against the unchanged root Cargo.lock; no network package
or live/production data sourcing was used. Rust library compilation uses
`forbid(unsafe_code)`. Slint-generated code is checked in a separate synthetic
integration target with the existing native `deny(unsafe_code)` policy; no new
unsafe block or blanket Clippy suppression is added to native code.

- 40 Rust tests passed: 24 new achievement/presence tests plus 16 original
  supporting Hub/protocol/CriticalIntents tests.
- One isolated Slint synthetic test passed for RU/EN own/peer/loading/model
  changes and pointer/Return/Space/Escape close (16 callbacks), using an in-memory
  MinimalSoftwareWindow, not an OS window.
- Own/peer exact loopback RPC arguments; server error/null/malformed/oversize;
  duplicate identity/keys; held-response Leave, Logout, shutdown, timeout and new
  selected view; accepted error state and durable own/peer/failure/stale/Logout.
- Standard definitions checked against unchanged achievements.ts and both
  catalogs. Exact RU/EN progress, thresholds, plural groups, local midnight/DST,
  all month names and changed-year cases checked. Date strings additionally
  compared with local Node Intl; no files or live timestamps were sourced.
- Scoped rustfmt check and isolated `cargo clippy --offline --all-targets --
  -D warnings` passed after final code edits, including the Slint integration
  target. All harness registry dependencies matched exact root-lock versions.
- Child-only `graphify update .` reached its 120s clustering timeout; completed
  `graphify update . --no-cluster` updated the raw AST graph: 14168 nodes / 32877
  edges. The child lacks primary's ignored graph-scope configuration, so these
  counts are not comparable to primary's narrower map. No primary graph was
  changed and no HTML/semantic/clustered-map acceptance is claimed. Nine zero-node
  files are reported by Graphify; docs/Slint semantic extraction remains pending.
  Read-only graph diagnosis reports 1951 dangling endpoints, one self-loop and
  49 directed same-endpoint collapse risks. This raw navigation map is incomplete
  and is not code-quality or whole-client proof.
- Temporary harness, fixtures, lock and targets are cleanup-only artifacts, not
  committed deliverables; they are removed after checks.

No full neural/media/UI/raster matrix, physical OS resources, production backend,
release pins, source import, push, merge or publication is part of this subset.

## Knowledge-only follow-up: completed audit, incomplete global resolution

This section supersedes the historical raw-Graphify status above, not the
verified functional source milestone. All four Rust files and achievements.slint
remain byte-identical to `01ed463e6e222599e579ab9014aa71b9376527f6`. No tests or
source compilation were repeated, and no primary graph/source was written.

The installed library's supported deterministic NetworkX Louvain fallback
completed: 539 communities, 2.768s partition time and 14.962s total community
analysis. The earlier 120s update-pipeline timeout did not measure its exact slow
phase; it was not evidence that clustering alone could not complete.

Child outputs, all relative to this worktree:

- `graphify-out/GRAPH_REPORT.md`: full scope/provenance/diagnostic report and
  library topology report, with explicit incomplete-map flags.
- `graphify-out/graph.json`: original wide raw 14168-node / 32877-record map,
  preserved byte-identical; SHA-256
  `2af01ff6385fbced608120dbb68b56b0216d6aecf670bd96f9859807b099e46a`.
- `graphify-out/graph.clustered.json`: the same nodes and every original ordered
  edge record, annotated with communities and explicit directed/multigraph
  schema. No edge is removed, remapped or promoted in confidence.
- `graphify-out/native-achievements-audit-01ed463/`: source-byte/baseline
  verification, raw diagnostics, per-edge dangling/self-loop/parallel provenance
  ledgers, and numeric community/cohesion analysis.
- `graphify-out/native-achievements-authored/`: separately named six-authored-file
  code/Slint/doc knowledge map, report, explicit semantic resource, source hashes,
  diagnostics and scope-exclusion ledger. It is not a replacement or shrink of
  either root or primary's narrower map. Lexical external-reference nodes are
  explicitly not resolved external implementations; documented primary mapping
  remains a required binding, not an invented runtime call.

The full root retains 1951 dangling records: 1938 originate in baseline files
whose repository content matches `3c873d45ac326f2a50fc68bd1e3477dd48403a34`, and
13 originate in the authored Rust files. This is verified origin/source
attribution, not a claim to have reconstructed an identical pre-authored whole
graph. All 13 authored entries are lexical `imports_from` records: Rust
`use` extraction emits bare final-segment IDs without resolving/creating those
module/type endpoints (`graphify/extractors/rust.py:323-331`). They are parser
representation gaps, not evidence that the previously verified Rust imports fail.

The one self-loop is baseline vendor `wexchange/__init__.py:30`,
`from . import c_export`, collapsed to the package-file node; it is not a recursive
code call. All 49 directed same-pair groups are baseline relation variants; none
is authored. The annotated multigraph preserves every variant. All records remain
in the maps and audit ledgers; excluding dangling endpoints or collapsing
parallel connectivity happens only inside the documented partition projection.

Root projection is 30926 valid directed records / 30727 unweighted undirected
topology edges, including all original nodes. Numeric cohesion scores refer to
that algorithmic topology, not a silently repaired raw graph. The root's 32057
EXTRACTED / 820 INFERRED tags are retained; global inferred-edge correctness was
not fully semantically audited. Raw code/Markdown headings do not establish full
Slint/doc semantic coverage; the separately scoped resource supplies explicit
authored declarations/contracts without making that global claim.

`healthy_graph=false` remains authoritative for root. Full module/symbol
resolution repair requires language-aware treatment of external references and
provenance-preserving multigraph construction beyond this authorized child-only
operation. Unrelated source/extractor changes, force/shrink overrides, primary
map replacement, HTML/model/API/network operations and physical/full-client
acceptance were not performed. Temporary local graph scripts are removed after
the final graph verification.

## Checked primary library integration

Source01ed463 was imported as05034dd; primary library module wiring is saved as
`de2347dcb5c1a93bdfa6ee2099dcffe327ace416`. Both pure modules now compile as part
of zabor-native with unchanged Cargo/dependency/reference/locale pins. Graph-only
doc65e02ad was imported as36fe295 after the source check; Rust/Slint stayed exact.

Primary group `sh_1bc0be6a1001tiQL8xrgtQ8p2r` passed24newtests (16achievements/
8presence), fmt, both native-media all-targets Clippy modes, normal native build
and Graphify. Eleven files were frozen during the group; immutable verification
retains all their hashes. Save guard separately retains ten code/pin hashes to
permit later documented doc-only updates without changing the checked code.
Evidence: `migration-artifacts/native-data/achievements-presence-integration-2026-11-08/`.
Main graph7521nodes/19103edges/311communities;7priorzero-nodewarnings/HTML5000limit.
Normal executable SHA-256:
`f00cf622850daf01b96eadc3059138645a70da0d303a84e143848bc1aa1f4e71`.
Temporary runner removed; no primary library verification job remains.

This proves module integration only. Registered primary Session worker commands,
actual profile/modal/navigation/focus binding of AchievementsPanel, owner-bound
durable hidden preparation/atomic acceptance and ISO/local-time/clock OS binding
remain mandatory. The child synthetic Slint test is not an integrated app test.
Physical/raster/motion/screen-reader/full-client/release acceptance stays false.
