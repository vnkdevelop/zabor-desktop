## graphify

This project has a graphify knowledge graph at graphify-out/.

Rules:
- ОБЯЗАТЕЛЬНО: Перед анализом файлов исходного кода для решения любых задач, агент должен ПЕРВЫМ ДЕЛОМ обращаться к графу знаний (`graphify-out/graph.json`, `graphify query "<question>"`, `graphify path`, `graphify explain` или `graphify-out/GRAPH_REPORT.md`), чтобы сразу определить где что находится и с чем связано, минимизируя расход токенов.
- Если существует `graphify-out/wiki/index.md` или `graphify-out/GRAPH_REPORT.md`, использовать их для быстрого понимания всей архитектуры приложения.
- ПОСЛЕ ЛЮБЫХ ИЗМЕНЕНИЙ в коде или структуре проекта агент ОБЯЗАН обновлять граф знаний, выполняя `graphify update .` (или `graphify extract . --code-only`), чтобы граф всегда содержал актуальную информацию.