# Backend .NET/Rust: controlled kernel comparison

Редакция / updated: 2026-11-05.

## Назначение и статус

Изолированный эксперимент, **не новый backend**. После подтверждения завершения стрима выполнены шесть повторов с 102 worker records; [отчёт](../../docs/migration/backend-rust-benchmark-2026-11-05.md) не обосновывает полную перепись backend ради ускорения. Будущие нагрузки также запускаются только после подтверждения idle-состояния владельцем.

Сравниваются:

1. Действующие `EncryptionService.VerifyPassword` и `CallSessionRegistry`, подключённые из локального backend как read-only C# source links.
2. C# контрольный реестр с одним владельцем состояния, без mutex, но с прежними ID, строковыми ключами, immutable session records и правилами 45-секундного resume.
3. Rust эксперимент с одним владельцем, string/Arc keys, UUID v4, проверкой PBKDF2-SHA256 (600000/100000 iterations), legacy SHA-256 и constant-time comparisons.

Для auth сравниваются только действующий C# и Rust; C# контроль нужен для отделения эффектов алгоритма/владения от эффекта языка в реестре. Текущий backend, SFU, БД, пользователи, сигнализация и активные звонки не запускаются и не меняются.

## Методика

- Release builds; одни и те же случайные синтетические пароль и salt передаются только через stdin. Значения не сохраняются в репозитории, результатах или командной строке.
- Четыре auth cases: current valid, rejected, legacy PBKDF2, legacy SHA-256. Стоимость password verification не снижается.
- Три state cases: lookup, start/suspend/resume/stale-resume/end, recreate/suspend/expire 1000 session pairs.
- Минимум пять независимых повторов; порядок движков меняется между повторами. В каждом новом процессе выполняются warmup и отдельное ожидание tiered-JIT, не включённые в timed samples.
- Сверяются semantic counters и общий digest синтетических test vectors; хеши двух подключённых backend source files проверяются до/после прогона.
- Host измеряет CPU и память **своих дочерних benchmark processes**. Windows private committed bytes и Linux private resident pages имеют разные определения; сравнения выполняются в пределах одной ОС.
- Сохраняются raw batch samples и их p50/p95/p99 после нормализации на batch size. Это percentiles of **batch-mean operation latency**, не p99 отдельного сетевого запроса. `registry-expire` включает пересоздание и приостановку 1000 пар перед sweep.
- Короткие workloads увеличены до длительных серий. CPU comparison помечается недостоверным и не получает процент, если timed phase короче 500 мс или процесс набрал меньше 125 мс CPU: нулевое значение coarse OS counter не означает нулевую стоимость.
- Каждый успешный worker record немедленно сохраняется в уникальный `backend-comparison-<run-id>.jsonl`; полный итог содержит только завершённую сверенную серию. Предыдущий итог архивируется, а не теряется при новом запуске.
- Startup показатель включает создание fixtures, PBKDF и seed sessions; это не время готовности настоящего сервера. Framework-dependent .NET worker footprint не равен footprint всей поставки с runtime.
- Rust использует process-local monotonic clock, C# — system tick count. Семантика 45 секунд сохранена, абсолютные значения часов не сравниваются.

## Запуск только после завершения стрима

```text
cmd.exe /c python benchmarks/backend/run.py --backend-root "<backend-source-directory>" --idle-confirmed --repeats 5
```

Без `--idle-confirmed` runner отказывается запускать нагрузку. Сборки размещаются в отдельном временном каталоге, автоматически очищаемом после прогона; backend source directory не получает bin/obj и не открывается как БД. Итог: `migration-artifacts/backend-comparison.json` (не отслеживается Git).

Тест отказа меняет пароль ненулевым префиксом. Добавление завершающего NUL непригодно как гарантированно неверный HMAC/PBKDF2 test vector для короткого ключа из-за нулевого padding HMAC.

## Интерпретация

Эти цифры могут обосновать дальнейший эксперимент с auth/state core, но **не доказывают**, что весь Rust backend будет быстрее или надёжнее. В них нет SignalR framing/transport, EF/SQLite, авторизации hub commands, online routing, channel snapshots, SFU, contention/load, сети и data-preserving rollback. Нужно отдельно сравнить с точечным улучшением .NET, затем пройти end-to-end и mixed-client gates на тестовых аккаунтах. Production cutover не разрешается этим benchmark.

В обследованном `UserService` есть сканирование online cache по username и SQL с `ToLower()`. Стоимость таких алгоритмов не исчезает от замены C# на Rust; поэтому до массового переписывания нужно профилирование и сравнение с .NET оптимизациями. PBKDF в .NET уже использует системную криптографическую реализацию; преимущество Rust не предполагается заранее.

## Benchmark-only dependencies

Не входят в desktop runtime или release resources. Лицензия собственных benchmark sources — GPL-3.0-only.

| Crate | Version | Upstream license |
| --- | --- | --- |
| ring | 0.17.14 | Apache-2.0 AND ISC |
| base64 | 0.23.1 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| uuid | 1.29.0 | Apache-2.0 OR MIT |
| subtle | 2.6.1 | BSD-3-Clause |

## English summary

This is an isolated **kernel experiment**, not a Rust backend implementation. The owner confirmed idle/stream-ended status; a six-repetition, 102-record comparison completed and is documented in the linked report. It compares the actual C# password verifier and call registry against a Rust single-owner candidate and a C# single-owner control. Synthetic credentials use stdin only; no production database, users, hub or SFU are touched. Alternating repetitions preserve raw batch timings, semantic counters and own-child CPU/memory measurements. Batch-mean percentiles, fixture-inclusive startup and worker memory must not be presented as whole-server latency or full deployment footprint. The observed mixed gains do not justify a full rewrite; separate end-to-end, compatibility, concurrency and rollback validation is still required.
