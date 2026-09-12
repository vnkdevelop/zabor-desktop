# Политика в отношении обработки персональных данных ZABOR / ZABOR Privacy Policy

> Редакция от 11 сентября 2026 года.
> Русская версия является основной и имеет преимущественную силу при расхождении с переводом.
> The Russian version is authoritative; the English translation follows below.

Настоящая Политика является отдельным документом, опубликованным во исполнение части 2 статьи 18.1 Федерального закона от 27.07.2006 № 152-ФЗ «О персональных данных», и не заменяет [TERMS.md](TERMS.md): Условия регулируют доступ к Сервису, Политика — обработку персональных данных.

---

## Русская версия

### 1. Оператор

Оператором персональных данных является физическое лицо — vnkdevelop (далее — **«Оператор»**).

Обращения по вопросам обработки персональных данных, включая реализацию прав субъекта, направляются через [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues) с пометкой `privacy`. Сообщения об уязвимостях — через [Security Advisories](https://github.com/vnkdevelop/zabor-desktop/security/advisories/new).

### 2. Область применения

Политика распространяется на обработку персональных данных в связи с использованием **сервиса ZABOR** — серверной инфраструктуры, обеспечивающей регистрацию пользователей, обмен сигнальными сообщениями, голосовые каналы, звонки, обмен сообщениями и синхронизацию состояния (**«Сервис»**).

Обработка персональных данных осуществляется в связи с использованием **серверов Оператора**. Использование приложения ZABOR с собственным сервером не влечёт обработки данных Оператором.

### 3. Категории обрабатываемых данных

#### 3.1. Данные, предоставляемые пользователем

| Данные | Обязательность | Цель |
| --- | --- | --- |
| Имя пользователя (логин) | Обязательно | Идентификация учётной записи |
| Пароль (хранится в виде хеша) | Обязательно | Аутентификация |
| Отображаемое имя | Необязательно | Отображение в интерфейсе |
| Аватар и его цвет | Необязательно | Отображение в интерфейсе |
| Текст «о себе» | Необязательно | Отображение в профиле |
| Настройки звука | Формируется автоматически | Работа голосовой связи |

Оператор **не запрашивает** фамилию, имя, отчество, адрес электронной почты, номер телефона, документы, сведения о месте жительства и иные идентифицирующие сведения. Логин выбирается пользователем самостоятельно.

#### 3.2. Данные, формируемые автоматически

- статус присутствия в сети;
- данные достижений в приложении;
- технические журналы подключений: версия и канал сборки приложения, время подключения, результат проверки подписи сборки, сетевой адрес подключения;
- идентификатор учётной записи во внутренних структурах сервера.

#### 3.3. Метаданные обмена сообщениями

При использовании встроенного обмена сообщениями Оператор обрабатывает **только метаданные**, необходимые для доставки и синхронизации:

- кто с кем обменивается сообщениями (пара участников);
- время отправки, порядковые номера сообщений;
- состояние доставки и прочтения;
- размер и контрольная сумма передаваемых файлов.

**Содержание сообщений, файлов, голоса, видео и трансляции экрана Оператору недоступно** — см. раздел 4.

### 4. Что не обрабатывается на сервере

1. **Содержание сообщений.** Текст сообщений передаётся в зашифрованном виде, ключи формируются на устройствах участников и не передаются Оператору. Сервер не располагает средствами расшифрования.
2. **Голос, видео, трансляция экрана.** Передаются напрямую между участниками по технологии WebRTC. Через сервер проходят только сигнальные сообщения, необходимые для установления соединения.
3. **Содержимое передаваемых файлов.** Файлы передаются напрямую между участниками; при этом Оператор обрабатывает метаданные, указанные в разделе 3.3.

Прикладное шифрование (AES-GCM с ключом, выведенным по ECDH) применяется, когда сообщение передаётся через сервер Оператора; при прямом соединении используется транспортное шифрование WebRTC. В обоих случаях содержание Оператору недоступно, но механизмы защиты различаются — это описано в [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md) и [docs/chat-data-flow.md](docs/chat-data-flow.md).

Для ряда соединений используется ретрансляция трафика (TURN), при которой ретранслирующий узел технически видит сетевые адреса участников.

### 5. Цели и правовые основания обработки

Обработка осуществляется в следующих целях:

1. **Предоставление доступа к Сервису** — регистрация, аутентификация, поддержание соединений, доставка сообщений. Правовое основание: исполнение соглашения об использовании Сервиса ([TERMS.md](TERMS.md)).
2. **Обеспечение работоспособности и безопасности** — защита от атак, ограничение частоты запросов, выявление нарушений Условий, проверка подлинности сборок приложения. Правовое основание: законный интерес Оператора.
3. **Ведение технических журналов** — диагностика неисправностей, разбор инцидентов. Правовое основание: законный интерес Оператора.

Оператор не использует данные для рекламы, профилирования, таргетинга и не принимает основанных на них автоматизированных решений, порождающих юридические последствия.

### 6. Обработка данных на устройстве пользователя

Значительная часть данных обрабатывается **локально на устройстве пользователя** и Оператору не передаётся:

| Данные | Место хранения | Срок |
| --- | --- | --- |
| История сообщений | локальная база данных браузерного хранилища (`IndexedDB`, `zabor-chat-v1`) | 14 суток, затем удаляется автоматически |
| Полученные и отправленные файлы | каталог данных приложения на диске | до удаления пользователем или очистки |
| Ключ шифрования переписки | файл `chat-identity.enc`, шифруется средствами операционной системы (DPAPI / Keychain) | до удаления данных приложения |
| Данные сессии | файл `session.enc`, шифруется средствами операционной системы | до выхода из учётной записи |
| Настройки шумоподавления, калибровка звука, достижения, отметки о прочтении | локальное хранилище приложения | до очистки данных приложения |

Удаление сообщения в интерфейсе удаляет его **на устройстве пользователя**; копия, уже переданная собеседнику, удалению Оператором не подлежит.

### 7. Передача третьим лицам

Оператор **не продаёт** персональные данные и **не передаёт** их третьим лицам для маркетинговых целей.

Передача ограничена следующими случаями:

1. **Установление соединений.** Для определения сетевых маршрутов используются внешние службы STUN, а для ретрансляции — службы TURN. Указанным службам технически доступны сетевые адреса участников. Выбор служб определяется конфигурацией приложения и сервера.
2. **Хостинг.** Данные размещаются у поставщика серверной инфраструктуры, обеспечивающего работу Сервиса.
3. **Требования закона.** Передача производится при поступлении законного требования уполномоченного государственного органа. Порядок рассмотрения таких требований описан в [docs/legal-requests.md](docs/legal-requests.md).

### 8. Трансграничная передача

При использовании внешних служб установления соединений, размещённых за пределами Российской Федерации, возможна трансграничная передача данных ограниченного состава (сетевые адреса участников).

Первичные базы данных, содержащие персональные данные граждан Российской Федерации, размещаются на территории Российской Федерации в соответствии с частью 5 статьи 18 Федерального закона № 152-ФЗ и Федеральным законом от 21.07.2014 № 242-ФЗ.

### 9. Сроки хранения и удаление

1. Данные учётной записи хранятся до её удаления.
2. Технические журналы хранятся не дольше срока, необходимого для целей раздела 5.
3. Метаданные обмена сообщениями хранятся в объёме и в сроки, определяемые конфигурацией сервера и требованиями законодательства.
4. Данные на устройстве пользователя хранятся в соответствии с разделом 6 и удаляются вместе с данными приложения.

Удаление учётной записи производится по обращению пользователя через [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues). Удаление учётной записи влечёт удаление данных учётной записи; данные, хранящиеся на устройствах участников обмена сообщениями, Оператором не контролируются.

### 10. Права субъекта персональных данных

Пользователь вправе:

1. получать сведения об обработке своих персональных данных (статья 14 Федерального закона № 152-ФЗ);
2. требовать уточнения, блокирования или уничтожения данных, если они неполны, устарели, неточны, получены незаконно или не нужны для целей обработки;
3. отозвать согласие на обработку в объёме, где обработка основана на согласии;
4. обжаловать действия Оператора в уполномоченный орган по защите прав субъектов персональных данных (Роскомнадзор) или в суд.

Обращение рассматривается в срок, установленный законодательством. Для защиты учётной записи Оператор вправе запросить подтверждение принадлежности учётной записи обратившемуся.

### 11. Меры защиты

Оператор принимает правовые, организационные и технические меры, включая: хранение паролей только в виде хешей, шифрование данных сессии и ключей переписки средствами операционной системы, ограничение доступа к серверной инфраструктуре, проверку подлинности сборок приложения, ограничение частоты запросов, применение шифрования при передаче данных.

При этом Оператор прямо указывает в разделе 10 [TERMS.md](TERMS.md): Сервис предоставляется «как есть», без гарантии бесперебойной работы и абсолютной защиты.

### 12. Несовершеннолетние

Сервис не предназначен для лиц младше 14 лет. Лица от 14 до 18 лет используют Сервис с согласия законных представителей. Оператор не осуществляет целенаправленный сбор данных о несовершеннолетних и не проверяет возраст документально; при выявлении использования Сервиса лицом младше 14 лет доступ ограничивается.

### 13. Изменение Политики

Оператор вправе изменять Политику. Действующая редакция публикуется в файле `PRIVACY.md` репозитория. Существенные изменения объявляются в описании релиза. Продолжение использования Сервиса после публикации новой редакции означает ознакомление с ней.

---

## English version

### 1. Controller

The controller of personal data is an individual, vnkdevelop (the **"Controller"**).

Enquiries regarding personal data processing, including the exercise of data subject rights, should be sent via [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues) labelled `privacy`. Vulnerability reports: via [Security Advisories](https://github.com/vnkdevelop/zabor-desktop/security/advisories/new).

### 2. Scope

This Policy applies to the processing of personal data in connection with use of the **ZABOR service** — the server infrastructure providing user registration, signaling, voice channels, calls, messaging and state synchronization (the **"Service"**).

Processing takes place in connection with use of the **Controller's servers**. Running the ZABOR application against your own server does not involve processing of your data by the Controller.

### 3. Categories of data processed

#### 3.1. Data provided by the user

| Data | Required | Purpose |
| --- | --- | --- |
| Username | Required | Account identification |
| Password (stored as a hash) | Required | Authentication |
| Display name | Optional | Display in the interface |
| Avatar and avatar colour | Optional | Display in the interface |
| "About me" text | Optional | Profile display |
| Audio settings | Generated automatically | Voice communication |

The Controller does **not** request given names, surnames, email addresses, phone numbers, identity documents, residence details or other identifying information. The username is chosen by the user.

#### 3.2. Automatically generated data

- online presence status;
- in-app achievement data;
- technical connection logs: application version and build channel, connection time, result of build signature verification, connection network address;
- account identifier within the Service's internal structures.

#### 3.3. Message metadata

When using the built-in messaging feature, the Controller processes **metadata only**, to the extent required for delivery and synchronization:

- which participants exchange messages (the pair of participants);
- send time and message sequence numbers;
- delivery and read state;
- size and checksum of transferred files.

**The content of messages, files, voice, video and screen shares is not available to the Controller** — see section 4.

### 4. What the server does not process

1. **Message content.** Message text is transmitted encrypted; keys are generated on participants' devices and are never transmitted to the Controller. The server holds no means of decryption.
2. **Voice, video, screen shares.** Transmitted directly between participants using WebRTC. Only the signaling messages needed to establish a connection traverse the server.
3. **Contents of transferred files.** Files are transferred directly between participants; the Controller processes the metadata listed in section 3.3.

Application-layer encryption (AES-GCM with a key derived via ECDH) is applied when a message is transmitted through the Controller's server; on a direct connection, WebRTC transport encryption is used. In both cases the content is unavailable to the Controller, but the protection mechanisms differ — this is described in [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md) and [docs/chat-data-flow.md](docs/chat-data-flow.md).

Some connections use traffic relaying (TURN), where the relaying node technically sees participants' network addresses.

### 5. Purposes and legal bases

Processing is carried out for the following purposes:

1. **Providing access to the Service** — registration, authentication, maintaining connections, message delivery. Legal basis: performance of the service agreement ([TERMS.md](TERMS.md)).
2. **Operating and securing the Service** — attack protection, rate limiting, detection of Terms violations, verification of application build authenticity. Legal basis: the Controller's legitimate interest.
3. **Maintaining technical logs** — fault diagnosis, incident analysis. Legal basis: the Controller's legitimate interest.

The Controller does not use the data for advertising, profiling or targeting, and does not take automated decisions producing legal effects.

### 6. Processing on the user's device

Much of the data is processed **locally on the user's device** and never reaches the Controller:

| Data | Storage location | Retention |
| --- | --- | --- |
| Message history | local browser storage database (`IndexedDB`, `zabor-chat-v1`) | 14 days, then deleted automatically |
| Received and sent files | application data directory on disk | until deleted by the user or cleaned |
| Messaging encryption key | `chat-identity.enc`, encrypted by the operating system (DPAPI / Keychain) | until application data is deleted |
| Session data | `session.enc`, encrypted by the operating system | until sign-out |
| Noise-suppression settings, audio calibration, achievements, read markers | local application storage | until application data is cleared |

Deleting a message in the interface deletes it **on the user's device**; a copy already delivered to the other participant is not subject to deletion by the Controller.

### 7. Disclosure to third parties

The Controller does **not sell** personal data and does **not disclose** it to third parties for marketing purposes.

Disclosure is limited to the following cases:

1. **Establishing connections.** External STUN services are used to determine network routes, and TURN services for relaying. Such services technically have access to participants' network addresses. The choice of services is determined by the application and server configuration.
2. **Hosting.** Data is hosted with a server infrastructure provider operating the Service.
3. **Legal requirements.** Disclosure occurs upon a lawful request from a competent state authority. The procedure is described in [docs/legal-requests.md](docs/legal-requests.md).

### 8. Cross-border transfer

Where external connection-establishment services located outside the Russian Federation are used, cross-border transfer of a limited set of data (participants' network addresses) is possible.

Primary databases containing personal data of citizens of the Russian Federation are located in the territory of the Russian Federation in accordance with part 5 of article 18 of Federal Law No. 152-FZ and Federal Law No. 242-FZ of 21 July 2014.

### 9. Retention and deletion

1. Account data is retained until the account is deleted.
2. Technical logs are retained no longer than necessary for the purposes in section 5.
3. Message metadata is retained in the scope and for the periods determined by the server configuration and legal requirements.
4. Data on the user's device is retained in accordance with section 6 and is deleted together with the application data.

Account deletion is performed on request via [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues). Account deletion results in deletion of account data; data held on the devices of messaging participants is not controlled by the Controller.

### 10. Data subject rights

The user has the right to:

1. obtain information about the processing of their personal data (article 14 of Federal Law No. 152-FZ);
2. demand rectification, blocking or destruction of data that is incomplete, outdated, inaccurate, unlawfully obtained or unnecessary for the purposes of processing;
3. withdraw consent to processing to the extent that processing is based on consent;
4. appeal the Controller's actions to the authorized personal data protection authority (Roskomnadzor) or to a court.

Requests are considered within the period established by law. To protect the account, the Controller may request confirmation that the account belongs to the requester.

### 11. Security measures

The Controller adopts legal, organizational and technical measures including: storing passwords only as hashes, encrypting session data and messaging keys using operating system facilities, restricting access to server infrastructure, verifying application build authenticity, rate limiting, and using encryption in transit.

At the same time, the Controller states expressly in section 10 of [TERMS.md](TERMS.md): the Service is provided "as is", without warranty of uninterrupted operation or absolute protection.

### 12. Minors

The Service is not intended for persons under 14. Persons aged 14 to 18 use the Service with the consent of their legal guardians. The Controller does not knowingly collect data on minors and does not verify age by documentary means; where use of the Service by a person under 14 is identified, access is restricted.

### 13. Changes to this Policy

The Controller may amend this Policy. The current version is published in the repository's `PRIVACY.md`. Material changes are announced in the release notes. Continued use of the Service after publication constitutes acknowledgement of the new version.

---

Copyright © 2026 vnkdevelop.
