# Полное завершение нативного переноса / complete native migration

Редакция / updated: 2026-11-09.

## Цель / objective

Завершить перенос клиента zabor на нативный Rust без Electron и WebView в поставке,
с сохранением функций, интерфейса, анимаций, русского и английского языков.
Исходный проект и профиль сохраняются; backend .NET и SFU Go не переписываются.

Complete the native Rust client without shipping Electron or a WebView, preserving
functionality, UI, motion, Russian and English. Preserve the original project and
profile. The .NET backend and Go SFU remain unchanged.

## Восстановленная граница / restored boundary

- Исходная сессия / original session: `ses_e4120347cffea4wVcsios6UOkv`.
- Продолжение / continuation: `ses_e40a15d13ffebrvSZmn2LyVGJc`.
- Ветка / branch: `rust/native-client`; worktree: `rust-p0-backend-bench`.
- Сохранённый HEAD при восстановлении / restored saved HEAD: `1f8c4f7`.
- Reference slice уже был перенесён, но не проверен и не сохранён коммитом.
  The reference slice was imported but not checked or committed.
- Source `172c4a8` chat-IO и `d331535` update integration сохраняются.
  Preserve the checked chat-IO and update integration sources.
- Reference integration сохранена в `e392f75`: 17 Rust + 45 неизменённых JS
  проверок, runtime-only tree и Graphify завершены; 34 scoped paths.
  Reference integration is saved in `e392f75`, with 62 distinct affected tests,
  completed runtime-only tree and Graphify checks in 34 scoped paths.
- Новый `src/shell/file_io.rs` пока не подключён и не проверен; он исключён из
  проверенного reference-коммита. The new FileIo scaffold is not yet wired or
  tested and is excluded from the checked reference commit.

## Порядок завершения / completion order

| Этап / stage | Исполнитель / owner | Условие завершения / completion condition |
| --- | --- | --- |
| Reference primary integration | parent | Исправлен shared test-module wiring; strict Clippy и affected Rust tests; сохранение неизменённых JS results / corrected shared test module, strict Clippy, affected Rust tests and unchanged JS result inheritance |
| Native developer launcher / CI | parent | Интеграция проверенной цепочки `02f31b3 → 173232b → 7d47a8 → 0ac8bb2`; primary regressions / checked chain integration and primary regressions |
| UI / member actions | `ses_e409ebb57ffdRrdt4gj33zUpFd`, `native-app-completion` | Checked outgoing-request race correction, actual post-Join Invite и корректный durable HiddenStore путь / race fix, actual post-Join Invite and correct durable HiddenStore path |
| Media / resource retirement | `ses_e409ebb4bffd4z4xTWS74TOr8L`, `native-media-finalization` | Проверенные original-owner teardown/MicTest changes и contextual integration / checked original-owner teardown and MicTest changes, contextual integration |
| RC / frame provenance | `ses_e409ebb55ffd2aVlOLlMtbfAd6`, `native-rc-completion` | Проверенные request/frame/focus/activity/release hooks; единственный input owner / checked hooks and sole input owner |
| Files / publication | `ses_e409ebb49ffeQW8hVwnkkqk3oc`, `native-file-publication` | Checked callback API, primary bound constructor и observed active drains / callback API, bound primary constructor and observed active drains |
| Import / recovery / rollback | `ses_e409d4160ffeID918DpFJ3Yyjd`, `native-import-completion` | Реальный native command route, independent account/key/consent inputs и retained exporter joins / real command route, independent account/key/consent inputs and retained exporter joins |
| Dependencies / legal texts | `ses_e409d407cffeX7JwknWtz3Aj01`, `native-delivery-completion` | Version-pinned dependency inventory, обязательные upstream license texts и reference closure / pinned inventory, required license texts and reference closure |
| Startup / update / app drain | parent after checked integration | Trusted bootstrap до открытия профиля; health после реальной readiness; завершение исходных ресурсов / trusted bootstrap before profile open, health after real readiness, original-resource completion |
| Final native acceptance | parent and owner where required | Full functional/parity/resource/package matrix; отдельно фиксируются physical и publisher approvals / complete functional, parity, resource and package matrix; physical and publisher approvals remain explicit |

## Правила интеграции / integration rules

Переносить проверенные contextual hunks; не заменять целиком `shell.rs` или root
Cargo lock. Сохранять ChatIo/SessionIo, helper packages, SDK/model/static-CRT pins.
Не повторять полные неизменённые проверки. После каждого завершённого этапа —
проверенный checkpoint, актуальный handoff, Graphify и настоящий compact.

Import checked contextual hunks, not whole shell or root-lock replacements.
Preserve resource owners, helpers and pins. Do not rerun unchanged full checks.
Save a checked checkpoint, current handoff, Graphify synchronization and real
compaction after each completed stage.

## Недопустимая подмена результата / no false completion

Запуск job, quota error, timeout, отмена, synthetic category futures или readiness
flags не доказывают завершение исходных процессов, ресурсов или всего переноса.
Проверки не используют production endpoints, реальный профиль или физический ввод.
`node_modules` является junction к исходному проекту; установка, пересборка и
рекурсивная очистка через него запрещены. Legacy deletion, публикация, установка,
push и выключение не выполняются до полной готовности и требуемых разрешений.

A job admission, quota error, timeout, cancellation, synthetic resource-category
futures or readiness flags do not prove resource retirement or migration completion.
Do not use production endpoints, a real profile or physical input in autonomous
tests. Do not install, rebuild or recursively clean the original-project
`node_modules` junction. Legacy removal, release, installation, push and shutdown
remain conditional on full readiness and the required approvals.
