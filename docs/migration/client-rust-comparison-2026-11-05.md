# Клиент: legacy и Rust voicing WASM

Редакция / updated: 2026-11-06.

## Реальный объём переноса

Rust backend отменён. Клиент сохраняет Electron/React, .NET/Go, WebRTC/Web Audio и проверенные модели. В Rust WASM вынесена собственная voicing autocorrelation, с одной реализацией в `zabor-dsp` для native tests и WASM. Это **не полностью нативный Rust-клиент** и не замена neural models/codec/capture stack.

Архитектурные изменения: compiled WebAssembly.Module кешируется до создания worklet; каждый processor владеет отдельным instance с фиксированной memory 256 KiB и рабочими буферами. На pitch window выполняется один блочный вызов с копированием 512 f32 samples; нет перехода JS/WASM на каждом отсчёте или frame allocations в успешном voicing path. JS ring остаётся актуальным, поэтому при отказе Rust legacy fallback использует те же данные, а не новый холодный state. Async initialization проверяет текущие context/epoch; teardown отдельно освобождает instance.

## Проверки

- Native Rust/TS numerical parity: 10 fixtures, прежние coefficients/precision.
- WASM/actual legacy method: ring offsets и filled boundaries, допуск `1e-12`, тот же lag.
- Полный собственный mic processor: десять секунд synthetic PCM, одинаковые выходные samples, VAD windows/sequence/endFrameId/RMS и voicing state; neural engine здесь identity fixture.
- Fallback после отказа Rust и 100 lifecycle циклов без роста WASM linear memory.
- **Реальный Electron AudioWorklet**: обе ветки получили frame 164; score `0.9999725101590385`, lag 94; Rust runtime действительно был `rust-voicing`. Проверен structured clone WebAssembly.Module и выполнение в worklet, не только Node mock.
- Desktop tiers 1–8, typecheck и release/updater compatibility tests сохраняются.

## Измерения в изолированном Node fixture

Шесть повторов с чередованием legacy/Rust, одинаковые ring inputs и фактические copy/call costs. Единицы — микросекунды.

| Voicing kernel | Legacy | Rust WASM |
| --- | ---: | ---: |
| p50 | 45.4 | 25.7 |
| p95 | 94.6 | 49.3 |
| p99 | 168.4 | 90.8 |
| total measured time, ms | 1076.25 | 559.61 |

Median hot-path time уменьшилось примерно на **43%**, суммарное kernel time — примерно на **48%**, включая копирование и WASM call. Это выигрыш данного участка, **не CPU/RAM всего приложения**.

| Full own mic processor, identity model fixture | Legacy | Rust |
| --- | ---: | ---: |
| callback p95 | 1824.7 | 1717.3 |
| callback p99 | 2809.5 | 2629.0 |
| total measured time, ms | 8409.70 | 8033.03 |

Суммарное время этой fixture цепочки уменьшилось примерно на **4.5%**. В неё не входит настоящая neural inference, driver, codec, сеть, GPU и UI. Node timing не является измерением реальной mouth-to-ear latency или гарантией AudioWorklet deadline на другом ПК. Нельзя объявлять уменьшение ресурсов всего клиента на 43–48%.

Raw result: `migration-artifacts/client-dsp-comparison.json`, synthetic data only. Linear memory instance — 256 KiB; bundled module около 22 KiB. Electron footprint пока остаётся. Следующее доказательство — owner A/B на двух устройствах с одинаковыми model/device/media settings и prolonged tests; критерии в [client-beta-testing.md](client-beta-testing.md).

## Updater и выпуск

Bridge `beta.rust.1` оставляет default legacy и старый допустимый manifest `enabledModules: []`, но обновляет parser. Rust `beta.rust.2` включает voicing default и честно указывает `rust_voicing`. Старый beta-0 принимает bridge, затем уже умеет обновиться на Rust пакет. Stable получает только stable; draft/checksum/publication authority разделены с CI. `release.bat`/`release-beta.bat` остаются обычными operator entry points с защищённой Git историей. Main/stable promotion сейчас не выполняется.

## English summary

Only voicing autocorrelation is migrated to the client's fixed-memory WASM AudioWorklet; Electron/React, neural models, media adapters and .NET/Go are unchanged. Native/WASM parity, fallback, lifecycle and actual Electron worklet execution passed. In six alternating synthetic Node-fixture repetitions kernel p50 was 45.4 us legacy versus 25.7 us Rust, and total kernel time fell about 48%, including copy/call costs. The full own processor with an identity model fixture improved only about 4.5%; neither number represents whole-client CPU/RAM or real network/audio latency. Honest bridge and Rust prereleases protect old beta updaters, while stable/main promotion remains gated on the owner's two-device and data review.
