# Участие в разработке ZABOR / Contributing to ZABOR

> Русская версия является основной. The Russian version is authoritative; English follows below.

---

## Русская версия

### Модель разработки

ZABOR — проект с **открытым исходным кодом и закрытой моделью контрибуций** (open source, closed contribution). Код открыт: его можно изучать, проверять, изменять для себя и распространять на условиях [GPL-3.0](LICENSE). Но развитие продукта ведёт единственный мейнтейнер.

Это осознанное решение, а не следствие невнимания к сообществу. Голосовое приложение с P2P-звонками, шумоподавлением и синхронизацией состояния через SignalR имеет плотно связанное ядро: изменение в обработке звука или в жизненном цикле соединения способно тихо испортить качество связи у всех пользователей. Поддерживать такую систему целостной может только тот, кто держит в голове её целиком.

Практически это означает:

- **Идеи и сообщения об ошибках очень нужны.** Они и определяют, что появится в следующих версиях.
- **Реализацию выполняет мейнтейнер.** Запрос функции — это не отказ, а постановка задачи в очередь.
- **Pull request с новой функциональностью по умолчанию не принимается.** Это не оценка качества вашего кода.

### Как принести пользу проекту

Самый ценный вклад — **хороший отчёт об ошибке или продуманный запрос функции**:

1. **Сообщить об ошибке** — [шаблон Bug report](https://github.com/vnkdevelop/zabor-desktop/issues/new/choose). Особенно ценны: версия приложения, версия Windows, модель микрофона и наушников, шаги воспроизведения и то, воспроизводится ли проблема при выключенном шумоподавлении.
2. **Предложить функцию** — [шаблон Feature request](https://github.com/vnkdevelop/zabor-desktop/issues/new/choose). Опишите задачу, которую нужно решить, а не только желаемую реализацию: часто находится более простой путь.
3. **Сообщить об уязвимости** — не создавайте публичный Issue. Порядок описан в разделе «Безопасность» ниже.
4. **Улучшить перевод** — сообщения о неточностях в `ru.json` и `en.json` принимаются и исправляются быстро.
5. **Предложить шутку** – [шаблон Запроса на добавление](https://github.com/vnkdevelop/zabor-desktop/issues/new/choose)предложить собственную шутку которая будет отображаться на главном экране в случайном порядке.


### Соглашение о вкладе (CLA)

Для любого принимаемого pull request требуется согласие с [CLA.md](CLA.md). Вы сохраняете авторские права на свой код и просто предоставляете правообладателю проекта широкую лицензию на его использование. Без этого проект теряет возможность менять условия распространения в будущем, поэтому исключений здесь нет.

Подтверждение оформляется одной строкой в описании PR:

```
Я прочитал CLA.md и согласен с его условиями.
```

### Форк проекта

Форк — ваше право по GPL-3.0, и оно не оспаривается. Два условия:

- переименование и замена брендинга — см. [TRADEMARK.md](TRADEMARK.md);
- собственный серверный бэкенд, подключение сторонних сборок к официальному серверу не допускается — см. [TERMS.md](TERMS.md).

### Безопасность

Об уязвимостях сообщайте **приватно**, не создавая публичный Issue: через [GitHub Security Advisories](https://github.com/vnkdevelop/zabor-desktop/security/advisories/new) репозитория. Пожалуйста, дайте время на выпуск исправления до публичного раскрытия.

---

## English version

### Development model

ZABOR is an **open source, closed contribution** project. The code is open: you may study it, audit it, modify it for your own use and redistribute it under [GPL-3.0](LICENSE). Product direction, however, is handled by a single maintainer.

This is a deliberate choice rather than neglect of the community. A voice application combining P2P calls, noise suppression and SignalR state synchronization has a tightly coupled core: a change in audio processing or in the connection lifecycle can quietly degrade call quality for every user. Keeping such a system coherent requires holding all of it in one head.

In practice:

- **Ideas and bug reports are genuinely needed.** They decide what ships next.
- **Implementation is done by the maintainer.** A feature request is not a rejection — it is a queued task.
- **Pull requests adding functionality are not accepted by default.** This is not a judgement on your code.

### How to help

The most valuable contribution is a **good bug report or a well-considered feature request**:

1. **Report a bug** — [Bug report template](https://github.com/vnkdevelop/zabor-desktop/issues/new/choose). Especially useful: app version, Windows version, microphone and headset model, reproduction steps, and whether the issue persists with noise suppression disabled.
2. **Request a feature** — [Feature request template](https://github.com/vnkdevelop/zabor-desktop/issues/new/choose). Describe the problem you need solved, not only the implementation you have in mind — a simpler path often exists.
3. **Report a vulnerability** — do not open a public Issue. See "Security" below.
4. **Improve translations** — reports of inaccuracies in `ru.json` and `en.json` are fixed quickly.

### Which pull requests are considered

Exceptions to "PRs are not accepted" exist, and they are narrow:

| Change type | Status |
| --- | --- |
| Typo fixes in the UI and documentation | Accepted |
| Translation fixes and refinements | Accepted |
| A localized fix for a clear bug with a maintainer-confirmed Issue | Considered |
| New features, UI changes, refactoring | Not accepted |
| Changes to audio processing, WebRTC, SignalR, noise suppression | Not accepted |
| Changes to licensing, `LICENSE`, `TRADEMARK.md`, `TERMS.md` | Not accepted |
| Adding or updating dependencies | Not accepted |

Always **open an Issue first** and wait for confirmation that the change will be considered. Unsolicited PRs are closed without code review — specifically so your time is not wasted.

### Contributor License Agreement (CLA)

Every accepted pull request requires agreement to [CLA.md](CLA.md). You retain copyright in your code and simply grant the project's copyright holder a broad license to use it. Without this the project loses the ability to change its distribution terms in the future, so there are no exceptions here.

Confirm with a single line in the PR description:

```
I have read CLA.md and I agree to its terms.
```

### Forking

Forking is your right under GPL-3.0 and is not disputed. Two conditions apply:

- rename and replace branding — see [TRADEMARK.md](TRADEMARK.md);
- run your own server backend; connecting third-party builds to the official server is not permitted — see [TERMS.md](TERMS.md).

### Security

Report vulnerabilities **privately**, without opening a public Issue, via the repository's [GitHub Security Advisories](https://github.com/vnkdevelop/zabor-desktop/security/advisories/new). Please allow time for a fix before public disclosure.
