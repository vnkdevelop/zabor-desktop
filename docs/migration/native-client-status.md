# Нативный клиент Rust / native Rust client

Редакция / updated: 2026-11-07.

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

Пять сравнений с неизменённым TypeScript прошли: tonal notches/EQ/deesser,
manual threshold/hold/lookahead/makeup, model-input gain/ALC, manual SmartGate
и voicing/attack/release/preroll на одинаковых VAD-решениях. Четыре теста микшера
проверили per-user/global volume, deafen/focus без повтора backlog, замену/отзыв
peer и ограничения PCM/AEC reverse queues. Последний полный прогон без neural
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
