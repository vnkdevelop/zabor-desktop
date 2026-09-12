# Условия использования сервиса ZABOR / ZABOR Terms of Service

> Редакция от 11 сентября 2026 года. Предыдущая редакция — от 18 августа 2026 года.
> Русская версия является основной и имеет преимущественную силу при расхождении с переводом.
> The Russian version is authoritative; the English translation follows below.
>
> Связанные документы, являющиеся неотъемлемой частью Условий: [PRIVACY.md](PRIVACY.md), [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md), [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md), [TRADEMARK.md](TRADEMARK.md).
>
> Related documents forming an integral part of these Terms: [PRIVACY.md](PRIVACY.md), [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md), [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md), [TRADEMARK.md](TRADEMARK.md).

---

## Русская версия

### 1. Общие положения

Настоящие условия (**«Условия»**) регулируют использование **сервиса ZABOR** — серверной инфраструктуры, обеспечивающей регистрацию пользователей, обмен сигнальными сообщениями, голосовые каналы, звонки и синхронизацию состояния (**«Сервис»**).

Сервис предоставляется физическим лицом — vnkdevelop (**«Оператор»**), безвозмездно.

#### 1.1. Что Условия регулируют, а что нет

Это принципиальное разграничение:

- Условия регулируют **доступ к Сервису**, то есть к серверам Оператора.
- Условия **не ограничивают** и не могут ограничивать ваши права на программное обеспечение ZABOR, предоставленные лицензией [GPL-3.0](LICENSE). Право использовать, изучать, изменять и распространять код сохраняется за вами в полном объёме независимо от согласия с настоящими Условиями.

Оператор не обязан предоставлять доступ к своим серверам кому-либо: лицензия GPL-3.0 распространяется на программу, а не на инфраструктуру Оператора. Отказ в доступе к Сервису не является ограничением прав по GPL-3.0.

### 2. Принятие условий

Использование Сервиса, включая создание учётной записи и подключение приложения к серверу, означает согласие с Условиями. Если вы не согласны — не используйте Сервис; вы по-прежнему можете использовать приложение с собственным сервером.

Использовать Сервис могут лица, достигшие 14 лет. Лица от 14 до 18 лет используют Сервис с согласия законных представителей. Возраст документально не проверяется; ограничение является условием использования, а не технической мерой. Требования к содержимому, передаваемому через Сервис, установлены в [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md), который является неотъемлемой частью Условий.

### 3. Учётная запись

1. При регистрации вы указываете имя пользователя и пароль. Вы отвечаете за сохранность пароля и за все действия, совершённые под вашей учётной записью.
2. Запрещено передавать доступ к учётной записи третьим лицам и регистрировать учётные записи автоматизированными средствами.
3. Оператор вправе отказать в регистрации имени, вводящего в заблуждение относительно принадлежности к Оператору либо нарушающего права третьих лиц.

### 4. Требования к клиентскому приложению

Это ключевой раздел. Он не ограничивает ваше право изменять код — он определяет, какие подключения принимает сервер Оператора.

1. К Сервису допускаются **только официальные сборки** приложения ZABOR, распространяемые Оператором через страницу [Releases](https://github.com/vnkdevelop/zabor-desktop/releases).
2. Официальные сборки при подключении передают серверу **подпись сборки**. Сервер проверяет её и вправе отклонить соединение при отсутствии либо недействительности подписи. Технические детали описаны в [docs/client-attestation.md](docs/client-attestation.md).
3. Запрещается подключать к Сервису:
   - самостоятельно собранные, изменённые и переупакованные сборки приложения;
   - форки и производные версии;
   - сторонние клиенты и программные средства, реализующие протокол Сервиса.
4. Запрещается обходить, отключать, подделывать и воспроизводить механизм проверки подписи сборки, а также извлекать для этих целей ключи из официальных сборок.
5. **Форки и производные версии обязаны использовать собственный серверный бэкенд.** Исходный код приложения открыт, и вы вправе развернуть собственный сервер; подключение производной версии к серверу Оператора не допускается.
6. Оператор вправе устанавливать минимальную поддерживаемую версию приложения и отклонять соединения устаревших версий, когда это необходимо для работоспособности и безопасности Сервиса.

Пункты 1–4 являются условиями доступа к инфраструктуре Оператора и не являются дополнительными ограничениями в смысле раздела 7 GPL-3.0 применительно к программе.

### 5. Запрещённые действия

При использовании Сервиса запрещается:

1. создавать чрезмерную нагрузку, проводить атаки на отказ в обслуживании, сканировать инфраструктуру, эксплуатировать уязвимости;
2. получать несанкционированный доступ к учётным записям и данным других пользователей;
3. рассылать спам, осуществлять массовые автоматизированные обращения, использовать Сервис для ботнетов и автоматизированного накопления учётных записей;
4. размещать и передавать материалы, распространение которых запрещено законом, а также материалы, нарушающие права третьих лиц;
5. осуществлять травлю, угрозы, преследование и умышленное создание помех связи другим пользователям;
6. записывать разговоры других участников без их согласия, когда такое согласие требуется по закону;
7. использовать Сервис для коммерческой перепродажи доступа без согласия Оператора;
8. выдавать себя за Оператора либо за официальную поддержку ZABOR;
9. передавать файлы и материалы, запрещённые разделом 2 [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md);
10. предлагать для включения в приложение материалы, не соответствующие разделу 3 и разделу 7 [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md).

Ответственность за содержимое, передаваемое через Сервис, несёт пользователь, его передавший. Оператор не осуществляет предварительную проверку содержимого: сообщения и файлы передаются в зашифрованном виде и Оператору недоступны, как указано в разделе 7.

### 6. Модерация и прекращение доступа

1. Оператор вправе ограничить, приостановить или прекратить доступ к Сервису при нарушении Условий — включая нарушение раздела 4 — без предварительного уведомления, если это необходимо для защиты Сервиса и его пользователей.
2. Оператор вправе применять серверные ограничения: отключение микрофона, исключение из канала, ограничение частоты запросов.
3. Оператор не обязан обосновывать решения о блокировке и восстанавливать доступ, но рассматривает обращения через [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues).

### 7. Данные пользователей

1. Сервер хранит: имя пользователя, хеш пароля, отображаемое имя, аватар, цвет аватара, текст «о себе», настройки звука, данные достижений и статус присутствия.
2. **Голос, видео и трансляции экрана передаются напрямую между участниками по технологии WebRTC (P2P) и не проходят через сервер Оператора.** Через сервер передаются только сигнальные сообщения, необходимые для установления соединения. При прямом соединении участники звонка получают сетевые адреса друг друга — это неотъемлемое свойство P2P-связи.
3. **Сообщения и файлы.** При использовании встроенного обмена сообщениями сервер обрабатывает только метаданные, необходимые для доставки: пару участников, время и порядковые номера сообщений, состояние доставки и прочтения, а также размер и контрольную сумму файлов. **Содержание сообщений и файлов серверу недоступно** — см. раздел 8.
4. **Хранение на устройстве.** История сообщений хранится локально на устройстве пользователя и удаляется автоматически по истечении 14 суток. Полученные файлы хранятся в каталоге данных приложения до их удаления пользователем.
5. **Удаление сообщения в интерфейсе удаляет его на устройстве пользователя.** Копия, уже переданная собеседнику, находится на его устройстве и Оператором не удаляется и не может быть удалена.
6. **Резервный маршрут и ретрансляция.** Если прямое соединение не установлено, зашифрованные пакеты сообщений передаются через сервер Оператора; содержимое при этом остаётся недоступным. Для части соединений применяется ретрансляция трафика (TURN), при которой сетевые адреса участников технически видит ретранслирующий узел. Для определения маршрутов используются внешние службы установления соединений (STUN).
7. Оператор не продаёт и не передаёт данные третьим лицам, за исключением случаев, предусмотренных законом.
8. Сервер ведёт технические журналы подключений, включая версию и канал сборки приложения, время подключения и результат проверки подписи. Журналы используются для диагностики и защиты Сервиса.
9. Удаление учётной записи возможно по обращению к Оператору.
10. Состав обрабатываемых данных, цели и правовые основания обработки, сроки хранения и порядок реализации прав субъекта определены в [PRIVACY.md](PRIVACY.md), который является неотъемлемой частью Условий.

### 8. Шифрование и недоступность содержимого

1. Обмен сообщениями защищён шифрованием: содержимое недоступно ни Оператору, ни третьим лицам, участвующим в передаче. Ключи формируются и хранятся **исключительно на устройствах пользователей**; Оператор не формирует, не получает и не хранит ключи шифрования переписки.
2. Защита устроена по-разному в зависимости от маршрута: при прямом соединении между участниками применяется транспортное шифрование WebRTC, при передаче через сервер Оператора — прикладное шифрование AES-GCM с ключом, выведенным по ECDH. Различие и его последствия описаны в [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md).
3. Оператор **не располагает средствами расшифрования** содержимого сообщений и файлов и не имеет технической возможности их получить. Средства расшифрования в серверной части не реализованы.
4. Применяемые криптографические средства, их назначение и **границы защитных свойств** — включая отсутствие прямой секретности, ограничения модели доверия при первом контакте и различие маршрутов — описаны в [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md), который является неотъемлемой частью Условий.
5. Следствия для пользователя: Оператор не может просмотреть, восстановить, выдать по запросу или удалить содержимое переписки, а также не может отредактировать отдельное сообщение, не затронув учётную запись.
6. Следствие для жалоб: Оператор не может подтвердить или опровергнуть содержание конкретного сообщения по существу. Меры применяются к учётной записи и к доступу к Сервису; порядок изложен в [docs/content-complaints.md](docs/content-complaints.md).

### 9. Запросы уполномоченных органов

1. Оператор исполняет законные требования уполномоченных государственных органов в пределах законодательства Российской Федерации и в пределах технической возможности, определяемой разделом 8.
2. Оператор может предоставить сведения учётной записи, технические журналы подключений и метаданные обмена сообщениями в объёме, указанном в [docs/legal-requests.md](docs/legal-requests.md).
3. Оператор **не может предоставить** содержимое сообщений и файлов, записи голоса, видео и трансляции экрана, а также ключи шифрования переписки, поскольку они у него отсутствуют.
4. Оператор не устанавливает на устройства пользователей средства сбора содержимого и не создаёт данные, которыми не располагает.
5. Порядок рассмотрения запросов, проверки их законности и фиксации результатов описан в [docs/legal-requests.md](docs/legal-requests.md).

### 10. Доступность Сервиса

Сервис предоставляется на условиях **«как есть»** и **«как доступно»**. Оператор не гарантирует бесперебойную работу, сохранность данных, определённое качество связи и совместимость с конкретным оборудованием. Возможны технические перерывы, изменение или прекращение работы Сервиса без предварительного уведомления.

### 11. Ограничение ответственности

Сервис предоставляется безвозмездно. В максимально допустимом законом объёме Оператор не несёт ответственности за упущенную выгоду, утрату данных, невозможность использования Сервиса, качество связи и любые косвенные убытки, возникшие в связи с использованием или невозможностью использования Сервиса.

Ничто в настоящем разделе не исключает ответственность, которая не может быть исключена по законодательству Российской Федерации.

### 12. Изменение Условий

Оператор вправе изменять Условия. Действующая редакция публикуется в файле `TERMS.md` репозитория. Существенные изменения раздела 4 объявляются в описании релиза. Продолжение использования Сервиса после публикации новой редакции означает согласие с ней.

### 13. Применимое право

К Условиям применяется право Российской Федерации. Споры, не урегулированные путём переговоров, подлежат рассмотрению по месту жительства Оператора в соответствии с законодательством Российской Федерации.

### 14. Контакты

Обращения по вопросам Сервиса: [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues).
Сообщения об уязвимостях — приватно, через [Security Advisories](https://github.com/vnkdevelop/zabor-desktop/security/advisories/new).

---

## English version

### 1. General

These terms (the **"Terms"**) govern use of the **ZABOR service** — the server infrastructure providing user registration, signaling, voice channels, calls and state synchronization (the **"Service"**).

The Service is provided by an individual, vnkdevelop (the **"Operator"**), free of charge.

#### 1.1. What the Terms do and do not govern

This distinction is fundamental:

- The Terms govern **access to the Service**, i.e. to the Operator's servers.
- The Terms do **not** restrict, and cannot restrict, your rights in the ZABOR software granted by the [GPL-3.0](LICENSE) license. Your rights to use, study, modify and redistribute the code remain fully intact regardless of whether you accept these Terms.

The Operator is under no obligation to provide anyone with access to its servers: GPL-3.0 covers the program, not the Operator's infrastructure. Denying access to the Service is not a restriction of GPL-3.0 rights.

### 2. Acceptance

Using the Service, including creating an account and connecting the application to the server, constitutes acceptance of these Terms. If you do not agree, do not use the Service; you may still run the application against your own server.

The Service may be used by persons aged 14 and over. Persons aged 14 to 18 must have the consent of their legal guardians. Age is not verified by documentary means; the restriction is a condition of use, not a technical measure. Requirements for content transmitted through the Service are set out in [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md), which forms an integral part of these Terms.

### 3. Accounts

1. Registration requires a username and password. You are responsible for keeping your password secure and for all activity under your account.
2. Sharing account access with third parties and creating accounts by automated means are prohibited.
3. The Operator may refuse a username that misleads as to affiliation with the Operator or infringes third-party rights.

### 4. Client application requirements

This is the key section. It does not limit your right to modify the code — it defines which connections the Operator's server accepts.

1. Only **official builds** of the ZABOR application, distributed by the Operator via the [Releases](https://github.com/vnkdevelop/zabor-desktop/releases) page, are admitted to the Service.
2. Official builds transmit a **build signature** on connection. The server verifies it and may reject connections presenting no signature or an invalid one. Technical details are described in [docs/client-attestation.md](docs/client-attestation.md).
3. You must not connect to the Service:
   - self-built, modified or repackaged builds of the application;
   - forks and derivative versions;
   - third-party clients or tools implementing the Service protocol.
4. You must not circumvent, disable, forge or reproduce the build-signature mechanism, nor extract keys from official builds for those purposes.
5. **Forks and derivatives must run their own server backend.** The application source is open and you are free to deploy your own server; connecting a derivative to the Operator's server is not permitted.
6. The Operator may set a minimum supported application version and reject connections from outdated versions where necessary for the Service's operability and security.

Sections 1–4 are conditions of access to the Operator's infrastructure and are not further restrictions within the meaning of GPL-3.0 section 7 as applied to the program.

### 5. Prohibited conduct

When using the Service, you must not:

1. create excessive load, conduct denial-of-service attacks, scan the infrastructure or exploit vulnerabilities;
2. gain unauthorized access to other users' accounts or data;
3. send spam, issue mass automated requests, or use the Service for botnets or automated account farming;
4. post or transmit material whose distribution is prohibited by law, or material infringing third-party rights;
5. engage in harassment, threats, stalking, or deliberate disruption of other users' communications;
6. record other participants' conversations without their consent where such consent is required by law;
7. commercially resell access to the Service without the Operator's consent;
8. impersonate the Operator or official ZABOR support;
9. transmit files or material prohibited by section 2 of [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md);
10. propose for inclusion in the application any material failing section 3 or section 7 of [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md).

Responsibility for content transmitted through the Service rests with the user who transmitted it. The Operator performs no pre-moderation of content: messages and files are transmitted encrypted and are not available to the Operator, as set out in section 7.

### 6. Moderation and termination

1. The Operator may restrict, suspend or terminate access to the Service upon breach of these Terms — including breach of section 4 — without prior notice where necessary to protect the Service and its users.
2. The Operator may apply server-side restrictions: server mute, removal from a channel, rate limiting.
3. The Operator is not obliged to justify enforcement decisions or restore access, but will consider appeals via [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues).

### 7. User data

1. The server stores: username, password hash, display name, avatar, avatar colour, "about me" text, audio settings, achievement data and presence status.
2. **Voice, video and screen shares are transmitted directly between participants over WebRTC (P2P) and do not pass through the Operator's server.** Only the signaling messages needed to establish a connection traverse the server. In a direct connection, call participants learn each other's network addresses — this is inherent to P2P communication.
3. **Messages and files.** When the built-in messaging feature is used, the server processes only the metadata required for delivery: the pair of participants, send time and message sequence numbers, delivery and read state, and the size and checksum of files. **The content of messages and files is not available to the server** — see section 8.
4. **On-device storage.** Message history is stored locally on the user's device and is deleted automatically after 14 days. Received files are stored in the application data directory until deleted by the user.
5. **Deleting a message in the interface deletes it on the user's device.** A copy already delivered to the other participant resides on their device and is not, and cannot be, deleted by the Operator.
6. **Fallback route and relaying.** Where a direct connection is not established, encrypted message packets are transmitted through the Operator's server; their content remains unavailable. Some connections use traffic relaying (TURN), where participants' network addresses are technically visible to the relaying node. External connection-establishment services (STUN) are used to determine routes.
7. The Operator does not sell or transfer data to third parties except as required by law.
8. The server keeps technical connection logs, including application version and build channel, connection time and signature verification result. Logs are used for diagnostics and to protect the Service.
9. Account deletion is available on request to the Operator.
10. The categories of data processed, the purposes and legal bases of processing, retention periods and the procedure for exercising data subject rights are set out in [PRIVACY.md](PRIVACY.md), which forms an integral part of these Terms.

### 8. Encryption and unavailability of content

1. Messaging is protected by encryption: the content is unavailable both to the Operator and to third parties involved in transmission. Keys are generated and stored **exclusively on users' devices**; the Operator does not generate, receive or store messaging encryption keys.
2. Protection differs by route: on a direct connection between participants, WebRTC transport encryption applies; when traffic passes through the Operator's server, application-layer AES-GCM encryption with an ECDH-derived key applies. The difference and its consequences are described in [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md).
3. The Operator has **no means of decrypting** message or file content and no technical means of obtaining it. No decryption facilities exist on the server side.
4. The cryptographic facilities used, their purpose and the **limits of their protective properties** — including the absence of forward secrecy, the limitations of the trust-on-first-use model and the difference between routes — are described in [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md), which forms an integral part of these Terms.
5. Consequences for the user: the Operator cannot view, restore, disclose on request or delete the content of conversations, and cannot edit an individual message without affecting the account.
6. Consequence for complaints: the Operator cannot confirm or refute the substance of a particular message. Measures apply to the account and to access to the Service; the procedure is set out in [docs/content-complaints.md](docs/content-complaints.md).

### 9. Requests from competent authorities

1. The Operator complies with lawful requests from competent state authorities within the law of the Russian Federation and within the technical capability defined by section 8.
2. The Operator may provide account details, technical connection logs and message metadata in the scope set out in [docs/legal-requests.md](docs/legal-requests.md).
3. The Operator **cannot provide** the content of messages and files, recordings of voice, video or screen shares, or messaging encryption keys, because it does not hold them.
4. The Operator does not install content-collection tools on users' devices and does not create data it does not hold.
5. The procedure for reviewing requests, verifying their lawfulness and recording outcomes is described in [docs/legal-requests.md](docs/legal-requests.md).

### 10. Availability

The Service is provided **"as is"** and **"as available"**. The Operator does not warrant uninterrupted operation, data retention, any particular call quality, or compatibility with specific hardware. Maintenance interruptions, changes or discontinuation of the Service may occur without prior notice.

### 11. Limitation of liability

The Service is provided free of charge. To the maximum extent permitted by law, the Operator is not liable for lost profits, data loss, inability to use the Service, call quality, or any indirect damages arising from use of or inability to use the Service.

Nothing in this section excludes liability that cannot be excluded under the legislation of the Russian Federation.

### 12. Changes to the Terms

The Operator may amend these Terms. The current version is published in the repository's `TERMS.md`. Material changes to section 4 will be announced in the release notes. Continued use of the Service after publication constitutes acceptance.

### 13. Governing law

These Terms are governed by the law of the Russian Federation. Disputes not resolved through negotiation shall be heard at the Operator's place of residence in accordance with the legislation of the Russian Federation.

### 14. Contact

Service enquiries: [GitHub Issues](https://github.com/vnkdevelop/zabor-desktop/issues).
Vulnerability reports: privately, via [Security Advisories](https://github.com/vnkdevelop/zabor-desktop/security/advisories/new).

---

Copyright © 2026 vnkdevelop.
