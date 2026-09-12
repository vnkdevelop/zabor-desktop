# Политика использования знаков ZABOR / ZABOR Trademark Policy

> Русская версия является основной и имеет преимущественную силу при расхождении с переводом.
> The Russian version is authoritative; the English translation follows below.

---

## Русская версия

### 1. Что не покрывается лицензией GPL-3.0

Исходный код ZABOR распространяется по лицензии [GNU General Public License v3.0 only](LICENSE). Лицензия GPL-3.0 предоставляет права на использование, изучение, изменение и распространение **кода**, но **не передаёт никаких прав на средства индивидуализации**. Это прямо предусмотрено разделом 7(e) GPL-3.0, который допускает отказ в предоставлении прав по законодательству о товарных знаках, не делая лицензию несвободной.

Следующие обозначения и материалы принадлежат правообладателю (vnkdevelop) и **не входят в объём лицензии GPL-3.0** — далее «Знаки»:

- словесное обозначение **ZABOR** в любом написании, регистре и транслитерации (ZABOR, Zabor, zabor, ЗАБОР, Забор);
- логотип и иконка приложения, в том числе файлы `build/icon.ico` и `resources/icon.png`;
- оформленные брендингом изображения интерфейса в каталоге `docs/images/`;
- идентификатор приложения: действующий — `com.zabor.desktop` (задаётся в `package.json`, секция `build`, которая является активной конфигурацией сборки); исторический — `com.zabor.app`, указанный в [electron-builder.yml](electron-builder.yml), который в текущей сборке не применяется; имя исполняемого файла `ZABOR.exe`, имена ярлыков и установщика;
- доменные имена и сетевые адреса официального сервиса ZABOR.

Получение копии кода по GPL-3.0 **не даёт** права выпускать продукт под Знаками.

### 2. Что разрешено без отдельного согласия

Разрешено добросовестное упоминание Знаков в описательных целях, когда это не создаёт ложного впечатления об источнике или одобрении:

- правдивые утверждения о совместимости: «совместимо с протоколом ZABOR», «форк на основе исходного кода ZABOR»;
- ссылки на официальный репозиторий и релизы;
- упоминание в обзорах, статьях, научных и учебных материалах;
- сохранение неизменённых сведений об авторстве и лицензии в исходных файлах — это, наоборот, требуется GPL-3.0.

### 3. Что не разрешено

Без предварительного письменного согласия правообладателя не разрешается:

- называть **Знаками** производную или изменённую версию, форк, сборку или переупакованный установщик;
- использовать Знаки в названии продукта, окне приложения, имени установщика, ярлыка или исполняемого файла производной версии;
- использовать Знаки или сходные до степени смешения обозначения в доменных именах, названиях аккаунтов, каналов и страниц в магазинах приложений;
- использовать логотип и иконку в любых производных версиях, включая перерисованные и стилистически переработанные варианты;
- создавать впечатление, что производная версия является официальной, поддерживается правообладателем или одобрена им;
- использовать Знаки в рекламе и в наименованиях платных услуг.

### 4. Требования к форкам и производным версиям

Форк допустим — этого требует GPL-3.0 — но он обязан быть однозначно отличим от официального продукта. Перед публикацией производной версии необходимо:

1. заменить `productName`, `executableName` и `shortcutName` в активной конфигурации сборки — секция `build` файла [package.json](package.json), а также в [electron-builder.yml](electron-builder.yml), если он используется;
2. заменить `appId` на собственный идентификатор, отличный от действующего `com.zabor.desktop` и исторического `com.zabor.app`;
3. заменить `name` в [package.json](package.json);
4. заменить файлы `build/icon.ico` и `resources/icon.png` собственными изображениями;
5. убрать Знаки из интерфейса, заголовка окна, экрана загрузки и текстов локализации;
6. убрать брендированные изображения из `docs/images/`;
7. указать в описании, что проект является независимым форком и не связан с ZABOR и его правообладателем;
8. настроить собственный серверный бэкенд — см. раздел 4 [TERMS.md](TERMS.md).

Требования 1–7 относятся к Знакам и не ограничивают ваши права на код по GPL-3.0. Удаление сведений об авторстве и лицензии при этом **не допускается**: это отдельное требование GPL-3.0.

### 5. Правовой статус Знаков

На момент публикации настоящего документа Знаки используются как **незарегистрированные обозначения**. В отношении них применяется символ «™». Символ «®» не используется и не должен использоваться до государственной регистрации знака. Отсутствие регистрации не означает отказа правообладателя от прав, в том числе от защиты от недобросовестной конкуренции и введения потребителей в заблуждение.

Настоящая политика может быть обновлена после государственной регистрации знака.

### 6. Запрос разрешения

Запросы на использование Знаков за пределами раздела 2 направляйте правообладателю через [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues) с пометкой `trademark`.

---

## English version

### 1. Not covered by the GPL-3.0 license

The ZABOR source code is distributed under the [GNU General Public License v3.0 only](LICENSE). GPL-3.0 grants rights to use, study, modify and redistribute the **code**, but grants **no rights to trademarks or other identifying marks**. This reservation is expressly permitted by GPL-3.0 section 7(e), which allows declining to grant rights under trademark law without rendering the license non-free.

The following are owned by the copyright holder (vnkdevelop) and are **outside the scope of the GPL-3.0 grant** — the "Marks":

- the word mark **ZABOR** in any spelling, case or transliteration (ZABOR, Zabor, zabor, ЗАБОР, Забор);
- the application logo and icon, including `build/icon.ico` and `resources/icon.png`;
- branded interface imagery in the `docs/images/` directory;
- the application identifier: the effective one is `com.zabor.desktop` (set in `package.json`, section `build`, which is the active build configuration); the historical one is `com.zabor.app`, set in [electron-builder.yml](electron-builder.yml), which is not applied by the current build; the executable name `ZABOR.exe`, and shortcut and installer names;
- domain names and network addresses of the official ZABOR service.

Receiving a copy of the code under GPL-3.0 does **not** grant permission to ship a product under the Marks.

### 2. Permitted without separate consent

Fair, descriptive references to the Marks are permitted where they do not imply origin or endorsement:

- truthful compatibility statements: "compatible with the ZABOR protocol", "a fork based on the ZABOR source code";
- links to the official repository and releases;
- references in reviews, articles, academic and educational materials;
- retaining unmodified authorship and license notices in source files — GPL-3.0 in fact requires this.

### 3. Not permitted

Without prior written consent of the copyright holder, you may not:

- name a derivative, modified version, fork, build or repackaged installer using the **Marks**;
- use the Marks in the product name, application window, installer, shortcut or executable name of a derivative;
- use the Marks or confusingly similar signs in domain names, account names, channels, or app store listings;
- use the logo or icon in any derivative, including redrawn or restyled variants;
- imply that a derivative is official, maintained by, or endorsed by the copyright holder;
- use the Marks in advertising or in the names of paid services.

### 4. Requirements for forks and derivatives

Forking is permitted — GPL-3.0 requires it — but a fork must be unambiguously distinguishable from the official product. Before publishing a derivative you must:

1. replace `productName`, `executableName` and `shortcutName` in the active build configuration — the `build` section of [package.json](package.json) — and also in [electron-builder.yml](electron-builder.yml) if that file is used;
2. replace `appId` with your own identifier, different from the effective `com.zabor.desktop` and the historical `com.zabor.app`;
3. replace `name` in [package.json](package.json);
4. replace `build/icon.ico` and `resources/icon.png` with your own artwork;
5. remove the Marks from the interface, window title, splash screen and localization files;
6. remove branded imagery from `docs/images/`;
7. state in your description that the project is an independent fork, unaffiliated with ZABOR and its copyright holder;
8. operate your own server backend — see section 4 of [TERMS.md](TERMS.md).

Requirements 1–7 concern the Marks only and do not restrict your GPL-3.0 rights in the code. Removing authorship and license notices remains **prohibited** — that is a separate GPL-3.0 requirement.

### 5. Legal status of the Marks

As of publication of this document, the Marks are used as **unregistered designations** and the "™" symbol applies. The "®" symbol is not used and must not be used prior to State registration of the mark. The absence of registration does not constitute a waiver of the copyright holder's rights, including protection against unfair competition and consumer confusion.

This policy may be updated following State registration of the mark.

### 6. Requesting permission

Requests to use the Marks beyond section 2 should be sent to the copyright holder via [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues) labelled `trademark`.

---

Copyright © 2026 vnkdevelop. ZABOR™.
