# P0 readiness и эксперимент backend / readiness and backend experiment

Редакция / updated: 2026-11-05.

## Изоляция

Работа выполняется в отдельном worktree и ветке `rust/p0-backend-bench`, созданных от `9e9f796`. Оригинальная рабочая папка, запущенная beta-сессия со стримом, её userData, исполняемые файлы и backend не меняются. Исправления ещё не включены в открытую сборку.

Владелец выбрал запуск нагрузочных измерений **после завершения стрима** и затем подтвердил завершение. Завершена и сверена серия из шести повторов и 102 worker records. [Результаты и ограничения](backend-rust-benchmark-2026-11-05.md) не дают оснований переписывать весь backend ради ускорения. Нет скрытого запуска серверов, автопубликации или переключения production.

## Закрытые программные блокеры

- `tsconfig.web.json` включает уже используемый shared diagnostics contract.
- Добавлены точные используемые типы fvad WASM и Electron/Chromium compatibility properties; strict не ослаблен, suppressions не добавлены.
- RC signal/button buffer types указывают на реально используемый обычный ArrayBuffer; wire bytes и повторное использование буферов сохранены. Входящие ArrayBuffer views по-прежнему копируются с исходным byteOffset/byteLength.
- После async offer creation сохранены проверки актуального peer и закрытых состояний, без неверного TypeScript narrowing.
- `prepareMicTest()` сохраняет попытанный AudioContext в области видимости catch и корректно проверяет сменившийся context. Отмена/закрытие не создаёт новый capture.
- Обработка удаления канала больше не обращается к отсутствующим store fields. Закрытие редактора удалённого канала выполняется у владельца локального editChannelId, не закрывая редактор другого канала.
- Fullscreen RC permission lookup использует ID выбранного streamer. Оформление, иконки и тексты интерфейса не изменены.
- Добавлена явная переносимая type annotation для уже существующего animation utility wrapper; поведение анимаций не меняется.

## Выполненные проверки

| Проверка | Статус |
| --- | --- |
| Полный `npm run typecheck` | проходит в изолированном worktree |
| `npm run test:readiness` | 4/4, включая production-method replay смены mic-test context |
| .NET benchmark source links, Release build | 0 ошибок, 0 предупреждений; isolated workloads сверены |
| Rust benchmark `cargo check`, clippy `-D warnings` | проходят; isolated workloads сверены |
| Python runner syntax | проходит |
| Полная desktop regression после завершения стрима | 176/176 + 16/16 beta tests |
| Rust core/DSP проверки после завершения стрима | 4 unit tests + 10/10 numeric parity |
| Сравнительные kernel measurements | 6 повторов, 102 records; semantic counters и vectors совпали |
| `npm run build` после этих правок | проходит в worktree; исходная открытая сборка не заменяется |
| Install/package lifecycle checks | отдельный gate; установка и публикация не выполнялись |

## Backend: полная перепись пока не обоснована

Подготовлен [benchmark protocol](../../benchmarks/backend/README.md): фактические C# password verifier/call registry, C# single-owner контроль и Rust single-owner кандидат. Запуск разрешается только с явным `--idle-confirmed` после подтверждения владельца; сохраняются независимые повторы, raw samples, semantic counters, source hashes, CPU/память и проценты по каждому kernel.

Даже будущие хорошие результаты этих kernels не означают ускорение всего backend: HTTP/SignalR, EF/SQLite, channel snapshots, конкуренция, сеть, SFU и обратимый перенос данных здесь не измеряются. Сначала нужны измерения, сравнение с .NET оптимизациями и отдельные end-to-end проверки. Backend по-прежнему не имеет подтверждённого Git checkpoint; до его фактической миграции этот блокер необходимо закрыть. Исходные файлы в эксперименте подключаются только на чтение.

## Остаток P0 и дальнейшие gates

Этот worktree не объявляет P0 или всю Rust-миграцию завершёнными. Ещё нужны безопасный supported IndexedDB export/import, continuity identity/fingerprints, retention/deletion-aware reverse merge, реальные install/update/downgrade/uninstall и визуальные/media проверки на целевых ПК. Tauri/native media, Rust input, storage, backend и SFU не включаются до своих критериев приёмки.

## English summary

Changes are isolated on `rust/p0-backend-bench`; the open build and production data are untouched. Full TypeScript checking and four focused readiness tests pass, without weakening strictness or changing UI styling. After the owner confirmed that streaming had ended, 176 desktop tests, 16 beta tests, four Rust unit tests and ten numeric parity tests passed. The separately scheduled kernel comparison completed six repetitions and 102 semantic-checked records: it does not justify a full rewrite for speed. This is not a full backend, SignalR/SFU benchmark or approval for production cutover. Profile import/reverse merge, real installation/media gates and a backend Git checkpoint remain pending.
