# Backend .NET/Rust: результаты контролируемого эксперимента

Редакция / updated: 2026-11-05. Измерение завершено: 19:20:54 UTC.

## Вывод

**Полная перепись backend на Rust ради ускорения сейчас не обоснована.** В проверенных kernels Rust не дал общего выигрыша: современная проверка пароля медленнее примерно на 41%, lookup и lifecycle реестра тоже медленнее. У Rust существенно меньше private committed memory этого маленького worker, но это не память полноценного backend. Есть отдельные выигрыши в legacy SHA и expiry workload; они не компенсируют автоматически совместимость, данные и сопровождение полной переписи.

Рабочая рекомендация: сохранить .NET backend и Go SFU; продолжить Rust-клиент по отдельным gates. Для сервера сначала профилировать реальные endpoints и сравнить с точечными .NET оптимизациями. Решение о полном Rust backend возможно только после отдельного SignalR/SQLite/concurrency/mixed-client/rollback эксперимента на тестовой среде и согласования владельца.

## Что реально измерено

- Один локальный Windows 11 host (`10.0.26300`), Intel Core i5-13420H, 12 logical processors, около 15.73 GiB доступной системе RAM.
- .NET SDK 10.0.301; Cargo 1.99.1; Release builds, текущие BCL/Rust library implementations, без снижения PBKDF2 iterations.
- Шесть независимых процессов на каждую комбинацию operation/engine; порядок движков чередуется. Всего **102 успешно проверенных worker records** в окончательной серии.
- Actual C# `EncryptionService.VerifyPassword` и `CallSessionRegistry` подключены как source links. Backend не запускался, source files не менялись, production SQLite, пользователи и SFU не использовались.
- Rust кандидат использует ring, стандартный HashMap с Arc, UUID v4 и single-owner state. C# контроль использует single-owner registry без lock. Это не предложение убрать синхронизацию из действующего сервера.
- Raw samples — timings batches. Таблица содержит pooled p50 **batch-mean operation latency**, а не p99 отдельного сетевого запроса. Для auth batch size равен 1.
- Warmup/JIT wait исключены из timing; fixture creation включено в отдельный ready-time показатель, не в latency. Проверены одинаковые synthetic vectors и semantic counters.

## Основные результаты

| Operation | Current .NET p50 | Rust p50 | Изменение времени Rust относительно .NET |
| --- | ---: | ---: | ---: |
| PBKDF2 current, 600000 iterations, valid | 57.537 ms | 80.890 ms | **+40.6%, медленнее** |
| Current hash, rejected password | 66.687 ms | 93.430 ms | **+40.1%, медленнее** |
| Legacy PBKDF2 verification path | 67.207 ms | 93.631 ms | **+39.3%, медленнее** |
| Legacy unsalted SHA-256 verifier | 0.3655 us | 0.2616 us | −28.4%, быстрее |
| Session lookup | 16.26 ns | 26.03 ns | **+60.1%, медленнее** |
| Start/suspend/resume/stale resume/end | 0.2840 us | 0.5755 us | **+102.7%, медленнее** |
| Recreate/suspend/expire 1000 session pairs | 0.6697 ms | 0.6041 ms | −9.8%, быстрее |

PBKDF2 current p50 по отдельным процессам: .NET **57.145–57.885 ms**, Rust **78.462–81.937 ms**. Диапазоны не перекрываются. По CPU измеренной phase current PBKDF2: .NET median **1148 ms**, Rust **1609 ms** — у Rust примерно **40% больше CPU** при одинаковом числе проверок.

Expiry p50 по процессам: .NET **0.598–0.694 ms**, Rust **0.597–0.639 ms**. Диапазоны частично перекрываются; агрегированный выигрыш 9.8% не следует объявлять устойчивым универсальным преимуществом. Legacy SHA не является предпочтительной схемой: настоящий `LoginUser` дополнительно переводит legacy hash на current PBKDF2, и стоимость rehash здесь не включена.

## Контроль алгоритма в C#

| Operation | Current .NET | C# single-owner control | Rust single-owner |
| --- | ---: | ---: | ---: |
| Lookup | 16.26 ns | **11.60 ns** | 26.03 ns |
| Lifecycle | 0.2840 us | **0.1761 us** | 0.5755 us |
| Recreate/expire 1000 pairs | 0.6697 ms | **0.2964 ms** | 0.6041 ms |

В этой однопоточной модели изменение владения/синхронизации C# оказалось полезнее смены языка. Это **не concurrency proof**: в настоящем backend такая модель должна обслуживаться сериализованным владельцем/actor, с прежними проверками доступа, teardown и параллельной сигнализацией.

## Ресурсы и ограничения

- Private committed memory current .NET worker: примерно **7.7–20.2 MiB**, Rust worker: примерно **1.0 MiB**. Снижение около 87–95% относится только к данному kernel process, без ASP.NET, EF/SQLite, room caches, HTTP/TLS, сети, SFU и production workload. Оно **не является** измеренным снижением RAM всего backend на 90%.
- Для lookup/lifecycle и части C# control samples CPU phase не прошла консервативный reliability threshold. Их CPU проценты не публикуются (`null`), а не трактуются как нулевая стоимость. Надёжный CPU выигрыш Rust не подтверждён для этих строк.
- Отдельный Rust worker binary: 303616 bytes; .NET worker assembly: 23552 bytes, но .NET здесь framework-dependent и использует установленный runtime. Сравнение этих двух чисел не является сравнением полных поставок.
- Эти цифры не сравнивают полноценные servers, не включают SignalR negotiate/framing/authorization, SQLite transactions/EF, реальные username lookups, snapshots, lock contention, network paths, prolonged load, reconnect и account/data rollback.
- Linux production host, его CPU и конфигурация не обследованы. Криптографический backend и scheduling там могут дать другие результаты. Измеренная разница включает library/provider/ownership choices и не является неизменным свойством C# или Rust.
- Первый короткий полный прогон архивирован и не используется для CPU выводов. Начальный незавершённый запуск также исключён: rejected fixture был ошибочно сформирован trailing NUL, который не гарантирует другой HMAC key из-за нулевого padding; окончательная серия использует ненулевой prefix.

## Куда направить серверную работу дальше

По актуальным исходникам, а не по языковой гипотезе:

1. `UserService.GetUserByUsername`: scan online cache и SQL `ToLower()` при наличии username index/NOCASE. Проверить планы и индексацию, сохранив legacy Unicode/case semantics, а не слепо менять сравнение.
2. `ChannelStateBroadcaster`: составление каждого channel list отдельной фильтрацией online users. Сравнить группировку одним проходом с прежним full snapshot wire и ordering/debounce 150/500 ms.
3. `HubStateFilter`: auth/voice mutation lifetime и конкуренция. Не убирать safety locks по результатам одного serialized kernel; отдельно проверить bounded auth work и authoritative state ownership.
4. Измерить .NET end-to-end на синтетической БД и независимом LoadTester; потом Rust compatibility candidate с теми же сценариями. Go SFU оставить независимым контрольным транспортом.

## Воспроизводимость

- Протокол и runner: [benchmarks/backend/README.md](../../benchmarks/backend/README.md).
- Raw final result в worktree: `migration-artifacts/backend-comparison.json`.
- SHA-256 raw final result: `ef37b2eb6d0a00ff0cbf76cb8c2eeebb95faae1f24f18b6bba75d8e11cc31630`.
- Raw audit series: `migration-artifacts/backend-comparison-<run-id>.jsonl`. Они не содержат password/salt values, сообщений пользователей или реальных session IDs и не отслеживаются Git.
- Linked source hash `EncryptionService.cs`: `83917c9f1f8291eca73d4fae1bce33ed9ecd7721895a15c28494e42104c54114`.
- Linked source hash `CallSessionRegistry.cs`: `5031cfdc4bd6fb13fc60ceff82355c7a67f30cdea9aa3b51cfa457e452cc5a85`.

## English summary

The six-repetition, 102-record **kernel experiment does not justify a full backend rewrite for speed**. On this Windows/i5-13420H host current .NET PBKDF2 verification took 57.537 ms versus 80.890 ms in the Rust/ring candidate (+40.6% time and about +40% CPU). Rust lookup and lifecycle batch-mean medians were also slower. Rust was faster in legacy SHA and modestly faster in the recreate/expiry aggregate; the latter has overlapping per-process median ranges. A C# single-owner control outperformed the Rust registry medians, pointing to ownership/algorithm choices rather than language alone. Rust's roughly 1 MiB private memory versus 7.7–20.2 MiB in these small C# workers must not be extrapolated to whole-server RAM. Some short C# phases failed the CPU reliability threshold and have no published CPU percentage. No production backend, database, accounts, SignalR service or SFU was changed or benchmarked. Keep .NET/Go for now, profile real server hotspots and validate any Rust candidate separately with full compatibility, concurrency, data and rollback gates.
