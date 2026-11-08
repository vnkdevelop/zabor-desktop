# Native profile actions: reference contracts and remaining integration

Read-only reference map returned by `ses_e443d8cbeffenyW4It2sUIXmEV` on 2026-11-08.
The profile/avatar source milestone is `f264f3c`; this document does not certify
implementation, physical resources, exact UI parity or complete-native acceptance.

## Selected profile and navigation

`App.tsx:628-644` refreshes a non-own selected profile through `GetUserByUsername`.
The reference cache uses lowercase-trimmed keys, 15s TTL, 100 entries and original
pending-request identity within the Session epoch (`signalr.ts:1509-1527`).
UserJoined/Updated/Left invalidate corresponding cached users. Selection/modal/
Session changes reject late UI results. `ViewProfile` has no current renderer call
site; opening the native modal must not introduce that RPC.

Profile sources are friends/channelMembers/voiceUsers/none. `closeProfileOnly`
closes only the profile; member-list context remains underneath it. Registered
channel membership is fetched through `GetChannelMembersList(channelId)` with
6s timeout and original request-generation/selected-channel checks
(`App.tsx:2326-2345`, `signalr.ts:1646-1653`). Native `channel_users` is voice
occupancy, not a substitute for the registered-member list.

## Remaining actions

| Surface | Reference operation | Required state and result contract |
| --- | --- | --- |
| Copy login | Native clipboard, no RPC | Copy username without @; optimistic feedback; duplicate suppression; 2s reset scoped to original selection/owner |
| Presence | Existing selected User | Own dot uses connection; peer uses isOnline; display only server-provided lastOnline, never infer hidden timestamps |
| Own achievements | GetMyAchievements() | Loading data null; open achievements; close only profile |
| Peer achievements | GetUserAchievements(id) | Available for friend and non-friend; original owner/selected view must accept result |
| Invite member | CallToChannel(userId,channelId,channelName) | Member source and selected channel equals current voice channel; offline feedback3s; suppress duplicate target; sent marker30s, not rolled back |
| Remove channel access | KickFromChannel(channelId,userId) | Selected channel owner must equal current owner; confirmation before dispatch; original channel/target; optimistic member removal with guarded rollback |
| Remove friend | RemoveFriend(userId) | Friend-only; close only profile; optimistic row removal with guarded rollback; server events remain authoritative for chat/transport permissions |
| Chat | Existing ChatOpen(id) | Friend-only, including offline history; switch friends tab; preserve independent voice/call, do not Leave |
| Call | Existing StartCall(id) | Friend-only and online; single-flight; Voice scope; literal true acknowledgement; failure feedback is not proof of offline presence |
| Add friend | SendFriendRequest(username) | Non-self/non-friend; keep profile open; loading/sent state; literal true acknowledgement; received-request indicator includes current owner ID |

The invitation is not `SendChannelInvite` and has no profile-button capacity
precheck. Kick removes registered access, not just voice occupancy. Context-menu
self-exclusion is not present in the reference profile kick JSX. Server
authorization remains required regardless of button visibility.

The generic native Modal command cannot be reused unchanged for profile add-friend:
it ignores the boolean and reloads/closes the modal. Add a typed original-query
result path instead. React reads outgoing state from camel/Pascal
friendRequestsReceived; native User does not yet carry that field.

Native worker FriendRemoved/UserDeleted handling already removes allowed friend
IDs and updates ChatEngine/transport (`shell.rs` worker event handling). Optimistic
UI removal alone must not be treated as confirmed cryptographic membership.

## Achievement data and locale reuse

Achievement RPCs return a JSON string containing stats/unlockedIds/
visitedChannelIds/totalHiddenCount/unlockedHiddenDefs; accept the corresponding
PascalCase envelope names (`signalr.ts:1462-1505`). Defaults are object/empty arrays/
10 hidden entries. Hidden definitions include id/icon/maxValue/statKey and optional
unit/category plus RU/EN title/description. Own hidden definitions persist; peer
definitions do not. Unknown-hidden placeholders are own-view only. Fetching or
viewing a profile does not grant achievements; AchievementUnlocked is separate.

Reuse immutable locale keys profile.copyLogin/loginCopied/inviteToChannel/inviting/
kick/removeFriend/chat/call/userOffline, modals.addFriend.title/sent and achievements.*.
Kick description includes interpolation and React `<1>` markup; native rendering
must not expose those tags. Presence minutes/hours require explicit RU one/few/many
and EN one/other selection because native Strings interpolation does not choose
plural keys automatically.

`utils/presence.ts:3-42`: missing/invalid timestamp is offline; under60s justNow;
under60min minutes; same local day hours; previous local day yesterday; otherwise
localized date, with year only when different. Preserve server redaction.

## Acceptance plan

### Checked primary subset after64319fa

Checked and saved as `5d4a540e2132465ac1501627aafb415d1e39a57e`: original Session-owned
GetUserByUsername/cache, typed profile-specific true-only SendFriendRequest,
camel/Pascal received-request indicator, fresh selected profile retention against
stale roster snapshots and explicit user-event updates. Cache15sTTL/100FIFOentries
also enforces64MiB retained serialized user bytes and8 pending operations.
Invalidated pending replies cannot repopulate the cache; no automatic retry or
extra ViewProfile RPC is introduced. Both requests use the existing observed
primary Session worker and preserve original tickets through Leave.

UI add-friend retains the profile and loading/sent suppression. Profile chat
switches to friends without ending Voice. Offline profile calls are disabled;
native callback rechecks friendship/auth/connection/busy and selected presence.
Source/member return, clipboard feedback, exact last-online text, achievements,
channel actions/removal and call-failure3s toast remain outside this subset.

Passed affected evidence:
`migration-artifacts/native-ui/peer-profile-functional-2026-11-08/checked-v1/`.
138 unique tests, both native-media all-targets Clippy modes, fmt, normal build,
strict RU/EN pointer/keyboard/native query guards and Graphify passed. Saved8native
paths/12source+pinhashes. Normal binary SHA-256:
`2c389b1f29662c06411f598064bcf8e8fbfe20553b940bc29babc3ae0d2621df`.
Graph7195nodes/18135edges/307communities;7priorwarnings/HTML5000limit remain.
No source changes during that group; the temporary runner is removed. This is not
a physical/full-native or complete UI/release acceptance proof.

Reuse original SessionWork/run_session for non-voice commands so priority Leave
does not restart them and Logout cancels/rejects late results. Retain operation,
owner, selection/view generation and channel permission checks at dispatch and UI
acceptance. New exhaustive Command/MainResult/UiEvent cases remain primary-owned.

Extend loopback SignalR fixtures with exact argument order, true/false/null/server
error/timeout, held-response Leave, Logout and late selection/channel/view results.
Extend synthetic Slint checks with offline disabled call, profile-specific add
status, clipboard/timer injection, confirmation and member-return navigation.
No physical input, live profile or production service is needed for this group.

Exact raster/motion/accessibility, physical clipboard/focus and full-client gates
remain final acceptance tasks, not established by this reference map.
