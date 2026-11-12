# Продолжение сверки native-переноса / native parity continuation

Редакция / updated: 2026-11-12. Ветка `audit/native-parity-release`, база `5f27f71`.

## Итог / outcome

**Релиз по-прежнему не принят.** Закрыта часть пунктов, которые прошлая сверка
оставила открытыми: системный трей, автозапуск, сворачивание в трей и действия
приватности/аккаунта. Пункты, требующие подписей, физических устройств, второго
компьютера и решения владельца продукта, остаются открытыми и не могут быть
закрыты локальными проверками.

**Release acceptance is still not established.** This pass closes the tray,
autostart, hide-to-tray and privacy/account actions from the previous audit.
Items that require signing keys, physical devices, a second machine or an owner
decision remain open and cannot be closed by local checks.

## Что сделано / implemented

| Пункт | Реализация |
| --- | --- |
| Win32 tray | Новый крейт `zabor-desktop-platform`: скрытое окно-владелец, `Shell_NotifyIconW`, меню «открыть zabor» / «выйти», левый клик и `NIN_KEYSELECT` восстанавливают окно, `WM_CONTEXTMENU` открывает меню, `TaskbarCreated` пересоздаёт значок после перезапуска проводника |
| Значок | 16×16 значок рисуется в коде (скруглённый квадрат цветом `#C81E70` и белая «z»), создаётся через `CreateIcon` и освобождается `DestroyIcon`; сторонние ассеты не добавлены |
| Автозапуск | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, значение `zabor-native-beta`, команда `"<путь>\zabor-native.exe" --app`; проверка пути (абсолютный, `.exe`, без кавычек, управляющих символов и `..`) сохранена |
| Сворачивание в трей | Настройка «сворачивать в трей»: закрытие прячет окно только при готовом значке, иначе приложение завершается. Явный пункт «выйти» в меню трея всегда завершает процесс |
| Общие настройки | Вкладка «общие» получила «система»: запуск вместе с Windows и сворачивание в трей; состояние читается из реестра при старте, устаревший путь автозапуска (после переноса файла) обновляется на текущий исполняемый файл |
| Приватность | Переключатель «показывать время последнего входа» (`SetShowLastOnline`), смена пароля (`UpdateUserPassword`) с локальной проверкой длины/символов и сохранением нового пароля в DPAPI-сессии, удаление аккаунта (`DeleteMyAccount`) с подтверждающим словом |
| Локальные параметры | `nativeDesktop` в `settings.json` beta-профиля: только `openAtLogin` и `minimizeToTray`, исключены из payload `SaveAudioSettings` так же, как `nativeAudioSync` |
| Клавиатура | `AudioToggle` активируется `Space` только после нажатия на нём же, `Enter` не повторяется при удержании, фокус-ринг и `reduced-motion` наследуются от `ControlMotion` |
| Ошибка процессора микрофона | `MicProcessor::process_shared` больше не публикует `processor_failed`, `speaking` и `mic_db` до проверки исходного capture-commit. Публикация остаётся в `AudioFlow::capture`/`submit_frame`, поэтому устаревший кадр не может сбросить состояние нового захвата. Это исправляет ранее падавший unit-тест `shared_processor_success_and_failure_do_not_publish_before_original_capture_commit` |
| Developer-запуск | Явный `--developer`: обходит только обёртку доверия к установке, использует профиль `zabor-desktop-native-dev` и значение автозапуска `zabor-native-dev`, не смешивается с `--app`/preview/snapshot/`--profile`. Допуск к серверу не обходится: attestation остаётся из `ZABOR_CLIENT_SECRET` |

## Проверки / checks

| Команда | Результат |
| --- | --- |
| `cargo check -p zabor-desktop-platform` | без ошибок и предупреждений |
| `cargo check -p zabor-native --features camera-test-source,stream-audio-test-source` | без ошибок; 90 предупреждений `tract-core` остаются видимыми |
| `cargo test -p zabor-native --lib audio_runtime::` | 33 passed |
| `cargo test -p zabor-native --lib audio_preferences::` | 3 passed |
| `cargo test -p zabor-native --lib tray::` | 4 passed |
| `cargo test -p zabor-native --lib autostart::` | 3 passed |
| `cargo test -p zabor-native --lib strings::` | 2 passed (после добавления новых экранов) |
| `cargo test -p zabor-native --lib auth::` | 5 passed, включая проверку пароля без логина |
| `cargo test -p zabor-native --lib startup_route::` | 3 passed, включая запрет смешивания `--developer` с другими маршрутами |
| `cargo test -p zabor-native --test audio_preferences_local` | 12 passed |
| `cargo fmt -p zabor-native -p zabor-desktop-platform -- --check` | без diff |
| `zabor-native.exe --developer --server http://127.0.0.1:9/...` | окно «zabor», профиль `zabor-desktop-native-dev`, процесс жив |
| `ZABOR-Dev-Setup.exe /quiet` | установка в `%LOCALAPPDATA%\ZaborDev`, ярлыки и запись удаления созданы |
| `ZABOR-Dev-Setup.exe --uninstall /quiet` | каталог, ярлыки и запись удаления удалены; профиль в `%APPDATA%` сохранён |
| повторная установка | exit 0, файлы совпадают по SHA-256 |

- `audio_preferences_local.rs` дополнительно утверждает, что `nativeDesktop`
  не попадает в payload `SaveAudioSettings`.
- Новые unit-тесты: локальные desktop-параметры (разбор, round-trip через
  локальный файл, отсутствие серверной синхронизации), правило
  «прятать окно только при готовом трее» и проверка пароля без логина.
- Тест `native_views_reference_existing_bilingual_keys` проверяет, что все
  ключи, использованные в новых `.slint`-экранах, существуют в обоих каталогах
  (`assets/catalogs/ru.json`, `en.json`).
- Тесты `startup::` и `update_helper/host/io` в этом прогоне не пригодны:
  им нужны `ZABOR_UPDATE_TEST_DIRECTORY` и `ZABOR_UPDATE_TEST_HELPER` из
  `scripts/native/packaging/verify-update-*.py`. Без них они падают на
  предполёте, а не из-за кода. Это ограничение прогона, а не успех.
- Полный набор из 20 integration-файлов в этой итерации повторно не
  прогонялся: изменения не затрагивают chat/media/voice, а `audio_preferences_local`
  прогнан отдельно (12 passed). Предыдущий baseline 132 passed относится к
  коммиту `5f27f71`.
- Артефакт `release/native-audit-3.8.0-rust-x64/` собран из коммита `364b50f`
  и не перезаписывался. Актуальная сборка этой итерации — отдельный каталог
  ниже.

## Сборка этой итерации / build

`cargo build --release -p zabor-native --offline` (default features:
`native-media`, `neural-dsp`, static CRT, LTO) завершился за 21 м 36 с.
Результат скопирован в `release/native-continuation-2026-11-12/`:

| Файл | Байт | SHA-256 |
| --- | --- | --- |
| `zabor-native.exe` | 86 907 904 | `b02812b72188f0c76e7b8c6a4299f2445d698acf6509a4ef7c94f56bd724b275` |
| `onnxruntime.dll` | 17 253 728 | `cb2fa518b18356617f6f9f460c012501da2819ec7e5c1d80bd8f5de20492db0c` |
| `onnxruntime_providers_shared.dll` | 21 816 | `8cea20be64259950cda8643af0ed42c2a424400fc3bb9bb29f9a35cee6b3419f` |

`build-receipt.json` фиксирует `applicationSourceCommit 8483162`, `signed: false`,
`nativeClientReady: false`, `installationAllowed: false`. `dumpbin /dependents`
показывает только системные Windows DLL и `onnxruntime.dll` — Electron, Node.js,
WebView2 и Python в импортах отсутствуют. **Это оптимизированный артефакт
разработки, а не установщик и не release acceptance.**

### Developer-установщик

`release/ZABOR-Dev-Setup.exe` (50 125 824 Б, SHA-256
`fc921686414db75b06004902b87ca592a1b6ad704797705a60461d95cda585a5`) собран из
`scripts/native/packaging/dev-installer.cs` с встроенным payload. Он ставит файлы
в `%LOCALAPPDATA%\ZaborDev`, создаёт ярлык «zabor (dev)» на рабочем столе и в
меню «Пуск», запись удаления в `HKCU` и не требует администратора. Ярлык
запускает клиент с `--developer`. В payload входят `LICENSE` и
`THIRD-PARTY-NOTICES.md`. Установщик **не подписан**, не публикуется и
не является релизом; при запущенном клиенте он отказывается работать, потому что
закрытие окна сворачивает приложение в трей — выходить нужно пунктом «выйти».

## Остаётся / remaining

1. Полный original-resource app drain и доверенная установка: гейты
   `primary-original-app-drain-binding-required` не снимались.
2. Рабочий updater: нужны подписанный manifest, сертифицированный
   update-helper и реальная проверка обновления на чистой машине.
3. Сверка экранов и анимаций финального бинарника: RU/EN, четыре DPI,
   resize, прерванные состояния, `reduced-motion`, доступность.
4. Импорт старого профиля как пользовательский сценарий (не только CLI) и
   совместимость данных на чистой машине.
5. Физические Windows 10/11, два устройства native↔legacy, SFU/RC,
   hotplug/sleep/AEC, длительные сессии и утечки.
6. Ключи издателя, подписанные приложение и helper, финальная инвентаризация
   пакета и Corresponding Source.
7. Полный комплект лицензий транзитивных зависимостей и Corresponding Source
   для будущей нативной поставки: новый крейт уже описан в
   `THIRD-PARTY-NOTICES.md`, но итоговый bundle собирается упаковщиком.

## Границы заявлений / claim boundaries

Трей, автозапуск и действия аккаунта проверены локально: сборка, unit-тесты и
чтение кода. Они **не** проверены на физическом рабочем столе с реальным
проводником, не подтверждены на двух машинах и не означают, что подписанный
установщик готов. Значение автозапуска пишется только по явному действию
пользователя; в тестах реестр не изменялся.
