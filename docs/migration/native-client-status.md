# Нативный клиент Rust / native Rust client

Редакция / updated: 2026-11-08.

## Решения владельца для полного переноса

Подтверждено 2026-11-06: целевые ОС — Windows 10/11 x64. Приложение должно быть нативным Rust без Electron/WebView; сторонние нативные C/C++ медиабиблиотеки (включая libwebrtc и Opus) допустимы. Интерфейс и поведение сверяются с действующим React-клиентом. Beta использует отдельный профиль с импортом-копированием и сохранением исходного профиля. В автономном проходе разрешены только локальные интеграционные проверки. Этапы разрешено фиксировать коммитами в `rust/native-client`; после готовности полноценного клиента — push ветки и публикация GitHub beta `3.8.0-rust`. Автоматическое слияние в main не согласовано. Backend остаётся .NET, SFU — Go.

Owner decisions confirmed on 2026-11-06: target Windows 10/11 x64; a native Rust application without Electron/WebView, with native C/C++ media libraries allowed. Preserve the React client's UI and behavior. Use a separate beta profile with copy-based import that preserves the source. Unattended integration checks are local only. Checkpoint commits on `rust/native-client` are authorized; push and the `3.8.0-rust` GitHub beta release follow completion of the full client. Automatic merging to main is not authorized. The backend remains .NET and SFU remains Go.

Контекст для compact и последующего продолжения: [native-client-handoff.md](native-client-handoff.md). Он сохраняет текущие ограничения, checkpoints, блокеры и весь оставшийся план; compact не означает завершения переноса.

Compaction/continuation context: [native-client-handoff.md](native-client-handoff.md). It preserves constraints, checkpoints, blockers and the complete remaining plan; compaction does not imply migration completion.

## Где остановилась исходная сессия

Сессия «Миграция zabor на Rust» перешла от WASM voicing beta к полному нативному клиенту. Последнее подтверждённое действие — съёмка шести Slint-кадров updater при 100% DPI в `migration-artifacts/native-ui/native-v3`. Итогового сравнения и завершённых проверок нативного этапа не было.

Рабочая ветка: `rust/native-client`. Checkpoint рабочего Electron/WASM-клиента перед переносом: `d088795`. Нативный код находится в `rust/crates/zabor-native`; состояние этой ветки не означает перенос в основную пользовательскую папку или публикацию релиза.

## Реализовано

После checkpoint `d1a33f6` начат связанный нативный клиент. `native.bat` теперь открывает основную оболочку, а updater запускается явно через `--preview updater`.

- Slint-формы входа, подтверждения регистрации и настройки профиля используют исходные RU/EN каталоги. Реальные TextInput поддерживают клавиатуру, пароль и ограничения длины; базовая форма входа сверена с React.
- Реализованы SignalR JSON framing, HTTP negotiate, WebSocket, invocation/completion, ping/timeout/cancel, ограниченные очереди; `CheckUserExists`, `Login`, `Register`, captcha и загрузка каналов/друзей/настроек. UI обновляется по событиям, без постоянного render polling.
- Основная оболочка связана с данными, созданием канала, запросом дружбы, выбором собеседника, текстовым чатом и экспериментальным голосовым состоянием. Добавляются личные звонки и реконнект.
- Нативный libwebrtc 0.4.0 собран с Windows SDK `webrtc-a545de4`. Короткий путь SDK и статический MSVC CRT заданы в `.cargo/config.toml`; исправлены реальные ошибки длинных путей и MT/MD несовместимости. Локальный тест подтверждает SCTP и декодированный PCM без физического микрофона/сервера.
- Отдельный профиль `zabor-desktop-native-beta` сохраняет сессию/ключ через current-user DPAPI, настройки в JSON и историю/TOFU в SQLite. Резервный текстовый чат реализует P-256/AES-GCM, ACK/read/sync/delete, сохранение при выходе и 14-дневную очистку. Криптоформат проверяется отдельным Node WebCrypto тестом.
- Подпись сборки сохраняет HMAC-SHA256 контракт; ключ берётся из окружения. Настоящие серверы и пользовательские профили в автоматических проверках не используются.
- Синхронизация roster охватывает полный снимок каналов, вход/выход/перемещение участников, создание/изменение/удаление каналов, заявки и добавление/удаление друзей, приглашения, presence, speaking и серверный mute/deafen. Запоздалый выход из старого канала не удаляет участника нового. Частые speaking/state события обновляют строки без повторного декодирования аватаров.
- Добавлены экспериментальный data-only P2P chat transport и native screen/window capture с преобразованием BGRA/I420, ограничением размера и выводом декодированных кадров в Slint. Повторный старт видеоподписки и отбрасывание stale callbacks проверены локально; полный stream/call reconnect ещё требует доведения.
- Файловый обмен соединён end-to-end: выбор/перетаскивание до 20 файлов, подтверждение отправки, фоновый staging, metadata, запрос получателя, SCTP, progress/cancel/retry, SHA-256 и атомарное сохранение. Очистка кэша распространяется control packet и переживает reconnect через SQLite tombstones; внешний сохранённый файл показывается в проводнике. Отмена/ошибка/удаление друга очищают partial files; при старте восстанавливаются прерванные загрузки. Нативный локальный тест передаёт и сохраняет 2 MiB по настоящему SCTP.

Контрольная точка связанного ядра и roster: `7e0ea3d`. Следующий локальный этап усиливает lifecycle: каждое событие WebRTC несёт срок жизни конкретного peer; закрытие/замена инвалидирует уже поставленные в очередь ICE/track/data-channel callbacks. Прямые chat callbacks дополнительно привязаны к конкретному data channel. SDP offer epochs уникальны в процессе и ограничены безопасным диапазоном JavaScript. VideoReceiver сохраняет RTP track при остановке подписки и повторно подключает его при старте без обязательного нового `on_track`; устаревшие кадры проверяются также перед выводом в UI. `StreamDropped` использует исходный grace interval 30 секунд, отменяемый новым стартом.

Lifecycle checkpoint: `fda236c`. После него подключена нативная боковая панель уведомлений с заявками в друзья и приглашениями. Кнопки используют исходные `AcceptFriendRequest`, `DeclineFriendRequest`, `AcceptChannelInvite`, `DeclineChannelInvite`; `false` и ошибки сохраняют уведомление для повторной попытки, прежний `null` completion считается успешным. Принятое приглашение запускает вход в канал; при уже активном голосе сохраняется подтверждение смены канала. Для просмотра на синтетических данных доступен `native.bat --app --preview notifications`.

Панель проверена нативными interaction/model checks на RU/100% и EN/200% с reduced-motion; финальные кадры просмотрены в `migration-artifacts/native-ui/notifications-2026-11-06/`. Исправлены равная ширина кнопок и отступ текста приглашения. Эти два native capture не являются полной React/native parity-матрицей основной оболочки. Диагностические кадры до исправления удалены.

Последний полностью выполненный Rust-прогон ядра: **33 unit tests и 17 integration tests**, fmt/Clippy `-D warnings` и свежая сборка пройдены. Добавлены полный local SCTP file workflow и четыре сценария файлов: cancel/retry/read receipts, повреждение/timeout/revocation, deletion-before-metadata/restart и отменённый staging. Медиапроверки включают PCM/SCTP, замену peer с устаревшими callbacks/SDP answer и трёхкратное возобновление видео на существующем RTP receiver. Это проверка отдельных механизмов, а не полная приёмка приложения. Файловый UI снят на RU/100% и EN/200% в `migration-artifacts/native-ui/files-2026-11-06`; финальный EN-кадр просмотрен, исправлены позиция progress/подписей и clipping списка. Ранее проверенный auth capture `migration-artifacts/native-auth/checkpoint-roster-2026-11-06` содержит 24 пары и 8 interaction runs RU/EN при четырёх DPI; максимум средней RGB-ошибки — 0.4279/255. Полная визуальная матрица основной оболочки ещё требуется. Graphify обновлён до 1758 узлов/4230 связей; пять JSON-каталогов не дают структурных узлов.

### Аудио/DSP: локальное продолжение 2026-11-07

Добавлены нормализованные совместимые настройки, TonalSuppressor/VoiceShaper,
ручной gate, SmartGate, RNNoise/DeepFilterNet3/Silero adapters, WASAPI device threads
и AudioMixer с atomic mute/deafen/moderation, ограниченными очередями и отзывом
устаревших peer-lifetimes. После neural checkpoint эти части соединены с
`VoiceSession` и `shell.rs` через `microphone.rs`, `native_audio.rs` и
`audio_playback.rs`; platform ADM не приобретается. Кодовая интеграция проходит
новый local-only прогон и ещё не является полной audio/client приёмкой.

UI mute/deafen меняет когерентный atomic state до сетевой очереди. Capture/playback
epochs отбрасывают старый PCM при коротком on/off; final submission повторно
проверяет restriction/failure/cancel/epoch. Owning-thread DSP использует 48k/480,
NativeAudioSource без capture queue; WASAPI backlog — максимум 4×10 ms.
Received tracks имеют bounded sink и peer-lifetime mixer lanes; старый Drop не
удаляет replacement lane. AEC reverse добавляется после успешного WASAPI write,
с оценкой device delay; физическая эффективность ещё не подтверждена.

Manual threshold сохраняет smart branch; ultra-low-latency обходит smart/gain/peak.
DFN3 reset клонирует pristine runtime state/compiled plans на owning thread,
не компилируя модель повторно. Settings bootstrap сохраняет unknown fields и
локальный язык. Settings/device/per-user volume UI, mic-test, hotplug/recovery,
raw/Silero и 4x outer WebAudio parity ещё требуют завершения. Новый прогон
`migration-artifacts/native-audio/wiring-final-2026-11-07` прошёл: 36 unit +
45 integration tests, 9 HTTP tests, pinned artifacts, fmt/Clippy/build/source
freeze. Neural metrics сохранились. RNNoise runtime reset занял 155 μs,
DFN3 — 3343 μs в этом прогоне, без recompilation. После него исправлены
cancel-after-model-init и settings/lane insertion race; neural-disabled
compatibility прошла 74 теста и Clippy `-D warnings`. Финальный default-feature
прогон с этими правками также **прошёл**: 36 unit + 45 integration tests,
9 HTTP tests, pinned artifacts, fmt/Clippy/build/source freeze. Отчёт:
`migration-artifacts/native-audio/wiring-verified-2026-11-07/verification.json`.
Повторно сверены 392 source hashes и бинарный SHA-256 — differences=0.
Neural PCM metrics не изменились; runtime reset RNNoise 108 μs / DFN3 3723 μs.
Graphify: 5131 nodes / 12101 edges / 240 communities; семь metadata zero-node
warnings, HTML выше лимита 5000 пропущен. Это не full audio acceptance.

Code-level microphone/playback integration now connects owning-thread WASAPI,
DSP, unbuffered NativeAudioSource and bounded peer playback to VoiceSession/shell
without acquiring the platform ADM. Shared packed states/epochs enforce immediate
mute/deafen and invalidate stale frames; AEC reverse follows successful device
writes, not an assumed hardware AEC. Neural reset retains compiled DFN3 plans.
The local-only run passed 36 unit and 45 integration tests, nine HTTP tests,
pinned artifacts, fmt/Clippy/build and source freeze. Two subsequent cancellation/
gain-race fixes also passed the final source-frozen run: 36 unit + 45 integration
tests, nine HTTP tests, pinned artifacts, fmt/Clippy/build; 392 source hashes and
the fresh binary hash match. The report is under `wiring-verified-2026-11-07`.
Neural metrics are unchanged; reset measured 108 μs for RNNoise and 3723 μs for
DFN3. Neural-disabled compatibility passed 74 tests and Clippy with `-D warnings`.
The graph has 5131 nodes/12101 edges; metadata/HTML limitations remain recorded.
Physical AEC, devices/hotplug/recovery,
settings/microphone-test UI, raw/Silero and outer WebAudio 4x parity remain unfinished;
this is not full audio/client acceptance or a release.

Mic/playback checkpoint: `6db8897` on `rust/native-client`. Проверенная кодовая
интеграция сохранена; push, merge, release и shutdown не выполнялись. Далее —
audio settings/device/mic-test UI и recovery/parity, затем остальные этапы handoff.

Verified code wiring is saved in `6db8897`; no push, merge, release or shutdown.
Next: audio settings/device/microphone-test UI and recovery/parity, followed by
all remaining handoff stages.

Device-runtime код после `498fb9e` проверен: managed WASAPI wrappers сохраняют owning
DSP thread, source/mixer/RTC subscriptions при endpoint switch. Explicit selection
и default/hotplug inventory, safe default fallback без переписывания выбранного ID,
bounded retry 250–2000 ms/30 s и old-route PCM/AEC guards реализованы в коде.
Первый frozen-source прогон прошёл: **43 unit + 51 integration tests**, fmt,
Clippy/build и source freeze, neural metrics unchanged, в
`migration-artifacts/native-audio/device-runtime-2026-11-07`. После него добавлены
backlog switch guard, 2 PCM tests и local RTC continuity при трёх output/capture
resets. Neural-disabled прошла **45 unit + 44 integration tests** и Clippy.
Детерминированный deafen guard в RTC test проверен финальным combined job:
`device-runtime-no-neural-final-2026-11-07` → `device-runtime-verified-2026-11-07`.
Финальный default-feature результат: **45 unit + 51 integration tests**, 9 HTTP
tests, pinned artifacts, fmt/Clippy/build/source freeze. Без neural features —
**45 unit + 44 integration tests**, Clippy/source freeze. Все **394 source hashes**
совпали между modes и files, binary hash matched; neural PCM metrics unchanged.
RNNoise reset44 μs / DFN3 2489 μs. Физическая приёмка
hotplug/AEC/latency, settings/save/mic-test UI и full audio acceptance не завершены.
Непрерываемые OS COM calls не дают hard-realtime гарантии; deadline проверяется
между operations. Backlog switch guard и final regression passed; отчёты — в handoff.
Graphify device-runtime: 5179 nodes / 12269 edges / 237 communities; семь
metadata zero-node warnings, HTML выше лимита5000 пропущен.

Verified device-runtime code retains owning DSP threads and source/mixer/RTC subscriptions
across endpoint switches, with system-default fallback preserving selected IDs,
250–2000 ms retries, a 30-second recovery window and stale-route PCM/AEC guards.
The first frozen-source run passed 43 unit + 51 integration tests and fmt/Clippy/
build, with unchanged neural metrics. A later backlog guard and RTC continuity
checks passed 45 unit + 44 integration tests with neural features disabled. A
test-only deterministic deafen guard also passed the final sequential run:
45 unit + 51 integration tests with default features, nine HTTP tests, pinned
artifacts and fmt/Clippy/build/source freeze; 45 unit + 44 integration tests and
Clippy/source freeze without neural features. All 394 source hashes match across
modes and current files; the fresh default binary hash matches. Neural metrics
are unchanged; reset measured44 μs for RNNoise and2489 μs for DFN3.
Physical hotplug/AEC/latency,
settings/save/microphone-test UI and full acceptance remain unfinished; in-flight
OS COM calls are not safely interruptible and no hard-realtime guarantee is made.

Device-runtime checkpoint: **`8e4b7d3`** on `rust/native-client`. Код и проверенные
результаты сохранены; push/merge/release/shutdown не выполнялись. Следующий этап —
settings/device/per-user volume UI и local/server save, затем mic-test и весь
оставшийся план `docs/migration/native-client-handoff.md`.

Verified device recovery is saved in `8e4b7d3`. No push, merge, release or shutdown.
Next: settings/device/per-user volume UI and local/server saving, then microphone
testing and all remaining handoff stages. Full audio/client acceptance stays false.

Settings/UI WIP после `534f1d4`: native audio/device/per-user gain controls и
latest-wins persistence/server sync реализованы, но финальная проверка ещё нужна.
Local JSON writer и network task независимы; язык и unknown fields сохраняются,
pending привязан к owner/session, local-only `nativeAudioSync` не отправляется
серверу. Relay-only применяется к новым RTC constructors, не к уже созданным;
анонимность/скрытие IP не гарантируются. Mic-test, standalone meter и полная
settings/UI/audio/client acceptance всё ещё не завершены. Подробности — handoff.

Settings/UI is work in progress: native audio/device/per-user gain controls and
latest-wins saving are implemented, with independent local/network tasks and
owner/session guards. Language and unknown fields are preserved; local pending
metadata is not sent to the server. Relay-only affects new RTC constructions,
not existing connections, and makes no anonymity guarantee. Final source-frozen
checks, microphone testing and complete UI/audio/client acceptance remain pending.

Первый settings-code run прошёл:104 default-feature tests,97 neural-disabled,
fmt/Clippy/build,9 HTTP tests,397 matching source hashes и unchanged neural metrics.
Auth:24 captures/8 interaction runs passed. Settings capture runner остановился
на cmd quoting; manual first PNG выявила layout defects, которые исправлены вместе
с stale user-volume modal guard. Новый build/UI interaction/capture run выполняется;
latest corrected sources требуют нового final source-frozen report. Это не full
visual/UI/mic-test/audio/client acceptance. Логи/текущие jobs/точный next step — handoff.

The first settings code run passed104 default-feature and97 neural-disabled tests,
fmt/Clippy/build, nine HTTP tests and397 matching source hashes; neural metrics
are unchanged. Auth captures/interactions passed. A later settings capture command
failed on cmd quoting; its manual retry exposed layout defects now corrected with
the volume-dialog lifetime guard. The fresh build/UI run is in progress; corrected
sources still require final frozen-source verification and full acceptance.

Финальный settings code run прошёл: **104 default-feature /97 neural-disabled
tests**, fmt/Clippy/build,9HTTP/pinned artifacts и **398 matching source hashes**;
neural metrics unchanged. Strict visible64 captures/8 interactions также прошли,
но initial UI binary hash отличается от final rebuilt/tested binary. Сейчас
UI/auth повторяются только на финальном бинарнике; guard не ослабляется. Полная
visual/UI/mic-test/physical/audio/client readiness остаётсяfalse; checkpoint ещё
не committed. Точный pending job/output — в handoff.

Final settings code checks passed104 default-feature and97 neural-disabled tests,
fmt/Clippy/build, nine HTTP/pinned-artifact checks and398 matching source hashes.
Neural metrics are unchanged. Strict visible UI captures/interactions passed on
an earlier build; their binary hash differs from the final tested executable.
UI/auth evidence is being regenerated only on that final binary, retaining the
hash guard. Complete visual/UI/microphone-test/physical/client readiness is false.

**Settings/save/UI functionality проверена на final binary.**104 default-feature
tests (47 unit+57 integration),97 no-neural tests,9HTTP/pinned artifacts/fmt/
Clippy/build и398matching source hashes прошли.64 distinct-state settings PNG и
8interaction runs,24auth native/reference pairs и8additional runs прошли на том
же final tested binary.24native auth PNG byte-identical accepted checkpoint;
64settings PNG identical preceding same-source build. Хеши final code/UI/auth/
current.exe совпадают; neural metrics unchanged (resetRNNoise63 μs/DFN3 658 μs).
Reports: native-audio/settings-verified-2026-11-07, native-audio/settings-no-neural-
final-2026-11-07, native-ui/audio-settings-final-2026-11-07, native-auth/settings-
verified-2026-11-07 под migration-artifacts/. Не full visual/UI/audio/client
acceptance: mic-test/standalone meter/raw-Silero/outer4x/physical acceptance и
общий remaining plan ещё нужны. Следующий этап — mic-test; no push/merge/release/
shutdown, native/audioReady=false.

**Settings saving and native UI functionality are verified on the final binary.**
104 default-feature and97 neural-disabled tests, nine HTTP/pinned-artifact checks,
fmt/Clippy/build and398 matching source hashes passed.64 distinct-state settings
captures/eight interaction runs and24auth capture pairs/eight additional runs use
the same final tested executable. Auth images match the accepted checkpoint byte
for byte; all64settings images match the preceding same-source build. Final code,
UI/auth and current binary hashes match; neural metrics are unchanged. Full visual/
UI/audio/client acceptance remains false. Next: microphone testing/standalone
meter, then raw/Silero/outer4x and every remaining handoff stage. No push, merge,
release or shutdown was performed.

Checkpoint **`b1666ac`** сохраняет verified settings saving/device/per-user audio UI
на `rust/native-client`. All code/capture hashes passed; all verification jobs
completed. Далее — mic-test/standalone meter (план в handoff), затем raw/Silero/
outer4x/full UI и весь remaining migration plan. Полная readiness=false;
push/merge/release/shutdown не выполнялись.

Verified audio-preferences saving and UI functionality are saved in `b1666ac`.
Next: memory-only microphone recording/playback and standalone meter, preserving
network mute restrictions and owning DSP threads, then every remaining handoff
stage. This checkpoint is not full client/audio acceptance or a published beta.

### Mic-test core WIP после `ce84086`

Добавлены memory-only5s Float32 clip/state machine и opt-in local processed tap,
отдельный от network PCM. Generation/route/write guards,120ms settle/280ms flush,
rounded-peak silence, hidden pause/blocked auto-play и memory cleanup реализованы
в ядре. Новый local test suite проверяет сроки/буфер/seek/playback/restrictions и
RNNoise/DFN3 matching PCM; развитие проверок ещё идёт. NativeAudio/VoiceSession/UI
пока **не** подключены: это не готовые mic-test/standalone meter и не full audio
acceptance. Следующая интеграция — shared voice/monitor runtime, selected local
output/combined AEC reverse, затем Slint panel. Текущие jobs/точный план — handoff.

A memory-only five-second Float32 clip/state machine and opt-in processed local
tap, separate from network PCM, are implemented as a core. Tests cover timing,
buffer bounds, generation/route/write guards, pause/seek, restrictions and neural
PCM comparisons; verification is in progress. NativeAudio/VoiceSession/UI are
not yet wired. Shared voice/monitor runtime, selected local output/AEC feedback,
Slint panel and full microphone/standalone-meter acceptance remain pending.

Mic-test core development checks passed:14local integration tests/defaultClippy,
including RNNoise/DFN3 local-vs-existing PCM and retained network restrictions.
Self-review then added2unit tests and corrected successful-write ACK contention
and the default meter state. Full default/no-neural/source-frozen/UI regression
is now in progress; these are still core checks, not shared-runtime/Slint wiring
or complete microphone/physical/client acceptance. Следующий этап — runtime/UI;
pending jobs и результаты — handoff. No release/shutdown.

### Mic-test core/tap проверен / verified

Финальный прогон: **120 default-feature tests (49unit+71integration)**,
**112 neural-disabled (49unit+63integration)**,9HTTP/pinned artifacts/fmt/Clippy/
build и **400 matching source hashes**.14core/local-tap integration tests и2unit
tests прошли; networkmute/deafen/epoch guards и exact RNNoise/DFN3 localPCM
сохранены. Neural metrics unchanged.64settings и24auth regression images с8+8
interaction runs побайтово совпали с previous checkpoint и используют finaltested
binary;0source/capture/hash mismatches. Reports под migration-artifacts/native-
audio/mic-test-core-verified-2026-11-07, mic-test-core-no-neural-2026-11-07 и
native-ui/native-auth/mic-test-core-regression-2026-11-07. Это **ядро без runtime/
UI wiring**, не готовая кнопка mic-test/standalone meter или fullaudio/client.
Далее sharedvoice/monitorruntime иSlint panel, затем весьremaining handoff plan.
Readinessnative/audio/micTestUi=false; no push/merge/release/shutdown.

The memory-only microphone-test core and opt-in local tap passed120 default-
feature and112 neural-disabled tests, nine HTTP/pinned-artifact checks, fmt,
Clippy, build and400 matching source hashes. Neural metrics and all64settings/
24auth regression images are unchanged; tests and captures use the same final
binary. Shared-runtime/VoiceSession/Slint wiring is not yet implemented, so this
does not constitute microphone-test UI, standalone-meter or fullclient readiness.
Next: sharedvoice/monitor audio ownership, local playback/AEC wiring andSlint
panel, preserving every remaining handoff stage and release/shutdown condition.

Checkpoint **`ef6e860`** сохраняет verified mic-test memorycore/local tap на
`rust/native-client`. All verification jobs завершены. Далее — **shared voice/
monitor NativeAudio runtime +Slint wiring**, не повторять core/DSP/recovery/
preferences. Settings close/unmount долженdispose/revoke clip, tabhide — pause;
network ограничения сохраняются. Полная native/audio/micTestUi readiness=false;
push/merge/release/shutdown не выполнялись.

The verified microphone-test core/local tap is saved in `ef6e860`. Next: shared
voice/monitor NativeAudio ownership andSlint wiring. Closing/unmounting settings
must dispose the clip; hiding a tab pauses it. This is not a wired microphone-test
UI, standalone meter, full audio/client acceptance or a published beta.

### Shared voice/monitor runtime: WIP

App-scoped NativeAudio ownership, independent voice/monitor leases and one
cancellable pending startup are implemented after `cb6e8a1`; cancellation keeps
the owning factory quarantined until it returns. Voice teardown preserves local
capture/playback demand; idle release is ten seconds. `AudioFlow` connects the
existing owning capture/DSP and selected output to MicTest, retaining network
mute/deafen/epoch guards and suppressing local-only network PCM/speaking. Remote
and local output remain separate until mixing; only successful current-route
WASAPI writes acknowledge playback and append combined AEC reverse. New fake-
runtime/local-source/neural tests and Clippy are in progress. Native Slint panel,
standalone meter and physical/full client acceptance remain pending; no release
or shutdown. Точные jobs и план продолжения — в handoff.

Shared runtime development passed14tests/defaultClippy. Self-review then added
an owning-capture generation reset for activation during local DSP, deferred
network metrics until final voice-generation commit, three unit tests and a
late-local-neural-frame/source comparison. The complete default/no-neural/frozen-
source/final-binary/settings/auth regression is in progress. This is not Slint
panel/standalone-meter or physical/full acceptance. On success, save runtime
checkpoint and proceed to UI wiring; all release/shutdown constraints remain.

### Shared voice/monitor runtime проверен / verified

Финальный прогон: **137default-feature tests (52unit+85integration)**,
**128neural-disabled (52unit+76integration)**,9HTTP/pinned artifacts/fmt/Clippy/
build и **402matching source hashes**. Общий capture/DSP/runtime и voice/monitor
leases, cancellation quarantine, idle10s, final voice/route/epoch guards,
local-only no-network metrics/PCM и late local-preroll reset проверены локально.
64settings и24auth изображения/8+8interaction runs unchanged на finaltestedbinary;
0source/capture/hash mismatches, neural metrics unchanged. Reports под
migration-artifacts/native-audio/shared-runtime-verified-2026-11-07,
shared-runtime-no-neural-2026-11-07 и native-ui/native-auth/
shared-runtime-regression-2026-11-07. Далее **Slint mic-test panel/standalone local
meter**, не переписывать runtime/DSP/core/device/preferences. UI/physical/full
native/audio acceptance не завершены; readinessfalse, no push/release/shutdown.

The shared voice/monitor runtime passed137default-feature and128neural-disabled
tests, nine HTTP/pinned-artifact checks, fmt, Clippy, build and402matching source
hashes. Capture/DSP ownership, cancellation quarantine, ten-second idle release,
final generation/route guards, local-only PCM/metrics restrictions and late local
preroll resets passed synthetic/local-source checks. All64settings/24auth images
and neural metrics are unchanged on the finaltestedbinary. Next: Slint microphone-
test panel and standalone local meter on the existing runtime; full UI, physical
and client/audio acceptance remain unfinished. No publication or shutdown.

Verified shared runtime checkpoint **`8826de6`** is saved on `rust/native-client`.
All verification jobs completed; next is **Slint microphone-test panel and
standalone local meter**, preserving the settings-modal monitor lease across tab
switches and disposing it on close/disable/logout. No second capture/DSP runtime
or rewrite of checked core/preferences/device recovery is needed. Full client/
audio/micTestUi readiness remainsfalse; no push, merge, release or shutdown.

Slint microphone-test panel/local-meter wiring is implemented as WIP after
`b755fe5`: settings-modal monitor lifetime, independent local callbacks and
preparation, immediate close/logout disposal, session-reset owner guards and
native accessible record/play/pause/scrub/retake states. The unchanged React
reference passed14development captures at1x; current controller tests/build and
full UI/source/binary regression remain pending. This is not full physical,
motion, audio or client acceptance; readinessfalse, no publication/shutdown.

Panel/controller development passed7tests, Clippy and build. Native pointer
checks then found and fixed real Slint default-centering defects in player,
playback lane and filled track; panel stacking/reveal and separate pulse/dot
phases were corrected. The complete two-feature/source/binary/native-reference/
settings/auth regression is in progress. Exact reference spring/motion/pixel and
physical/full audio/client acceptance remain unfinished; no release/shutdown.

First full panel pipeline passed144default/135no-neural tests,409matching
source hashes,56panel pairs,64settings and24auth captures with unchanged
protected UI/neural regressions. Subsequent image/platform-font review corrected
dot centering, actual border/track insets, issue/retake Semibold face and added a
release-without-pointer-move seek case. Final reviewed-source pipeline is running;
the previous pass is not evidence for these last edits. Exact motion/pixel,
physical/audio/client acceptance remains pending; no publication or shutdown.

Функциональная Slint-панель/local meter прошли финальный прогон:
**144default (52unit+92integration) /135no-neural (52unit+83integration)**,
9HTTP/artifacts/fmt/Clippy/build и409matching sources.56RU/EN/four-DPI native/
reference pairs+8interactions,64settings+8interactions,24auth+8interactions на
одном final tested/current binary;0hash mismatches, protected UI/neural unchanged.
Settings-modal lease, independent local callbacks/preparation, pause/retain on
hide, close/disable/logout disposal и release-without-move seek checked.
Далее local-only DSP-health/retry/meter reset, exact font/spring/motion/pixel and
raw/Silero/outer4x/physical/full acceptance; **не переписывать panel/runtime/core**.
Readinessfalse; no push/merge/release/shutdown.

Local-only DSP-health/meter/explicit-retry changes are under final verification
after `29eb20a`. Development checks passed13health tests including the actual30s
deadline,13shared-runtime,13core,7UI and8microphone tests plus Clippy. In-place
owning-thread recovery preserves the source/mixer/working output and completed
clip; stale input revisions and canceled recovery waiters are guarded. Full
two-feature/neural/UI/source/binary regression is running; this is not yet a
saved/accepted health milestone or complete physical/client acceptance.

Local DSP-health/explicit recovery финально проверены:
**158default (52unit+106integration) /148no-neural (52unit+96integration)**,
410matching sources,9HTTP/pinned artifacts/fmt/Clippy/build.56panel/64settings/
24auth and8+8+8interactions use one final tested/current binary; all images and
neural metrics unchanged,0hash mismatches. Processor-only failure resets meter
and preserves completed clip/working output; stale input tokens, owning-thread
single-flight recovery, actual30s deadline and reset/cancel/late results checked.
No auto retry or second capture runtime. Next confirmed font-family fallback
correction and exact font/spring/motion/pixel acceptance, then remaining audio/
physical/client stages. Native/audio/micTestUi readinessfalse; no release/shutdown.

Verified DSP-health milestone saved as **`7607a04`** on `rust/native-client`.
Next: actual native family selection correction and reference font/spring/motion
evidence. The full client remains unfinished; no push, merge, release or shutdown.

Actual panel font-family/capture-child health проверены:
**159default (53unit+106integration) /149no-neural (53unit+96integration)**,
410equal sources,9HTTP/pinned artifacts/fmt/Clippy/build;56panel/64settings/24auth/
8+8+8interactions on one final tested/current binary.32expectedretake/issue font
images changed; idle/arming/recording/settings/auth/reference/neural unchanged,
0source/image/binary mismatches. Native actual Segoe UI600 selection checked;
canceled capture/source children cannot be mistaken for recoverable DSP-only
failure. Residual baseline/raster/layout/font-pixel and actual1280ms press/1880ms
pop motion/focus acceptance remain pending, alongside physical/full-client work.
No push/merge/release/shutdown; native/audio/micTestUi readinessfalse.

Latest checked milestone **`87513e2`** saves actual font-family selection and
canceled audio-child health on `rust/native-client`;159/149tests,410sources,
56panel/64settings/24auth and8+8+8interactions passed. Next exact baseline/raster,
1280ms press/1880ms pop/continuous motion/focus, then remaining audio/client work.
Do not repeat verified font-family/DSP-health/panel/runtime/core wiring. No push,
merge, release or shutdown; complete client/audio/UI acceptance still unfinished.

Reference thumb spring implementation is under final verification after67d378c:
1280ms press/1880ms pop with current-scale interruption and reduced-motion/
hide/retake cancellation.3162pure millisecond samples match installedAnime4.5.0;
native323frame timing and unchangedReact liveCDP/RAF development checks passed.
Snapshot-free timing is separate from PNGreplays to avoid capture-induced stalls.
The final two-feature/fullUI/motion matrix is running; this is not a saved final
spring/UI/client acceptance checkpoint. All readiness flags remain false.

Reference solver/native thumb lifecycle финально проверены локально:
**162default (56unit+106integration) /152no-neural (56unit+96integration)**,
412matching sources,9HTTP/pinned artifacts/fmt/Clippy/freshbuild.56panel/64settings/
24auth +8+8+8interactions, staticimages/reference/neural unchanged,0hash mismatch.
Finalcode/current.exe/allUI/auth/motion binary1959b537… is verified. Actual1280ms
press/1880ms pop with current-scale interruption and reduced/hide/retake guards;
all3162solver samples match installedAnime4.5.0.8native13phase/2550timingframes and
8explicitlywarmedReact5phase/2324frames, separately241native+40referencePNGs,
at4DPI/RUEN passed. Warm-up scope is explicit; coldReact200%≈194msgap retained
unaccepted. Next baseline/raster/layout/focus, synchronized/cold/card/pulse/dot
and remaining audio/physical/client work. Allreadinessfalse; no release/shutdown.

Verified solver/native spring-lifecycle milestone saved as **`c270cc6`** on
`rust/native-client`.162/152tests,412sources, allstaticimages/neural unchanged,
8native/8explicitlywarmedReact timing runs and separatePNG evidence passed.
This does not certify cold/synchronized/pixel/focus/card/pulse/dot/fullclient
acceptance. Next baseline/raster/focus and remainingmotion/audio/client work;
do not redo checked solver/font/health/runtime/core. No release or shutdown.

Caption baseline placement checked locally: **162default /152no-neural tests**,
413matching sources,9HTTP/pinned artifacts/fmt/Clippy/fresh build;56panel/64settings/
24auth +8+8+8interactions. At4DPI/RUEN,48glyph-row measurements passed: maximum
retake/issue centroid errors0.01123/0.03740px (0.1px gate); primary pixels unchanged.
Only30expected caption snapshots changed, settings/auth/reference/neural unchanged.
Current binary **1b01d96bfd486e143d5c88616a34a48c06c21af805ca8440256717a2417fd8cb**
matches audio/UI/auth/motion/review reports `mic-baseline-local-2026-11-07`.
8native/2544timingframes and8explicitlywarmedReact/2325frames with241+40separate
PNGs passed. This is not exact raster/focus/fullUI/cold/physical/client acceptance;
remaining plan and allfalse readiness flags persist. No publication or shutdown.

Caption baseline checkpoint saved as **`96aeb73`** on `rust/native-client`.
Current413sources/binary1b01d96b… match complete local regression reports.
Next remaining raster/layout/focus and synchronized/cold/card/pulse/dot acceptance;
checked baseline/font/solver/runtime/DSP work must not be repeated. Full client
readiness remainsfalse. No push/merge/release/shutdown.

### Visible-panel focus / Space checked locally

Current regression `mic-focus-final-2026-11-07`: **162default/152no-neural tests**,
417matching sources,9HTTP/pinned/fmt/Clippy/fresh build;56panel/64settings/24auth
+8+8+8interactions and48unchanged caption-row checks. **800RU/EN four-DPI focus
pairs** passed;176focused/272no-outline selected-control annulus checks stayed
within1physicalpx. New pointer focus hides outline, same-target pointer preserves
keyboard outline; down-focus/Tab/reverseTab/Shift/modifiers/armedSpace/unarmed/
repeat/focus-transfer releases checked. Static panel/settings/auth/reference/
neural unchanged. Current binary **6cb78662f9dee911862d53ff94b1f29e7c4e38bba68c49628cedabeba54baf81**
matches all audio/UI/auth/motion/focus/review reports;0post-result hash mismatches.
8native/2554frames and8explicitlywarmedReact/2328frames with241+40separatePNGs passed.
This doesn't establish exact focus raster/pressed keyboard/full-modal/hiddenDOM/
programmatic/window/accessibility/cold/physical/full-client acceptance. All
readinessflags remainfalse; remaining migration plan unchanged. No release/shutdown.

Visible-panel focus/Space checkpoint saved as **`5530cc7`** on `rust/native-client`.
Current417sources/binary6cb78662… match `mic-focus-final-2026-11-07` reports.
Next remaining exact raster/pressed-keyboard/focus-lifetime/full-modal and
synchronized/cold/card/pulse/dot acceptance, then remaining migration plan.
Checked focus/caption/font/solver/runtime/DSP fixes must not be repeated.
All jobs complete; full-client readinessfalse. No push/merge/release/shutdown.

### Pause contour checked locally

Latest evidence: `migration-artifacts/native-ui/mic-pause-final-2026-11-07/`
(root verification; audio-default/audio-no-neural/panel/icon-review/baseline/
settings/auth/motion/focus/focus-review). **162default/152no-neural tests**,
419matching sources,9HTTP/pinned/fmt/Clippy/fresh build;56panel/64settings/24auth
+8+8+8interactions,48captionrows,800focuspairs/0differences and448annulus checks
within1physicalpx. Native2549frames/warmedReact2326frames;241+40separatePNGs.
Actual binary **3fb998b289334e6abddeb1ff852a1f9477ca3f11fbbf98061b64220a4cd014bc**
matches all reports,0post-result source/binary/evidence mismatches.
Only production change: pause SVG contour matches unchanged React, fixing bar
widths.16pause measurements improve strictly; all40nonpause panel images,
8play images/asset, settings/auth/reference/neural/captions remain unchanged.
DirectPath and play-contour experiments failed pixel gates and were rejected.
`playContourAccepted=false`; exact pause/full raster, pressed keyboard/modal/
cold/physical/full-client acceptance remains pending. Next primary keyboard
press/outline and retake active-scale evidence, then remaining migration plan.
No release/shutdown; all readinessfalse and no pending test jobs.

Pause-only checkpoint saved as **`579da60`** on `rust/native-client`.
Current419sources/binary3fb998b2… match `mic-pause-final-2026-11-07` reports.
Next isolated primary keyboard-press/transform/outline and retake no-active-scale
evidence; remaining play/raster/modal/cold/physical/full migration stays pending.
Do not redo checked pause/font/caption/focus/Space/spring/DSP/runtime work.
All jobs complete; no push/merge/release/shutdown, readinessfalse.

### Microphone feedback WIP preserved; functional blocks continue

Primary/outline0.99 and retake immobility passed296development pairs/16outline
checks at100% DPI. Final code passed162default/152neural-disabled tests,423source
hashes and static/caption/settings/auth preservation. Final motion failed on a
native cold406ms initial observation; remaining focus800pairs/448annulus checks
passed, but pressed stopped at RU200%90second deadline after888matching pairs
at100/125/150%. Full regression and pressed-stage acceptance remainfalse.
`mic-pressed-final-2026-11-07/preservation-boundary.json` retains all4439artifacts
and the exact final executablef4bcd0a9… under `binary-frozen/`; no failures/gates
were hidden or weakened. This is saved WIP, not a verified pressed milestone.
Per owner direction, continue functional call/reconnect/media/profile/packaging
blocks; keep outstanding visual/physical/full-client acceptance separate.
No pending GUI jobs, publication, merge or shutdown.

### Call-resume protocol checked locally

After saved mic WIP `a6c861b`, native call recovery now implements session-scoped
CallSession/Suspended/Resumed/Ended, optional EnableCallResume, LoginAndResume
and ResumeCall with bounded retries/stale-response guards. RTC peer replacement
retains the existing audio source/runtime lease; UI/DSP/runtime/media/core sources
are unchanged. Eight controller tests and three actual loopback Hub contract
tests were added. Full173default/163neural-disabled tests, Clippy/fmt/pinned/9HTTP
and fresh build passed;425sources and all final evidence/current+frozen binary
hashes match with0mismatches, neural metrics unchanged. Binarye2c28d73… and reports
are retained under `native-calls/recovery-final-2026-11-07/` (finished Nov08).
This verifies call recovery protocol, not full call/client acceptance. Immediate Leave/
Logout cancellation, total reconnect/resource timing, stream restoration, full
call UI and physical/two-device checks still require completion. All prior media/
profile/import/packaging/visual/full-client blockers remain in the handoff.

### Priority Leave/Logout: components checked locally

After verified call protocol `1a89638`, cancellation work adds separate owner/voice
epochs, critical actions outside the16-slot queue, preserved async file-dialog
backpressure, guarded UI delivery and scoped pinned-future cancellation. Leave
immediately revokes only existing AudioDemand voice generation; independent
MicTest/runtime/preferences survive. Existing DSP/Slint/React/media sources are
unchanged. Interrupted call starts/teardown retain the bounded EndCall hint.

Final `native-calls/critical-final-2026-11-08/` passed188default/178neural-disabled
tests, both Clippy modes/fmt/pinned/9HTTP/fresh build and8inert RUEN/four-DPI
auth/roster/audio/MicTest interaction runs. Neural metrics are unchanged.
Independent428primary+3reference source/all24evidence/current+frozen binary checks
found0mismatches. Final executable SHA-256:
`5ca812849fe0be28649112cd04e945cbfc67ca064d95de2065f121da8893e73f`.
Earlier development failures remain retained. This verifies intent/queue/UI
dispatch/resource/actual local-Hub cancellation components, not all worker waits,
full-modal/window/continuous-input/endpoint/thread/physical/two-device behavior.
Direct chat/file owner cancellation and publication/lifetime handling are next;
Leave must preserve unrelated session work without restarting/dropping its future.
Worker/audio/call/full-client acceptance staysfalse; no push/merge/release/shutdown.
Owner requests compact after each saved/verified stage and unattended continuation.

### Chat/file owner cancellation: components checked locally

After saved critical-intent checkpoint `da86276`, owner-token/session-guard
work preserves chat/file futures across Leave and cancels them on Logout or window
stop. Per-packet and queued UI guards retain the original owner ticket; native file
jobs and transport callbacks use independent child owner tokens. Original SCTP
labels/crypto/retention/Hub deadlines/audio/MicTest/Slint/React code is preserved.
Development fmt/Clippy,5intent/3transport/1filejob/6queueUI/6session/25local tests
passed, including complete existing SCTP file transfer/save and actual stalled
local Hub cancellation/Leave preservation.430primary+3reference hashes match.
Final `native-calls/chat-owner-final-2026-11-08/` passed197default/187neural-disabled
tests, both Clippy modes/fmt/pinned/9HTTP/freshbuild and8inert RUEN/four-DPI auth,
roster, audio and MicTest interactions. Protected neural metrics are unchanged.
Independent430primary+3reference/all24evidence/current+frozen executable+ONNX DLL
checks found0mismatches; exactly4expected existing sources changed and2session
modules added. Final executable SHA-256:
`c74e57e1924fc8d9b01ea7113de64212327e33526286ef0c65d23a98112a0ed0`.
chatFileOwnerWorkChecked=true is narrow component acceptance; remaining Leave
dequeue/teardown/stream hints/blocking-I/O commit/resource/UI/physical/full-client
gates remain open. Worker/audio/call/full-client acceptance staysfalse;
Saved `49fb39b`; post-save Graphify/source/evidence/clean-tree guard passed.
No push/merge/release/shutdown; continue remaining functional/media/UI/profile/import/packaging work.

### Leave during pinned session waits: components checked locally

The current typed driver keeps chat/file futures pinned and services same-owner
Leave separately: voice/media ownership, video/recovery/join/speaking state and
voice audio demand are released without resetting MicTest/runtime/preferences.
Direct Leave uses the same helper. Cleanup hints have250ms StopStream and750ms
EndCall/LeaveChannel budgets, retain unsuccessful hints and are revoked by newer
voice/owner intents. There are no additional cleanup tasks or restarted futures.
Compile-only Clippy passed after removing an async callback lifetime failure.
Development fmt/Clippy and54profile/local tests passed (7intent/3transport/1filejob/
6queueUI/6historicalsession/6leave/25local);432primary+3reference hashes match.
The real local Hub/inert native VoiceSession fixture kept one invocation stalled
through16filled FIFO slots and3repeated priority Leave; first cleanup measured1ms,
independent monitor/240000MicTest samples survived. Blocked StopStream250ms budget
measured258ms and still attempted EndCall. These do not prove physical timing.
Final `native-calls/leave-session-final-2026-11-08/` passed205default/195neural-disabled
tests, both Clippy modes/fmt/pinned/9HTTP/freshbuild and8inert RUEN/four-DPI
auth/roster/audio/MicTest interactions; protected neural metrics unchanged.
Independent432primary+3reference/all24evidence/current+frozen executable+ONNX DLL
audit found0mismatches and verified3expected changed sources+2new leave modules.
Actual/frozen executable SHA-256:
`6c81d59b7a7862a7d667ed7b11f16485f9ac54112b88b452e3db8bb191fd756d`.
leaveDuringSessionWorkChecked=true is narrow component acceptance. Main session
waits/raw stream/ICE/OS-blocking teardown/resource/physical/full-client gates
remain open; workerResponsivenessAccepted, audioIntegrationAccepted,
callIntegrationAccepted and nativeClientReady stayfalse. Saved `a1ac1e0`;
post-save Graphify/source/evidence/clean-tree guard passed; no push/release/shutdown.

### Voice ICE/stream publications: components checked locally

Original voice-ticket/window-stop guards wrap raw ICE, stream-view hints and
voice UI/video delivery. Native peer lifetime retains its atomic alive read and
adds revocation notification for stalled waits; queued peer UI and frame mailbox
recheck lifetime/ticket rather than adopting a fresh owner/voice context.
Session roster/chat/incoming-call events remain Session-scoped and all checked
MicTest/runtime/preferences/DSP/Slint/React behavior is unchanged.
Development fmt/Clippy and63profile/local/media tests passed;434primary+3reference
hashes match. Actual stalled local Hub waits were canceled by Leave/Logout/peer
replacement with no tail runs; native RTP mailbox frames were rejected by original
voice ticket before teardown, only fresh subscription accepted. These are local
synthetic tests, not physical endpoints or all worker waits.
Final `native-calls/voice-publications-final-2026-11-08/` passed211default/201neural-disabled
tests, both Clippy modes/fmt/pinned/9HTTP/freshbuild and8inert RUEN/four-DPI
auth/roster/audio/MicTest interactions; protected neural metrics unchanged.
Independent434primary+3reference/all24evidence/current+frozen executable+ONNX DLL
audit found0mismatches and verified3expected source changes+2new voice modules.
Actual/frozen executable SHA-256:
`1e993163b6674f27a23ba561936714fc2196d7ffb15a67d28c902d98cc2cb3a2`.
voicePublicationGuardsChecked=true is narrow component acceptance. Main session
waits/save races/native resource timing/same-peer reconnect messages without
epochs/physical/full-client gates remain open; workerResponsivenessAccepted,
audioIntegrationAccepted, callIntegrationAccepted and nativeClientReady stayfalse.
Save milestone/post-save Graphify and compact; no push/release/shutdown.

Functional Slint panel/local-meter wiring passed144default and135neural-disabled
tests,409matching source hashes, nine HTTP/pinned-artifact checks, fmt, Clippy and
build. All56native/reference pairs,64settings and24auth captures share the final
tested binary; protected UI/neural regressions are unchanged. Local callbacks,
settings-modal ownership and record/play/pause/seek/retake lifecycle are checked.
Exact font/raster/spring/motion parity and local-only DSP health, physical and
complete audio/client acceptance remain pending. This is a saved-development
milestone, not a published/fully accepted client.

Checkpoint **`005b5f1`** сохранён на `rust/native-client`: functional mic-test
panel/local meter,144/135tests,409matching sources,56panel/64settings/24auth and
8+8+8interactions on finaltestedbinary. All jobs completed; next local-only
DSP-health/meter/explicit retry, then exact font/spring/motion/pixel and remaining
audio/client stages. Не повторять проверенную panel/runtime/core wiring.
Readinessfalse; no push/merge/release/shutdown.

Пять сравнений с неизменённым TypeScript прошли: tonal notches/EQ/deesser,
manual threshold/hold/lookahead/makeup, model-input gain/ALC, manual SmartGate
и voicing/attack/release/preroll на одинаковых VAD-решениях. Четыре теста микшера
проверили per-user/global volume, deafen/focus без повтора backlog, замену/отзыв
peer и ограничения PCM/AEC reverse queues. Ранний foundation checkpoint без neural
features: **35 unit tests и 29 integration tests**, включая прежний file/media/crypto/roster workflow.

Дополнительно прошли два device-free теста SmartGate: invalid PCM/model output
и очистка очередей при mute. Нативный WebRTC VAD mode 2 / hangover 150 мс
совпал с существующим WASM, включая сбросы. Совместная линковка с libwebrtc
выявила общие C-символы; проверенный `webrtc-vad` 0.4.0 сохранён в
`rust/vendor/webrtc-vad-0.4.0` с изолированными `ZaborFvad_` символами.
Алгоритм и публичный `fvad_*` API не менялись; загрузка Git submodule не нужна.
Повторный полный прогон подтвердил совместную линковку VAD и libwebrtc,
WASM-сверку VAD и сохранность всех прежних локальных тестов.

DeepFilterNet на исходной ревизии требует Tract 0.21.4; 0.21.6 уже меняет symbol API,
а более новые версии также меняют ndarray. В `rust/vendor/tract-core-0.21.4`
сохранён проверенный upstream archive с одним исправлением коллизии имён
различных констант. Проверки графа не отключены, веса и вычисления не изменены.
Отдельный временный native harness загрузил исходный DFN3 и прошёл PCM-сверку:
mean `1.8255001080535624e-9`, max `3.4691766e-8`; harness и его файлы удалены.

Silero ранее прошёл reset/gap/invalid-input проверку на Microsoft ONNX Runtime
1.31.1 через ort 2.0.0-rc.15, с telemetry=false. Публичный NuGet runtime проверяется
по SHA-512, а DLL/lib/license cache — по закреплённым SHA-256; differing DLL
заменяются рядом с native executable/test binaries. Подготовка RNNoise переведена
на bounded Range/resume с полной SHA-256-проверкой и очисткой partial files;
девять локальных HTTP-тестов загрузчика прошли, включая ETag смену, неверную длину,
encoded ranges, простой и сохранность существующего файла. Большие ответы Xiph обрываются;
одна 8 KiB-докачка исчерпала лимит после 466944 из 22270507 байт, partial file
удалён. В следующем ограниченном проходе используется IPv4-first, новые HTTP
соединения и backoff. Полный default-feature build и waveform проверка RNNoise
пока не подтверждены. Стандартные nnnoiseless-веса
не заменяют веса существующего клиента.

Дополнительная проверка компиляции с `rnnoise2/runtime-model` (только диагностика,
не замена статических весов и не waveform-приёмка RNNoise) выявила отсутствующий
libclang для bindgen. `prepare-libclang.cjs` подготовил libclang 18.1.1 из
проверенного Windows x64 wheel в `rust/target/libclang-win-x64-18.1.1`;
DLL и лицензия закреплены SHA-256, системная установка LLVM не выполнялась.
После подготовки libclang diagnostic `cargo check` прошёл. Targeted DFN3 test
в этом diagnostic runtime-model режиме не запустился: RNNoise runtime-model
linking требует отсутствующий `init_rnnoise`. Это не результат DFN3 waveform
и не подтверждение готовности RNNoise; полная статическая сборка по-прежнему
требует исходный archive. Отдельная ранее выполненная DFN3 PCM-сверка остаётся
действительной. Проверка исходных vendor archives подтвердила ровно две
локализованные правки: `tract-core/src/model/typed.rs` и `webrtc-vad/build.rs`.
Граф кода обновлён: 4627 узлов и 10976 связей; шесть JSON/metadata файлов
не дали AST-узлов. Семантическое содержимое юридических документов этим
code-only прогоном не извлекалось.

Состав зависимостей и статус незавершённой аудио-интеграции синхронизированы
в LICENSE, THIRD-PARTY-NOTICES.md, TRADEMARK.md, PRIVACY.md, TERMS.md,
ACCEPTABLE-USE.md и docs/chat-data-flow.md. Лицензия package.json остаётся
GPL-3.0-only; Electron-функциональность не менялась. Нативного установщика,
публикации `3.8.0-rust`, push/merge или завершения Windows не было.

После compact exact RNNoise archive получен из content-addressed NixOS cache:
22270507 bytes, исходный SHA-256 подтверждён. Загрузчик сохраняет Xiph fallback
и одинаковые hash/length проверки; девять HTTP tests повторно прошли. Первый
static default-feature run слинковался, но RNNoise PCM выявил несовместимость
более позднего C algorithm в rnnoise2-sys с правильными старыми весами (mean
0.04653851915744112/max 0.13982199). Допуски не менялись. `build.rs` локальной
проверенной копии rnnoise2-sys выбирает неизменённые `denoise.c` и `rnn.c` из
RNNoise `2e3c812c62c32b3ac486c3cd4f4894e6f57d45fd` — фактического submodule
существующего WASM-пакета. Исходники/лицензии сохранены, provenance и хеши
записаны в `rust/vendor/rnnoise2-sys-0.1.2/ZABOR-PATCH.md`. Прогон с этими
sources прошёл 6/6 neural tests без изменения допусков: RNNoise PCM mean
`0.00018198545129978868`, max `0.001667805`; DFN3 mean
`1.8255001080535624e-9`, max `3.4691766e-8`. Подтверждены source/model hashes,
Tract regression и Silero tests. Это не full audio/client acceptance.

Финальный default-feature прогон всех native tests, fmt/Clippy/fresh build
**прошёл** через `scripts/native/verify-audio.cjs`: 35 unit + 35 integration tests,
девять HTTP downloader tests и verification pinned artifacts. Логи восьми шагов,
387 source hashes и actual neural metrics сохранены в
`migration-artifacts/native-audio/neural-2026-11-07/verification.json`.
Source-freeze gate и дополнительная сверка бинарного SHA-256 прошли. Tract
сохраняет 90 upstream warnings; harness считает 91 diagnostic line вместе
с итоговой cargo summary. Native Clippy с `-D warnings` прошёл, vendor cleanup
не выполнялась. Это device-free/local verification, без production services
и визуальной матрицы; `nativeClientReady=false` / `audioIntegrationAccepted=false`.

Проверенный neural checkpoint: `ea46136`, ветка `rust/native-client`; не pushed
или merged, релиз/выключение не выполнялись. Следующий этап — подключение
нового owning-thread mic DSP и bounded playback к voice/media/shell.

Verified neural checkpoint: `ea46136` on `rust/native-client`; no push, merge,
release or shutdown. Next: connect owning-thread microphone DSP and bounded
playback to voice/media/shell.

Новая сверка Silero исполняет исходный TypeScript worker через существующий
onnxruntime-web WASM на 96 frames, включая reset и sequence gap; оба Silero
теста прошли, max probability difference `0.000001013279`. Это не замена
подключению mic/playback, настройкам или полному client acceptance.

После новых vendor/test sources code graph обновлён до 5049 nodes / 11869 edges
/ 247 communities. Семь JSON/metadata файлов не дали AST-узлов. HTML generation
пропущена из-за лимита 5000 nodes; прежний HTML не соответствует новой topology.
Semantic extraction юридических документов этим прогоном не выполнялась.

### Ранее проверенный updater checkpoint

- Rust + Slint 1.18.1, winit и femtovg; нативное окно 420 × 240 логических пикселей. Браузер используется только отдельным инструментом съёмки React-эталона.
- RU/EN каталоги окна, автоматически сверяемые тестом с ключами и значениями `src/renderer/src/locales/{ru,en}.json`.
- Три демонстрационных состояния: скачивание (42%), установка, ошибка проверки beta. Реальные SVG-path Phosphor, размеры, цвета, границы, easing, spinner и shimmer воспроизводят React-эталон.
- Tab/Shift+Tab с обходом доступных действий, Enter, Space на отпускании, повтор, отмена, закрытие и нативное перетаскивание titlebar. После повтора фокус сбрасывается с исчезнувших кнопок ошибки.
- Модель скачивания/установки с идентификатором операции: устаревшие progress/completion/error не меняют новую или закрытую операцию; повтор не переиспользует переполненный счётчик.
- Политика версий с явным каналом, SemVer precedence без учёта build metadata, поддержкой `v`/`V`, запретом downgrade и попадания prerelease в stable. Это чистая модель, а не работающий сетевой updater.
- Воспроизводимый capture/compare harness. React viewport фиксируется отдельно от округлений физического окна Windows; проверяются DPR, размеры PNG и reduced-motion. Временный Electron-профиль удаляется родительским процессом после завершения дочернего.
- CI `rust-migration.yml` включает ветку `rust/native-client`; существующий `test:rust` проверяет весь workspace.

## Запуск и проверка

Требования: Windows, Rust 1.92+ / MSVC и C++ Build Tools. Для эталонных кадров: Node.js 24, npm-зависимости проекта, Python с Pillow. Segoe UI берётся из Windows. Установка с нуля всё ещё относится к рабочему Electron-клиенту, а не этому preview.

```text
cmd.exe /c native.bat
cmd.exe /c native.bat --preview updater --locale en --state error
cmd.exe /c native.bat --preview updater --state installing --reduced-motion
cmd.exe /c native.bat check
```

Альтернативная команда проверки с явным новым каталогом:

```text
cmd.exe /c node scripts/native/verify.cjs --output migration-artifacts/native-ui/my-check
```

Проверка выполняет fmt, clippy `-D warnings`, unit tests и свежую сборку, затем снимает **54 пары кадров**: RU/EN × три состояния; DPI 100/125/150/200% при 550 мс; дополнительные моменты 0/275/1100/1399 мс при 100%; reduced-motion при 550 мс. Восьми прогонами проверяются клавиатура/мышь при обоих языках и четырёх DPI. Действия в preview изменяют только локальную модель и не скачивают/не запускают установщик.

`verification.json` сохраняет хеши исходников, бинарника и кадров; изменение исходников во время прогона запрещает отметку успеха. `comparison/comparison.json` содержит RGB-ошибку целого кадра и отдельных областей. Парные изображения показывают React слева и native справа. Нулевая побайтовая разница не ожидается автоматически: Chromium и femtovg используют разные способы сглаживания текста. Численные показатели не заменяют проверку геометрии, интерактивных состояний и доступности.

Каталоги сравнения — локальные воспроизводимые артефакты под Git ignore. Инструменты не перезаписывают существующие кадры. Незавершённый прогон не создаёт `verification.json` с `automatedChecks: passed`.

### Результаты продолжения 2026-11-06

| Проверка | Результат |
| --- | --- |
| Native fmt, clippy с `-D warnings`, сборка | Пройдены |
| Native unit tests | 12/12 |
| Реальная съёмка React/Slint | 54/54 пары, одинаковые размеры каждого кадра |
| Нативные keyboard/pointer checks | 8/8 прогонов |
| TypeScript typecheck | Пройден |
| Существующие desktop tiers 1–8 | 176/176 |
| Существующие migration/updater tests | 22/22 |

Финальный локальный отчёт: `migration-artifacts/native-ui/verified-native-updater-2026-11-06/verification.json`. Средняя абсолютная RGB-ошибка по кадрам — **1.4164–2.9074 из 255**, доля пикселей с разницей любого канала больше 16 — **1.7292–3.9780%**. Просмотрены парные кадры состояний и масштабов, включая исправленный reduced-motion. Различия сглаживания/метрик текста и растеризации границ остаются; pixel-perfect паритет, доступность со screen reader и перетаскивание между реальными мониторами этим прогоном не подтверждены.

Первый диагностический прогон выявил ложный React-эталон при скрытом окне: viewport/композитор расходились после смены масштаба. Финальный capture использует отдельный offscreen Chromium, CDP viewport, отключение background throttling и ожидание стабильной раскладки перед кадром. Его временные профили очищаются после выхода процесса. Промежуточные артефакты этого продолжения удалены.

## Что ещё не перенесено

| Этап | Работа | Состояние |
| --- | --- | --- |
| N1 | Визуальная и интерактивная основа updater | Preview и автоматическая матрица реализованы; это не install/update acceptance |
| N2 | Общая оболочка, вход/регистрация, главный экран, каналы, друзья, контекстные меню, модалки | Формы, оболочка и основные привязки реализованы; полная UI/behavior parity в работе |
| N3 | Настройки, профили, аватары/GIF, калибровка, все интерактивные состояния/локализации | Есть язык и выбор статического аватара; полные настройки, crop/GIF и калибровка в работе |
| N4 | Auth/SignalR, аккаунты, reconnect, presence, HTTP-контракты | WebSocket/auth/базовый reconnect, roster и friend/channel notification actions реализованы; полное покрытие событий и LongPolling ещё нужны |
| N5 | Нативные capture/playback, WebRTC/codecs, DSP, neural models, stream/SFU, remote control | Mic/playback DSP подключён и проверен локально: 81 default-feature / 74 neural-disabled tests; settings/mic-test/hotplug/physical AEC и full audio acceptance ещё нужны; camera/SFU/control не завершены |
| N6 | Чат/файлы, encryption/key storage, совместимый импорт/откат профилей | DPAPI, SQLite, encrypted fallback/P2P text и файловый обмен соединены; расширенные delivery-retry/sync, import/rollback ещё в работе |
| N7 | Загрузка/проверка native updates, независимый install helper, NSIS, подписи/лицензии, bridge старых beta | Не реализовано; кнопки preview не выполняют эти операции |
| N8 | Полная parity/regression матрица, два устройства, длительные media sessions, CPU/RAM/latency, упаковка `3.8.0-rust` | Не выполнено |
| N9 | Переключение основной папки и release/publish батников, удаление Electron после переноса его функций | Не выполнено |

Текущая работа идёт по N2–N6. Следующие обязательные части — завершение lifecycle звонков, native video/capture, полноценные настройки/neural DSP, P2P/file transport и миграция данных, затем packaging/update. Требование пользователя — сохранить интерфейс и поведение; наличие формы или транспорта отдельно не завершает перенос функции.

## Переход со старых версий

- `3.8.0-rust` остаётся целевой prerelease-версией. Нативного установщика с этой версией пока нет.
- Stable-клиент не должен автоматически получать beta. На первом этапе переход — отдельная явная установка beta.
- Валидатор опубликованной Electron beta определяет beta по первому prerelease-идентификатору `beta`. Поэтому он не примет прямой manifest `3.8.0-rust`, даже при `channel: beta`. Нужен отдельный совместимый bridge-релиз семейства `*-beta.*`, реализующий переход, либо явная установка native beta. Существующий bridge `.1` для WASM `.2` не решает этот новый переход.
- `UpgradeRoute::LegacyBetaBridge` обозначает требуемый маршрут, а не наличие реализованного bridge. Будущий manifest обязан указывать фактическую архитектуру, schema, checksum и совместимый установщик.
- `release.bat`, `release-beta.bat` и `promote-beta.bat` пока обслуживают Electron/WASM-ветки. Публиковать этим pipeline нативный preview под видом полного `3.8.0-rust` нельзя. Местные ignored батники вне worktree не изменялись.
- Backend остаётся .NET, SFU — Go. Полный native перенос требует совместимости с их действующими контрактами.

## English summary

The original session stopped after six native updater captures without a final comparison. Work continues on `rust/native-client`, with `d088795` as the pre-native checkpoint and `d1a33f6` as the verified updater checkpoint. Development now includes a connected native shell, authentication, SignalR/WebSocket, native WebRTC/WASAPI integration, DPAPI/SQLite and encrypted fallback text chat. It is not yet a complete native client or network updater.

Implemented: three updater fixture states, matching RU/EN catalog tests, Phosphor SVGs, animation timing/reduced motion, keyboard traversal/retry/cancel/close, native titlebar dragging, operation-scoped lifecycle guards and channel-aware SemVer policy. Windows DPI viewport rounding is checked explicitly. Temporary Electron profiles are removed after the child exits. The 54-pair matrix covers four DPI scales, five animation times at 100%, reduced motion and eight keyboard/mouse runs. Reports pin source/binary/image hashes; pixel measurements are not a claim of complete interaction or rendering equivalence.

Continuation results: fmt/clippy/build, 12 native unit tests, 54 capture pairs, eight interaction runs, TypeScript checking, 176 existing desktop tests and 22 migration/updater tests passed. Whole-frame mean absolute RGB error is 1.4164–2.9074/255; 1.7292–3.9780% of pixels exceed a 16-level channel difference. Text/edge rasterization differences remain; screen-reader behavior and real multi-monitor dragging are not verified. The final reference uses offscreen Chromium/CDP and stable layout checks to avoid stale hidden-window compositor frames.

The core passed 33 unit tests and 17 integration tests. File handling is now connected end-to-end: picker/drop, confirmation, background staging, metadata/request, DTLS/SCTP chunks, progress/cancel/retry, SHA-256, atomic saving and Explorer reveal. A local test transfers and saves 2 MiB over real SCTP; cancellation, corrupt/stalled transfers, friend revocation, deletion replay, interrupted recovery and preservation of read receipts/source files are covered. Roster, notification actions and peer/video lifetime checks remain covered. Remaining: full UI/settings/secondary-event parity; long-poll fallback, robust message retry/sync and full call/stream/reconnect acceptance; neural DSP, camera/system audio/SFU/control; profile import/rollback; installer/updater/legacy bridge; two-device, long-session and resource acceptance; final packaging. `3.8.0-rust` remains a target. Existing beta validators require a `beta` prefix and need a compatible bridge or explicit installation. .NET backend and Go SFU remain the compatibility targets.

Neural checkpoint history: the exact 22270507-byte RNNoise archive was verified from the NixOS content-addressed cache, retaining Xiph as a hash-checked fallback. A static default-feature build linked, but RNNoise PCM failed because the binding crate's later C output-layer architecture differs from the existing WASM revision. A verified local crate selects two unchanged source files from the actual WASM submodule `2e3c812c62c32b3ac486c3cd4f4894e6f57d45fd`. Neural tests passed without relaxed tolerances: RNNoise PCM mean 0.00018198545129978868/max 0.001667805; DFN3 mean 1.8255001080535624e-9/max 3.4691766e-8; source/model hashes and the Tract regression passed. Silero matched the unchanged TypeScript/WASM worker (maximum probability error 0.000001013279, resets and sequence gaps covered). The initial neural checkpoint passed 35 unit + 35 integration tests; eight logs and 387 hashes are preserved under `neural-2026-11-07`. The subsequent microphone/playback wiring passed 36 unit + 45 integration tests, nine HTTP tests, pinned artifacts, fmt/Clippy/build and source freeze under `wiring-verified-2026-11-07`, with 392 source hashes and the matching binary hash. Neural-disabled compatibility passed 74 tests. Code wiring is complete, but full audio/client acceptance is not; readiness flags remain false. Tract retains 90 upstream warnings (91 diagnostic lines including its Cargo summary). The current graph has 5131 nodes/12101 edges; seven metadata files produced no AST nodes, legal-document semantics were not extracted, and HTML generation was skipped above its 5000-node limit.
