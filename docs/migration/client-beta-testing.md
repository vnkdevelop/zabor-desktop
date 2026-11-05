# Клиентская Rust beta: выпуск, два устройства и дальнейший stable

Редакция / updated: 2026-11-06.

## Границы этой beta

- .NET backend и Go SFU не переписываются; Rust backend отменён владельцем.
- Перенесён собственный voicing autocorrelation в WASM AudioWorklet. Остальные DSP/model/media adapters, Electron и React сохранены. Это не обещание полностью нативного Rust-клиента.
- Матрица проверяет одинаковые PCM output, VAD windows/sequence/state и lag; Rust использует f64 accumulation и фиксированную память 256 KiB. JS/WASM переход выполняется на pitch window, не на каждом отсчёте. При отказе Rust используется актуальный legacy ring, без второго постоянно работающего DSP.
- Импорт stable-профиля пока не включён. Для испытаний по умолчанию нужны **два разных тестовых аккаунта**; одновременные входы одного аккаунта могут вытеснять друг друга.

## Версии и старый updater

1. `3.7.2-beta.rust.1` — legacy bridge: честный manifest с `enabledModules: []`, новый updater умеет проверять metadata будущих модулей. Старый beta-0 updater пропускает Rust beta, но принимает bridge.
2. `3.7.2-beta.rust.2` — Rust voicing по умолчанию, manifest `enabledModules: ["rust_voicing"]`. Bridge обновляется на неё обычным beta updater.
3. Bridge-релиз и его immutable assets сохранять, пока поддерживаются старые beta-0 установки. Не менять tag, installer или manifest уже опубликованной версии.

Stable читает stable `/releases/latest`, beta — пагинированный список prereleases с channel/architecture/source data-schema window/checksum. Beta никогда не назначается GitHub latest и не переключается на stable автоматически. Поля модулей описывают **новый пакет**, а не включают неизвестный код в старом клиенте. Только один publisher создаёт/публикует release; CI проверяет и сохраняет artifacts, но не загружает конкурирующую сборку в release.

## A/B на том же профиле

1. Запустить обычный `ZABOR Beta` и проверить стандартные сценарии; фактический runtime сообщается в console сообщением `Mic voicing runtime`.
2. Полностью выйти из zabor, не просто свернуть в трей.
3. Запустить Start-menu shortcut `ZABOR Beta (legacy dsp)` или `resources/start-legacy-dsp.bat`. Аргумент `--legacy-dsp` выбирает legacy без изменения account/data schema, UI, модели, codec, resolution или bitrate.
4. Повторить одинаковые сценарии и настройки. Обычный следующий запуск вновь выбирает build default. Режим не переключается mid-call/mid-transfer.

## Матрица двух устройств

| Сценарий | Что проверить |
| --- | --- |
| Вход и профиль | разные test accounts, друзья/заявки, профиль/аватар/достижения |
| Микрофон | manual/auto, DeepFilter/RNNoise, Silero/WebRTC VAD, NS on/off, calibration, mic test, mute/deafen |
| Устройства | default/USB/Bluetooth, смена input/output во время звонка, hotplug, permission/busy errors |
| Звонки | одновременно start/accept/end, stop во время reconnect, краткий разрыв SignalR при живом медиа, resume |
| Стрим | окно/монитор/камера, профили качества, системный звук, source close/change, PiP/fullscreen, DPI |
| Управление | request/accept/revoke, вмешательство владельца, panic/reset, удерживаемые клавиши, restart стрима |
| Чат и файлы | offline send/delete/read, duplicate/ACK, повторная загрузка из истории, cancel, крупный файл одновременно с голосом |
| Updater | beta-0→bridge→Rust beta, beta→beta, отмена/retry, mismatch checksum, ручной предыдущий совместимый beta installer |
| Соседние установки | stable и beta имеют разные ярлыки/профили; install/uninstall одного не удаляет другого |
| UI | те же иконки/цвета/границы/анимации, оба языка, модалки/меню, одинаковые window size и DPI |

Для полноценной проверки SFU перехода 2→3→2 нужны три активных зрителя; двух устройств для этой строки недостаточно. Не считать успех пары устройств доказательством всех NAT/TURN/GPU/network случаев. Перед приёмкой нужны длительные прогоны и повторная проверка соответствующих gates после существенных исправлений.

## Git и один батник

- `release.bat`: на `main` stable, на `beta/rust-migration` beta; в другой ветке — отказ. `release-beta.bat`: только beta.
- Перед запуском код должен быть проверен и закоммичен. Unknown files не индексируются автоматически, branches не переключаются, rebase/autostash/reset/force-push не выполняются.
- Батник спрашивает версию и подтверждение публикации. Изменение версии коммитит только package/lock fields; существующие public tags/releases не двигаются и не перезаписываются.
- Установщик и manifest привязаны к commit/version/channel и проверяются перед публикацией draft. Ошибка upload оставляет draft для безопасного retry, не публикует неполную версию. Build-generated helper binaries восстанавливаются только если их не меняли извне.
- Старый локальный `release.bat` перед включением нового tracked wrapper сохраняется в `migration-artifacts` исходной папки. Оператор может сравнить/восстановить его; он не теряется из-за ignored-file overwrite.

## Переход в main — позже

Не запускать сейчас. После двух устройств, фиксов и data/rollback review, при закрытом приложении:

```text
cmd.exe /c promote-beta.bat
cmd.exe /c promote-beta.bat --apply --owner-approved --data-gates-approved --app-closed
cmd.exe /c release.bat
```

Первый вызов — plan only. Apply допускает только clean `main`/`beta/rust-migration`; main должен быть предком tested beta, иначе нужно интегрировать stable fixes в beta и повторить тесты. Только explicit apply переключает beta→main и делает fast-forward, готовит stable version и acceptance fingerprint; сам stable release не публикует. Publisher main с Rust default требует текущего owner-approved client source fingerprint. Если client source меняется, approval не считается действительным.

После stable релиза main changes следует вернуть в beta обычным проверенным merge/fast-forward на чистом дереве; publisher не выполняет скрытую синхронизацию. Следующая beta от stable version получает новую базовую patch/version и suffix. Реальные изменения данных beta-периода не переносить простым восстановлением холодного snapshot — нужен подтверждённый supported merge/import либо явно ограниченный test-account сценарий.

## English summary

The Rust backend rewrite is cancelled. This beta keeps Electron/React, .NET/Go and media/model adapters; only voicing autocorrelation runs in the fixed-memory Rust WASM AudioWorklet. Use two distinct test accounts and the normal versus `--legacy-dsp` launch mode with the same settings/profile. Beta-0 updates through a truthful legacy bridge before Rust beta; prerelease assets stay immutable and are never stable latest. One guarded batch publisher chooses the channel from main/beta, does not stage unknown work or rewrite Git history, and validates draft artifacts/manifests. CI does not race the publisher. Main/stable promotion is a later explicit, app-closed, owner/data-approved fast-forward and fingerprinted acceptance step, not an automatic beta release action. Profile import, real devices/networks and prolonged tests remain separate gates.
