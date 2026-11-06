# Beta: idle audio ownership and installation progress

Редакция / updated: 2026-11-06. Статус: исправления в изолированном `rust/client-beta` worktree; новые installer assets пока не опубликованы. Уже опубликованные `.1` и `.2` не изменены.

## Снимок установленной beta до исправлений

Read-only measurement: `migration-artifacts/client-idle-before-2026-11-06.json`. Пользователь сообщил, что приложение вне голосового канала; канал/настройки внутри renderer этим диагностическим инструментом не читались. Использована private working set, не сумма working set с повторно учитываемыми shared pages. CPU — короткий process sample, не длительный benchmark.

| Процесс | Private working set, MiB | CPU sample, % |
| --- | ---: | ---: |
| Основной / browser | 72.88 | 0.01 |
| Renderer | 200.98 | 4.01 |
| GPU | 235.25 | 1.23 |
| Network service | 7.33 | 0.00 |
| Video capture service | 21.43 | 0.00 |
| Audio service | 6.45 | 0.13 |

Это **не before/after сравнение нового установщика**: исправления ещё не установлены на пользовательский профиль. В частности, 235 MiB GPU allocation нельзя объявлять устранёнными переносом voicing в Rust. Electron/GPU overhead остаётся; hardware acceleration, качество изображения и безопасность не отключались.

## Что изменено

- На старте приложение перечисляет аудиоустройства, но не создаёт полный processed microphone/DSP/output graph заранее. При необходимости получения названий устройств кратковременный raw capture сохраняет прежний permission/fallback путь.
- Подготовка моделей не запускается из одних настроек без worklet, старта аудио или калибровки. Маленький compiled Rust voicing module и model-byte cache сохраняются для повторного старта; inference worker и AudioContexts больше не должны оставаться постоянно работающими без потребителя.
- После 10 секунд без канала/звонка/peer/SFU/стрима/теста/калибровки освобождаются processed stream, Rust worklet instance, VAD worker и output mixer. Это grace period для быстрого повторного входа, не пауза работающего соединения.
- Видимый измеритель уровня сохраняет лёгкий raw capture, а тест микрофона/калибровка удерживают необходимый DSP. Закрытие тестовой панели освобождает preparation lease; сам факт «не в канале» недостаточен для idle, если пользователь работает со звуком в настройках.
- Повторный вход отменяет idle timer; старт нового графа ждёт уже начатую очистку. Новый receiver, появившийся во время асинхронного закрытия input context, защищает свой output mixer. Idle cleanup не вызывает network teardown и не отключает SignalR/чат.
- Background microphone startup стал single-flight. Для VAD добавлены generation/owner guards: поздний callback отменённого worker не может завершить или пометить готовым новый worker. Удерживаемая ссылка на старый calibrated gain/context очищается явно; calibration/level seeds сохраняются.
- При активной работе Silero входные tensors/feeds переиспользуются на тех же фиксированных buffers. Модель, sample rate, gain/saturation, state/reset, queue limit и speech thresholds не изменены. На 320 кадров создано **3 input tensors вместо 641** и один feeds object вместо 320. Это доказательство меньших аллокаций, не процент ускорения всего клиента.

## Холодный вход и быстрые переходы между каналами

Удаление startup prewarm без дополнительной защиты могло бы вернуть прежний стартовый дефект: `startLocalStream()` ранее завершался до полной готовности DSP. Теперь cold start имеет отдельный admission gate:

1. Созданный smart processed track выключен для передачи. Raw input продолжает поступать локально в DSP; возможность доставки неподготовленных кадров через ранний peer/offer исключается независимо от порядка сетевых callbacks.
2. Worklet подтверждает фактическую готовность модели и 32 последовательных обработанных свежих кадра. Проверяются frame length 480, clock 48 kHz, отсутствие overflow/накопленной очереди и больших стартовых пропусков callbacks. Также сравниваются wall-clock duration и длительность обработанного аудио в этом окне: быстрый catch-up burst не считается готовностью и начинает окно заново. Проверка учитывает batching render quanta; она не меняет playback rate. Пустой результат denoiser не считается подготовленным кадром.
3. Только затем `startLocalStream()` возвращает успех и применяет актуальный mute/server-mute к передающему track. Ошибка/неверная частота/timeout не включают неподготовленный микрофон. Первичная подготовка при mute выполняется локально с нулевым выходом, после чего возвращается прежний дешёвый muted path.
4. Звук успешного входа в канал проигрывается после готовности микрофона и принятого `JoinChannel`, а не в момент нажатия. Состояние joining сохраняется во время подготовки. При выходе во время подготовки отменяется и VAD-loading, и ожидание worklet; поздняя готовность старого графа не включает новый track и не трактуется как mic-failure новой операции.

32 кадра — pre-roll **перед передачей**, не новая постоянная задержка звонка и не растяжение/ускорение PCM. Существующие lookahead/VAD задержки не удалялись и не объявляются нулевыми. В тесте подготовка не изменяет unmuted PCM sample-for-sample.

Прямой A→B→C при том же устройстве/модели/настройках использует тот же живой `AudioContext`, raw/processed track и DSP instance. `micGraphSignature` не зависит от channel ID; готовность кешируется для конкретного worklet. Повторный pre-roll/пересоздание/смена sample rate при таком переходе не выполняются. После обычного выхода действует 10-секундный grace period; только после настоящей idle-очистки следующий вход становится холодным и снова проходит admission gate.

Начальный настоящий Electron прогон с production RNNoise/DeepFilter worklets и synthetic oscillator, при выключенном Silero в этой fixture, дал приблизительно **377 ms / 715 ms** от подготовки до подтверждения. Это не обещание одинакового времени на всех ПК, не запись пользовательского голоса и не измерение network/mouth-to-ear latency. Команда дополнительного production-worklet smoke после сборки: `cmd.exe /c npm run test:neural-startup`.

Дополнительно проверена подготовка через выключенный `MediaStreamAudioDestination` без вывода в физические динамики: DSP продолжает готовиться, но destination остаётся muted. Начальный raw result с source hashes сохранён в `migration-artifacts/client-neural-startup-2026-11-06.json`; результаты относятся к своей редакции source и не являются CPU/RAM benchmark.

Финальная редакция с проверкой темпа окна повторно проверена на production worklets: RNNoise **681 ms**, DeepFilter **1171 ms** до допуска передачи; обработанное окно — 310 ms аудио при wall-clock span 310/301 ms. Результат: `migration-artifacts/client-neural-startup-final-2026-11-06.json`, с hashes актуальных `webrtc.ts`/`mic-pipeline-core.ts`. Более поздний допуск относительно ранних прогонов — намеренное ожидание стабильного clock после startup burst, не снижение скорости воспроизведения. Эти single-run synthetic цифры не заменяют измерение на реальном микрофоне, с Silero, сетью и принимающим устройством.

Причина исторического «басистого/быстрого/медленного» голоса не считается доказанной одним описанием. Admission gate закрывает конкретный риск неподготовленного DSP и стартовой очереди; реальные driver/AEC, ULL clock/jitter и receiver случаи остаются отдельной проверкой.

## Окно установки

Beta NSIS больше не завершает splash до запуска новой версии. В начале установки используется helper из нового installer, с PID именно этого installer и отдельным window title. Перед завершением старого приложения установщик ограниченно ожидает видимость нового окна; старый helper закрывается только после появления replacement splash.

Beta helper ждёт завершения installer и видимого окна нового `ZABOR-Beta` процесса, созданного после старта helper. Старый процесс/renderer без окна не считаются завершённым обновлением. Убрано применение 15-секундного auto-close к beta; прежний общий safety timeout 10 минут и возможность закрытия пользователем остаются. Stable ветка поведения и её NSIS hooks не изменялись. Уже опубликованные `.1`/`.2` не перезаписываются: эти исправления должны войти в следующий beta installer.

## Проверки

- TypeScript typecheck и production Electron/Vite bundle.
- 176 desktop regressions; 22 updater/release tests, включая настоящий C# compile и headless native close-policy execution.
- Idle lifecycle fixtures выполняют методы из актуального source: channel/call/peer/SFU/stream/test/calibration guards, отмена при rejoin, receiver во время cleanup, single-flight capture, VAD stale completion, dispose и сохранность level seeds.
- Настоящая Silero ONNX WASM-модель в изолированном Node fixture: 320 одинаковых synthetic frames, silence/tone transitions, sequence gap/reset, одинаковые probability metadata и финальное recurrent state. Тест не использует пользовательскую запись и не заменяет проверку полной neural/audio цепочки в канале.
- Rust WASM numerical/PCM/VAD/state/fallback/lifecycle tests и отдельный настоящий Electron AudioWorklet smoke.
- Cold admission fixtures: запрещённое раннее включение track, stale ready/неверный clock/timeout, отмена во время VAD loading, настоящий `switchChannel()` A→B→C с сохранением графа и отложенный join sound; одинаковый unmuted PCM, muted pre-roll, reset после старта с stall/catch-up burst и отказ при пустом denoiser output.
- Production RNNoise и DeepFilter выполнены в настоящем Electron AudioWorklet, не только mock; их frame/clock/runtime readiness проверяется отдельно от полной сетевой сессии.
- NSIS include скомпилирован в изолированной fixture. Fixture installer **не запускался** на пользовательском компьютере; это syntax gate, не выполненное обновление профиля.

## Оставшиеся gates

Следующий beta installer необходимо отдельно выпустить без изменения существующих assets, затем измерить private working set/CPU на двух устройствах: после холодного запуска, после 10–30 секунд idle с закрытыми audio settings, в активном канале и после нескольких join/leave циклов. Проверить холодный старт модели/первые слова, mic test/calibration, hotplug, mute/unmute, incoming call, SignalR reconnect с живым медиа, SFU/P2P и обновление из `.1`/`.2`.

Уменьшение времени/частоты inference, смена модели/кодека/битрейта, отключение AEC/hardware acceleration или полная замена Electron не выполнялись. Нельзя обещать конкретное снижение 600 MB либо CPU активного клиента без новых device measurements. Main/stable promotion остаётся pending.

## English summary

The installed-beta process snapshot shows roughly 201 MiB renderer and 235 MiB GPU private working set; it is a baseline, not an after measurement. The patch removes unconditional startup DSP construction, adds demand-driven idle disposal with a 10-second rejoin grace period, single-flight capture and VAD generation guards, and protects a receiver arriving during asynchronous cleanup. SignalR request contracts, ICE/TURN, codecs, neural models and quality parameters are unchanged. Cold smart-microphone transmission remains disabled until the real worklet confirms model readiness, 32 fresh processed frames and the correct 48 kHz/480-sample clock. This is pre-roll before transmission, not a new permanent audio delay or PCM time stretching. Ready A→B→C switches preserve the same live graph/tracks and do not repeat cold preparation; cancelled loading cannot enable a stale graph, and successful join sound now follows readiness/accepted membership. Silero reuses its input tensors: 3 versus 641 allocations across 320 frames with matching real WASM-model probabilities/state, not a whole-client CPU improvement claim. Production RNNoise/DeepFilter startup is also tested in Electron with synthetic input; real driver/AEC, first-word and receiver/jitter checks are still required. The new beta installer owns its progress helper and waits for the replacement application window rather than killing progress before relaunch. Published bridge/Rust `.1` and `.2` remain immutable. Real installation, two-device cold-start/active/idle measurements and prolonged connection/media gates remain pending before any stable/main promotion.
