# Rust migration: состояние первого checkpoint / first checkpoint status

Редакция / updated: 5 ноября 2026 года.

## Исходное состояние

Обновление направления: перепись backend на Rust отменена владельцем. Текущий этап — клиентская voicing WASM beta и безопасные release/updater workflows; .NET backend и Go SFU остаются прежними. Подготовка двух устройств описана в [client-beta-testing.md](client-beta-testing.md).

- План владельца: `zabor-rust-migration-beta-plan-2026-11-05.md`, разделы 0, 6, 18, 23.
- Desktop baseline: `a4934d378a498fadecbd680e381bad794546634e`, версия `3.7.1`.
- Baseline-тег: `migration-baseline-2026-11-05`; история не переписывалась.
- Создана отдельная `beta/rust-migration` от актуальной `main`, ветка опубликована.
- Текущая версия-кандидат: `3.7.2-beta.rust.0`. Это подготовка beta-0, не опубликованный релиз и не утверждение о завершении P0.
- Backend-каталог по-прежнему не является Git-репозиторием. До его изменения нужен отдельный проверенный checkpoint; исходники обследованы только для карты контрактов.
- Production-серверы, .NET backend и Go SFU не менялись.

Последующий checkpoint в изолированной ветке `rust/p0-backend-bench`: [P0 readiness](p0-readiness-backend-experiment.md). В нём закрыты перечисленные ниже исторические renderer type blockers и выполнен [сравнительный backend-kernel эксперимент](backend-rust-benchmark-2026-11-05.md). Открытая сборка из `beta/rust-migration` не заменяется автоматически; этот документ сохраняет результаты первого checkpoint отдельно от последующих gates.

## Выполнено

1. Использованы оба графа знаний, затем выполнена структурная инвентаризация desktop/backend. `migration-artifacts/inventory.json` содержит актуальные количества и пути исходников (включая Rust), символы, IPC/SignalR/UI/test вызовы, хеши assets/существующих установщиков и обоих lock-файлов. Динамические вызовы явно помечены; это навигационный каталог, не доказательство функционального паритета.
2. Добавлены корректное сравнение SemVer, числовых prerelease identifiers, `v`-префиксов и build metadata; совместимость legacy четырёхкомпонентного сравнения сохранена.
3. Stable updater продолжает использовать stable endpoint, но не принимает prerelease. Beta использует пагинированный список releases и обязательный manifest с commit, channel, architecture, schema и SHA-256. Отсутствие совместимой beta не переключает клиент на stable.
4. Beta получает отдельные appId, executable, install directory, shortcut, autostart entry, userData/sessionData, updater temp directory и splash namespace. Внешний вид React, иконки и модели не изменены. Для новых ошибок beta updater добавлены только соответствующие RU/EN строки в нижнем регистре; старые тексты и оформление сохранены.
5. `release-beta.bat` теперь отслеживается Git и вызывает beta-only pipeline: без `git add .`, rebase/autostash, автоматического коммита или push неизвестной ветки. Stable packaging заблокирован вне main. Публикация разрешена только как prerelease, без назначения latest.
6. Добавлен Cargo workspace: собственные `zabor-core` и `zabor-dsp`, без сторонних crates, без изменения лицензии. Rust DSP использует f32 storage / f64 accumulation, фиксированные буферы и блочные вызовы. Коэффициенты первого прототипа поступают из действующей legacy-реализации.
7. Офлайн parity harness извлекает именно действующие `decimateVadSample` и `measureVoicing` из AST TS, а не их упрощённую копию. Проверяются silence, warmup, pitch boundaries, quiet tones, harmonics, impulses/clipping, high-frequency rejection и deterministic noise.
8. Исправлена команда typecheck: проверяются реальные main/renderer проекты. Для main добавлены уже используемые `src/shared` файлы. Splash запускается только после проверки установщика; несуществующий вызов `exec` больше не используется.
9. Синхронизированы `PRIVACY.md`, `TERMS.md`, `CRYPTO-NOTICE.md`, `docs/chat-data-flow.md` (RU/EN): отдельный beta-профиль, отсутствие автоматического импорта и прежние encryption/retention rules.

## Проверки

| Проверка | Результат |
| --- | --- |
| Baseline desktop tiers 1–8 | 176/176 |
| Tiers 1–8 после изменений | 176/176 |
| `tsc --noEmit -p tsconfig.node.json` | проходит |
| `npm run build` | проходит; DeepFilter WASM/model hashes проверены |
| `npm run dist:win:beta` | проходит; локальный NSIS и manifest привязаны к проверенному commit, SHA-256 установщика проверен повторно; установка и публикация не выполнялись |
| Cargo fmt, clippy с `-D warnings`, unit tests | проходят; 4 unit tests |
| Native Rust ↔ действующий TS DSP | 10/10; допуск `1e-12`, pitch lag совпадает |
| Beta policy/update tests | 16/16; pagination, schema/channel isolation, download/install, checksum, build metadata, prepared-installer preservation, RU/EN error mapping |
| Реальный install/uninstall, два ПК, DPI, devices, prolonged media | ещё не проведены |
| CPU/RAM/GPU/startup/latency benchmarks | ещё не проведены; выигрыша пока не заявлено |

## Блокеры и оставшиеся gates P0

- Root `tsc --noEmit` ранее не проверял исходники. Реальная проверка renderer выявила существовавшие до миграции ошибки: shared include, nullable index в App, устаревшие channelEdit поля, неверный async return type, отсутствующие fvad typings, `ctx`, DOM/WebRTC typings и ArrayBuffer/SharedArrayBuffer. Они не замаскированы директивами или ослаблением strict. Полный typecheck и pipeline публикации пока заблокированы; соседний media/UI код в этом checkpoint не переписывался.
- Импорт stable-профиля, защищённый перенос identity/fingerprints, staging/report, retention-aware backup cleanup и актуальный reverse export/merge ещё не реализованы. Реальный stable-профиль не копировать вручную и не использовать для испытаний. По умолчанию нужны отдельные тестовые аккаунты.
- Раздельные профили не означают два одновременных входа одного аккаунта: сохраняется серверный user → primary connection.
- Совместимость `safeStorage` и прямого DPAPI не проверена; новый crypto adapter не включён.
- Новые Rust modules не включены в packaged runtime (`enabledModules: []`). Нет Node-API/WASM production binding, переключения store/wire/media или второго владельца ресурса. P1/P2 не принимаются только по успешной компиляции прототипа.
- До выпуска beta-0: исправить baseline blockers отдельными точечными изменениями, проверить NSIS package на чистой Windows, beta→beta update/downgrade, uninstall без воздействия на stable, import/reverse-merge drill на синтетическом профиле, baseline screenshots и ресурсы.
- Подтвердить серверный допуск официальной beta без изменения attestation channel и без отключения проверок. Фактическая production-конфигурация не обследована.

## Команды

В Windows каждую команду выполнять через `cmd.exe /c`:

```text
npm test
npm run test:migration
npm run test:rust
npm run typecheck
npm run migration:inventory -- --backend "<backend-source-directory>"
npm run build
npm run dist:win:beta
```

`dist:win:beta` требует чистой проверенной beta-ветки; build-generated изменения двух существующих C# helper binaries допускаются только при последующей сверке manifest. `npm run release:beta` дополнительно требует всех проверок, доступа к GitHub и публикует prerelease. В ходе этого checkpoint release publishing не выполняется.

## English summary

A later isolated `rust/p0-backend-bench` checkpoint closes the historical renderer type blockers and records a completed backend-kernel comparison. See the linked readiness and measurement reports above. It does not automatically replace the currently open beta build or complete the remaining migration gates.

The migration has started on an isolated beta branch from the stable baseline. P0 is **in progress**, not accepted. Beta channel/profile separation and dependency-free Rust core/DSP prototypes are implemented. Existing desktop tests and native DSP parity tests pass; the actual main typecheck, Electron build and local NSIS packaging/manifest verification pass. The installer was neither installed nor published. The actual renderer typecheck exposes pre-existing errors and blocks publishing. Import/reverse merging, real Windows install/uninstall, mixed-client media and performance acceptance remain pending. Rust is not enabled in the live or packaged media path. UI styling and existing copy are preserved; new beta failures have lowercase RU/EN translations. Codecs/models, storage format, .NET backend and Go SFU remain unchanged. No release or production cutover was performed.
