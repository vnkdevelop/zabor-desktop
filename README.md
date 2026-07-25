# ZABOR Desktop (v3.2.2)

Легковесное десктопное приложение для высококачественного голосового общения и демонстрации экрана.

## 🚀 Основные особенности (v3.2.2)
- **Нейросетевое шумоподавление DeepFilterNet 3 WASM**: Интеграция нейросети DeepFilterNet 3 в формате WebAssembly/SIMD для непрерывного отсечения фонового шума без задержек.
- **Нейросетевой детектор речи Silero VAD**: Высокоточный локальный нейросетевой Voice Activity Detector на базе моделей **Silero VAD** (ONNX Runtime Web), исключающий ложные срабатывания микрофона от кликанья клавиатуры, дыхания или фонового шума.
- **Адаптивная калибровка акустики помещения**: Автоматическое определение спектрального уровня шума вашей комнаты (диапазон 65–100 дБ) с запасом по высоте (+25% Headroom) для 100% защиты от просачивания помех.
- **P2P Mesh WebRTC**: Прямая передача звука между участниками с поддержкой кодека Opus и минимальной задержкой.
- **Премиальный Deep Dark / Neon-Magenta UI**: Интерфейс с аппаратным ускорением на React 18, Tailwind CSS и Material Design 3.

## 🛠 Технологический стек
- **Frontend:** React 18, TypeScript, Tailwind CSS, Zustand.
- **Desktop:** Electron (electron-vite).
- **Backend:** C# / .NET 10, SignalR.
- **AI & Audio Pipeline:** WebRTC, WebAudio API, Silero VAD (ONNX WASM), DeepFilterNet 3 WASM.

## 💻 Разработка

### Предварительные условия
Убедитесь, что у вас установлены Node.js (v18+) и npm.

### Установка зависимостей
```bash
npm install
```

### Запуск в режиме разработки
```bash
npm run dev
```

### Сборка приложения (Windows)
```bash
npm run dist:win
```

## 📦 Релизы
Вы можете скачать последнюю версию инсталлятора на странице [Releases](https://github.com/vnkdevelop/zabor-desktop/releases).
