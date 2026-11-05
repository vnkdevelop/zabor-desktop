# Rust client beta: итог выпуска 2026-11-06

## Выпущено

| Версия | Назначение | Commit установщика | SHA-256 |
| --- | --- | --- | --- |
| `3.7.2-beta.rust.1` | legacy bridge для старого beta updater | `1899745a997a0bbc33f5aa16b714de77d91a045e` | `b796617c3aefab3ff71c52e185e48e047a53328e91faf495057e5325a33b662c` |
| `3.7.2-beta.rust.2` | Rust voicing default, legacy A/B launch | `59d1105887908d976f3bbda726ab4d9d941f7376` | `fdad9796e352159649b8c1fb094cffe273f916a80938d7b9c0373c430b4bedad` |

- Bridge: <https://github.com/vnkdevelop/zabor-desktop/releases/tag/v3.7.2-beta.rust.1>
- Rust client: <https://github.com/vnkdevelop/zabor-desktop/releases/tag/v3.7.2-beta.rust.2>
- Оба — опубликованные GitHub **prereleases**, не drafts и не stable latest. Stable latest после выпуска остался `v3.7.1`.
- Локальные installers: `release/beta/ZABOR-Beta-Setup-3.7.2-beta.rust.1-x64.exe` (165545664 bytes) и `release/beta/ZABOR-Beta-Setup-3.7.2-beta.rust.2-x64.exe` (165541007 bytes).
- Remote asset sizes/digests сверены с локальными installers; manifest и packaged commit/runtime проверены. Git tags не двигались. Release provenance metadata указывает на существующие точные beta commits, не на main.

## Реально проверено

- Полный TypeScript typecheck; 176 desktop tests; 20 updater/release compatibility tests; 4 readiness tests; 4 Rust unit tests и 10 native numerical fixtures; 4 client WASM/PCM/state/fallback/lifecycle tests.
- Отдельный настоящий Electron AudioWorklet подтвердил работу Rust, тот же lag/score и активные frames. Это больше, чем только mock или native Rust-only benchmark.
- Оба установщика собраны NSIS после проверок. Батник `release-beta.bat` использован для bridge, `release.bat` auto-channel — для Rust beta. Установка на пользовательские устройства не выполнялась.
- Публичные manifests скачаны заново: beta-0 validator принимает bridge (`enabledModules: []`), не принимает Rust beta напрямую; современный validator принимает Rust (`rust_voicing`). Проверен путь beta-0→bridge→Rust без ложных flags.
- EOL-normalized source fingerprints устраняют Windows/Linux Git CRLF/LF mismatch. Секреты `.env` не копировались в worktree и не публиковались в исходниках/отчёте.
- Публикация проходит через draft и проверку artifacts; после обнаружения особенностей GitHub draft lookup parser исправлен. Ни неполный draft, ни неподтверждённые assets не назначались stable latest.
- CI не является вторым publisher и не перезаписывает assets релизов, созданных батником.

## Клиентская архитектура и сравнение

Это частичный Rust DSP клиент, **не новая нативная оболочка**: Electron, React, codecs, capture, DeepFilter/RNNoise/Silero, .NET backend и Go SFU сохранены. Backend Rust отменён владельцем.

Voicing выполняет одну block operation на pitch window, использует shared `zabor-dsp` math, f32 storage/f64 accumulation, preallocated views и фиксированную memory 256 KiB на instance. Compiled module готовится до worklet processing, есть guards context/epoch и explicit dispose. Legacy ring поддерживается постоянно без второго постоянно работающего voicing path; на отказе Rust fallback не стартует с пустой истории.

В шестиповторном synthetic benchmark, включая JS/WASM copy/call: voicing p50 **45.4→25.7 us** (около 43% меньше времени), total kernel около 48% меньше. В full own processor с identity model total **8409.70→8033.03 ms** (около 4.5% меньше). Эти результаты **не** измеряют CPU/RAM всего приложения, реальную neural inference, network/mouth-to-ear latency или все hardware cases. Подробнее: [client-rust-comparison](client-rust-comparison-2026-11-05.md).

## Git и будущие выпуски

- В обычной папке сохранена ветка `beta/rust-migration`; изменения включены проверенным fast-forward. Main/`origin/main` остались на `a4934d378a498fadecbd680e381bad794546634e`.
- Старый локальный ignored `release.bat` предварительно сохранён в `migration-artifacts/release-bat-before-rust-migration.bat`; он не потерян при включении нового tracked wrapper.
- `release.bat` выбирает канал по main/beta, `release-beta.bat` принудительно beta. Unknown files не индексируются, stash/rebase/reset/force-push не выполняются; public tags/assets не перезаписываются. Build-generated helper modifications восстанавливаются только если их не меняли извне.
- Stable promotion **не выполнена**. `client-release-approval.json` остаётся pending; Rust stable требует актуального owner-approved client source fingerprint. Promotion helper по умолчанию — plan only, explicit apply требует двух устройств, data review и app-closed.
- Forward data-schema/module metadata относится к новому пакету, не включает неизвестный модуль в старом runtime. Отдельные owner/compatibility gates для настоящего изменения хранилища не отменяются.

## Что сделать владельцу

1. На двух устройствах поставить Rust beta `.2`, использовать два разных тестовых аккаунта.
2. Проверить голос/модели/устройства/калибровку/стрим/управление/чат/файлы/reconnect/update/uninstall по [матрице](client-beta-testing.md).
3. Для A/B полностью выйти и запустить Start-menu `ZABOR Beta (legacy dsp)` либо `resources/start-legacy-dsp.bat`.
4. Прислать сценарии и локальные diagnostics для исправлений. Не копировать реальный stable userData вручную — supported profile import ещё не включён.
5. Только после исправлений, data/rollback review и согласованной beta-стабильности выполнять main promotion и отдельный stable release. Два устройства не доказывают все NAT/TURN/GPU/SFU/long-session случаи.

## English summary

Bridge `.1` and Rust-client `.2` are published prereleases with verified local/remote digests and exact existing beta commit provenance. Stable latest is still `v3.7.1`, main is unchanged, and stable acceptance remains pending. The old beta validator accepts the truthful legacy bridge; the updated validator then accepts Rust beta. The actual worklet Rust path, numeric/PCM/state parity, fallback/lifecycle, full typecheck, regression and NSIS packaging passed. Only voicing is migrated; whole-client resource/network gains are not claimed. One batch publisher owns releases, preserves Git history/unknown work, validates drafts and avoids CI upload races. Owner two-device, data, media and prolonged tests remain mandatory before any stable/main promotion.
