# Криптография в ZABOR: состав и свойства / ZABOR Cryptography Notice

> Редакция от 11 сентября 2026 года.
> Русская версия является основной и имеет преимущественную силу при расхождении с переводом.
> The Russian version is authoritative; the English translation follows below.

Документ описывает применяемые криптографические средства, их назначение и **границы защитных свойств**. Он предназначен для пользователей, исследователей безопасности и при подготовке ответов на запросы уполномоченных органов.

---

## Русская версия

### 1. Назначение

Криптография в ZABOR применяется для защиты **обмена сообщениями, файлами и служебными пакетами** между участниками, а также для защиты **локально хранимых данных** на устройстве пользователя. Защита голосового и видеотракта обеспечивается штатными средствами WebRTC (DTLS-SRTP) и в настоящем документе не рассматривается.

### 2. Применяемые алгоритмы

| Назначение | Алгоритм | Реализация | Где применяется |
| --- | --- | --- | --- |
| Защита прямого соединения | DTLS-SRTP | WebRTC | Прямой маршрут (P2P) |
| Согласование ключа обмена сообщениями | ECDH, кривая P-256 | Web Crypto API | Только резервный маршрут |
| Шифрование сообщений и служебных пакетов | AES-GCM, ключ 256 бит, 96-битный вектор инициализации | Web Crypto API | Только резервный маршрут |
| Генерация вектора инициализации | `crypto.getRandomValues` | Web Crypto API | Резервный маршрут |
| Проверка целостности передаваемых файлов | SHA-256 | `node:crypto` | Оба маршрута |
| Защита данных сессии и ключа переписки | Шифрование средствами операционной системы: DPAPI (Windows), Keychain (macOS) | Electron `safeStorage` | Локально на устройстве |

### 3. Как защищается содержимое: два маршрута

Защита содержимого устроена по-разному в зависимости от маршрута, и это различие принципиально.

**Прямой маршрут (P2P).** Соединение устанавливается напрямую между участниками по WebRTC. Содержимое защищено **транспортным шифрованием WebRTC (DTLS)**. Прикладной слой шифрования на этом маршруте **не применяется**: приложение передаёт данные по каналу данных WebRTC, полагаясь на защиту самого транспорта. Сервер при прямом соединении содержимого не видит.

**Резервный маршрут через сервер.** Когда прямое соединение не установлено, пакеты передаются через сервер приложения. На этом маршруте применяется прикладное шифрование:

1. На устройстве каждого пользователя формируется долговременная пара ключей ECDH P-256. Открытый ключ передаётся собеседнику; закрытый ключ не покидает устройство.
2. Из общего секрета ECDH выводится симметричный ключ AES-GCM-256, которым шифруются сообщения и служебные пакеты.
3. Доверие к открытому ключу собеседника устанавливается по модели **trust on first use**: отпечаток ключа сохраняется при первом контакте, дальнейшее изменение отпечатка считается признаком подмены и пакет не принимается.

**Следствие, которое Оператор указывает прямо:** согласование ключей ECDH и проверка отпечатка относятся **только к резервному маршруту**. На прямом маршруте прикладного согласования ключей не происходит, поэтому данная проверка его не защищает; там действует защита транспорта WebRTC. Документ [docs/chat-data-flow.md](docs/chat-data-flow.md) фиксирует это соответствие, а [docs/compliance-checklist.md](docs/compliance-checklist.md) — как открытый вопрос.

### 4. Границы защитных свойств

Оператор прямо указывает ограничения, чтобы заявления о защите не толковались шире, чем они есть:

1. **Отсутствует прямая секретность (forward secrecy).** Долговременный ключ не меняется от сессии к сессии; компрометация закрытого ключа, в том числе при полном доступе к устройству, позволяет расшифровать ранее перехваченные сообщения.
2. **Ключ выводится непосредственно из общего секрета ECDH** без отдельной функции растяжения ключа.
3. **Используется модель trust on first use.** Проверка отпечатка ключа происходит автоматически, средства сверки отпечатка между участниками в интерфейсе не предусмотрены, поэтому защита от активной подмены ключа при первом контакте ограничена.
4. **Защита локальных данных зависит от операционной системы.** Ключ переписки и данные сессии шифруются средствами ОС; при отсутствии такой возможности в системе защита не применяется.
5. **Метаданные не защищаются.** Оператору доступны сведения о том, кто с кем, когда и в каком объёме обменивается сообщениями (см. раздел 3.3 [PRIVACY.md](PRIVACY.md)).
6. **Резервный маршрут использует сервер.** При невозможности прямого соединения шифрованные пакеты передаются через сервер, который видит метаданные, но не содержимое.

### 5. Ключи и их местонахождение

Закрытые ключи формируются и хранятся **исключительно на устройствах пользователей**. Оператор не формирует, не получает, не хранит и не имеет технической возможности получить ключи шифрования переписки. Средства расшифрования содержимого сообщений в серверной части не реализованы.

### 6. Защита от подделки сборок

Приложение проверяет подлинность сборки, передавая серверу подпись сборки. Механизм описан в [docs/client-attestation.md](docs/client-attestation.md). Он предназначен для ограничения доступа к серверам Оператора и **не является** средством защиты содержимого переписки.

### 7. Публично доступный исходный код и экспортный контроль

1. Исходный код приложения распространяется открыто по лицензии [GPL-3.0](LICENSE).
2. Средства криптографической защиты, описанные в настоящем документе, **не разработаны Оператором**: используется интерфейс Web Crypto API, входящий в состав среды исполнения, и штатные средства WebRTC.
3. Оператор не поставляет отдельного криптографического программного обеспечения, не предоставляет услуг шифрования связи и не осуществляет деятельность по техническому обслуживанию шифровальных средств.
4. **Оператор является физическим лицом и не является лицензиатом ФСБ России** по видам деятельности, связанным с шифровальными (криптографическими) средствами. Использование криптографической защиты в открыто распространяемом приложении оценивается отдельно; см. [docs/compliance-checklist.md](docs/compliance-checklist.md).

### 8. Сообщения об уязвимостях

Порядок сообщения о недостатках криптографической защиты — в [SECURITY.md](SECURITY.md).

---

## English version

### 1. Purpose

Cryptography in ZABOR protects the **exchange of messages, files and control packets** between participants, and protects **data stored locally** on the user's device. Protection of the voice and video path is provided by native WebRTC facilities (DTLS-SRTP) and is not covered by this document.

### 2. Algorithms used

| Purpose | Algorithm | Implementation | Where applied |
| --- | --- | --- | --- |
| Direct connection protection | DTLS-SRTP | WebRTC | Direct route (P2P) |
| Messaging key agreement | ECDH, curve P-256 | Web Crypto API | Fallback route only |
| Message and control packet encryption | AES-GCM, 256-bit key, 96-bit IV | Web Crypto API | Fallback route only |
| IV generation | `crypto.getRandomValues` | Web Crypto API | Fallback route |
| Integrity of transferred files | SHA-256 | `node:crypto` | Both routes |
| Session data and messaging key protection | Operating system encryption: DPAPI (Windows), Keychain (macOS) | Electron `safeStorage` | Locally on device |

### 3. How content is protected: two routes

Protection differs by route, and the difference is fundamental.

**Direct route (P2P).** The connection is established directly between participants over WebRTC. Content is protected by **WebRTC transport encryption (DTLS)**. No application-layer encryption is applied on this route: the application sends data over the WebRTC data channel and relies on the transport's own protection. On a direct connection the server does not see the content.

**Fallback route through the server.** Where a direct connection is not established, packets are transmitted through the application server. Application-layer encryption applies on this route:

1. Each user's device generates a long-term ECDH P-256 key pair. The public key is shared with the other participant; the private key never leaves the device.
2. A symmetric AES-GCM-256 key is derived from the ECDH shared secret and used to encrypt messages and control packets.
3. Trust in the other party's public key follows a **trust on first use** model: the key fingerprint is recorded on first contact, and any later change of fingerprint is treated as a sign of substitution and the packet is not accepted.

**A consequence the Controller states expressly:** ECDH key agreement and fingerprint checking apply to the **fallback route only**. On the direct route there is no application-layer key agreement, so this check does not protect it; WebRTC transport protection applies there instead. [docs/chat-data-flow.md](docs/chat-data-flow.md) records this correspondence, and [docs/compliance-checklist.md](docs/compliance-checklist.md) lists it as an open issue.

### 4. Limits of the protective properties

The Controller states these limitations expressly so that protection claims are not read more broadly than they are:

1. **There is no forward secrecy.** The long-term key does not change from session to session; compromise of the private key, including full device access, allows decryption of previously intercepted messages.
2. **The key is derived directly from the ECDH shared secret** without a separate key-derivation function.
3. **A trust on first use model is used.** Fingerprint checking is automatic; the interface provides no means for participants to compare fingerprints, so protection against active key substitution on first contact is limited.
4. **Local data protection depends on the operating system.** The messaging key and session data are encrypted with OS facilities; where the system does not provide them, protection is not applied.
5. **Metadata is not protected.** The Controller can see who exchanges messages with whom, when and in what volume (see section 3.3 of [PRIVACY.md](PRIVACY.md)).
6. **The fallback route uses the server.** Where a direct connection cannot be established, encrypted packets pass through the server, which sees metadata but not content.

### 5. Keys and their location

Private keys are generated and stored **exclusively on users' devices**. The Controller does not generate, receive, store or have any technical means of obtaining messaging encryption keys. No decryption facilities for message content exist on the server side.

### 6. Build authenticity protection

The application verifies build authenticity by transmitting a build signature to the server. The mechanism is described in [docs/client-attestation.md](docs/client-attestation.md). It exists to restrict access to the Controller's servers and is **not** a means of protecting message content.

### 7. Publicly available source code and export control

1. The application source code is distributed openly under the [GPL-3.0](LICENSE) license.
2. The cryptographic facilities described here are **not developed by the Controller**: the Web Crypto API provided by the runtime and native WebRTC facilities are used.
3. The Controller does not supply separate cryptographic software, does not provide communication encryption services, and does not carry out activities involving the maintenance of encryption facilities.
4. **The Controller is an individual and is not an FSB-licensed entity** for activities related to encryption (cryptographic) facilities. The use of cryptographic protection in an openly distributed application is assessed separately; see [docs/compliance-checklist.md](docs/compliance-checklist.md).

### 8. Vulnerability reports

For reporting weaknesses in cryptographic protection, see [SECURITY.md](SECURITY.md).

---

Copyright © 2026 vnkdevelop.
