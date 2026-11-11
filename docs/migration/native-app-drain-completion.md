# Native App Drain Completion & HubLifetime Architecture — 2026-11-11

## Scope and Provenance

This document records the delivery and architectural integration of Stage 5 (App Drain Lifetime & Drain Token) into the primary Windows native client (`rust/native-client`).

The implementation integrates:
- `rust/crates/zabor-native/src/hub_lifetime.rs`: The canonical `HubLifetime` coordinator providing bounded job admission, cancellation, socket shutdown synchronization, and sealed drain completion.
- `scripts/native/verify-hub-lifetime.py`: Independent loopback verification harness running in an isolated cargo environment with pinned dependency subset enforcement.
- Surgical integration in `audio_preferences.rs` (`run_local_checked`), `hub.rs` (`HubLifetime` re-export and worker delegation), `shell/session_io.rs` (`hub_io` field and coordinated drain finish), `shell.rs` (redirecting connection establishment through `session_io.hub_io`), and `tests/audio_preferences_local.rs` (checked save error and persistence validation).

## Architecture & Integration Contracts

### 1. HubLifetime Concurrency & Drain Authority
- **Bounded Concurrency**: Enforces `MAX_JOBS = 256` via atomic active job accounting. Any job spawned past saturation or after drain sealing is immediately rejected with `HubError::Busy`.
- **Sealed Drain**: Once `finish()` is initiated, the coordinator seals further job admissions, signals the atomic socket cancellation token, and asynchronously awaits all in-flight workers. Any worker panic is captured and transformed into an explicit drain failure.
- **Socket Stop Synchronization**: `socket_stop()` returns a clone of the cancellation token tied directly to the lifetime instance, ensuring that worker loops terminate cooperatively before socket close.

### 2. SessionIo Integration Seam
- `SessionIo` holds an `Arc<HubLifetime>` in `pub(super) hub_io`, instantiated as `HubLifetime::default()`.
- During application teardown, logout, or account switch, `SessionIo::finish()` executes a coordinated drain:
  ```rust
  if self.hub_io.finish().await.is_err() {
      self.failed.store(true, Ordering::Release);
  }
  if self.chat_io.finish().await.is_err() {
      self.failed.store(true, Ordering::Release);
  }
  if self.file_io.finish().await.is_err() {
      self.failed.store(true, Ordering::Release);
  }
  ```
- If any hub task, chat task, or file transfer fails to drain cleanly, the failure flag is latched with `Ordering::Release`, surfaced directly to the app drain coordinator and update host.

### 3. Checked Audio Preferences Save (`run_local_checked`)
- Standard `run_local` asynchronously saves dirty settings and emits UI toasts on failure. For clean application exit, logout, and updater handoff, `run_local_checked` provides synchronous verification:
  ```rust
  pub async fn run_local_checked(
      self: Arc<Self>,
      profile: Option<Arc<Profile>>,
      stop: CancellationToken,
      failed: Arc<dyn Fn() + Send + Sync>,
  ) -> Result<(), ProfileError>
  ```
- Catches I/O errors, storage lock contention, or missing snapshot data, returning an explicit `Err(ProfileError::Storage)` to prevent silent settings loss on exit.

## Verification & Test Evidence

The integration is verified across multiple independent suites:
1. **Loopback Hub Lifetime Suite** (`scripts/native/verify-hub-lifetime.py`):
   - 8/8 passing tests in isolated harness:
     - `hub::lifetime::tests::cancelled_drain_completes_without_leaking_handles`
     - `hub::lifetime::tests::original_panic_surfaces_as_explicit_drain_failure`
     - `hub::lifetime::tests::bounded_admission_rejects_past_saturation`
     - `hub::lifetime::tests::dropped_connect_cancels_worker_loop`
     - `hub::lifetime::tests::live_loopback_dispatches_bidirectional_records`
     - `hub::lifetime::tests::lifetime_shutdown_aborts_active_socket`
     - Plus companion avatar & protocol tests verifying lock/dependency isolation.
2. **Audio Preferences Checked Save** (`tests/audio_preferences_local.rs`):
   - `checked_final_save_returns_failure_instead_of_only_emitting_a_toast`: proves storage failure returns explicit error and triggers failure callback.
   - `checked_final_save_observes_latest_original_settings_and_preserves_untouched_bytes`: proves byte preservation and correct dirty synchronization.
3. **HiddenStore Durability Invariants** (`tests/achievement_store_durability_tests.rs`):
   - Confirms SQLite committed state authority, crash recovery checkpoints, and intent isolation remain 100% green without regressions.
