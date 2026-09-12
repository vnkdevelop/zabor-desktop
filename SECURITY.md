# Политика раскрытия уязвимостей ZABOR / ZABOR Security Policy

> Редакция от 11 сентября 2026 года.
> Русская версия является основной и имеет преимущественную силу при расхождении с переводом.
> The Russian version is authoritative; the English translation follows below.

---

## Русская версия

### 1. Как сообщить об уязвимости

Сообщения об уязвимостях принимаются **приватно** через [Security Advisories](https://github.com/vnkdevelop/zabor-desktop/security/advisories/new).

Публичные issue для сообщений об уязвимостях не используются: до выпуска исправления публичное раскрытие ставит под угрозу пользователей Сервиса.

### 2. Что просим указать в сообщении

1. Версию приложения и операционную систему.
2. Описание уязвимости и её влияние.
3. Пошаговый сценарий воспроизведения.
4. Проверочный код (proof of concept), если он есть.
5. Ваши предположения о возможном исправлении.

### 3. Область действия

Политика распространяется на:

- клиентское приложение ZABOR Desktop и его сборки из раздела [Releases](https://github.com/vnkdevelop/zabor-desktop/releases);
- механизм проверки подписи сборки и описанный в [docs/client-attestation.md](docs/client-attestation.md);
- серверную инфраструктуру Оператора, к которой предоставлен доступ;
- криптографическую часть обмена сообщениями: [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md).

Политика **не** распространяется на:

- уязвимости сторонних библиотек и моделей — сообщайте их авторам, перечень в [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md);
- серверы, развёрнутые третьими лицами;
- форки и производные версии.

### 4. Правила добросовестного исследования

Разрешено исследовать **исключительно собственные учётные записи и собственные устройства**, а также инфраструктуру при наличии явного письменного разрешения Оператора.

Категорически запрещено:

1. получать доступ к учётным записям, данным и переписке других пользователей;
2. нарушать работу Сервиса, создавать чрезмерную нагрузку, проводить атаки на отказ в обслуживании;
3. применять социальную инженерию в отношении пользователей и Оператора;
4. использовать полученный доступ для извлечения данных, изменения данных или закрепления в инфраструктуре;
5. раскрывать сведения об уязвимости третьим лицам до её устранения.

При соблюдении этих правил Оператор не будет инициировать преследование за исследование. Настоящее положение не является отказом от прав и не действует в отношении действий, нарушающих законодательство Российской Федерации.

### 5. Что Оператор считает наиболее важным

1. Обход, отключение или подделка механизма проверки подписи сборки.
2. Компрометация ключей шифрования переписки и нарушение конфиденциальности сообщений.
3. Получение доступа к учётным записям других пользователей.
4. Выполнение кода на устройствах пользователей через передаваемые данные.
5. Утечка данных сессии или ключей из локального хранилища.

### 6. Сроки и порядок

1. Оператор подтверждает получение сообщения.
2. Оператор оценивает уязвимость и сообщает о решении: принять, отклонить или запросить уточнения.
3. Оператор уведомляет автора сообщения о выпуске исправления.
4. Публичное раскрытие производится по согласованию с автором сообщения и после выпуска исправления.

Оператор является физическим лицом и обеспечивает поддержку в разумные сроки, без гарантии круглосуточной реакции. Программа вознаграждений не предусмотрена; при публикации исправления авторство сообщения указывается по желанию автора.

### 7. Приоритетные версии

Поддерживается только последняя выпущенная версия приложения. Устаревшие версии могут отклоняться сервером в соответствии с разделом 4 [TERMS.md](TERMS.md).

---

## English version

### 1. How to report a vulnerability

Vulnerability reports are accepted **privately** via [Security Advisories](https://github.com/vnkdevelop/zabor-desktop/security/advisories/new).

Public issues are not used for vulnerability reports: prior to a fix, public disclosure puts users of the Service at risk.

### 2. What to include

1. Application version and operating system.
2. Description of the vulnerability and its impact.
3. Step-by-step reproduction scenario.
4. Proof of concept, if available.
5. Your suggestions for a possible fix.

### 3. Scope

This policy covers:

- the ZABOR Desktop client application and the builds published on the [Releases](https://github.com/vnkdevelop/zabor-desktop/releases) page;
- the build-signature mechanism described in [docs/client-attestation.md](docs/client-attestation.md);
- the Controller's server infrastructure to which you have been granted access;
- the messaging cryptography described in [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md).

This policy does **not** cover:

- vulnerabilities in third-party libraries and models — report them to their authors; the list is in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md);
- servers deployed by third parties;
- forks and derivative versions.

### 4. Responsible research rules

You may test **only your own accounts and your own devices**, or infrastructure for which you hold explicit written permission from the Controller.

You must not:

1. access other users' accounts, data or conversations;
2. disrupt the Service, create excessive load, or conduct denial-of-service attacks;
3. use social engineering against users or the Controller;
4. use any access obtained to exfiltrate data, alter data, or establish persistence in the infrastructure;
5. disclose vulnerability details to third parties before a fix is released.

Subject to these rules, the Controller will not pursue legal action for research. This statement is not a waiver of rights and does not apply to conduct that violates the law of the Russian Federation.

### 5. What the Controller considers most critical

1. Circumventing, disabling or forging the build-signature mechanism.
2. Compromise of messaging encryption keys and breach of message confidentiality.
3. Gaining access to other users' accounts.
4. Code execution on user devices through transmitted data.
5. Leakage of session data or keys from local storage.

### 6. Timelines and process

1. The Controller acknowledges receipt of the report.
2. The Controller assesses the vulnerability and responds: accept, decline, or request clarification.
3. The Controller notifies the reporter when a fix is released.
4. Public disclosure takes place in coordination with the reporter and after the fix is released.

The Controller is an individual and provides support within a reasonable time, without a guaranteed round-the-clock response. No bounty programme is offered; when a fix is published, credit is given at the reporter's discretion.

### 7. Supported versions

Only the latest released version of the application is supported. Outdated versions may be rejected by the server in accordance with section 4 of [TERMS.md](TERMS.md).

---

Copyright © 2026 vnkdevelop.
