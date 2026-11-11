# Native startup integration

Scope: `native-startup-completion`, prerequisite
`3037e5c207a6c28c38a1c87879e9a9144446daba`. This is the startup owner, strict main
dispatch, trusted held-package reader, and local readiness contract. It does not
approve publisher trust, complete-native acceptance, licensing, baseline
installation, or physical/two-device acceptance. `update_host.rs`, the shell's
original FileIo/ChatIo/SessionIo owners, media/RC/import/UI, manifests, lockfiles,
and resource/dependency/helper pins are not integration targets here.

## What main now does

- Exact `--help` and developer-only preview/snapshot/interaction routes require
  no live installation trust and never gain a production profile admission.
- Empty arguments and `--app` are deployed startup. Trust must be independently
  provisioned; no missing-root initialization or self-hashed trust fallback exists.
- Only the single argument `--native-update-health` is a health startup. Mixing
  app, preview, locale, profile, or other arguments with it is rejected.
- Only explicit `--native-establish-baseline --bundle SIGNED.json --archive
  PACKAGE.zip` requests baseline creation. It still verifies the independently
  pinned certified helper, signed manifest and acceptance, and protected empty
  root. No keys, acceptance receipts, certificates, approvals or signing are minted.

Required nonsecret provisioned configuration, never logged by startup:

| Variable | Meaning |
| --- | --- |
| `ZABOR_NATIVE_INSTALL_ROOT` | Absolute already-protected installation root |
| `ZABOR_NATIVE_UPDATE_HELPER` | Absolute approved host-capable helper path |
| `ZABOR_NATIVE_UPDATE_HELPER_SHA256` | Independently approved helper SHA-256 |
| `ZABOR_NATIVE_MANIFEST_KEY_SEC1` | Lowercase hex SEC1 public verification key |
| `ZABOR_NATIVE_ACCEPTANCE_KEY_SEC1` | Lowercase hex SEC1 acceptance public key |
| `ZABOR_NATIVE_PUBLISHER_CERTIFICATE_SHA256` | Independently approved publisher pin |

The environment is a provisioning interface, not a new trust authority. Human
deployment must protect the configuration/launcher from untrusted modification.
Keep actual provisioning outside the repository; use the existing ignored `.env`
mechanism if needed. Startup never prints values or stores secret material.

## Retained trust and ownership

`StartupOwner::prepare` owns its original Tokio runtime and `UpdateHost` registry.
It admits the blocking stdin read, exact-helper pin/certification, bootstrap and
held package verification before entering the shell/profile code. It observes
the original joins on failure and after shell exit; waiter cancellation is not
treated as original work completion.

Normal startup uses `UpdateHost::bootstrap`, allowing selector recovery only before
profile acquisition, and retains the exclusive `Arc<UpdateRoot>` for the entire
shell lifetime. The actual executing image must be the verified selected version;
a byte-identical copy outside that namespace does not qualify. This is not an
untrusted launcher fallback or a new process-spawn path.

Health deliberately does **not** call `UpdateRoot::open`, `recover`, `begin`, or
`current_executable`. Its private internal reader opens the lock file read-only
without taking the host's exclusive lock. It reads the protected journal/selector,
requires `AwaitingHealth`, independently validates both retained signed receipts,
acceptance, archive/package/inventory/file hashes and publisher, and binds the
executing image to the pending selected target. The detached internal root cannot
escape through the public returned type and has no recovery side effect.

`HeldStartupPackage` retains executing-image, namespace, identity, archive, package
metadata, every package payload and the same certified-helper leases. Held identity
bytes are checked against the decrypted state entropy. Helper bytes must match
both the independent helper pin and the signed package helper inventory. These
leases survive health acknowledgment and the application lifetime without holding
mutable journal/selector handles that would obstruct the host's durable commit.

`HealthBootstrap::validate_execution` checks independently obtained root/manifest,
the original pending operation and protected challenge, and bounds the envelope
deadline by the journal deadline. `acknowledge_execution` rechecks expiry/binding
immediately before connecting. Input must be an exact frame/name followed by EOF;
interactive stdin and trailing data are rejected. The existing native helper owns
the restricted inherited handles and authenticated local named-pipe protocol.

## Exact contextual shell integration recipe (parent-owned)

The shell must be changed **after** its current media/UI/original-owner work lands.
This branch does not edit `shell.rs`. Main already protects the shell admission;
until the following callback is wired, health cannot acknowledge and finishes with
`startup-real-local-ui-readiness-not-observed` rather than certifying a window.

1. Change the existing signature, retaining its body and original owners:

   ```rust
   pub fn run(
       arguments: &[String],
       startup: Option<zabor_native::startup::ShellAdmission>,
   ) -> Result<(), Box<dyn std::error::Error>>
   ```

   The two exact main call-site changes are:

   ```rust
   StartupRoute::Preview { updater: false } => return shell::run(&arguments, None),
   ```

   and in the already-protected app/health branch:

   ```rust
   return startup.finish(shell::run(&shell_arguments, Some(startup.admission())));
   ```

2. Before passing/moving the production `profile` into existing settings workers,
   retain its `Arc` for the readiness callback. Create one original-worker readiness
   oneshot and pass its **sender** into the existing `worker`; retain the receiver
   in the local UI's one-shot readiness callback. Do not replace SessionIo load or
   any original worker with a new category-shaped future.

3. The original worker sends `Result<()>` only after its real local initialization:
   attestation setup, original SessionIo load/recovery, actual local settings, and
   local worker state construction. `Ok(None)` from a successful session load is
   a valid logged-out state; load errors, cancellation, early return and panic are
   not success. Send errors, or let the receiver observe sender loss. No login,
   hub connection, device/capture startup or production request is readiness proof.

4. After handlers/models/settings are bound, arrange an actual Slint event-loop
   dispatch (not a timer-based readiness guess). Only that UI callback invokes:

   ```rust
   let readiness = admission.local_ui_ready(profile, original_worker_receiver)?;
   ```

   Retain and await this returned `HostJob<()>`; it is not a detachable fire-and-
   forget acknowledgment. `ShellAdmission` registers the original task, waits the
   original worker's channel with cancellation/deadline handling, rereads and
   validates real profile settings, checks the default native-beta profile for
   health, and only then acknowledges the independently verified held execution.
   Errors must reach the existing UI/shell failure path and request original-owner
   cleanup. Never pass a precompleted `Ok(())` channel or a readiness boolean.

5. In health startup, hold the original worker's automatic saved-session retry
   behind a retained proceed oneshot. Send proceed only after the readiness job's
   actual success; cancellation, channel loss and errors stop the same original
   worker. Do not replace live retry/login with a preview worker, and do not open
   another profile. Readiness is local initialization; this project performed no
   production request or physical startup test.

6. The parent must retain the exact original readiness waiter alongside its real
   startup/drain owners and observe it before its runtime is dropped. The outer
   `StartupOwner` retains protected-root/image leases until the shell has drained
   its original resources and returned, then observes its startup registry. Preserve
   the parent's live original AppDrain binding and its fail-closed evidence rules.

## Verification and remaining boundaries

Local synthetic tests cover strict argv, no health route smuggling, explicit-only
baseline routing, read-only pending state with the original exclusive root owner,
selector/journal preservation, held namespace/image/proof binding, wrong trust/
scope, original readiness ordering, missing/late worker failures, cancellation and
observed original blocking exits. They do not execute a production native client.

Check receipts, source/prerequisite Git blob inventory and contextual patch are
retained in the worktree's ignored
`migration-artifacts/native-data/startup-completion/`. Final check results and
checked save identifiers are appended after the original jobs complete.

Human provisioning, accepted initial installation, exact real shell readiness,
parent original-app drain, hostile OS/namespace/ACL/administrator tests, Windows
10/11 physical/two-device/long-session/full-client acceptance and legal approval
remain separate gates. No install, publication, push, main merge, legacy deletion,
user-profile access, production endpoint, machine shutdown or trust self-approval
is authorized by these receipts.
