# ZABOR 

[![Build](https://github.com/vnkdevelop/zabor-desktop/actions/workflows/build.yml/badge.svg)](https://github.com/vnkdevelop/zabor-desktop/actions/workflows/build.yml)
[![Version](https://img.shields.io/github/package-json/v/vnkdevelop/zabor-desktop?label=version)](https://github.com/vnkdevelop/zabor-desktop/releases)
[![Electron](https://img.shields.io/badge/Electron-33-47848F?logo=electron&logoColor=white)](https://www.electronjs.org/)
[![React](https://img.shields.io/badge/React-18-61DAFB?logo=react&logoColor=111827)](https://react.dev/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)

ZABOR - приложение для голосового общения, P2P-звонков и трансляции экрана с низкой задержкой.

![Главный экран ZABOR Desktop](docs/images/zabor-overview.png)


## Возможности

- Голосовые каналы на базе P2P Mesh WebRTC.
- Система друзей, профили и статусы в реальном времени.
- Трансляция экрана или камеры.
- Нейросетевое шумоподавление DeepFilterNet 3 и Silero VAD.
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

Backend разрабатывается отдельно; этот репозиторий содержит desktop-клиент.

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

Сообщения об ошибках и предложения можно создавать через подготовленные [GitHub Issue Templates](https://github.com/vnkdevelop/zabor-desktop/issues/new/choose).
