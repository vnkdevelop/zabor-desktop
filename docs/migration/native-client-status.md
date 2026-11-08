# Нативный клиент Rust / native Rust client

Редакция / updated: 2026-11-08.

## Решения владельца для полного переноса

Подтверждено 2026-11-06: целевые ОС — Windows 10/11 x64. Приложение должно быть нативным Rust без Electron/WebView; сторонние нативные C/C++ медиабиблиотеки (включая libwebrtc и Opus) допустимы. Интерфейс и поведение сверяются с действующим React-клиентом. Beta использует отдельный профиль с импортом-копированием и сохранением исходного профиля. В автономном проходе разрешены только локальные интеграционные проверки. Этапы разрешено фиксировать коммитами в `rust/native-client`; после готовности полноценного клиента — push ветки и публикация GitHub beta `3.8.0-rust`. Автоматическое слияние в main не согласовано. Backend остаётся .NET, SFU — Go.

Owner decisions confirmed on 2026-11-06: target Windows 10/11 x64; a native Rust application without Electron/WebView, with native C/C++ media libraries allowed. Preserve the React client's UI and behavior. Use a separate beta profile with copy-based import that preserves the source. Unattended integration checks are local only. Checkpoint commits on `rust/native-client` are authorized; push and the `3.8.0-rust` GitHub beta release follow completion of the full client. Automatic merging to main is not authorized. The backend remains .NET and SFU remains Go.

Контекст для compact и последующего продолжения: [native-client-handoff.md](native-client-handoff.md). Он сохраняет текущие ограничения, checkpoints, блокеры и весь оставшийся план; compact не означает завершения переноса.

Compaction/continuation context: [native-client-handoff.md](native-client-handoff.md). It preserves constraints, checkpoints, blockers and the complete remaining plan; compaction does not imply migration completion.

## Где остановилась исходная сессия

Сохранённый этап **`0dcfef75fa2dfcd35cac38b0724c27b412682445`** после `5b8d656`/`e3cc8cd`: миниатюры desktop/window/camera,
два столбца источников, явное исключение собственного HWND и обновление раз в пять
секунд с сохранением живого выбора. Прошли 130 затронутых тестов, normal/fixture
Clippy, native build и расширенные RU/EN inert проверки. Старые миниатюры очищаются
при ошибке превью/закрытии выбора/retirement/exit. Это не готовность клиента.
Исходный зарегистрированный native actor и Voice/query tickets
сохраняются через отмену, UI не ждёт весь preview pass. Pinned SDK уже исключает окна
своего процесса; клиент добавляет точный HWND guard. Подробности и оставшиеся границы:
[native-picker-thumbnails.md](native-picker-thumbnails.md).
Graphify: 6502 nodes/16134 edges/283 communities; прежние семь zero-node warnings
и HTML limit 5000 остаются. Следующий блок — adaptive bitrate/viewer interest/SFU/
remote control. Physical/full-client acceptance, публикация и shutdown не выполнены.

После `5e4430b` проверены функциональные компоненты adaptive bitrate/viewer interest/
SFU: 153 уникальных затронутых теста, fmt, normal/fixture Clippy, native build и RU/EN
inert interactions. Отдельный SFU publisher несёт video/share audio без микрофона;
subscriber receive-only, переключение после декодированного кадра, snapshots и
P2P fallback сохраняют Voice/peer/frame ownership. Исправлен порядок late P2P/SFU
callbacks. Источник сохранён **`2d254a2243dd478bc0f33c2fb0da9648dedeb16c`**;
Graphify 6645 nodes/16503 edges/290 communities,9 native paths/13 checked hashes.
RC input/consent/UI ещё не
перенесены. Подробности: [native-stream-delivery.md](native-stream-delivery.md).

После `b4b52bc` проверены функциональные компоненты native RC: standalone `zabor-input`,
ordered negotiated `zabor-rc-v1`/4243, явное согласие/один контроллер/ACK v2 и v1,
original Voice/peer/channel/capture leases, registered input cleanup, реальная
геометрия кадров и Slint viewer/revoke controls. Прошли 179 уникальных затронутых
тестов, fmt, оба режима Clippy, normal build и RU/EN inert interactions. Источник
сохранён **`2108f866ce719a210bab679c7b81192094c20445`**, Graphify 6870 nodes/
17027 edges/289 communities;23nativepaths/24checkedhashes. Это не физическая или whole-client приёмка; topmost
host pill/cursor capture/IME/latency parity и прежние ownership/data/release gates
остаются открыты. Подробности: [native-remote-control.md](native-remote-control.md).

После `3ba1ad8` сохранён native profile/avatar/GIF этап
**`f264f3cde6e5784f82a5fa16fa131af87475911d`**: просмотр/редактирование собственного
профиля, crop 200px/PNG 400px, исходный packed GIF, bounded native animation,
20/150 UTF-16 и исходные Session/query/owner guards. Прошли 134 разных затронутых
теста, fmt, оба режима Clippy, normal build и строгие RU/EN inert pointer/keyboard
проверки. 8 MiB record bound проверен локальным near-5-MiB GIF roundtrip; настоящие
.NET/proxy и aggregate roster limits этим не подтверждены. Graphify:7002 nodes/
17423 edges/298 communities,7 прежних zero-node warnings/HTML5000 limit остаются.
Подробности: [native-profile-avatar.md](native-profile-avatar.md). Fresh peer lookup,
login-copy/full profile actions, интеграция copy-import/updater/RC helpers и все
отложенные physical/full-client/visual/release gates ещё впереди. Готовность полного
клиента=false; публикация и выключение компьютера не выполнялись.

В основной ветке проверена интеграция base упаковки/обновления и copy-import:
**`f27950ea21d6c5f5efd5a1999402ea90cc58e897`**. Прошли94 разных локальных теста
(64 Rust/17 Python/13 Node), fmt, оба native-media Clippy режима и normal build.
Graphify7135nodes/17914edges/306communities; прежние7warnings/HTML5000limit остаются.
Временные export fixtures/runner удалены. Source/pin hashes не менялись во время
прогона. Это не подтверждает helper/UI/import account-proof, реальный Chromium,
durable whole-app update/drain, лицензии/подпись издателя или готовность установщика.
Публикация/выключение по-прежнему запрещены до полной приёмки.

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
Saved `3d69683`; post-save Graphify/source/evidence/clean-tree guard passed;
no push/release/shutdown.

### Main session waits and guarded owner commits: components checked locally

Pure Session computation frees VoiceLeave while auth/modal/notification waits
remain pinned. Connection/relogin keep owner work; final command scope checks
reject old voice commands. Synchronous commit gates serialize with Logout;
DPAPI staged preparation runs outside that gate and abandoned output cleans itself.
Development fmt/Clippy/78tests passed;436primary+3reference hashes match.
13actual stalled protocol Leave/Logout matrix and staged/in-flight DPAPI cleanup
passed. Compatible late resume selector
now uses original voice ticket after CheckUserExists, avoiding post-Leave resume
while preserving the same auth future. Initial failure was an invalid32hex test
session ID, production validation retained.
Final `native-calls/main-session-final-2026-11-08/` passed225default/215neural-disabled
fulltests, both Clippy modes/fmt/pinned/9HTTP/neural/freshbuild and8inert RUEN/DPI
auth/roster/audio/MicTest interactions; protected neural metrics unchanged.
Independent436primary+3reference/all24evidence/current+frozen executable+ONNX DLL
audit found0mismatches, verified5expected source changes+2new main modules and
13protocol/late-resume/profile-owner/temporary-cleanup proofs in both modes.
Actual/frozen executable SHA-256:
`11306b6ed4a73cb800dce4d72b66101c129734b32070b0ede40d35fd2a7bb8f2`.
mainSessionWorkChecked/guardedSessionCommitChecked=true are narrow component
acceptance. Final filesystem/critical-lock commit bounds, chat/file final-save
races, native OS resource timing and full worker/audio/call/client gates remain
unaccepted. workerResponsivenessAccepted/audioIntegrationAccepted/
callIntegrationAccepted/nativeClientReady stayfalse. Saved `457552c3d115bed4e50f26de9f9d2ce781376863`;
post-save Graphify/source/evidence/clean-tree guard passed, compact completed;
no push/release/shutdown.

### Session filesystem commits: components checked locally

Session-only Profile resource serialization and tracked off-worker startup/save/
clear jobs remove filesystem I/O from production intent-state commit closures.
A provisional save is accepted by its original owner/window guard or rolled back
to encrypted previous bytes; revoked owner cleanup clears credentials without
changing identity/settings. Readers and next-owner saves use the same resource
gate; application close drains registered jobs, including DPAPI preparation.
Initial development stopped on a new test's Receiver Send/Sync error; ownership
fixture corrected.438-source v2development passed, including5profile +6SessionIo
local cancellation/rollback/lock/sharing tests. Explicit Logout clear registration
survives an unpolled future, old EndCall does not await disk, legacy invalid-session
startup fallback preserved. v2development89tests/fmt/Clippy passed,438/3hashes match.
Late rollback Logout recheck and removal-on-restore-failure added; ciphertext-only
pending-session journal blocks auto-login after reopen if native rollback/removal
both fail. Original encrypted backup retained, current session.enc DPAPI format
unchanged. v3development91tests/fmt/Clippy passed; independent438/3hashes/counts match.
Final `native-calls/session-filesystem-final-2026-11-08/` passed238default/
228neural-disabled tests, both Clippy modes/fmt/pinned/9HTTP/neural/freshbuild/
8inert RUEN/DPI interactions; protected neural metrics unchanged. Independent
438primary+3reference/all24evidence/current+frozen executable+ONNX DLL audit found
0mismatches; verified2existing changes+2new modules and10profile/6SessionIo/
10main-session tests plus retained voice/Leave/protocol controls in both modes.
Actual/frozen executable SHA-256:
`407bcd45195fc919fcaaf4d60d76d43f69d9e64990f6d104c442610db0ae195f`.
sessionFilesystemWorkChecked=true is narrow component acceptance only.
Saved `03b151364b1d94f420fe6d971736cd07a3a69948`; post-save Graphify/
438-source/3reference/24evidence/binary/clean-tree guard passed, compact completed;
no push/release/shutdown.
Final native syscall/resource-lock duration, audio restoration gate and whole
worker/audio/call/client readiness remainunaccepted.

### Audio restoration resource admission: components checked locally

Preference/control settings preparation now tries both resource mutexes before
the strict owner commit; same legacy merge/dirty-owner/device semantics retained.
Prepared application uses swaps/atomics with no new resource acquisition under
the intent gate. Notifications, replaced-token cancellation and destructors run
after gate exit. Both production login/resume paths wait cooperatively with the
original Session ticket; repeated Leave does not restart, Logout/window abandons
without detached restoration. UI Logout submits priority before existing cleanup.
Initial development stopped on callback/VoiceLeave mutable-borrow conflicts.
Restoration now commits preferences/controls only; downstream bootstrap acceptance
separately rechecks the same original ticket after both resource guards release.
Added explicit between-stage Logout/next-owner regression; no check-then-mutate
or stale reticketing.438-source v2development103tests/fmt/Clippy passed;
independent438/3/freeze/exact suite counts match.13main/9preferences include6new
restoration/resource/owner/cancellation/legacy-save regressions.
Final `native-calls/audio-restore-final-2026-11-08/` passed244default/
234neural-disabled tests, both Clippy modes/fmt/pinned/9HTTP/neural/freshbuild/
8inert RUEN/four-DPI interactions; protected neural metrics unchanged.
Independent438primary+3reference/all24evidence/current+frozen executable+ONNX
audit found0mismatches and exactly4existing changes/no added modules.
Verified13main/9preferences/10profile/6SessionIo proof suites and all retained
13stalled protocol/late-resume/original-ticket RTP/voice/Leave controls both modes.
Actual/frozen executable SHA-256:
`561f80f11e6d21bb134ef145d9170f99904710b299a4e0c24ef35116b5d5e217`.
audioRestorationWorkChecked=true is narrow component acceptance only;
saved23fb7141d3c723f9ee47bf0d80a99d5afdaf6906/post-save Graphify/438-source/
3reference/24evidence/current+frozen binary/clean-tree guard passed, compact done;
no push/release/shutdown.
Failed-auth/runtime cleanup and resume
ChatEngine::set_friends storage work still have external waits under intent gate;
actual UI/OS/worker/client bounds and readiness remainfalse. No new acceptance.

### Failed-auth audio reset intent gate: components checked locally

Preference/demand/MicTest/runtime preparation tries all resource locks before
the original-ticket intent commit. Application uses only extraction/swaps/atomics;
pending factory has a revocation marker before deferred cancellation and remains
quarantined until its owning startup returns. Deferred old-token/watch/device
cleanup runs after intent/resource guards release. Worker-state acceptance uses
a second strict commit with the same ticket, extracting old objects for teardown
outside gate. Contention wait is pinned and Leave-aware, Logout/window-cancelable.
438-source development108tests/fmt/Clippy passed; independent438/3/freeze/exact
counts match.7new resource/cancel/owner/clip/single-start/legacy preference tests
passed. Preserved legacy partial cleanup for individual poisoned resource locks
without clearing poison, returning explicit incompletefalse; added2poison tests.
Corrected v2development110tests passed; independent438/3/freeze/exact suite counts
match. All four poison cases retained legacy partial cleanup/poison/false-result.
Final `native-calls/audio-reset-final-2026-11-08/` passed253default/243neural-disabled
tests, both Clippy modes/fmt/pinned/9HTTP/neural/freshbuild and8inert RUEN/four-DPI
interactions; protected neural metrics unchanged. Independent438primary+3reference/
all24evidence/current+frozen executable+ONNX audit found0mismatches/exact7existing
changes/no added modules.9new tests and16main/10preferences/6runtime/3MicTest/
10profile/6SessionIo suites plus13stalled protocol/late-resume/original-ticket
RTP/voice/Leave controls verified in both modes. Executable SHA-256:
`c98d1f357c56568041318d68a35bd2362fa3303023efa61a79b6c7407e1c6ab3`.
audioResetIntentWorkChecked=true is narrow component acceptance only;
saved5aa25a0192a008cb2c50d7e69eca89e7c846265c/post-save Graphify/438-source/
3reference/24evidence/current+frozen binary/clean-tree guard passed, compact done;
no push/release/shutdown.
Normal worker/
UI/native synchronous cleanup, chat-storage/actualOS/fullclient bounds remainfalse.

### Logout cleanup admission: components checked locally

Normal Logout and failed-auth register original-ticket SessionIo.clear before
resource reset/preparation/retry and old-object destruction. Registration stays
synchronous/tracked, while Profile final-clear acceptance stays owner-guarded
outside intent gate and durable across unpolled window exit; failure latch/drain
unchanged. Captured old-connection StopStream/EndCall obligations survive missing
live Voice, using250/750ms budgets and window cancellation. Dropped polled cleanup
closes the captured connection including its live clones; no detached sends.
Initial development fmt/Clippy/56prefix tests passed, main17passed/1failed on a
new fixture's incorrect current-monitor assertion after global window stop.
All10Leave tests and actual normal-worker clear-before-held-cleanup passed.
Corrected only test expectation; window properly revokes monitor through shared
stop, while canceled reset must preserve demand/MicTest state. Corrected438-source
development116tests/fmt/Clippy passed; independent438/3/freeze/exact suite counts
match, all6new actual-worker/admission/DPAPI/protocol/deadline/cancellation tests
passed. Final `native-calls/logout-admission-final-2026-11-08/` passed259default/
249neural-disabled tests,both Clippy modes/fmt/pinned/9HTTP/neural/freshbuild/8inert
RUEN/four-DPI interactions; protected neural metrics unchanged. Independent438/
3/all24evidence/current+frozen executable+ONNX audit found0mismatches,exact4existing
source changes/zero added modules. All6new tests/10Leave+18main+6SessionIo+
10preferences+10Profile+6runtime+3MicTest suites and previous253/243proof tests
retained in both modes, including13stalled protocol/original-ticket controls.
Executable SHA-256:
`a26533a9a9c7b4844ffc7bb3e6db1f3b5f61f22fdec3a6b5c3d3c2595dd8b8c7`.
logoutCleanupAdmissionChecked=true is narrow component acceptance only;
saved02b7c32a35d6550b6bfe245cf6efb6bb8dc94156/post-save Graphify/438-source/
3reference/24evidence/current+frozen binary/clean-tree guard passed, compact done;
no push/release/shutdown.
Native synchronous cleanup can
still block normal worker/UI and reaching hints; syscall/job-lock/drain/native
endpoint/fullclient completion bounds remainfalse; no full-client acceptance.

### Normal worker Logout intent work: components checked locally

Normal Logout now reuses nonwaiting resource preparation and pinned Leave-aware
reset waiting; Logout/window cancellation rejects the original Session ticket.
Separate downstream worker extraction uses the same strict ticket, retaining
pending/live StopStream and abandoned/live EndCall obligations across Leave.
Old-object storage/native teardown stays outside intent gate but synchronous;
VoiceEnded/SignedOut/partial-poison failure now owner-guarded. Existing durable
clear registration remains first, tracked through unpolled canceled window exit.
Two new actual-worker repeatedLeave/16staleVoiceFIFO/replacement/window and
partial-poison tests; existing actual window proof strengthened to exit before
held preference reset released, preserving canceled demand/MicTest state.
Initial fmt/Clippy/56prefix tests passed,main19passed/1failed on new fixture's
incorrect video-generation count after independent normal worker shutdown.
Window/no late signout/preserved MicTest proof passed; count corrected and
accepted cleanup count checked separately before/after shutdown. Exact legacy
pre-chat/FileJobs/chat/post-chat destructor order restored outside intent gate.
Corrected438-source development118tests/fmt/Clippy passed; independent438/3/freeze/
exact12suite counts match, all20main/10Leave/6SessionIo and new actual-worker
repeatedLeave/window/replacement/partialpoison controls passed. Final
`native-calls/normal-logout-final-2026-11-08/` passed261default/251neural-disabled
tests,both Clippy modes/fmt/pinned/9HTTP/neural/freshbuild/8inert RUEN/four-DPI
interactions; protected neural metrics unchanged. Independent438/3/all24evidence/
current+frozen executable+ONNX audit found0mismatches,exact2existing source changes/
zero added modules. All2new actual-worker tests/20main+10Leave+6SessionIo+
10preferences+10Profile+6runtime+3MicTest suites and previous259/249full proof
names retained in both modes, including13stalled protocol/original-ticket controls.
Source-owner/strict extraction/guarded publications/legacy destructor order audit
passed. Executable SHA-256:
`14b0f8a9168042920eba9bd0ae08957ed7e2132cc815e5d99b570e1c3a439a82`.
normalLogoutIntentWorkChecked=true is narrow component acceptance only;
saved0315f56dbeb8bee7980eb57a75c12108f6922e32/post-save Graphify/438-source/
3reference/24evidence/current+frozen binary/clean-tree guard passed, compact done;
no push/release/shutdown.
Shared Voice/lease destructor mutations
must not be deferred onto a new owner's runtime without additional guards.
Native/UI/worker/resource/syscall/drain/full-client bounds remainfalse.

### Stale Voice audio cleanup: components checked locally

Windows VoiceSession clears network status only through its original VoiceLease
generation under the AudioDemand mutex; unstarted or stale objects cannot clear
fresh-owner metrics. Known-stale VoiceLease drop skips shared maintain/Notify;
current and unknown-poisoned cleanup retain synchronous legacy maintenance, public revoke API
and status/playback/lease drop order. Three new tests cover fresh-owner held
MicTest/runtime locks, current/poisoned cleanup and actual unstarted VoiceSession
stop/drop. Final `native-calls/stale-voice-cleanup-final-2026-11-08/` passed
264default/254neural-disabled tests,both Clippy modes/fmt/pinned/9HTTP/neural/
freshbuild/8inert RUEN/four-DPI interactions; neural metrics unchanged.
Independent438/3/24evidence/current+frozen executable+ONNX audit found0mismatches;
2existing sources/zero new files, all prior261/251full test names retained and
new3tests/runtime8+voice1/20main/10Leave/6SessionIo/10preferences/10Profile/3MicTest
passed in both modes. Source text outside approved functions/tests unchanged;
generation guard/poison/public revoke API/legacy drop order audited.
Executable SHA-256:
`86d91dfd719b3d5a579620bc2ecc1f171e93c25ff3fa0585154c476f5dee86a8`.
staleVoiceAudioCleanupChecked=true is narrow known-stale component acceptance;
savedb78527771433c98fd38b265e05df50bbff8c7f2c/post-save Graphify/438/3/24/
current+frozen binary/proof/clean-tree guard passed, compact done; no publication.
Unknown poisoned ownership/general cleanup races and blocking voice/resource/native/
notification/destructor cleanup still require prepared detachment and tracked
completion. No off-thread cleanup introduced or native responsiveness claimed.

### Healthy current Voice post-revoke maintenance: components checked locally

Current VoiceLease cleanup prepares runtime/MicTest guards and native-health
snapshot before the short voice mutex, then rechecks original revoked Voice and
captured monitor generation before any timing/failure/runtime/pending mutation.
Accepted pending cancellation quarantines before deferred token/watch/native work;
all cancellation/destruction/Notify follows resource+voice guard release.
Prepared MicTest time/failure replacement retains retired buffers until outside
that gate. Known-stale skip/public revoke/status/drop order and unknown-poison
legacy synchronous behavior retained; general runtime/monitor/worker/UI unchanged.
Final `native-calls/voice-maintenance-final-2026-11-08/` passed269default/
259neural-disabled tests,both Clippy/fmt/pinned/9HTTP/neural/freshbuild/8inert
RUEN/four-DPI interactions; protected neural metrics unchanged. Independent
438primary/3reference/24evidence/current+frozen executable+ONNX audit found0mismatches;
all prior264/254full test names retained with unchanged multiplicity,new5tests/
runtime12/MicTest4/voice1/main20/Leave10/SessionIo6/preferences10/Profile10 passed.
Each mode verified32new resource/replacement/native-health/quarantine/partial-poison/
MicTest timer/failure/abandoned/idempotent/deferred-buffer evidence markers.
Exact source scope/generation/resource-release/legacy drop-order audit passed.
Executable SHA-256:
`7f0e4fcb76ec07e2572cfc358ee9c46ff48cc3eb5b47e02636bf9d06fd190db5`.
voiceMaintenanceOwnershipChecked=true and healthyCurrentRevokeMaintenanceRaceChecked=true
are narrow healthy-path component acceptance;saved1245d6b7934d1a719241da69fb007e1241d179f6/
post-save Graphify/438/3/24/binary/proof/clean-tree guard passed,compact done.
Resource/health/unknown-poison/general-maintenance/token/watch/destructor waits and
actual native cleanup completion remain open. No off-thread/worker/native/full-client
readiness claim; no push/release/merge/shutdown.

### Native owning-thread join and registered cleanup: components checked locally

DeviceStream shared/idempotent join retains owning thread handles and sticky panic/
poison failure;lock-free completion health does not wait for the join mutex.
NativeAudio joins capture and render independently even after one error with live
device/source/mixer Arcs. SharedAudio registers retirement before synchronous cancel,
tracks finish_shutdown and owned-Arc destruction plus all scheduled factories through
late rejected-device retirement;unsupported completion/errors/panics stay failures.
Wait cancellation leaves jobs tracked;multiple waiters register Notify before zero
check. Shell close waits registered native work before final runtime disposal.
Final `native-calls/native-cleanup-final-2026-11-08/` passed277default/
267neural-disabled tests,both Clippy/fmt/pinned/9HTTP/neural/freshbuild/8inert
RUEN/four-DPI interactions;protected neural metrics unchanged. Independent
438primary/3reference/24evidence/current+frozen executable+ONNX audit found0mismatches;
all prior269/259full test names retained with unchanged multiplicity,new8tests/
runtime16/device13/native1/MicTest4/voice1/main20/Leave10/SessionIo6/preferences10/
Profile10 suites and original owner/protocol/quarantine/partial-poison controls passed.
19new markers and32retained Voice/MicTest maintenance markers verified in each mode;
exact source scope/lifecycle registration/owned-Drop/legacy owner-ticket order passed.
Executable SHA-256:
`c7d1d39ed311805f5c47a4fc2404ab4442d0d74f0e35022c92017b514a3843e6`.
nativeAudioThreadJoinChecked=true/registeredAudioCleanupChecked=true are narrow
component acceptance;saved5255b3b61f48d48f7ad4da62a5c31492084e94ba/post-save
Graphify/438/3/24/binary/proof/clean-tree guard passed,compact done.
No sealed future admission/external last-Arc source/RTC/physical endpoint completion
or native responsiveness accepted;normal worker/UI/Leave/disconnect/cancel/resource/
unknown-poison/general callback/native/system/filesystem/drain gates remain open.

### Playback/native-stream observed task cleanup: components checked locally

SharedAudio cleanup registers playback tasks before spawn and observes JoinHandle
completion after original future/NativeAudioStream destruction,including requested
abort. Unrequested cancellation/body/Drop panic/unobserved runtime shutdown remains
sticky failure. Subscription retains unchanged cancel/lane/abort order via AbortHandle;
standalone AudioPlayback has wait_for_cleanup and Windows VoiceSession connects it
to existing shared final audio drain. No new worker/peer/RTP ticket mutations.
Final `native-calls/playback-cleanup-final-2026-11-08/` passed282default/
272neural-disabled tests,both Clippy/fmt/pinned/9HTTP/neural/freshbuild/8inert
RUEN/four-DPI interactions;protected neural metrics unchanged. Independent438primary/
3reference/24evidence/current+frozen executable+ONNX audit found0mismatches;all prior
277/267full names retained with unchanged multiplicity,new5tests/runtime19/playback2/
device13/native1/MicTest4/voice1/main20/Leave10/SessionIo6/preferences10/Profile10
suites and original peer-ticket/RTP/quarantine/partial-poison/protocol gates passed.
15new markers and19native-cleanup/32Voice-MicTest markers verified in each mode;
exact source/lifecycle/legacy playback/Voice/drop ordering and SDK hashes matched.
Executable SHA-256:
`30245c3d8b4c71afd8553848d4a41bc17c7737723cad4e082396a2657587b56f`.
playbackTaskJoinChecked=true is narrow observed-task cleanup acceptance;
saved dc4344c9983a0e0c3b7eef955a02bc36a454eb03/post-save Graphify/438/3/24/
binary/dependency/proof/clean-tree guard passed.
No generalized off-thread teardown/native destructor/physical endpoint/UI/worker
bounds or sealed future admission/external last-Arc/full-client acceptance.

### Sealed audio-cleanup root admission: components checked locally

Final shell closes audio-cleanup root admission after producer joins/SessionIo.finish,
then drains admitted jobs even on seal/supervisor error. New roots and seal linearize
on a short mutex without native callbacks/await;poison stays poison. Playback registers
preparation before old-lane/native-stream mutation;after seal attach rejects without
touching the current subscription/Voice/monitor controls. Previously admitted
preparations/factories keep drain open and may register descendant retirement after
seal before parent release. Late unaffiliated retirement is never deferred:sticky
failure plus same-thread cancellation/finish/owned disposal. Legacy task-join/
Subscription/Voice/peer/worker/RTP/Logout order unchanged.
Final `native-calls/sealed-cleanup-final-2026-11-08/` passed287default/
277neural-disabled tests,both Clippy/fmt/pinned/9HTTP/neural/freshbuild/8inert
RUEN/four-DPI interactions;protected neural metrics unchanged. Independent438primary/
3reference/24evidence/current+frozen executable+ONNX/SDK audit found0mismatches;
all prior282/272full names retained with unchanged multiplicity,new5tests/runtime23/
playback3/device13/native1/MicTest4/voice1/main20/Leave10/SessionIo6/preferences10/
Profile10 and original-ticket/RTP/quarantine/partial-poison/protocol controls passed.
9new and66retained playback/native-cleanup/Voice-MicTest markers verified per mode;
exact source/lifecycle/order audit passed. Original Voice wait stdout diagnostic
interleave normalized only for pass-name parsing;both Leave/Logout tail=0 checked.
Executable SHA-256:
`13dbfec2475a869c3f1dbe8c8b0312b6ff7503b6703d6b071a442f6c28edab69`.
sealedAudioCleanupAdmissionChecked=true is narrow background-audio root acceptance;
saved1edab768492097d7941e9649365f411d4e7e3c7d/post-save Graphify/438/3/24/
binary/dependency/proof/clean-tree guard passed.
This is not sealed admission for all operations/external last-Arc/physical native
completion or generalized off-thread Voice/peer/worker/UI bounds;full readiness=false.

### Original-ticket worker Voice extraction: components checked locally

VoiceWork.commit retains the original CriticalIntents mutation mutex and rechecks
window stop inside it. take_voice compares original stored resource ticket under
the gate,keeps owned Voice/media receiver and FnMut metadata capture outside its
closure,and returns ownership for Voice-before-receiver destruction outside the
gate. Four CallEnded/ForceLeaveVoice/audio-failure/rejoin-error sites use it;
successful audio-failure leave flag acceptance also rechecks the original ticket.
Normal Logout/Leave/peer/RTP/native-audio/shared seal/Voice Drop order unchanged.
Four new tests cover ready-result rejection/strict gate/held-gate stop plus actual
configured native session/receiver extraction and callback-capture Drop/unwind.
Development58native-bin tests/Clippy/Graphify passed;source audit passed.
Final `native-calls/voice-extraction-final-2026-11-08/` passed291default/
281neural-disabled tests,both Clippy/fmt/pinned/9HTTP/neural/freshbuild/8inert
RUEN/four-DPI interactions;protected neural metrics unchanged. Independent438primary/
3reference/24evidence/current+frozen executable+ONNX/SDK audit found0mismatches;
all prior287/277names/multiplicity retained,new4tests and main22/VoiceWork8/Leave10/
SessionIo6/preferences10/Profile10/runtime23/playback3/device13/native1/MicTest4/
VoiceSession1 suites passed. 18new and75retained component markers per mode plus
owner/RTP/quarantine/partial-poison/protocol/actual-worker controls verified.
Exact source reconstruction/gate/original-ticket/owned disposal order audited again.
Executable SHA-256:
`b466d6892dc902784b063c6e77903b8853a3b74561651fad2caa11a175a74229`.
voiceResourceExtractionChecked=true is narrow four-site extraction acceptance;
saved2e5921295b30b78840ceb0901b766bd6ccf1122d/post-save Graphify/438/3/24/
current+frozen executable+ONNX/dependency/proof/clean-tree guard passed.
Temporary helpers deleted.
Acquisition/install/peer/shared/video gates and native/worker/UI deadlines remain
open;no generalized off-thread teardown or complete-client acceptance.

### Prepared Voice installation: components checked locally

install_voice uses unchanged original VoiceWork.commit and inside-gate window stop.
Prepared/replaced VoiceSession/media receiver and FnMut captures stay outside its
closure through rejection/unwind;previous resources returned for outside-gate
Voice-before-receiver disposal. Join/Call/AnswerCall installation and existing
CallRecovery begin/connected/call_connected metadata are accepted atomically.
Acquisition/prepare_call/protocol/server predicates,prior4extraction sites and
normal Logout/Leave/Voice/peer/RTP/shared-audio/DSP/UI sources remain unchanged.
Three local-only configured-native tests cover54ready resource/slot/priority cases,
5callback-capture Drop/unwind cases and3priority/window-held-gate cases;61native-bin
tests/source audit/Graphify passed. Final `native-calls/voice-installation-final-2026-11-08/`
passed294default/284neural-disabled tests,both Clippy/fmt/pinned/9HTTP/neural/freshbuild/
8inert RUEN/four-DPI interactions;protected neural metrics unchanged. Independent
438primary/3reference/24evidence/current+frozen executable+ONNX/3SDK audit passed
0mismatches;all prior291/281names/multiplicity retained,new3tests/main25/VoiceWork8/
Leave10/SessionIo6/preferences10/Profile10/runtime23/playback3/device13/native1/
MicTest4/VoiceSession1 suites and62new/93prior markers verified in both modes.
Exact source/gate/ownership/drop-order reconstruction passed;only3existing sources
changed,no new sources. Raw diagnostic interleave normalized only for full-name
parsing;local filesystem diagnostic independently retained,not a relaxed test.
Executable SHA-256:
`df3466c5f8ce94e1714a9c34f8d1c4e36b1f4f324ae3c0befeacbafe30474aee`.
voiceResourceInstallationChecked=true is narrow3site resource installation;
saved25ca1673f2a48b887f94391fcf503909afeea6e8/post-save Graphify/438/3/24/
current+frozen executable+ONNX/dependency/proof/clean-tree guard passed.
Temporary helpers deleted.
Active native audio/actual calls/peer/shared/video/
acquisition/destructor/worker/UI deadlines remain open;full readiness=false.

### Previous Voice extraction before preparation: components checked locally

Original stored resource ticket gates Join/Call/AnswerCall previous Voice extraction;
missing/mismatched/revoked nonempty ownership rejects,empty slots do not invent tickets.
Owned previous tuple/FnMut captures stay outside unchanged strict intent mutex.
Join/Call return Voice+receiver for outside-gate Voice-before-receiver disposal;
AnswerCall retains old receiver in original slot/ticket until existing installation.
Original EndCall send versus leave EndCall/LeaveChannel invoke arguments/order
unchanged;Join abandoned clear after leave and Call/Answer sent/connected flags
recheck original work after awaits. Native calls/Drop/cancel remain outside gate.
Four configured-native local tests cover72ready/slot/ticket cases,14Drop/unwind,
6held-gate priority/window cases and20hint/after-wait flag cases,no active native
VoiceLease/physical calls.65native-bin tests/source audit passed. Initial new test
fixture receiver-lifetime failure retained;fixed only new fixture. Final
`native-calls/previous-voice-final-2026-11-08/` passed298default/288neural-disabled,
both Clippy/fmt/pinned/9HTTP/neural/freshbuild/8inert RUEN/four-DPI interactions,
protected neural metrics unchanged. Independent438primary/3reference/24evidence/
current+frozen executable+ONNX/3SDK audit passed0mismatches;all294/284previous
test names/multiplicity retained,4new tests/main29/VoiceWork8 and previous suites
verified in both modes.112new/155prior markers and pre-existing diagnostic
signatures/multiplicity verified;raw logs/source/deadlines unchanged. Exact source/
gate/original-ticket/owned-tuple/Answer-receiver/hint/drop-order reconstruction passed,
only3existing source changes,no new sources. Executable SHA-256:
`a74128687159206326a5d9bc1879d78d0bf80ae9fa8288b5b9ead43f11a55e17`.
previousVoiceResourceExtractionChecked=true is narrow3site old-resource extraction;
saved3ac1e18b57d2526848c836415725995156bcda87/post-save Graphify/438/3/24/current+
frozen binary+ONNX/dependency/proof/clean-tree guard passed. Temporary helpers deleted.
Reused Voice/join errors/other
call-audio-resume-video/peer/shared/acquisition/native-worker bounds still open;
no full-client readiness,no generalized off-thread teardown.

### Original-resource worker metadata: components checked locally

14metadata sites now use original stored resource-ticket acceptance,borrowed FnMut
captures outside unchanged strict VoiceWork mutex. Join flags/native wait/new-error
extraction,CheckAudio sent/local-user-speaking/original350ms release and call recovery
connected/session/suspended/disconnected/poll/ended/retry/restored flags covered.
Two successful restoration callbacks extract old VideoReceiver to outer locals;
receiver Drop,stream-drop grace cancellation,publication and native waits remain
outside gate. General peer/video/shared native lifecycle acceptance remains open.
Six new tests cover96generic metadata cases,7capture/3heldgate,8native-mute/local-wire,
12configured Join,8recovery contract and24configured receiver retirement cases;
71native-bin tests/source scope audit passed. No physical audio/active video subscriptions
or real successful native Join/call restoration proven. Final
`native-calls/resource-metadata-final-2026-11-08/` passed304default/294neural-disabled,
both Clippy/fmt/pinned/9HTTP/neural/freshbuild/8inert RUEN/four-DPI interactions;
protected neural metrics unchanged. Independent438primary/3reference/24evidence/
current+frozen executable+ONNX/3SDK/proofs matched0mismatches;all298/288previous
test names/multiplicity retained,six new tests/main32/VoiceWork11/previous suites
verified in both modes.158new/267prior markers and previous diagnostic signatures/
multiplicity verified;raw logs/source/deadlines unchanged. Source reconstruction
outside1helper/6new tests/7worker metadata scopes exactly equals saved3ac1e18;
four existing sources changed,no new sources. Tested executable SHA-256:
`d2f889d682fbba98d97205cdd613826d61c4be696e08a3aaf874be05108d8c07`.
voiceResourceMetadataChecked=true is narrow worker metadata/two old-video extraction
acceptance. Saved9f983971dc81b7729933cc03d332ef9b90ae2987/post-saveGraphify/438/3/24/
current-frozen binary+ONNX/dependency/proofs/clean-tree guard passed;helpers removed.
General VideoReceiver/peer-exclusive/shared/native-resource/worker/UI bounds and
full-client/worker/audio/call readiness remain false,no generalized off-thread teardown.

### Peer-exclusive incoming-video acceptance: components checked locally

PeerLifetime metadata mutex linearizes commit versus atomic invalidation;poison fails
admission closed,cancellation remains outside mutex. Original Arc identity and stored
Voice ticket participate in intent->peer acceptance,window stop rechecked after peer
wait. Staged install_video receiver/FnMut/replaced resources stay outside both gates.
One incoming video Track scope only:guarded old extraction/max3 watch calculation,
outside-gate Drop/native start/watching-send,final original-ticket/peer install.
Seven new tests/source-scope438/3/3SDK audit passed;18filtered tests include all7new.
Final `native-calls/peer-acceptance-final-2026-11-08/` passed311default/301neural-disabled,
bothClippy/fmt/pinned/9HTTP/neural/freshbuild/8inert RUEN/four-DPI interactions;
protected neural metrics unchanged. Independent438primary/3reference/24evidence/
current+frozen executable+ONNX/3SDK/proofs matched0mismatches;all304/294previous
test names/multiplicity retained,seven new tests/main32/VoiceWork15/media3/previous
suites verified in both modes.71new/425prior markers and previous diagnostic
signatures/multiplicity verified;raw logs/source/deadlines unchanged. Independent
source reconstruction outside PeerState/2helpers/7new tests/1incoming-video scope
exactly equals saved9f98397;four existing sources changed,no new sources. Tested
executable SHA-256:
`3a2b502bc32b0e342d8ba716c80d4a1b962fdbb5d75f3439c92c468591eb0a83`.
peerExclusiveCommitChecked=true/incomingVideoResourceAcceptanceChecked=true only
for exclusive metadata and one incoming-track extraction/install scope. Callback
unwind retains installed receiver with poisoned peer admission,not rollback/health
acceptance. Savedf2c5340fae2819072f161c81c98a978d3f92210e/post-saveGraphify/438/3/24/
current-frozen binary+ONNX/dependency/proofs/clean-tree guard passed;helpers removed.
Other video restart/stop/prune/shared-resource/native lifecycle/timing/full readiness
remain open,no generalized off-thread teardown/native-start/stop deadline claim.

### Video transition and retirement resource acceptance: components checked locally

take_video requires original Voice ticket/receiver Arc identity;metadata predicate
sees target in max3 count before extraction. Missing peer allowed only for absent
slot. Native start/stop/watching-send outside gates,three original-resource installs
recheck afterward. retire_videos stages selected live receivers under intent->peer;
monotonic dead receivers require original intent only. Live poison stays quarantined,
dead poison can retire without reopening;Some(retired) is not complete/bounded cleanup.
Four take sites/three install sites/three UserLeft-event-Join retirement sites only.
Grace cancellation/UI outside gates;peer poison does not suppress UserLeft Voice.handle
or Session processing. Three existing sources/two helpers/four tests/438/3/3SDK source
scope audit passed;four filtered tests cover132new cases,including12active local video
and5max3/local wire/30second grace/expiry UI. Final
`native-calls/video-transition-final-2026-11-08/` passed315default/305neural-disabled,
bothClippy/fmt/pinned/9HTTP/neural/freshbuild/8inert RUEN/four-DPI+Graphify;
protected neural metrics unchanged. Independent438/3/24/current-frozen exe+ONNX/
3SDK/proofs matched0mismatches;all311/301previous pass names/multiplicity retained;
four new tests/main32/Voice19/media3/all previous suites verified in both modes.
132new/496prior markers and diagnostic signatures/multiplicity checked. Parser
correction only handles unchanged Leave diagnostic interleaved atone raw pass line;
original proof retained,no logs/sources/counts/deadlines changed. Independent source
reconstruction outside2helpers/4tests/4worker scopes exactly equals savedf2c5340.
Three existing sources changed,no sources added;previous strict gates/Voice/PeerLifetime/
audio/native video/incoming-track runtime unchanged. Tested executable SHA-256:
`2418139faf5cf14f7d5780491edb70d619d762fde007da2621757dbaa7f63682`.
videoTransitionResourceAcceptanceChecked=true/videoRetirementResourceAcceptanceChecked
=true are narrow4take/3install/3retirement site acceptance;Saved82f4d95/post-save
Graphify/438/3/24/current-frozen binary+ONNX/3SDK/proofs/clean-tree guard passed.
All previous temporary helpers removed. Native subscription task join/remove_sink/
native timing/shared/runtime/full-client acceptance remain false;no native teardown
deadline/generalized off-thread Drop claim.

### Whole Voice/media bundle transfer: components checked locally

Three takes only:authenticated reconnect,Join from call,accepted AnswerCall. Outer
Voice/events/video tuple preserves native owner-before-receiver Drop across waits and
unwind. Original Voice ticket required for any occupied native slot;orphan-live/poison
video maps refused without Voice owner. One reconnect restore requires monotonic-dead
original map and original ticket,preserves exact optional events without recreation or
reticketing. FnMut captures/native operations/Drop/UI remain outside gates. Existing
Call/Leave/Logout/idle/worker-end/native video/PeerLifetime/strict intent/audio methods
unchanged. Three existing sources/two helpers/four tests+fixture/source438/3/3SDK audit
passed. Revised162case filtered tests passed4;Graphify passed5992nodes/14777edges.
Full `native-calls/voice-media-bundle-final-2026-11-08/` passed319default/309neural-disabled,
bothClippy/fmt/pinned/9HTTP/neural/freshbuild/8inert RUEN/four-DPI/Graphify;protected
neural metrics unchanged. Independent438/3/24/current-frozen exe+ONNX/3SDK/proofs
matched0mismatches. All315/305previous full test names/multiplicity/diagnostics and
main36/Voice19/media3/all previous suites checked in both modes.162new/628prior
component-video markers retained;separate audit verifies each162case exactly once
per mode. Existing strict helpers/native implementations and other worker branches
exactly equal saved82f4d95. Tested executable SHA-256:
`803c9ea2dda86f07a48199429888585e40c111b7d23db55be988896b64230e34`.
voiceMediaBundleExtractionChecked=true/voiceMediaBundleRestoreChecked=true are narrow
three owner-transfer takes/one same-original-ticket optional-events restore only;
Saved54519c7/post-saveGraphify/438/3/24/current-frozen exe+ONNX/3SDK/proofs/clean-tree
guard passed;temporary launcher removed immediately.
Native subscriptions/task join/remove_sink/callback/syscall/physical/full readiness
remain false;this is owner transfer,not generalized native off-thread teardown.

### Call media transition: components checked locally

One Call prefix/one helper/three tests only. Valid friend extracts original-ticket
Voice/events/video tuple through unchanged prior EndCall send or LeaveChannel invoke.
Invalid friend retires original video receivers under intent->peer gates but retains
original Voice/events;live poison remains quarantined/dead poison never reopened.
Empty native slots preserve absent-ticket/fresh-epoch behavior. Metadata clearing
rechecks original intent after staged partial retirement;not atomic full-map clear.
Native Drop/borrowed FnMut capture disposal outside gates. Existing strict helpers/
native implementations/tests/fixtures/other worker scopes exactly equal saved54519c7;
source438/3/3SDK audit passed. Revised143case filtered tests passed3;Graphify passed
5996nodes/14806edges/264communities. Full `native-calls/call-media-transition-final-
2026-11-08/` passed322default/312neural-disabled/bothClippy/fmt/pinned/9HTTP/neural/
freshbuild/8inert RUEN/four-DPI/Graphify;protected neural metrics unchanged. Independent
438/3/24/current-frozen exe+ONNX/3SDK/proofs matched0mismatches. All319/309previous
test names/multiplicity/836diagnostic signatures and main39/Voice19/media3/all previous
suites retained.143exact new cases/790prior component-video markers per mode;separate
audit individually verifies120take/7held gate/16native local wait cases exactly once.
Previous strict helpers/native implementations/tests/fixtures/other worker branches
and Call protocol/preparation/StartCall/install tail byte-for-byte unchanged.
Tested executable SHA-256:
`aa6298239c160e9c0e4e15324f2eb623360093d05819229b7945fad53678e87f`.
callMediaTransitionChecked=true covers only one Call prefix with separate accepted
whole-owner/rejected-original-Voice-preserving retirement;Saved a97cbcb/post-save
Graphify/438/3/24/current-frozen exe+ONNX/3SDK/proofs/clean-tree guard passed;
temporary launcher removed immediately. Leave/idle/worker-end/video subscription task join/native
sink callbacks/syscalls/shared resources/physical/full-client readiness remain false.

### Idle media retirement: components checked locally

One idle prefix/one helper/three tests only. Absent Voice required;original stored
Voice ticket retained before extraction and until original optional media events
are absent. Unchanged original-ticket take_previous_media with retain_events=true
refuses live/live-poisoned orphan receivers;only monotonic-dead map transfers.
Empty native slots preserve missing-ticket/fresh-epoch behavior. Idle metadata
reset linearized under original intent;extracted tuple/FnMut captures dispose
outside gates on rejection/unwind. No reticket/recreated events. Source438/3/3SDK/
predecessor audit passed;all previous helpers/native implementations/fixtures/tests/
other worker branches including Call/Leave/Logout/worker-end exactly saved a97cbcb.
New fixture compile inference and inactive recovery deadline errors corrected;
latest74case filtered job sh_1ba056e53001h129kHa5NqpjGC passed3,74unique markers
independently counted. Full `native-calls/idle-media-retirement-final-2026-11-08/`
passed325default/315neural-disabled/bothClippy/fmt/pinned/9HTTP/neural/freshbuild/
8inert RUEN/four-DPI auth-roster-audio-MicTest/Graphify;neural metrics unchanged.
FinalGraphify6000nodes/14828edges/257communities,existing warnings retained.
Independent438/3/24/current-frozen exe+ONNX/3SDK/proofs matched0mismatches. All322/312
previous full test names/multiplicity/979diagnostic signatures and main42-Voice19-
media3/all previous suites retained.948prior component diagnostics measured per
explicit regex,all previous component/video/Call subsets unchanged. Each60take/
6held gate/8native local invalidation case exactly once per mode,74new markers.
Separate evidence recheck passed,all previous Checked flags retained. Existing
helpers/native implementations/tests/fixtures/Call-Leave-Logout-worker-end scopes
byte-for-byte unchanged outside idle prefix/helper/three new tests.
Tested executable SHA-256:
`18dce57d9bb5646646633f9e4861d24e3aedf10028e49fa647e3c56bade58b53`.
idleMediaRetirementChecked=true covers only one original-ticket idle prefix;
Saved4b5ff3a/post-saveGraphify/438/3/24/current-frozen exe+ONNX/3SDK/proofs/clean-tree
guard passed,temporary launcher removed immediately.
Priority revoked-owner retirement/native sink/task/
syscall/shared-resource/worker/physical/full-client readiness remains false.

### Terminal media retirement: components checked locally

One explicit Leave/entered worker-end prefix and one concrete exclusive-slot helper,
three tests only. Whole original Voice/events/video map/optional original ticket
transferred unconditionally for retirement,including revoked/missing tickets and
live/live-poisoned orphan receivers. No renewed current-Voice/peer admission or
individual video mutation. Whole tuple drops Voice then events then video outside
gates,on unwind/cancellation. Leave keeps original demand-only reset/metadata/hints
and drops before publication/hints;worker extracts before unchanged hub.close then
original generation increment and tuple Drop. Source438/3/3SDK/predecessor audit
passed;all other worker/native/helper/fixture/test scopes exactly saved4b5ff3a.
Initial new Leave test epoch assertion corrected to exact original demand.reset
capture_epoch+1;production/previous tests/deadlines/flags unchanged. Filtered83case
job sh_1ba14d90d001wyyJfT487Q2XoF passed3,83unique markers independently counted.
Full `native-calls/terminal-media-retirement-final-2026-11-08/` passed328default/
318neural-disabled/bothClippy/fmt/pinned/9HTTP/neural/freshbuild/8inert RUEN/four-DPI
auth-roster-audio-MicTest/Graphify,neural metrics unchanged. Graphify retained6005
nodes/14853edges/256communities,existing7metadata warnings/HTML5000limit retained.
Independent438/3/24/current-frozen exe+ONNX/3SDK/proofs0mismatches. All325/315prior
pass names/multiplicity/1053diagnostic signatures-markers/multiplicity retained;
previous component/video/Call/idle subsets and all previous suites checked,main45/
Voice19/media3. Each55whole-owner/8held-gate/20actual Leave case exactly once per
mode,83newmarkers. Separate evidence recheck passed,all previous Checked flags
retained;all code/native/helpers/fixtures/previous tests outside one helper/three
tests/Leave prefix/entered worker-end exactly saved4b5ff3a.
Tested executable SHA-256:
`58b1418a9799814254e18393f9e7591aa1ecd8b7c8afabfd088ce44cc9b1afa5`.
terminalMediaRetirementChecked=true covers only explicit Leave/entered worker-end
ownership;Saved2ccbc75/post-saveGraphify/438/3/24/current-frozen exe+ONNX/3SDK/proofs/
clean-tree guard passed,temporary launcher removed.
Worker-future abort before terminal prefix,
native stop/close/sink/callback/task termination/driver/worker/physical/full-client
readiness remain false;no generalized off-thread Voice/peer teardown authority.

### Video task join: targeted components checked

VideoReceiver now prepares existing private AudioCleanup root before replacement/
native stream acquisition,retains original future/JoinHandle in unchanged observer;
Drop invalidates lease,cancels token,aborts without synchronously waiting. New public
with_audio_runtime wrapper bridges bin/library without opening tracker;one worker
constructor shares existing app drain. Default constructor/API/frame loop/lease-peer-
receiver/currentIntent/storedVoiceTicket/Leave-worker-end-bundle/other worker/native/
audio/strict helpers/old fixtures-tests unchanged. Source438/3/5SDK/fullproof chain,
exact reconstruction outside registered video ownership/five new tests/one constructor.
Initial E0624 compile failure retained;corrected with private-tracker wrapper only.
Revised video suite sh_1ba23e680001ETi5NMSs4R3XTc passed7,19cases;full333/323 not yet
launched. По уточнению владельца 2026-11-08 полный default/neural/RUEN/four-DPI/
точный сравнительный и многоуровневый hash audit отложены до завершения функционального
переноса. По ходу сохраняются сборка/типы/тесты затронутой логики и проверки безопасности
данных/авторизации/шифрования/native-владения. Неиспользованный временный full-verifier
удалён;отложенные проверки не считаются пройденными. Initial Graphify passed before
API correction;final update sh_1ba2a8a38001H8yZRrQdIbHyu4 passed6029nodes/14953edges/
258communities,existing7warnings/HTML5000limit retained.
Targeted fmt/Clippy(native-media,all-targets)/shell-Voice-Leave/local RTP job
sh_1ba29c630001WoePMXEEBqhjKx passed92shell/3localRTP;with7video tests102targeted
tests passed. videoTaskJoinChecked=true narrow registration/original task completion;
full/default/neural/visual acceptance still deferred. Initial outer cmd quoting failure corrected
before Rust execution,not a test failure.
Native acquisition/remove_sink/Drop/syscall/driver/lastArc/worker abort before terminal
prefix/generalized off-thread teardown/shared runtime/physical/full-client remain open.
Saved **4446514**,clean tree confirmed;no full333/323/default-neural/UI-matrix proof.

### Stream picker: functional components checked

Screen/window tabs,localized labels,tab selection reset,default low/low-high quality
forwarding,loading/empty/busy/cancel connected to original-ticket Voice work and
StartStream(quality). Low1280x720/30,high1920x1080/60,medium1280x720/60 preparation;
no original locale edits. Late inventory no longer reopens cancelled picker.
Affected run sh_1ba30e63d001MG8tDoeNm4fHww passed under native-streams/picker-
functional-2026-11-08:fmt/3screen/92shell/3localRTP/Clippy/native-media build/two inert
RUEN100% interactions. Physical camera/system audio/RC/thumbnails/native bitrate/
SFU/restore still unfinished. Exact comparison/full regression remains final-stage
work per owner;no complete-client acceptance/publication yet. Passed98tests and
fmt/Clippy/build/two inert interactions. High quality forwarding checked with direct
property assignment;pointer coverage of the quality controls remains to add.
Saved **f05ec02**;Graphify sh_1ba398377001AhJ1MiBUF9OklG passed6035nodes/14965edges/
260communities with existing7warnings/HTML5000limit.

### Native sender quality: functional components checked

Native RTP low2.5M/30,medium3.5M/60,high6M/60 with maintain-framerate/high network
priority wired for existing/late peers through set_video_with_quality. Old default
API retained;camera defaults unchanged;invalid quality does not replace resources;
stop/close clear profile. Five new media tests and actual quality-control pointer
coverage added. Initial sh_1ba3d40d3001nY6Vx42SzT89fj failed new native-source test
without Tokio reactor;corrected test annotation only,logs retained. Completed/passed
sh_1ba410b0c0019U21vXJt0y1hgK:fmt/8media/3localRTP/Clippy/
build/two inert RUEN100% actual quality-pointer interactions. Adaptive bitrate/stream restoration/SFU/camera/
system audio/physical/full comparison still pending;nativeClientReady=false.
Revision sh_1ba3e8f28001i1Vex6mqM2ZAbT exposed sender reuse with old limits;production
now captures modified-field baseline and restores it before removal via fresh RTP
parameters. SSRC/transaction identity retained;unset degradation maps to equivalent
Balanced default. Local RTP decoding test starts with low profile;desktop→camera/
stop/default reset scenarios expanded. Corrected affected checks passed;Graphify
sh_1ba45e633001ny2Ado7o4yDH2O passed6053nodes/15032edges/247communities with existing
7warnings/HTML5000limit. Saved **43122d4**,clean tree confirmed;full-client and
physical readiness remain false. The following stage checks live stream reannouncement
after channel/call recovery under original VoiceWork without membership loss.

### Stream reannouncement: functional components checked

Retained ScreenShare owns original capture+quality. New restore_share only invokes
StartStream for live capture/enabled native track after membership/call recovery;
does not reacquire capture,replace track or rebuild restored peers. All3shell recovery
entrypoints run it under original VoiceWork after bundle install/call commit,not in
the5s call timeout. Rejection/network failure uses existing streamStartFailed toast
without losing channel/call. Passed sh_1ba4caa600012Yfu1lpRZZXhlw:7local protocol
unit fixtures in10.69s,synthetic video and panic-on-physical-audio factory. Passed
sh_1ba4e42b1001fAK2bVUJXxBt89:133total tests (7stream/8media/3screen/8call/1old Voice/
92shell/14local hub-call-intent-RTP),fmt/native-media all-targets Clippy/build.
Graphify sh_1ba4ed340001WhdTGG9NXaXA9C passed6085nodes/15120edges/252communities,
existing7warnings/HTML5000limit retained.
Shell/native channel
rejoin with actual audio,physical capture,universal acquisition/Drop deadlines and
full-client acceptance remain unverified. Saved **1993b1f**,clean tree confirmed;
no pending jobs. Next missing scenario group:camera/system-audio share,with safe
backend/COM-owner selection before adopting blocking camera dependencies.
No publication/shutdown.

### Camera backend ownership: checked components, not camera readiness

Added standalone zabor-camera with asynchronous Media Foundation reader,one MTA/COM
owner and owned-byte-only CXX bridge. zabor-native unsafe forbid and existing pinned
WebRTC/DSP/ORT/CRT dependencies unchanged;nokhwa not installed. Passed8initial and
10expanded synthetic contract tests:owner isolation,non-driver stop/Drop,late open/
callback cleanup,failed read/cleanup and checked padded/bottom-up RGB32 copy.
No physical enumeration/acquisition. Synthetic sources are test-source-feature-gated.
Strict /W4 /WX found bridge unwind/stream enum-conversion compilation issues;fixed
with /EHsc/explicit DWORD casts,not weaker checks. Corrected
sh_1ba6170c00013iZqMweqnzeSXP passed10camera/7stream tests,fmt,normal+test-source
camera Clippy/standalone build and native-media all-targets Clippy/client build.
Final Graphify sh_1ba6344f4001vcee7XhZ7vY9gO passed,no topology changes after casts;
retained6273nodes/15507edges/275communities,existing7warnings. No pending jobs;scoped
diff check passed;save completed below. Native app does not yet use this backend;camera UI/tab,
original Voice admission and registered cleanup observation remain unwired. Hung
driver and universal callback teardown deadlines are not proved. System audio must
preserve selected-process capture or app-tree-excluding screen capture,not naive
endpoint loopback. Full-client acceptance/publication/shutdown remain false.

Saved **99f59ca9dfb5f6da9239c48ada629b4e30f7f629**,clean tree confirmed;final
sh_1ba6748ff0012MPhx4bOTVDv5s passedGraphify6273nodes/15507edges/276communities and
scoped staged-path/diff/save checks,existing7warnings/HTML5000limit retained.
Delayed callback test now also asserts elapsed cleanup wait;all10camera tests/
Clippy/fmt passed again. No pending jobs. Next:original observer registration before
camera acquisition,CameraCapture/native source/Voice retained-quality integration
and third picker tab with existing RUEN labels;camera cannot add stream audio/RC.
No camera/full-client readiness or publication/shutdown.

### Camera application integration: WIP under final local checks

CameraCapture now registers its original blocking-task cleanup observation before
camera/source acquisition;public Drop and cancelled startup/inventory waits request
stop only. NativeOwner keeps Completion through native join,including actor unwind;
actual blocking-task join and cleanup_failed propagate to the application tracker.
Voice retains Desktop/Camera variants and camera quality;the shell takes/reinstalls
the original Voice/events tuple with its original ticket outside native mutation.
Third camera picker tab uses exact IDs,RUEN keys,separate loading,cancelled-query
identity and forced camera quality;desktop quality/audio/RC remain absent for camera.

First tiny4x2 integration aborted in WebRTC;height guard and160x96 video fixture
were added without changing original4x2 byte tests. Corrected intermediate10camera/
9stream tests passed;Clippy then rejected a new test's chunks_exact,use as_chunks
instead without suppression. sh_1ba79eec5001sQqjV0dyhM1lLn failed8/11integration:
requiring WebRTC delivery before creating the sender track/sink caused a startup
cycle. Valid producer-frame readiness restored;actual delivery test stays after
track/sink attachment. sh_1ba7cd525001Jtur8b4gShu9uN is the pending corrected final
targeted run,including11camera original-intent cancellation tests,93shell guards,
normal/feature Clippy/build and RUEN inert picker interactions. First integration
Graphify passed6330nodes/15698edges/259communities,existing7warnings/HTML5000limit.
Current source is uncommitted WIP after99f59ca;physical camera/audio/production
access,full regression/exact visual comparison,readiness/publication/shutdown=false.

Corrected sh_1ba7cd525001Jtur8b4gShu9uN passed134targeted tests/fmt/normal+feature
Clippy/native build. Inert UI invocation failed at cmd relativeforwardslash path,
not a passed UI result. NativeOwner now additionally keeps the source through its
original camera join on exit/unwind;actual-source unwind test updated. Pending
sh_1ba8abb0c001pbcInUCM8oMibX reruns134checks and RUEN inert picker with Windows
path,then verifies normal binary lacks synthetic camera markers. Save/readiness
remain conditional;no physical devices/production/publication/shutdown.

Camera application targeted result:final134tests passed(10backend/11camera/
9stream/93shell/8media/3screen),fmt,normal+feature all-targets Clippy and normal
native-media build passed. First inert checker invocation used home but auth-first
test requires login;corrected login-fixture RUEN checks passed,including new camera
tab/loading/empty/busy/exact id/camera quality/query cancel. Normal binary lacks
synthetic camera markers;stage SHA2560f599ada78fe5cfe... in verified-owner-order
evidence is not whole-client attestation. Graphify6332nodes/15703edges/276communities
passed,existing7warnings/HTML5000limit remain. No pending jobs;save next. Camera
thumbnails/physical/hotplug/long-session/native deadlines and isolated stream audio,
remaining full migration/data-integrity/visual/release gates remain unfinished.
Full-client/publication/shutdown acceptance=false.

Saved source **df43be2e329a8db4cf6a7cffae002fdd6e8e35f4** on rust/native-client,
clean tree confirmed after scoped16path/source/executable guard and commit.
134targeted tests/RUEN picker/fmt/normal+feature Clippy/native build/Graphify passed.
Evidence:camera-integration-functional-2026-11-08/save-verification.json and
verified-owner-order/verification.json. No pending build/test jobs;post-save metadata
sync only. Next functional work is isolated process/system stream audio,then
thumbnails/adaptive bitrate/SFU/RC and remaining full UI/profile/import/packaging,
ownership/data-integrity issues. No push/merge/publication/shutdown or whole-client
acceptance. Camera hardware/hotplug/privacy/long-session/native deadlines remain open.

### Isolated stream audio: functional WIP

Baseline source df43be2/metadata9985c45;added separate registered blocking native
process-loopback owner using unchanged wasapi0.26/windows0.62.2. Screen excludes
the actual native app process tree/window includes exact HWND PID tree,no endpoint
fallback.48kHzfloatstereo with silent-null/bounds/ReleaseBuffer/Stop handling stays
outside mic/DSP/MicTest;owner teardown precedes WebRTC source disposal. Original
blocking JoinHandle observed,public Drop stop-only,activation/driver waits unbounded
and never accepted from an await cancellation. Optional desktop audio retained in
Voice/original-ticket shell tuple,video continues on audiofailure,cameraaudiofalse.
Second audio sender/remote SDP-mid classification/stereo focus playback and default
true localized picker shareAudio option implemented,under expanded targeted checks.

6backend/9expanded audio ownership tests passed. Sender test found pinned SDK's
null set_track dereference;avoided via remove_track/readd and no empty-setter calls,
no vendor/pin changes. Two follow-up tests assumed local track label as opaque
senderMSID;corrected to real separate-routing/stable-slot contract. Pending
sh_1bab14d49001hQ3zgZXyWVJDmB runs sender/share/playback/mixer/original-shell guards,
normal+fixture Clippy/build and inert RUEN picker checks. No physical/production
access,save/push/publication/shutdown;nativeClientReady=false. Initial Graphify
6447nodes/15982edges/276communities passed,existingwarnings/HTMLlimit unchanged.

Isolated stream audio final targeted result:180tests passed(6boundary/12media/
9capture/12share/3playback/30existinglocalmedia-mixer-microphone-route-runtime/
94shell/11camera/3screen),fmt,normal+fixture all-targets Clippy including boundary,
normal native-media build and inert RUEN login-fixture picker/audio-toggle passed.
21consumedsourcehashes unchanged;normal executable has no synthetic camera/audio
markers,SHA2560fd77bc79320671c... is stage-only,notwholeclientattestation. Temporary
runner removed and post-runsource/binary checked. Graphify6448nodes/15985edges/
285communities passed,existing7warnings/HTML5000limit remain. No pendingjobs;
guarded scoped save next. Physical/Opus/native-driver/long-session/exact/full
regression acceptance remainsfalse,nextsourcepicker/thumbnails/own-windowfilter,
adaptive bitrate/SFU/RC and remainingfullUI/profile/import/packaging/data-integrity.
No push/merge/publication/shutdown,nativeClientReady=false.

Saved isolated-audio source **5b8d6563e46827e60b25c03d103752e8e7a40588** on
rust/native-client after guarded24path/21sourcehash/binary/180tests/RUEN/Graphify
checks;post-commit tree clean. Source stage includes the retained camera milestone,
separate window/process-loopback stream audio,second receiver-only-until-start
audio slot and unchanged old microphone/SCTP behavior. Evidence:isolated-audio-
functional-2026-11-08/final-checked/verification.json and save-verification.json.
No pending build/test jobs;post-save metadata/graph sync only. Next missinggroup:
picker thumbnails/own-windowfilter,adaptive bitrate/SFU/RC,then remaining full
UI/profile/import/packaging and ownership/data-integrity issues. Native physical/
privacy/hotplug/clock/long-session/driver/last-Arc and exact parity acceptance remain
false. No push/merge/publication/shutdown,nativeClientReady=false.

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
