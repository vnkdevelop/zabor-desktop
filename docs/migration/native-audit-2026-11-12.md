# Проверка нативного переноса / native migration audit

Редакция / updated: 2026-11-12.

## Итог / outcome

**Полная готовность native-клиента не подтверждена.** Отчёт Gemini от 11 ноября
нельзя считать release acceptance. Проверка выполнена на базе `8500535` в ветке
`audit/native-parity-release`; исходный Electron-проект пользователя не изменён.

**Whole-native readiness is not established.** Gemini's 11 November summary is
not release acceptance. This audit starts from `8500535` on
`audit/native-parity-release`, without modifying the original Electron project.

## Обнаруженные расхождения / findings

| Утверждение | Фактическая граница |
| --- | --- |
| 176 native E2E passed | `tests/e2e/runner.mjs` в исходном проекте запускает TypeScript suites старого клиента, не Slint binary |
| Системный трей реализован | `tray.rs` содержит строки и выбор действия закрытия, но не создаёт Win32 tray icon; shell не использует модуль |
| Автозапуск реализован | Модуль реестра есть, но UI не подключён; прежний `--autostart` отвергался `StartupRoute` |
| Полная Settings/Privacy parity | General содержит язык, Privacy — relay-only, Account — редактирование профиля и logout; showLastOnline/change-password/delete-account UI отсутствуют |
| Полный нативный updater | Live `attest_app_drain` остаётся закрыт ошибкой `primary-original-app-drain-binding-required` |
| Runtime dependencies = 0 | Нет Electron/Node/WebView runtime, но neural-сборка требует закреплённые `onnxruntime.dll` и `onnxruntime_providers_shared.dll` |
| FemtoVG = DirectX | Выбран femtovg/winit; renderer использует OpenGL. Отсутствие веб-движка не означает DirectX |
| Всё завершено после четырёх тестов | Эти проверки не покрывают установку, доверенный startup, tray, full UI/motion, реальные устройства или two-device acceptance |
| EXE меньше 35 МБ | Фактическая полная optimized-сборка — 82,12 МиБ; `.text` — 53,44 МиБ, `.rdata` — 26,89 МиБ, без debug-секций |

These are implementation and evidence boundaries, not grounds for removing
security gates. In particular, local tests never provision publisher trust,
approve installation, or establish physical Windows 10/11/two-device acceptance.

## Исправлено / fixes

- Кнопки Slint: Space активирует только ранее нажатую кнопку; перенос фокуса,
  повторный release и disabled state не вызывают чужое действие. Enter не
  повторяет действие при удержании. Transform/opacity используют фирменную
  кривую; reduced motion распространяется на вложенные controls.
- Маскировщик логов сохраняет Unicode и полностью удаляет экранированный либо
  оборванный JSON password/JWK `d`, включая хвост после `\"` и пробельный перенос.
- Команда автозапуска использует поддерживаемый `--app`; относительные пути,
  не-EXE, кавычки, управляющие символы и `..` отклоняются. Реестр пользователя
  не изменялся в проверках, настройка ещё не подключена к UI.
- Shell больше не abort-ит основной worker перед cleanup. Final settings writer
  использует `run_local_checked`; ошибки записи и завершения workers возвращаются
  как ошибки, а не теряются после закрытия окна. Это не доказательство полного
  original-resource `AppDrainEvidence`.
- Проверяемые npm-эталоны materialize-ятся из 122 закреплённых архивов без npm
  install/postinstall. Проверены 12 628 файлов, lock/Git provenance, SHA-256/SRI и
  структура. Разрешение только через явно заданный
  `ZABOR_NATIVE_PACKAGE_REFERENCE_ROOT`, без fallback к shared node_modules.
- Старый клиент сохранён в `legacy/zabor-electron-c5d07348.zip`: 250 tracked files,
  каждый побайтно сверен с Git blob. Инвентаризация — `legacy/manifest.json`.
- Отключён incremental cache в `.cargo/config.toml`; generated `rust/target-*`
  исключены из Git. Удалены только compiler intermediates, исходники/WIP, готовые
  application binaries, SDK/model pins, эталоны и база сессий сохранены.
- В PE composition policy добавлены существующие системные Windows DLL:
  `combase.dll`, `msdmo.dll`, `uiautomationcore.dll`. До этого full native binary
  ошибочно считался зависимым от отсутствующих сторонних библиотек. Никакие
  версии или внешние runtime-получатели не добавлены; arbitrary DLL по-прежнему
  запрещены.

The fixes preserve storage formats, encryption, retention, backend/SFU contracts
and model versions. No production account, user profile, physical audio device,
remote-control session or signing credential was used.

## Выполненные проверки / executed checks

| Проверки | Результат |
| --- | --- |
| Все 20 integration suites `zabor-native/tests/*.rs` | 132 passed |
| Новый Slint keyboard/focus/reduced-motion test | 1 passed |
| Logger unit tests | 6 passed |
| Autostart unit tests | 3 passed |
| Package security tests | 29 passed, включая regression Windows COM/media/accessibility imports |
| Новый package-reference verifier | 7 passed; финальные 122 packages / 12 628 files проверены |
| Immutable reference contract | 28 passed |
| Marked cleanup/freeze fixtures | 17 passed |
| Cargo fmt, Git diff check, Node syntax | passed |
| Native library, binary и новый control test: Clippy `-D warnings` | passed; 90 upstream warnings в vendored tract-core остаются явно видимыми |
| Native preview login RU / home EN, 125% DPI | окна отрисованы и закрыты, без physical devices; не full visual acceptance |

Непройденные первые запуски сохранены как диагностика, не как дефекты runtime:
отсутствующий `cargo` в PATH; незамкнутый reference package gate; недостающие
markers изолированного test runner; полный диск; linker environment и
30-минутный timeout упаковщика. Счётчики успешных тестов не суммируют повторные
прогоны одних и тех же случаев.

### DSP comparison

| Модель | Измеренное расхождение |
| --- | --- |
| RNNoise | mean `0.00018198545129978868`, max `0.001667805` |
| DeepFilter | mean `0.0000000018255001080535624`, max `0.000000034691766` |
| Silero | max probability difference `0.000001013279` |

Existing tolerances were retained. These synthetic waveforms do not prove
microphone quality, AEC/hotplug behaviour, CPU/RAM budgets, or the complete
capture-to-network graph on physical devices.

### Очистка / cleanup

Измеренный объём `~/.local/share/opencode` после очистки старых и завершённых
debug-сборок снизился с 169,28 до 19,46 ГиБ; свободное место на диске — 185,43 ГиБ
на момент проверки. Активная release-сборка не затронута. `opencode.db` (0,33 ГиБ),
snapshot/session history, Git worktrees/WIP, application binaries, SDK/model pins
и reference archives сохранены. Это промежуточный размер во время компиляции,
не обещание фиксированного будущего объёма.

Measured OpenCode storage fell from 169.28 to 19.46 GiB after removing old and
completed debug-build intermediates. The active release build, session database,
snapshots, source/WIP, binaries and pinned resources were retained. Future builds
still need space; the current project's incremental cache is disabled.

## Команды / commands

From a configured MSVC x64 developer environment:

```cmd
cmd.exe /c python -B scripts/native/package-reference.py prepare
cmd.exe /c python -B scripts/native/package-reference.py check
cmd.exe /c python -B scripts/native/test_package_reference.py -v
cmd.exe /c python -B scripts/native/archive-legacy.py check
```

`prepare` refuses to overwrite an existing reference. Before Rust reference
tests, explicitly set `ZABOR_NATIVE_PACKAGE_REFERENCE_ROOT` to the absolute
`rust/target/native-reference-packages` directory. Only developer-side tests use
this variable; it is not shipped or needed by the native runtime.

## Оставшаяся работа / remaining work

1. Actual Win32 tray ownership, restore/Quit/Explorer restart, UI and persistence.
2. General/Privacy/Account actions, owner-bound password/delete confirmation and
   error/reconnect reconciliation. Existing RPC methods alone are insufficient.
3. Full original app drain, trusted installed startup, real update health and
   certified update-helper integration; preserve all fail-closed guards.
4. Complete reference capture admission and full final-binary screen/modal,
   keyboard/accessibility, RU/EN, DPI/resize and interrupted/reduced-motion matrix.
5. Explicit profile import/recovery UX and clean-machine data compatibility.
6. Physical Windows 10/11, two devices/native↔legacy, SFU/RC, hotplug/sleep/AEC,
   performance and long-session/leak acceptance.
7. Publisher/manifest/acceptance keys, signed app/helper, final package inventory,
   Corresponding Source and clean-machine installer/update/rollback validation.
8. Only then retire active legacy reference paths. The `legacy` archive remains
   permanently; removing Electron files cannot substitute for native acceptance.

Release compilation and publication are separate outcomes. An unsigned optimized
binary is a development artifact, not an accepted installable native release.

## Release-mode binary

Полная offline/LTO/static-CRT сборка выполнена с default features
`native-media` и `neural-dsp`. Код приложения — commit `364b50f`.

- Binary: `rust/target-native-release/release/zabor-native.exe`.
- Size: `86 109 184` bytes (82.12 MiB).
- SHA-256: `113dab9e41d35b15b4ab6b7545dbbe76236a4d5a1df921021ae65eb741a5a12c`.
- PE32+ AMD64, без CLR/Electron/Node/WebView imports; external runtime —
  закреплённый `onnxruntime.dll`, рядом сохраняется providers DLL.
- `--help` отработал; обычная установка/startup не приняты и не обходились.
- Source branch pushed: `audit/native-parity-release`. Release tag и GitHub
  installation release не опубликованы: отсутствуют complete-native acceptance,
  human Windows/two-device и publisher/helper/manifest trust.

Локальная папка сборки: `release/native-audit-3.8.0-rust-x64/`. В ней EXE,
две закреплённые ONNX DLL, 561 компонент полной license inventory, SHA-256/PE
receipt и явный `BUILD-STATUS.txt`. Это **development release-mode build**, не
installer: `nativeClientReady=false`, `installationAllowed=false`, подписи нет.
