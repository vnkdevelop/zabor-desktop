# Контекст продолжения нативного переноса

Редакция: 2026-11-07. Это постоянный handoff для compact, не отчёт о готовности релиза.

## Задача и решения владельца

- Полностью перенести ZABOR Desktop на native Rust для Windows 10/11 x64, без Electron/WebView в конечном клиенте; native C/C++ библиотеки разрешены.
- Действующий React/Electron остаётся функциональным и визуальным эталоном: окна, анимации, цвета, состояния, взаимодействия, RU/EN и совместимость протокола сохраняются.
- Backend остаётся .NET, SFU — Go. Автономные интеграционные проверки только локальные; production-сервисы не менять и не тестировать.
- Beta-профиль `zabor-desktop-native-beta`; импорт только копированием, оригинальный профиль не изменять. Сессия/JWK: current-user DPAPI; настройки JSON; history/TOFU: owner-isolated SQLite, retention 14 суток.
- Рабочая ветка `rust/native-client`. Проверенные этапы сохранять коммитами. Push и публикация `3.8.0-rust` разрешены только после готовности полного клиента; merge в main не разрешён.
- После полного завершения, проверки и сохранения результатов выполнить штатное завершение Windows без принудительного закрытия приложений. До полной готовности **не выключать**. Выключение ещё не выполнялось.
- Не останавливаться для запроса «продолжи». После каждого завершённого и проверенного этапа сохранять handoff/commit и выполнять compact сессии, не теряя текущие и последующие задачи.

## Правила выполнения

- Все терминальные команды через явный `cmd.exe /c ...`.
- Graphify сначала: `graphify-out/GRAPH_REPORT.md` / graph query; после правок `graphify update .`. Завершающее обновление запускать с автоматическим уведомлением о завершении, не опрашивать процесс.
- Не сбрасывать и не переписывать существующий рабочий код; не добавлять комментарии. Существующие комментарии, включая лицензии vendor-кода, сохранять.
- Не менять локализацию/стили/соседние компоненты без необходимости переноса. Все новые UI-строки из RU/EN каталогов; UI — lowercase, кроме технических обозначений.
- No-tracking. Не сохранять секреты в репозитории или handoff; реквизиты только из untracked `.env`, без воспроизведения значений.
- Не терять/отменять незавершённые фоновые задачи при compact. Использовать их автоматические уведомления; не poll/sleep/check output для ожидания, не дублировать выполняющийся запрос.
- Удалять собственные временные файлы после проверки. Не удалять пользовательские изменения, исходный профиль, сохранённые файлы или чужие артефакты.
- Синхронизировать затронутые юридические документы и обе языковые версии; не обещать в них незавершённую функциональность.

## Сохранённые предыдущие milestones

- `d088795`: pre-native Electron/WASM checkpoint.
- `d1a33f6`: проверенный native updater preview.
- `7e0ea3d`: connected native core / roster.
- `fda236c`: media/peer/video lifecycle.
- `bcf1371`: friend/channel notifications.
- `076f116`: полный файловый workflow. Это последний commit перед текущими аудио-правками; actual HEAD проверять через Git.
- Не считать preview, транспорт или отдельный пройденный тест завершением полного клиента.

## Уже сохранённая функциональность

- Slint 1.18.1 / winit / femtovg; auth/registration/captcha, SignalR negotiate/WebSocket/framing/invocations/timeouts, сессии, basic reconnect.
- Roster, присутствие/speaking/moderation, каналы/друзья/заявки/приглашения. Поздний выход из старого канала не удаляет новое членство; notification actions сохраняют `null` success / `false` rejection.
- P-256/AES-GCM encrypted fallback и native P2P text, ACK/read/sync/delete, TOFU и 14-дневная очистка.
- File picker/drop/confirmation/staging, metadata/request, DTLS/SCTP `zabor-chat-files-v1`, progress/cancel/retry/SHA-256/atomic save/Explorer reveal/recovery. SignalR не передаёт файловые байты; originals и внешние сохранённые копии сохраняются.
- Peer-lifetime guards отбрасывают stale callbacks, offer epochs process-unique; video subscriptions возобновляются на прежнем RTP receiver; `StreamDropped` сохраняет 30-секундный grace.
- File checkpoint: 33 unit + 17 integration tests, fmt/Clippy/build; реальная локальная передача 2 MiB SCTP. Не заменяет full-client acceptance.
- Auth RU/EN × four DPI: 24 capture pairs и 8 interaction runs в `migration-artifacts/native-auth/checkpoint-roster-2026-11-06`, max mean RGB error 0.4279/255. File UI: `migration-artifacts/native-ui/files-2026-11-06`. Полная shell parity ещё не выполнена.

## Текущий аудио-этап

- Добавлены `audio_settings.rs`, `voice_filters.rs`, `manual_gate.rs`, `smart_gate.rs`, `voice_activity.rs`, `audio_device.rs`, `audio_mix.rs`, `neural.rs` и локальные reference tests.
- **Новые DSP/device/mixer модули ещё не подключены к `VoiceSession` / `shell.rs`.** Приложение использует прежний platform ADM. Не отмечать native audio settings/mic-test/hotplug/AEC как готовые.
- Последний полный прогон **без neural features**: 35 unit + 29 integration tests. В этом режиме fmt и Clippy `-D warnings` прошли.
- Пять PCM-сравнений прошли: tonal notches/EQ/deesser, manual gate, model-input gain/ALC, manual SmartGate, voicing/attack/release/preroll с одинаковыми VAD-решениями. VAD mode 2 / hangover 150 ms совпал с WASM, включая reset; линкуется совместно с libwebrtc.
- Mixer: global/per-user gain, deafen/focus без replay backlog, bounded PCM/reverse queues, peer replacement/revocation. Это ещё не подключённый playback/AEC.
- DeepFilterNet3 отдельный native harness прошёл: mean 1.8255001080535624e-9, max 3.4691766e-8. Временный harness удалён. Основной neural integration test проверяется отдельно.
- После аудио-правок синхронизированы LICENSE, THIRD-PARTY-NOTICES.md, TRADEMARK.md, PRIVACY.md, TERMS.md, ACCEPTABLE-USE.md, docs/chat-data-flow.md. Product licence остаётся GPL-3.0-only, Electron-код не менялся.

## Точные зависимости и опасные несовместимости

- libwebrtc 0.4.0 / webrtc-sys 0.3.53, SDK `webrtc-a545de4`, short path и static MSVC CRT в `.cargo/config.toml`.
- DeepFilterNet `d375b2d8309e0935d165700c91da9de862a99c31`, исходные DFN3 weights. Tract family `=0.21.4`, ndarray 0.15. 0.21.6 меняет symbol API; последующие версии также несовместимы по ndarray.
- `rust/vendor/tract-core-0.21.4`: verified upstream archive, единственная правка `SpecialOps::add_const` устраняет коллизию имён разных констант. Не отключать integrity/debug assertions, не менять веса.
- `rust/vendor/webrtc-vad-0.4.0`: verified upstream, build-only prefix `ZaborFvad_` для C `WebRtc*` symbols; без force-multiple linker flags и без изменения алгоритма.
- DF state non-Send: создавать и удерживать на owning device thread. Не добавлять unsafe Send.
- RNNoise: rnnoise2 0.1.2, исходный upstream model archive SHA-256 `4ac81c5c0884ec4bd5907026aaae16209b7b76cd9d7f71af582094a2f98f4b43`, 22270507 bytes; target `rust/target/rnnoise-4ac81c5c/rnnoise-modeltar.gz`. Имя специально заканчивается `tar.gz` для build logic.
- Стандартные nnnoiseless weights не совпали с текущим WASM — не использовать как замену. WASM destructor ломается после многих frames; reference работает в отдельном Node process без этого destructor, legacy code не менять.
- Silero: existing local model, ort 2.0.0-rc.15 / Microsoft ONNX Runtime 1.31.1, telemetry=false. Старый ORT 1.17.1 DLL вызывает API [31] failure; pinned DLL должен находиться рядом с binaries и `deps`.
- `prepare-onnx.cjs`: pinned NuGet SHA-512 и DLL/lib/licence SHA-256. `prepare-libclang.cjs`: local libclang 18.1.1 из pinned Windows x64 wheel для bindgen; системного LLVM install не было. `.cargo/config.toml` задаёт `LIBCLANG_PATH`.
- `--features rnnoise2/runtime-model` используется **только как diagnostic compilation path**, не заменяет статические RNNoise weights и не подтверждает waveform/готовность RNNoise. После local libclang эта compilation check прошла.
- DSP: 48 kHz / 480 neural samples, manual gate 128; Silero 16 kHz / 512 + 64 context / 256 state; fixed threshold 0.4, calibration disabled. Gain 0–200%, raw boost 1.7; suppression 5–30 dB, manual −60…−12 dB/default −42, beta 0.02…0.05.

## Фоновые задачи и блокеры на момент handoff

- RNNoise preparation `sh_1b4978e02001nqEfMEDZoswZXl` **завершилась ошибкой**: 0/22270507 bytes, UND_ERR_CONNECT_TIMEOUT. Предыдущий bounded resumed run остановился на 466944 bytes. Partial files очищены. Нового download job сейчас нет; не считать модель подготовленной.
- `prepare-rnnoise.cjs` уже имеет bounded Range/resume/backoff, IPv4-first, новые HTTP connections, full SHA-256, unique owned temporary directory и no-clobber publication. Девять local HTTP tests прошли. Xiph network остаётся реальным блокером, веса/проверки не ослаблять.
- Native diagnostic compilation `sh_1b4a4dbc7001OQtF5PL0kd446E` с runtime-model **прошла**. Tract upstream имеет 90 предупреждений; собственный native Clippy без neural прошёл. Не делать постороннюю чистку vendor source.
- Targeted DeepFilter neural test `sh_1b4a4dca9001TYQ4lsYTWlAC86` **завершился до запуска тестов с LNK2019/LNK1120**: RNNoise diagnostic runtime-model path не предоставляет `init_rnnoise`. Не считать этот тест пройденным; отдельный временный DFN3 waveform harness ранее прошёл. Не исправлять diagnostic path фальшивой реализацией и не подменять static weights.
- Все перечисленные выше фоновые commands уже завершены. На момент этой записи выполняющихся jobs нет; следующий шаг запускать осознанно, а не ожидать отсутствующего уведомления.
- Полный default-feature build, RNNoise waveform/reference test и full audio integration пока **не подтверждены**.
- На момент предыдущего graph update: 4627 nodes / 10976 edges. Code-only graph не извлекает семантику legal docs; JSON/metadata zero-node warnings не скрывать. После новых файлов обновить ещё раз.
- Physical two-device/production acceptance недоступны в текущем local-only scope. Не публиковать preview под видом полной beta.

## Следующие этапы (не терять при compact)

1. Подготовить exact RNNoise weights / разрешить network blocker; затем actual default-feature compile, neural_local (RNNoise, DFN3, Silero, constant-name regression), waveform tests, fmt/Clippy/build. Нельзя зачесть diagnostic runtime-model как этот шаг.
2. Подключить owning-thread mic DSP, WASAPI capture/render, NativeAudioSource и bounded AudioMixer к voice/media/UI; immediate mute/deafen/moderation, per-user volume, devices/hotplug, settings, mic-test и AEC. Синтетические device-free и local media tests, затем UI parity.
3. Довести call/reconnect/cancellation responsiveness; chat delivery retries, bounded sync и tombstone lifecycle.
4. Camera, system audio, screen/stream routing, existing SFU contracts и remote control; .NET/Go не менять.
5. Полные settings/profile/GIF/crop/avatars и все UI/interactions/RU/EN states, без регрессий React reference.
6. Copy-based profile import, schemas, rollback/recovery, preserving originals.
7. Native packaging/updater/independent install helper/NSIS/signatures/licence bundle и legacy bridge. Existing Electron beta validator требует prefix `beta`; прямой `3.8.0-rust` им не принимается. Нужен compatible bridge либо explicit native install.
8. Full parity/regression/resource/latency/long-session acceptance и сохраняемые результаты; честно фиксировать недоступные проверки.
9. Только после полной готовности: commit/push `rust/native-client`, publish beta `3.8.0-rust`, без merge в main; затем штатное выключение Windows без force.

## Порядок checkpoint / compact

Сначала проверить этап и сохранить результаты/код/документы; обновить этот handoff и Graphify, сохранить commit. Затем штатным OpenCode V2 session compact поставить compact текущей сессии на безопасную границу. Session ID брать из текущего `OPENCODE_SESSION_ID`, не угадывать и не compact другую сессию. API route: `POST /api/session/{sessionID}/compact`, body `{}` или явный delivery; пользоваться `opencode api`, а не извлекать credentials.

После compact сначала восстановить архитектурную карту из Graphify, затем прочитать этот handoff, `docs/migration/native-client-status.md`, actual Git status/log и сохранённые результаты фоновых задач. Отделять завершённые checkpoints, WIP, блокеры и весь оставшийся план. Не изменять условия публикации/выключения из-за сокращённого контекста.
