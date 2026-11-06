# Нативный клиент Rust / native Rust client

Редакция / updated: 2026-11-07.

## Решения владельца для полного переноса

Подтверждено 2026-11-06: целевые ОС — Windows 10/11 x64. Приложение должно быть нативным Rust без Electron/WebView; сторонние нативные C/C++ медиабиблиотеки (включая libwebrtc и Opus) допустимы. Интерфейс и поведение сверяются с действующим React-клиентом. Beta использует отдельный профиль с импортом-копированием и сохранением исходного профиля. В автономном проходе разрешены только локальные интеграционные проверки. Этапы разрешено фиксировать коммитами в `rust/native-client`; после готовности полноценного клиента — push ветки и публикация GitHub beta `3.8.0-rust`. Автоматическое слияние в main не согласовано. Backend остаётся .NET, SFU — Go.

Owner decisions confirmed on 2026-11-06: target Windows 10/11 x64; a native Rust application without Electron/WebView, with native C/C++ media libraries allowed. Preserve the React client's UI and behavior. Use a separate beta profile with copy-based import that preserves the source. Unattended integration checks are local only. Checkpoint commits on `rust/native-client` are authorized; push and the `3.8.0-rust` GitHub beta release follow completion of the full client. Automatic merging to main is not authorized. The backend remains .NET and SFU remains Go.

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

Ниже сохранён состав ранее проверенного updater checkpoint:

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
| N5 | Нативные capture/playback, WebRTC/codecs, DSP, neural models, stream/SFU, remote control | Транспорт/PCM/SCTP проверен локально; channel/call и screen/video wiring экспериментальные; neural DSP, camera/SFU/control ещё нужны |
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
