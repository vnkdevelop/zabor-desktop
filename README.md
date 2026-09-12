# ZABOR 

[![Build](https://github.com/vnkdevelop/zabor-desktop/actions/workflows/build.yml/badge.svg)](https://github.com/vnkdevelop/zabor-desktop/actions/workflows/build.yml)
[![Version](https://img.shields.io/github/package-json/v/vnkdevelop/zabor-desktop?label=version)](https://github.com/vnkdevelop/zabor-desktop/releases)
[![License: GPL v3](https://img.shields.io/badge/license-GPLv3-blue.svg)](LICENSE)
[![Electron](https://img.shields.io/badge/Electron-33-47848F?logo=electron&logoColor=white)](https://www.electronjs.org/)
[![React](https://img.shields.io/badge/React-18-61DAFB?logo=react&logoColor=111827)](https://react.dev/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)

ZABOR - приложение для голосового общения, P2P-звонков и трансляции экрана с низкой задержкой.

![Главный экран ZABOR Desktop](docs/images/zabor-overview.png)


## Возможности

- Голосовые каналы на базе P2P Mesh WebRTC.
- Личные сообщения и передача файлов со сквозным шифрованием.
- Система друзей, профили и статусы в реальном времени.
- Трансляция экрана или камеры.
- Нейросетевое шумоподавление DeepFilterNet 3 и RNNoise.
- Скрытые достижения и ачивки.

## Быстрый старт

Требования: [Node.js 20+](https://nodejs.org/) и npm.

```bash
git clone https://github.com/vnkdevelop/zabor-desktop.git
cd zabor-desktop && npm ci
npm run dev
```

Приложение запускается в режиме разработки через `electron-vite`.

## Стек технологий

| Область                 | Технологии                                                  |
| ----------------------- | ----------------------------------------------------------- |
| Интерфейс               | React 18, TypeScript, Tailwind CSS, Framer Motion           |
| Desktop                 | Electron 33, electron-vite, electron-builder                |
| Состояние и локализация | Zustand, i18next                                            |
| Связь                   | WebRTC, ASP.NET Core SignalR                                |
| Медиа                   | Web Audio API, WebAssembly, ONNX Runtime Web, DeepFilterNet |
| Backend                 | C# / .NET 10, SQLite, Entity Framework Core                 |

Backend не входит в этот репозиторий.


## Команды

| Команда            | Назначение                             |
| ------------------ | -------------------------------------- |
| `npm run dev`      | Запуск приложения в режиме разработки  |
| `npm run build`    | Production-сборка приложения           |
| `npm run dist:win` | Создание Windows x64 NSIS-инсталлятора |

## Установка для пользователей

Готовый Windows-инсталлятор доступен на странице [Releases](https://github.com/vnkdevelop/zabor-desktop/releases).

> [!NOTE]
> Поскольку установщик не подписан сертификатом, Windows SmartScreen может показать предупреждение. Для продолжения выберите «Подробнее» и «Выполнить в любом случае».



## Участие в разработке

ZABOR развивается по модели **open source, closed contribution**: код открыт для изучения, проверки и изменения, но развитие продукта ведёт единственный мейнтейнер.

Сообщения об ошибках и предложения функций очень нужны — именно они определяют содержание следующих версий. Создавайте их через [Issue Templates](https://github.com/vnkdevelop/zabor-desktop/issues/new/choose).

Pull request с новой функциональностью по умолчанию не принимаются; узкий список исключений и порядок согласования — в [CONTRIBUTING.md](CONTRIBUTING.md). Для принимаемых pull request требуется согласие с [CLA.md](CLA.md).

## Лицензия и товарные знаки

Copyright (C) 2026 vnkdevelop. Код распространяется по лицензии [GNU General Public License v3.0](LICENSE).

Название **ZABOR**, логотип и иконки приложения лицензией GPL-3.0 **не покрываются**: GPL-3.0 не передаёт прав на средства индивидуализации, что прямо допускается её разделом 7(e). Форк проекта возможен и не оспаривается, но обязан быть переименован — требования и чек-лист в [TRADEMARK.md](TRADEMARK.md).

## Документы

Условия использования сервиса и связанные документы. Публикуются на двух языках; русская версия является основной.

| Документ | Предмет |
| --- | --- |
| [TERMS.md](TERMS.md) | Условия использования сервиса, требования к клиентским сборкам |
| [PRIVACY.md](PRIVACY.md) | Обработка персональных данных: состав, цели, сроки, права субъекта |
| [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md) | Правила допустимого использования и требования к содержимому |
| [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md) | Применяемая криптография и границы её защитных свойств |
| [SECURITY.md](SECURITY.md) | Порядок сообщения об уязвимостях |
| [TRADEMARK.md](TRADEMARK.md) | Политика использования знаков и требования к форкам |
| [CLA.md](CLA.md) | Соглашение о вкладе |
| [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) | Лицензии сторонних компонентов и внешних служб |
| [docs/chat-data-flow.md](docs/chat-data-flow.md) | Какие данные чата где хранятся и кому доступны |
| [docs/legal-requests.md](docs/legal-requests.md) | Порядок рассмотрения запросов уполномоченных органов |
| [docs/content-complaints.md](docs/content-complaints.md) | Порядок направления жалоб на содержимое |
| [docs/compliance-checklist.md](docs/compliance-checklist.md) | Состояние соответствия требованиям и открытые вопросы |

Приложение использует сквозное шифрование переписки. Ключи формируются и хранятся только на устройствах участников, поэтому содержимое сообщений и файлов недоступно Оператору — подробности и ограничения защитных свойств в [CRYPTO-NOTICE.md](CRYPTO-NOTICE.md).

## Шутка дня

В приложении есть раздел «шутейка». Предложить свою шутку можно через форму [«Предложить шутку»](https://github.com/vnkdevelop/zabor-desktop/issues/new/choose). Перед отправкой ознакомьтесь с требованиями к содержимому в [ACCEPTABLE-USE.md](ACCEPTABLE-USE.md).
