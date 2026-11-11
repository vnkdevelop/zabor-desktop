# Explicit native profile commands / Явные команды native-профиля

Development integration based on `1f8c4f7`; not release, real-profile or Chromium acceptance.

## RU: доверие и согласие до доступа к данным

`zabor_native::migration` и ранний dispatch `main.rs` выполняют отдельную операцию
без запуска shell/Slint, голосовой связи или updater. Смешивание с `--app`, preview,
неизвестными/повторяющимися параметрами отклоняется. Обычный beta-профиль до snapshot
не открывается и не создаётся. Неразрешённый import/session journal продолжает
блокировать штатный `Profile::open`; восстановление не запускается автоматически.

Перед запуском человек должен независимо подтвердить:

1. Точный `--server URL` и такой же `--approved-server URL`: HTTPS с обычной проверкой
   сертификата или HTTP только loopback. Никаких встроенных паролей/query tokens.
   Не запускайте самостоятельно собранный клиент против серверов Оператора без
   отдельного разрешения. Существующая attestation использует только окружение,
   без извлечения ключей или обхода допуска сервера.
2. Учётную запись: `ZABOR_PROFILE_USERNAME` и `ZABOR_PROFILE_PASSWORD` поступают через
   защищённое окружение запуска. Не вводите значения в argv, историю терминала,
   журналы, репозиторий или командные примеры. CLI не принимает пароль/логин в argv.
   `AuthClient::login` выполняет свежие `CheckUserExists`/`Login` на выбранном сервере;
   регистрации и доверия к exported `userId` нет. Это серверный вход: он может
   изменить presence/primary connection, поэтому source-клиент должен быть закрыт.
3. Открытый P-256 JWK выбранного аккаунта из независимого подтверждённого источника:
   `ZABOR_PROFILE_EXPECTED_PUBLIC_KEY` и `--confirmed-key`. Нельзя вычислять его из
   copied/exported identity. Private JWK отвергается. Альтернатива
   `--confirmed-no-identity` разрешает только действительно отсутствующую source
   identity и отсутствие переменной expected key; она не удаляет и не заменяет ключ.
4. Для copy-import: точные runtime/helper-файлы и **весь** deployed Electron bundle,
   включая DLL/resources. `ZABOR_PROFILE_HELPER_TRUSTED_SHA256` и
   `ZABOR_PROFILE_EXPORT_TRUSTED_SHA256` — заранее независимо одобренные SHA-256,
   не только что вычисленные self-approval. `--approved-runtime-bundle DIR` означает
   явное подтверждение provenance всего каталога runtime; runtime должен находиться
   непосредственно в нём. Код закрепляет EXE/helper, но не аттестует каждый sidecar.
5. Закрытый source, существующие отдельные source/scratch/parent каталоги, отсутствие
   работающих beta/chat/SQLite readers и точное действие `--consent`.

## Commands

All paths below are user-selected placeholders, not approved locations or trust values.
Credentials, public-key input and approved hashes are supplied separately by a secure launcher.

```text
zabor-native --import-profile SOURCE --scratch SCRATCH --parent PARENT --runtime ELECTRON_EXE --helper EXPORT_CJS --approved-runtime-bundle BUNDLE_DIR --server URL --approved-server URL --confirmed-key --consent copy-import
zabor-native --recover-profile publish-stage --parent PARENT --server URL --approved-server URL --confirmed-key --consent publish-stage
zabor-native --recover-profile confirm-published --parent PARENT --server URL --approved-server URL --confirmed-key --consent confirm-published
zabor-native --recover-profile abort-stage --parent PARENT --server URL --approved-server URL --confirmed-key --consent abort-stage
zabor-native --recover-profile finish-abort --parent PARENT --server URL --approved-server URL --confirmed-key --consent finish-abort
zabor-native --recover-profile finish-rollback --parent PARENT --server URL --approved-server URL --confirmed-key --consent finish-rollback
zabor-native --rollback-profile --parent PARENT --server URL --approved-server URL --confirmed-key --consent rollback
```

## EN: original ownership and failure handling

The library's `MigrationOperation::new(command, original_intents, original_ticket)`
registers exactly one helper entry before snapshot/process acquisition. `authenticate`
uses the supplied approved server and existing account API; `ImportOwner::verified`
is only constructed after successful independent login. Consent/server/key inputs
are immutable in that operation. No new ticket is issued on late completion.

Retain that original operation in the caller's registered blocking owner, with its
`MigrationCancellation`. `execute`, helper drain and Drop are blocking and must not
run on a UI thread. CLI owns this operation on its main thread and runs networking
on a two-worker Tokio runtime. Ctrl+C, original logout, auth connection retirement
or explicit cancellation revoke the same token and helper entry. Dropping a waiter
is not resource retirement; retain and join the original native owner. CLI explicitly
closes the operation and joins its own monitor/control tasks.

Copy-import uses `PreparedImport::copy` with `ExportRequest::copied_profile` only.
`finish` observes the original helper root/job/pipe/reader/handles; the same registry
is sealed/drained inside extraction, including a blocking unwind guard, before
snapshot closure and publication. Output remains a bounded zeroizing private pipe;
it is never redirected to a log, console or plaintext export file. No helper
`test-source` feature is enabled in the native manifest or normal feature tree.

Publication checks original account/intent/connection admission before and after
rename. Accepted import/recovery reopens through the normal profile gate and commits
the freshly authenticated credentials via existing session admission, not an old
exported password. CLI does not install chat state or launch the app afterward.
For normal use, launch separately after successful operation completion.

Recovery requires fresh explicit consent/login. `recover_authorized` checks journal
owner for every action; accepting actions additionally decrypt and check the saved
account/login and independently supplied key under the import gate without opening
a normal beta. It rechecks admission before stage rename and after acceptance;
revoked published data goes to the existing aborted quarantine. Malformed, foreign,
unavailable-DPAPI, incomplete or sharing-failed recovery stays blocked. Original
unauthenticated `recover` remains a low-level storage API, not CLI admission.

The key/no-identity choice is enforced when adopting an identity during copy-import
or accepting recovery. Abort/rollback never adopt an identity and use fresh
receipt/journal-bound account approval; their key choice does not delete or replace
the preserved identity.

Rollback drains the operation's helper registry first and requires exclusive profile
gates; an open normal beta/SQLite owner must be closed by its own application owner,
not forcibly killed. The complete beta is renamed to rollback quarantine. Original
Electron data is never written, removed, restored, relaunched or merged. Quarantines
and journals are not automatically deleted. Same-user malicious processes and
power-loss directory durability are not attested by these receipts or approvals.

## Synthetic verification boundary

`scripts/migration/native-import-mock.rs` is a standalone test-only Rust executable,
not a native Cargo target or deployed runtime. Compile it to the explicitly owned
approved Temp fixture directory as `mock.exe`; tests require that exact fixture path
through `ZABOR_TEST_TEMP`/`ZABOR_IMPORT_MOCK_RUNTIME`. Synthetic helper bytes and the
compiled mock are pinned only for these local tests. The mock reads marked snapshots,
never Electron/real profiles. Loopback auth and these mocks do not prove actual
Chromium origin/safeStorage/IndexedDB behavior. Real Chromium requires independent
runtime, whole-bundle and helper provenance plus separate synthetic roundtrip acceptance.

Enable native `profile-import-test-source` only for these mock-dependent tests;
it is empty and does not activate the helper's `test-source` or change production
behavior. Ordinary native tests/builds do not require the compiled mock. With the
approved fixture environment configured externally, run:

```text
cmd.exe /c rustc --edition 2021 scripts/migration/native-import-mock.rs -o APPROVED_TEST_TEMP/mock.exe
cmd.exe /c cargo test --manifest-path rust/Cargo.toml -p zabor-native --no-default-features --features profile-import-test-source --lib migration:: --offline --locked -j2 -- --test-threads=1
cmd.exe /c cargo test --manifest-path rust/Cargo.toml -p zabor-native --no-default-features --lib profile:: --offline --locked -j2 -- --test-threads=1
```

Completed affected verification, earlier failures and retained acceptance limits:
[native-import-checkpoint.md](native-import-checkpoint.md).

## Bounded resume: typed original-owner binding / Привязка исходного owner

`MigrationOperation::with_registry(command, original_intents, original_ticket,
original_registry)` is additive; `new`, `authenticate`, `execute`, `close` and all
helper/import APIs remain available. Provide the caller's dedicated registry at
original native-operation registration, not a replacement registry after completion.
The operation still prepares and retains the original cancellation before acquisition.

`MigrationRetirement` is minted only from that registry's completed drain and the
original cancellation entry's observation. It retains original operation/PID/exit,
owner/reader/root/job/handle observations, including failed exports. Its fields and
origin are private; it is not deserializable, a JSON consent flag, or a client-readiness
assertion. Copy-import cannot proceed from successful bytes without matching complete
retirement and successful original export. A failed export never retries through a
legacy helper and never becomes an empty successful profile.

Use `execute_for_reuse` in an existing application owner. For accepted import/recovery
it returns `MigrationPublication`, retaining the reopened Profile and its exclusive
gate. `publication.install(&original_operation, |profile, original_ticket| ...)`
transfers the actual Profile under the same intent commit. Logout/cancellation,
closed auth connection, another operation with an equal numeric helper ID, or a
replacement ticket cannot authorize installation. Do not replace this transfer with
an unchecked path-only reopen. Existing `execute` deliberately drops that publication
for compatibility; CLI holds it through explicit operation/monitor retirement, then
closes the profile without opening shell/chat. The parent must retain/join the original
blocking native owner; dropping a spawn_blocking waiter is not its retirement.

The async `close` waiter also leaves the original monitor JoinHandle in the operation
until its actual join returns; cancellation/drop of that waiter does not detach the
handle or create a replacement monitor. Exporter retirement still precedes that wait.

Не меняются алгоритмы, секреты или маршруты данных. Typed retirement receipt хранится
в памяти и содержит только наблюдения служебного процесса; runtime не создаёт новый
журнал credentials/JWK/history. Тестовый receipt writer включён только в mock tests
и сохраняет реальные не-секретные наблюдения в отдельно выбранный evidence-каталог.
Ввод account/key/consent остаётся независимым, отзыв исходного owner запрещает install.

### Necessary PRIMARY seams, not a shell replacement

PRIMARY `3963585` contains the saved developer-launcher transport. Its fixed
`cmd.exe /d /v:off /s /c "native.bat data"` route may carry nonsecret runtime argv
via `ZABOR_NATIVE_ARGV_JSON`, for example an array beginning `run, --import-profile`.
Keep credentials, independently confirmed public key and trusted hashes in the
separate secure process environment; never place credentials in argv/request files.
JSON transports argv only and does not grant account, bundle, consent or readiness proof.

Keep the existing three-line `main.rs` migration dispatch before any normal
`shell::run`/beta open, preserving unrelated startup/update-host admission. There
is no launcher/Cargo/Slint/helper-source replacement in this resume.

For an in-app request, PRIMARY binds the original intent/ticket/dedicated registry
and retained blocking owner to its existing resource registry, authenticates before
copying, then installs the returned Profile into its real SessionIo/ChatIo ownership
through `MigrationPublication::install`. Before rollback it must cancel/seal and
join its existing FileIo/ChatIo/SessionIo/SQLite/Profile owners and drop their profile
handles; helper retirement alone is not proof those application resources drained.
Use their actual completed receipts, never a reminted nonce or an invented boolean.
The concurrently running PRIMARY resource check is not read, polled, edited or
claimed complete here; binding to its final checked APIs remains PRIMARY-owned.
