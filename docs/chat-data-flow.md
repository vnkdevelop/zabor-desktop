# Карта данных чата и связи ZABOR / ZABOR chat and connection data map

> Редакция от 7 ноября 2026 года. Предыдущая редакция — от 6 ноября 2026 года.
> Техническое описание. Русская версия является основной.

Документ описывает, какие данные возникают при работе обмена сообщениями и связи, где они находятся, кому доступны и сколько хранятся. Он предназначен для внутреннего использования, для ответов на обращения пользователей и для подготовки ответов на запросы уполномоченных органов.

Соответствие коду проверяется при каждом изменении перечисленных файлов — см. раздел 6.

---

## 1. Общая схема

Native audio preferences применяются к shared controls сразу; local JSON writer и server sync независимы и используют bounded latest-wins snapshot. `SaveAudioSettings` получает один JSON string argument, не PCM/файлы; session epoch и owner не дают старому ACK подтвердить новый снимок. Local-only `nativeAudioSync` сохраняет owner/pending для восстановления после перезапуска и исключён из server payload. Language и unknown settings сохраняются. Relay-only читается при создании VoiceSession/ChatTransport; старые peers не пересоздаются и скрытие IP/анонимность не гарантируются. Полная UI/mic-test/physical acceptance остаётся отдельной незавершённой проверкой.

Native audio preferences apply immediately to shared controls; independent local JSON and server-sync tasks retain bounded latest-wins snapshots. `SaveAudioSettings` takes one JSON string, not PCM or files; session/owner guards prevent an old ACK from acknowledging new preferences. Local-only `nativeAudioSync` stores owner/pending state for restart recovery and is excluded from the server payload. Language and unknown settings are preserved. Relay-only is read when constructing VoiceSession/ChatTransport; existing peers are not rebuilt and IP hiding/anonymity is not guaranteed. Complete UI/microphone-test/physical acceptance remains unfinished.

Новое mic-test ядро пока не подключено к NativeAudio/VoiceSession/shell. Opt-in `MicProcessor::process_monitored` возвращает локальный float tap после existing gain/peak отдельно от restricted network PCM; mute/deafen/speaking и final submission guards не снимаются. `MicTest` собирает ровно240000 Float32 mono samples/48kHz (5s/960000bytes) в одном memory-only buffer, отзывает stale capture/playback по поколениям и признаёт playback progress только после validated successful-write ticket. Core не использует files/serde/hub/RTC; нового маршрута передачи или хранения чата/аудио не добавлено. Shared runtime/actual selected output/AEC combined-write wiring, native UI и полная приёмка ещё нужны.

The new microphone-test core is not yet wired to NativeAudio/VoiceSession/shell. Opt-in `MicProcessor::process_monitored` returns a local float tap after the existing gain/peak stage separately from restricted network PCM; mute/deafen/speaking and final-submission guards are retained. `MicTest` collects exactly240000 Float32 mono samples at48kHz (five seconds/960000bytes) in one memory-only buffer, revokes stale capture/playback generations, and advances playback only after a validated successful-write ticket. The core uses no files, serialization, hub or RTC APIs and adds no chat/audio transmission or storage route. Shared-runtime/selected-output/combined AEC-write wiring, native UI and complete acceptance remain pending.

WASAPI device runtime теперь переоткрывает только выбранный input/output endpoint на прежнем owning thread; `NativeAudioSource`, media track, peer subscriptions и mixer остаются прежними. Input/output selection epochs не позволяют старому маршруту отправлять PCM после смены настройки; output restart отзывает queued PCM и AEC reverse, не удаляя peer lanes. Endpoint inventory сверяется каждые 500 мс, явный selection — на каждом цикле; event stall 2 с запускает восстановление. Backoff 250–2000 мс, окно потери 30 с; непрерывные короткие отказы не сбрасывают deadline. После исчерпания окна существующий audio-failure workflow завершает voice session с явной ошибкой. Неизвестный browser ID временно использует default endpoint, original selection/profile не меняется. Устройства проверяются синтетически, физическая hotplug/AEC/latency-приёмка и settings/mic-test UI ещё нужны.

WASAPI device runtime reopens only the affected input/output endpoint on its existing owning thread; NativeAudioSource, media track, peer subscriptions and mixer remain stable. Input/output selection epochs reject PCM from superseded routes; output restart invalidates queued PCM/AEC reverse without deleting peer lanes. Inventory is reconciled every 500 ms and explicit selections every cycle; a 2-second event stall triggers recovery. Backoff is 250–2000 ms with a 30-second loss window; repeated short failures do not reset the deadline. Exhaustion uses the existing explicit voice-session audio-failure path. Unknown browser IDs temporarily use the default endpoint without changing the original selection/profile. Checks are synthetic; physical hotplug/AEC/latency acceptance and settings/microphone-test UI remain unfinished.

Нативные DSP/VAD-адаптеры подключены к VoiceSession/shell: owning-thread WASAPI → 480 mono samples/48 kHz → DSP → NativeAudioSource без дополнительной capture-очереди → libwebrtc. Received audio → bounded NativeAudioStream → peer-lifetime AudioLane → AudioMixer → WASAPI; AEC reverse пополняется после успешной записи устройства. Mute/deafen/moderation и capture/playback epochs отбрасывают устаревшие кадры; voice stop отзывает subscriptions и device threads. Runtime reset DFN3 клонирует исходное состояние с сохранением compiled plans на том же потоке, без повторной компиляции модели. Это кодовая интеграция и локальные испытания, не физическая или полная UI-приёмка: devices/hotplug/mic-test и outer WebAudio/4x peak-guard parity ещё требуют завершения. Модели вычисляются локально, телеметрия ONNX Runtime отключена. Подготовка сборки получает только публичные модели/библиотеки по HTTPS с проверкой закреплённых хешей. Пользовательский звук не передаётся этим источникам, аудиозапись на диск не добавляется; маршруты, шифрование и хранение чата/файлов не изменяются.

Native DSP/VAD adapters are wired to VoiceSession/shell: owning-thread WASAPI → 480 mono samples/48 kHz → DSP → unbuffered NativeAudioSource → libwebrtc. Received audio → bounded NativeAudioStream → peer-lifetime AudioLane → AudioMixer → WASAPI; AEC reverse is appended after successful device writes. Mute/deafen/moderation and capture/playback epochs discard stale frames; voice stop revokes subscriptions/device threads. DFN3 runtime reset clones its initial state, retaining compiled plans on the owning thread without recompiling the model. This is code integration/local testing, not physical or complete UI acceptance: devices/hotplug/mic-test and outer WebAudio/4x peak-guard parity remain unfinished. Model inference is local and ONNX Runtime telemetry is disabled. Build preparation obtains only public models/libraries over HTTPS and checks pinned hashes. User audio is not sent to those sources, no audio recording to disk is added, and chat/file routes, encryption and retention are unchanged.

Подготовка native build использует content-addressed кэш tarballs.nixos.org для RNNoise, media.xiph.org как резервный источник, NuGet и PyPI для runtime/build libraries. Источники видят сетевой адрес среды сборки, но не пользовательские данные. Оба RNNoise sources проверяются по одному SHA-256/размеру; C sources выбраны из той же ревизии, что использует существующий WASM. Модели и эти исходники не создают новый маршрут голоса/чата/файлов.

Native build preparation uses the tarballs.nixos.org content-addressed RNNoise cache, media.xiph.org as a fallback, and NuGet/PyPI for runtime/build libraries. Sources see the build environment's network address, not user data. Both RNNoise sources must match the same SHA-256/length; the C sources are selected from the existing WASM revision. Models and these sources add no new voice/chat/file route.

В ветке `rust/native-client` Slint-оболочка связана с SignalR JSON/WebSocket, авторизацией `CheckUserExists`/`Login`/`Register`, каналами, друзьями и native libwebrtc. Профиль `zabor-desktop-native-beta` хранит сессию/JWK под current-user DPAPI (`ZNS1`, контекст `zabor-native-v1`), настройки в JSON, историю/TOFU в SQLite. Текст и control packets используют DTLS/SCTP `zabor-chat-v1` с сигнализацией `zabor-chat:` либо P-256/AES-GCM fallback `SendChatRelay`/`ReceiveChatRelay`. История разделена по владельцу и очищается через 14 суток при входе и ежечасно; выход сохраняет историю/TOFU. Импорт старого профиля остаётся отдельной незавершённой частью.

Нативные файлы соединены end-to-end: выбор/перетаскивание → подтверждение → фоновый staging с сохранением оригинала → metadata `{name,size,sha256}` → `file-request` → DTLS/SCTP `zabor-chat-files-v1` → проверка offset/размера/SHA-256 → кэш → атомарная копия в выбранное место. Локальные source/storedName/savedPath не передаются в metadata; файл не отправляется байтами через SignalR. До 20 вложений за выбор, до восьми одновременных приёмов/отправок; очереди ограничены. Ошибка канала, отмена, удаление друга и 30-секундный простой завершают приём с очисткой `.part`. Повтор начинается заново. Сохранение file metadata не перезаписывает ACK/read. При старте восстанавливается состояние прерванных загрузок и очищаются именованные служебные partial files. File-deletion tombstones сохраняются в SQLite и очищаются через 14 суток после последнего пакета удаления. Показ внешней сохранённой копии использует проводник; очистка кэша не удаляет эту копию. Локальные проверки включают реальную передачу 2 MiB через два native SCTP peer, контроль целостности/отмены/повтора и атомарное сохранение.

In `rust/native-client`, the Slint shell is connected to SignalR JSON/WebSocket, `CheckUserExists`/`Login`/`Register`, channels, friends and native libwebrtc. The `zabor-desktop-native-beta` profile stores sessions/JWKs under current-user DPAPI (`ZNS1`, `zabor-native-v1` context), JSON settings and SQLite history/TOFU. Text/control packets use DTLS/SCTP `zabor-chat-v1` with `zabor-chat:` signaling or the P-256/AES-GCM `SendChatRelay`/`ReceiveChatRelay` fallback. History is owner-partitioned and expires after 14 days, with sign-in/hourly cleanup; sign-out retains history/TOFU. Legacy-profile import remains a separate unfinished component.

Native files are wired end-to-end: picker/drop → confirmation → background staging preserving the original → `{name,size,sha256}` metadata → `file-request` → DTLS/SCTP `zabor-chat-files-v1` → offset/size/SHA-256 validation → cache → atomic copy to the chosen destination. Local source/storedName/savedPath never enter transmitted metadata; SignalR does not carry file bytes. Selection permits up to 20 attachments and up to eight simultaneous receives/sends; queues are bounded. Channel failure, cancellation, friend removal and a 30-second stall terminate downloads and clean up `.part` files. Retry restarts from the beginning. Updating file metadata preserves ACK/read receipts. Startup recovers interrupted-download state and cleans named internal partial files. SQLite file-deletion tombstones expire 14 days after the latest deletion packet. External saved copies are revealed in Explorer and are not removed by cache cleanup. Local checks include a real 2 MiB transfer between two native SCTP peers, integrity/cancel/retry handling and atomic saving.

Клиентская beta может выполнять voicing autocorrelation в собственном Rust WASM-модуле внутри AudioWorklet. Используются те же 512-sample pitch ring, f32 storage/f64 accumulation и прежние кадры/пороги; доступен legacy fallback и запуск `--legacy-dsp`. Это не меняет маршруты голоса/чата/файлов, шифрование, server state или retention. UI и Electron не заменяются. Beta installer/update channel отделены от stable; новые stable installers не удаляют beta-профиль и не создают временную вторую копию истории.

Client beta may run voicing autocorrelation in its own Rust WASM module inside the existing AudioWorklet. The same 512-sample pitch ring, f32 storage/f64 accumulation and existing frames/thresholds are retained, with legacy fallback and a `--legacy-dsp` comparison mode. Voice/chat/file routes, encryption, server state and retention are unchanged. UI/Electron are not replaced. Beta installation/update channels are separate from stable; new stable installers do not remove the beta profile or create a temporary second history copy.

```
Отправитель                     Сервер Оператора                  Получатель
  |                                   |                                |
  |-- сигнализация (кто, кому) ------>|<---- сигнализация -------------|
  |                                   |                                |
  |=========== прямое соединение WebRTC (P2P) ========================>|
  |            защита транспорта DTLS, прикладного шифрования нет      |
  |                                                                     |
  |        при невозможности прямого соединения (чат):                 |
  |-- AES-GCM-256 (ключ по ECDH P-256) через сервер (SignalR) -------->|
  |        сервер видит метаданные, содержимого не видит                |
  |                                                                     |
  |        трансляция экрана при 3 и более активных зрителях (SFU):     |
  |-- WebRTC (DTLS-SRTP, 1 поток) --->|=== WebRTC (DTLS-SRTP) ========>|
  |        транзитная пересылка RTP в памяти узла SFU без хранения     |
```

Различие маршрутов принципиально: прикладное шифрование AES-GCM с ключом, выведенным по ECDH, применяется **только** при передаче сообщений чата через сервер. Прямое соединение защищено транспортным шифрованием WebRTC (DTLS). Пассивному серверу содержимое недоступно на обоих маршрутах, но механизмы разные, и проверка отпечатка ключа защищает только резервный маршрут; на прямом маршруте отпечаток DTLS согласуется через сервер сигнализации и вне канала не сверяется, поэтому активный сервер способен выступить посредником (MITM) — см. [CRYPTO-NOTICE.md](../CRYPTO-NOTICE.md), п. 4.7.

Ретрансляция (TURN) применяется, когда прямое соединение невозможно: трафик идёт через ретранслирующий узел, который технически видит сетевые адреса участников.

Для **трансляции экрана** (видео и системный звук стрима) применяется гибридная схема: при 1–2 активных зрителях поток передаётся напрямую (P2P), а при подключении 3-го и последующих активных зрителей (`>= 3`) автоматически переключается на селективную пересылку через серверный медиа-узел Оператора (**SFU**). При снижении числа активных зрителей до 1–2 раздача автоматически возвращается в P2P. Узел SFU расшифровывает транспортный пакет DTLS-SRTP от стримера и перешифровывает его в транспортные соединения DTLS-SRTP зрителей непосредственно в оперативной памяти (без декодирования видео/аудио и без записи на диск). Голосовая связь микрофонов всегда остаётся в режиме P2P.

---

## 2. Что обрабатывается на сервере Оператора

| Данные | Наличие | Назначение | Срок хранения |
| --- | --- | --- | --- |
| Пара участников обмена (кто с кем) | Да | Доставка сообщений | Определяется конфигурацией сервера |
| Время и порядковые номера сообщений | Да | Упорядочивание, синхронизация | Определяется конфигурацией сервера |
| Состояние доставки и прочтения | Да | Отображение статусов | Определяется конфигурацией сервера |
| Размер и контрольная сумма файла | Да | Проверка целостности передачи | Определяется конфигурацией сервера |
| Содержимое сообщений | **Нет** | — | — |
| Содержимое файлов | **Нет** | — | — |
| Голос (микрофон) | **Нет** | — | — |
| Трансляция экрана (видео и звук стрима) | **Транзит в ОЗУ (при >= 3 зрителях)** | Селективная раздача одного входящего потока нескольким зрителям через узел SFU | **Не хранится** (пересылается в оперативной памяти на лету, запись отсутствует) |
| Ключи шифрования переписки | **Нет** | — | — |

Технические журналы подключений (версия и канал сборки, время подключения, результат проверки подписи, сетевой адрес) ведутся отдельно от обмена сообщениями.

---

## 3. Что хранится на устройстве пользователя

Beta устанавливает `userData` и `sessionData` в отдельный профиль `zabor-desktop-beta` до получения single-instance lock и до чтения настроек. В нём находятся собственные IndexedDB, `chat-files`, настройки, сессия и `chat-identity.enc`; stable-профиль не открывается и не копируется автоматически. Beta-установщик использует отдельные appId, executable, ярлыки и ключ автозапуска; очистка данных ограничена beta-профилем текущего пользователя. Импорт/обратное слияние ещё не включены. Тесты используют отдельные аккаунты: раздельные профили не меняют серверное соответствие user → primary connection.

Beta sets `userData` and `sessionData` to a separate `zabor-desktop-beta` profile before obtaining the single-instance lock or reading settings. It holds its own IndexedDB, `chat-files`, settings, session and `chat-identity.enc`; stable is neither opened nor copied automatically. The beta installer uses separate appId, executable, shortcuts and autostart entry; data clearing is limited to the current user's beta profile. Import/reverse merging is not enabled yet. Tests use separate accounts: profile separation does not change the server's user → primary connection mapping.

| Данные | Место | Срок | Удаляется |
| --- | --- | --- | --- |
| История сообщений | `IndexedDB`, база `zabor-chat-v1`, хранилище `messages` | 14 суток от создания сообщения | Автоматически, фоновая очистка раз в час и при запуске |
| Полученные файлы | каталог данных приложения (`chat-files`) | До удаления пользователем | Кнопкой «очистить файл», удалением сообщения, очисткой данных приложения |
| Ключ шифрования переписки (закрытый) | `chat-identity.enc` в каталоге данных приложения | До удаления данных приложения | Вместе с данными приложения |
| Отпечатки ключей собеседников (доверие при первом контакте) | локальное хранилище, ключи `zabor_chat_key_<owner>_<friend>` | До очистки данных приложения | Вместе с данными приложения |
| Очередь отложенных удалений | локальное хранилище | До отправки собеседнику | При успешной отправке |
| Отметки о прочтении, состояние соединений | локальное хранилище и состояние приложения | В пределах сессии | При выходе из учётной записи |
| Данные сессии | `session.enc` | До выхода из учётной записи | При выходе |

Ключ переписки и данные сессии шифруются средствами операционной системы (DPAPI в Windows, Keychain в macOS) через `safeStorage`. Если операционная система не предоставляет шифрование, запись ключа не производится.

Выход из учётной записи (логаут) удаляет только данные сессии (`session.enc`) и очищает состояние приложения в памяти (открытые соединения, отметки о прочтении). История сообщений в базе `IndexedDB` `zabor-chat-v1` и полученные файлы в каталоге `chat-files` при логауте **не удаляются** и сохраняются между входами. Они удаляются только автоматической фоновой очисткой по сроку 14 суток, действиями пользователя (удаление сообщения, «очистить файл») или полной очисткой данных приложения. Благодаря этому после повторного входа история и файлы совпадают у обеих сторон.

---

## 4. Маршруты соединения

Обычный выход сохраняет также локальные отпечатки доверенных ключей собеседников: повторный вход не должен сбрасывать проверку ранее принятого ключа. Normal sign-out also retains trusted peer-key fingerprints: re-login must not reset verification of a previously accepted key.

Различаются состояния маршрутизации, определяемые приложением:

| Маршрут | Как обозначается | Защита содержимого | Что видит сервер |
| --- | --- | --- | --- |
| Прямое соединение | `p2p` | Транспортное шифрование WebRTC (DTLS / DTLS-SRTP) | Только сигнализацию: кто с кем устанавливает соединение |
| Ретрансляция | `turn` | Транспортное шифрование WebRTC (DTLS / DTLS-SRTP) | Сетевые адреса участников через ретранслирующий узел |
| Резервный маршрут через сервер (чат) | `signalr` | AES-GCM-256, ключ выведен по ECDH P-256 | Метаданные обмена; содержимое зашифровано и серверу недоступно |
| Селективная пересылка стрима (при >= 3 зрителях) | `sfu` | Транспортное шифрование WebRTC (DTLS-SRTP) на участках «стример — узел SFU» и «узел SFU — зритель» | Сетевые адреса стримера и зрителей; пакеты RTP проходят через оперативную память узла SFU для раздачи без записи и декодирования |

Приложение прибегает к резервному маршруту чата автоматически, если прямое соединение не установлено в течение 1,5 секунды. Принудительный режим «только ретрансляция» включается настройкой `zabor_relay_only_ice` и исключает прямое соединение.

Для трансляции экрана переключение на маршрут `sfu` происходит автоматически, когда количество активных зрителей достигает 3 и более (`>= 3`), и возвращается на `p2p` (или `turn`), когда активных зрителей остаётся 2 или меньше (`<= 2`).

Порог считает активных зрителей стрима в голосовом канале; микрофоны и личные звонки не переводятся на SFU. Публикация и подписки сериализуются, ответы SDP допускают серверное поле `sdp`. Для отключения прямого потока конкретному зрителю требуется подтверждение первого видеокадра (`SfuViewerReady` / `StreamViewerSfuReady`); до этого маршруты могут существовать параллельно. Для управляющего зрителя сохраняется P2P, при отказе SFU используется P2P/TURN. Для полного подтверждения готовности нужны обновлённые клиент и backend; смешанные версии могут сохранять параллельную передачу. Поколения SFU-сессий возрастают при повторном запуске, чтобы отложенная команда старой сессии не закрывала новую комнату.

При выходе отменяются незавершённые загрузки и ожидания отправки файлов, но история IndexedDB и завершённые файлы сохраняются. Подтверждение сохранения сообщения ожидает завершения транзакции IndexedDB, а не только успеха отдельного запроса. Загрузка истории привязана к владельцу и поколению сессии и объединяется с сообщениями, пришедшими во время чтения. Одна передача файла использует один выбранный канал данных; повторный запрос файла из истории остаётся доступен, пока исходная копия есть у отправителя.

The threshold counts active viewers in a voice channel; microphone audio and direct calls do not move to SFU. Publish and subscribe operations are serialized, and SDP replies accept the server's `sdp` field. Disabling a viewer's direct stream requires first-video-frame confirmation (`SfuViewerReady` / `StreamViewerSfuReady`); routes may coexist until then. A controlling viewer retains P2P, and SFU failure falls back to P2P/TURN. Full readiness confirmation requires updated client and backend versions; mixed versions may keep parallel transmission. SFU generations increase across restarts so delayed commands from an older session cannot close a new room.

Sign-out cancels unfinished downloads and blocked file sends but retains IndexedDB history and completed files. Message persistence waits for transaction completion, not merely request success. History loading is bound to the owner and session generation and merges messages received during the read. Each file transfer uses one selected data channel; files remain re-requestable from history while the sender retains the source copy.

Службы установления соединений: по умолчанию используются внешние службы STUN, а параметры ретрансляции и адреса медиа-узлов SFU выдаются сервером приложения. Внешние службы STUN технически получают сетевые адреса участников.

Личный звонок получает идентификатор восстановления только при объявленной поддержке обоих клиентов (`EnableCallResume`). После разрыва используются `LoginAndResume` и `ResumeCall`, проверяющие учётную запись, прежнюю пару и ограничение 45 секунд. При завершении, обычном входе без намерения возобновления или истечении срока запись недействительна; фоновая очистка освобождает состояние. Идентификатор находится только в памяти и не записывается в историю чата или `session.enc`.

Просматриваемое видео проверяется по росту `framesDecoded`: сначала запрашивается ключевой кадр, затем при продолжении остановки выполняется восстановление ICE либо возврат с SFU на P2P. Управление окном использует его текущие физические границы, управление экраном — границы выбранного монитора; ввод при недоступном источнике отклоняется. Запрос и подтверждение управления различаются в протоколе версии 2; одновременные противоположные роли на одном устройстве не захватывают системный ввод друг у друга.

Direct calls receive a recovery identifier only when both clients advertise support (`EnableCallResume`). `LoginAndResume` and `ResumeCall` check the authenticated account, previous pair and 45-second limit. Explicit termination, a normal login without resume intent, or expiry invalidates the entry; background cleanup releases the state. The identifier is memory-only and is not stored in chat history or `session.enc`.

Watched video is checked for progress in `framesDecoded`: a keyframe is requested first; a continuing stall triggers ICE recovery or a switch from SFU to P2P. Window control uses current physical window bounds, screen control uses the selected monitor bounds, and input is rejected when the source is unavailable. Control protocol v2 distinguishes requests from acceptance and arbitrates opposite roles on the same device.

---

## 5. Передача файлов

| Параметр | Значение |
| --- | --- |
| Предельный размер файла | Не ограничен приложением; ограничен свободным местом на устройствах участников |
| Размер блока передачи | чтение с диска блоками до 8 МБ, отправка кадрами до 192 КБ (ограничивается согласованным `maxMessageSize` канала данных), сборка у получателя блоками до 4 МБ перед записью на диск |
| Проверка целостности | SHA-256, сверяется у получателя |
| Хранение у получателя | Каталог данных приложения, до удаления пользователем |
| Пересылка файла третьему лицу | Не предусмотрена: файл сохраняется локально и передаётся повторно только по запросу из истории |
| Содержимое файла на сервере | Отсутствует |

Если отправитель удалил файл (или файл у него недоступен), получателю показывается пометка «файл удалён отправителем», а локальный кэш копии у получателя очищается; отдельно сохранённая получателем копия при этом остаётся нетронутой. Пока файл есть у отправителя, получатель может запросить его повторно из истории.

---

## 6. Обязанность синхронизации документа с кодом

Документ считается **устаревшим и вводящим в заблуждение**, если изменены перечисленные ниже файлы, но настоящий документ не обновлён:

| Файл | Что фиксирует документ |
| --- | --- |
| `src/renderer/src/chat/chatDb.ts` | Название базы, хранилища, срок хранения 14 суток |
| `src/renderer/src/chat/chatCrypto.ts` | Алгоритмы ECDH P-256 и AES-GCM-256, место хранения ключа |
| `src/renderer/src/services/chatPeer.ts` | Маршруты и их защита, резервный режим, пределы размеров, отпечатки ключей, срок хранения |
| `src/renderer/src/services/webrtc.ts` | Состав служб STUN и TURN для голосовой связи, гибридная маршрутизация трансляции экрана (P2P при 1–2 зрителях и SFU при 3 и более зрителях) |
| `src/renderer/src/services/signalr.ts` | Сигнализация, выдача параметров ретрансляции, сигнализация переключения P2P/SFU, резервная передача |
| `src/main/chatFiles.ts` | Место и правила хранения файлов на диске |
| `src/main/index.ts` | Шифрование ключа переписки и данных сессии, размер ограничения |

Изменение перечисленных файлов в части **хранения данных, шифрования, маршрутизации и сроков** требует одновременного обновления:

- [PRIVACY.md](../PRIVACY.md) — разделы 3.3, 4, 6;
- [CRYPTO-NOTICE.md](../CRYPTO-NOTICE.md) — разделы 2–4;
- [TERMS.md](../TERMS.md) — разделы 5, 7;
- настоящего документа.

---

Copyright © 2026 vnkdevelop.
