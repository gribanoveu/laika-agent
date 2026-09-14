# 4. Карта переноса из Alfa Atlas

Постатейно: что берём, откуда, в каком виде. Пути даны относительно корня Alfa Atlas.
Объёмы — фактические, включая инлайновые тесты (`#[cfg(test)] mod tests`), которых в
Rust-части проекта около 150 блоков и которые переносятся вместе с кодом.

Вердикты:

- **как есть** — копируется, правится только `use`-пути и, где надо, язык строк;
- **с правкой** — логика годится, но нужна доработка под кодинг-агента;
- **как образец** — код не берём, берём решение/структуру;
- **не брать** — специфика документационного продукта.

---

## 4.1 Агентный цикл — ядро переноса

| Что | Откуда | Объём | Вердикт |
|-----|--------|-------|---------|
| Многораундовый tool-calling loop | [services/llm_chat.rs](../src-tauri/src/services/llm_chat.rs) | 2807 | как есть |
| Типы протокола turn'а, трейт провайдера, pending/resume | [domain/llm.rs](../src-tauri/src/domain/llm.rs) | 1090 | как есть |

Что внутри и почему это дорого писать заново:

1. **Взвешенный бюджет цикла.** `MAX_TOOL_BUDGET` + `ToolName::loop_weight`: раунд стоит
   не «1», а сумму весов вызванных инструментов. Дешёвое чтение не съедает лимит,
   дорогой поиск обрывается на ~10 вызовах вместо 20. Плюс `MAX_TOOL_ITERATIONS` как
   backstop на случай нулевого/кривого веса.
2. **Дедупликация повторных результатов** (`dedupe_repeat_result`). Ключ — имя
   инструмента + сырые аргументы, гейт — хэш payload'а. Второе чтение того же файла
   отдаёт заглушку, отредактированный файл приезжает целиком. Это не корректность, это
   контекст: в трассе, из-за которой фича появилась, один файл читался трижды и каждый
   раз целиком пересылался во всех последующих раундах.
3. **Пауза на подтверждении с возобновлением.** `PendingApproval` / `PendingToolCall` /
   `ToolCallDecision` / `ResumePoint`. Состав `ResumePoint` разобран в
   [03-architecture.md](03-architecture.md#34-состав-resumepoint) — каждое поле там
   закрывает конкретную дыру.
4. **Ретраи по правильному признаку.** `is_peer_disconnected_error`: повтор только если
   провайдер не отдал ни байта, иначе можно продублировать side-effect уже выполненного
   инструмента. Пять повторов сверх исходного запроса.
5. **Обрыв по `max_tokens`.** `TRUNCATED_ROUND_NOTE` — модель не знает про лимит ответа,
   и обрезанный JSON приходит к ней как «невалидные аргументы», на что она отвечает тем
   же самым огромным вызовом. Явная подсказка «тебя обрезали, зови компактнее» ломает
   этот цикл. Приписывается только к **упавшему** вызову: успевший закрыться вызов
   выполнился нормально, и говорить ему об обрыве вредно.
6. **Санитизация аргументов** — `sanitize_tool_call_arguments` в `domain/llm.rs`:
   починка того, что приезжает из стрима кусками.
7. **Steering** — `SteeringNote`/`SteeringSource`, очередь заметок в идущий turn.

**Что выбросить при переносе:** подсказки, специфичные для документации, — `needs_diagram_nudge`
и `DIAGRAM_NOTE` (бэкстоп «попросили схему, а диаграммы не нарисовал»),
`history_has_a_visualize_call`, `last_user_message_asks_for_a_diagram`. Механизм
«бэкстоп-подсказка по эвристике над последним сообщением пользователя» при этом полезен —
его стоит сохранить как точку расширения, просто с другими эвристиками (например, «правил
код и ни разу не запустил тесты»).

**Что перевести:** все строки-подсказки модели в цикле на русском. Это сознательное
решение Atlas (чтобы модель не переключала язык посреди ответа) — в новом продукте
решение может быть другим, но принцип «служебные вставки на языке диалога» стоит
сохранить.

---

## 4.2 Граница инструментов

| Что | Откуда | Объём | Вердикт |
|-----|--------|-------|---------|
| Диспетчер `execute_tool` + `llm_tool_definitions` | [services/ai_tools/tools/mod.rs](../src-tauri/src/services/ai_tools/tools/mod.rs) | 617 | как есть |
| Типы вызова/результата, `ToolScope`, `ToolError`, `Task` | [domain/ai_tools.rs](../src-tauri/src/domain/ai_tools.rs) | 1679 | с правкой |
| Парсинг вызова и preflight | [services/ai_tools/parse.rs](../src-tauri/src/services/ai_tools/parse.rs) | 1429 | как есть |
| Резолв путей — точка enforce'а корня доступа | [services/ai_tools/resolve.rs](../src-tauri/src/services/ai_tools/resolve.rs) | 488 | как есть |
| Сборка scope из конфига проекта, авто-одобрение | [services/ai_tools/scope.rs](../src-tauri/src/services/ai_tools/scope.rs) | 916 | с правкой |
| Классификация инструментов и права | [domain/ai_access.rs](../src-tauri/src/domain/ai_access.rs) | 646 | с правкой |
| Тестовые хелперы для инструментов | [services/ai_tools/testing.rs](../src-tauri/src/services/ai_tools/testing.rs) | — | как есть |

Из `domain/ai_access.rs` переносятся механизмы, а не список: `AiAccessMode` (корень
доступа), `ToolName::requires_confirmation`, `auto_approvable`, `loop_weight`,
`from_wire_name`, `call_requires_confirmation` (подтверждение, зависящее от **аргументов**,
а не только от имени инструмента — ровно то, что понадобится для команд оболочки),
`default_allowed_tools`, `no_project_tools` (набор инструментов, когда проект не открыт).

Отдельно ценно правило: **явно настроенный пользователем allowlist не расширяется
молча**. Добавили в новой версии инструмент — он не появляется у того, кто список уже
кастомизировал. В Atlas это `ai_allowed_tools: Option<Vec<ToolName>>`, где `None` — «не
настраивал, бери дефолт режима».

### Конкретные инструменты

| Инструмент | Откуда | Объём | Вердикт |
|-----------|--------|-------|---------|
| `read_file` (диапазоны, клампинг, `outline`) | [tools/read_file.rs](../src-tauri/src/services/ai_tools/tools/read_file.rs) | 450 | как есть |
| `grep` (контекстные строки, лимит по хитам) | [tools/grep.rs](../src-tauri/src/services/ai_tools/tools/grep.rs) | 363 | как есть |
| `list_files` (глубина, glob, gitignore) | [tools/list_files.rs](../src-tauri/src/services/ai_tools/tools/list_files.rs) | 709 | с правкой |
| `write_file` | [tools/write_file.rs](../src-tauri/src/services/ai_tools/tools/write_file.rs) | 197 | как есть |
| `edit_file` | [tools/edit_file.rs](../src-tauri/src/services/ai_tools/tools/edit_file.rs) | 728 | с правкой |
| `delete_file`, `delete_directory`, `create_directory`, `move` | tools/ | 845 | с правкой |
| `todo` (write/update) | [tools/todo.rs](../src-tauri/src/services/ai_tools/tools/todo.rs) | 320 | как есть |
| `git` (diff/blame) | [tools/git.rs](../src-tauri/src/services/ai_tools/tools/git.rs) | 461 | как есть |
| `skill` (search/load/read) | [tools/skill.rs](../src-tauri/src/services/ai_tools/tools/skill.rs) | 135 | как есть |
| `semantic_search` (каскад тиров) | [tools/semantic_search.rs](../src-tauri/src/services/ai_tools/tools/semantic_search.rs) | 394 | с правкой |
| `plans` | [tools/plans.rs](../src-tauri/src/services/ai_tools/tools/plans.rs) | 220 | с правкой |
| `conversation` (смена режима) | [tools/conversation.rs](../src-tauri/src/services/ai_tools/tools/conversation.rs) | 140 | как образец |
| `check`, `asciidoc_templates`, `artifact`, `visualize` | tools/ | 1274 | не брать |

Правки по пунктам:

- `list_files` — убрать ветку «только документационные форматы»; для кодинг-агента
  всегда нужен вариант `scan_all`.
- `edit_file` — в Atlas правка идёт списком `{old, new}`. Для кода этого мало: нужна
  проверка уникальности якоря и требование, чтобы файл был прочитан агентом в этом
  turn'е (CA-3.6, CA-3.10).
- `move` — в Atlas он ещё и переписывает ссылки в документации
  (`services/reference_rewrite.rs`). Для кода эту часть выкинуть.
- `semantic_search` — оставить каскад и метаданные (`tiersUsed`, `weak`, `hint`), но
  подсказки на русском и RU→EN словарь корней (`уведомлен` → `Notification`) — это
  прикладная специфика, заменить или удалить.

### Диффы

[services/text_diff.rs](../src-tauri/src/services/text_diff.rs) (113 строк) — `diff_stats`
превращает старое/новое содержимое в `FileDiffStats { lines_added, lines_removed,
unified_diff, truncated }`. Переносится как есть. Важна не функция, а решение: **дифф
возвращается и в UI, и обратно модели** в результате инструмента. Иначе модель видит
только `{"path": "..."}` и не знает, что реально легло на диск.

---

## 4.3 Контекст и промпты

| Что | Откуда | Объём | Вердикт |
|-----|--------|-------|---------|
| Компакция истории | [src/lib/contextCompaction.ts](../src/lib/contextCompaction.ts) | 216 | с правкой |
| Сборка системного промпта и контекстных блоков | [src/lib/assistantConfig.ts](../src/lib/assistantConfig.ts) | 1787 | как образец |
| Режимы разговора и наборы инструментов | [domain/conversation_mode.rs](../src-tauri/src/domain/conversation_mode.rs) | 243 | как есть |

**Компакция** даёт готовый набор: `estimateWireChatTokens` (оценка до отправки),
`shouldCompact` (порог от лимита контекста), `planCompaction` (что сворачиваем),
`describeMessageForCompaction`, заметка в транскрипте вместо молчаливой потери истории,
кэш плана с проверкой валидности и `isContextLengthError` — распознавание ошибки
провайдера, чтобы сжать и повторить вместо падения.

**Важное замечание по слою.** В Atlas компакция и сборка промпта живут во фронтенде
(TypeScript), потому что там же живёт состояние чата. Для нового проекта это **надо
перенести в ядро**: CLI и headless-режим обязаны компактить так же, как UI, а
дублировать 1800 строк промпт-сборки на двух языках — гарантированный источник
расхождений. Логику берём, слой меняем.

Контекстные блоки, которые стоит воспроизвести (`assistantConfig.ts`): активный файл,
todo-список, активный план, загруженные skills (с лимитом ~48k символов), ответы
пользователя на вопросы агента, память, уведомления о смене режима и прав.
`estimateToolSchemaTokens` — учёт того, сколько стоят сами схемы инструментов, часто
забываемая статья расхода.

---

## 4.4 Сессии и персистентность

| Что | Откуда | Объём | Вердикт |
|-----|--------|-------|---------|
| Хранилище чатов (SQLite) | [infra/chat_store.rs](../src-tauri/src/infra/chat_store.rs) | 843 | как есть |
| Контракт формата | [src/lib/chatPersistence.contract.json](../src/lib/chatPersistence.contract.json) | — | как образец |
| Редьюсер событий turn'а, `seq`-протокол | [src/lib/chatTurnReducer.ts](../src/lib/chatTurnReducer.ts) | 294 | как есть |
| Сборка блоков транскрипта | [src/lib/chatBlocks.ts](../src/lib/chatBlocks.ts) | 1342 | с правкой |
| Экспорт транскрипта | [src/lib/chatExport.ts](../src/lib/chatExport.ts) | — | как есть |

`chat_store` уже умеет: список по репозиторию, загрузку, сохранение, архивацию, привязку
к корню репозитория, и служебное поле «до какого сообщения уже извлекали память». Всё
это ровно то, что нужно для `--continue` / `--resume`.

Наличие **контракта формата отдельным файлом** с тестом — практика, которую стоит
унести целиком: формат сессии переживёт много версий продукта.

---

## 4.5 Провайдеры и ключи

| Что | Откуда | Объём | Вердикт |
|-----|--------|-------|---------|
| Трейт `LlmProvider`, конфиги, пресеты, лимиты токенов | [domain/llm.rs](../src-tauri/src/domain/llm.rs) | 1090 | как есть |
| OpenAI-совместимый клиент (стрим и не-стрим) | [infra/llm_providers/openai_compatible.rs](../src-tauri/src/infra/llm_providers/openai_compatible.rs) | 1316 | как есть |
| Манифест пресетов | [infra/llm_provider_manifest.rs](../src-tauri/src/infra/llm_provider_manifest.rs) | — | с правкой |
| Отладочный лог запросов/ответов | [infra/llm_debug_log.rs](../src-tauri/src/infra/llm_debug_log.rs) | 212 | как есть |
| Rate limiting | [domain/llm_rate_limit.rs](../src-tauri/src/domain/llm_rate_limit.rs) | 885 | как есть |
| Разрешение провайдера/сессии | [services/llm_session.rs](../src-tauri/src/services/llm_session.rs) | — | как есть |
| Keychain + шифрованный fallback | [infra/master_key.rs](../src-tauri/src/infra/master_key.rs), [infra/secret_store.rs](../src-tauri/src/infra/secret_store.rs), [infra/key_management.rs](../src-tauri/src/infra/key_management.rs) | 1814 | как есть |

Детали, которые дороже всего воспроизводить:

- `build_agent(trusted_cert_pem)` — поддержка корпоративного шлюза с собственным CA.
  Мелочь, без которой в энтерпрайзе продукт просто не работает.
- Реакция на HTTP-ошибки, вынесенная в один хелпер вместо копипасты по вызовам.
- В `keyring 3.x` платформенные бэкенды — **opt-in по фичам**; без них крейт собирается
  с mock-хранилищем в памяти, которое молча теряет секрет при выходе из процесса. В
  Atlas фичи включены per-target, а `master_key` определяет mock в рантайме и честно
  уходит в файловый fallback. Это ловушка, на которую наступают ровно один раз.
- `zeroize` + `secrecy`: `SecretString` не выводится через `Debug`/`Serialize` и
  затирает буфер при Drop — ключ нельзя случайно уронить в лог или крэш-дамп.
- `rate_limit`: трейт `RateLimitPolicy` + `EvcSlidingWindow` + `NoopPolicy` + пресеты на
  провайдера. Переносится целиком, пресеты заменить.

**Чего нет:** нативного Anthropic Messages API. Трейт под второго провайдера готов,
реализации нет — см. [05-gaps.md](05-gaps.md).

---

## 4.6 Индекс и поиск

| Что | Откуда | Объём | Вердикт |
|-----|--------|-------|---------|
| Типы индекса, `LanguageIndexer`, `Symbol`, `FileId` | [domain/repo_index.rs](../src-tauri/src/domain/repo_index.rs) | 201 | как есть |
| Индекс репозитория | [services/repo_index.rs](../src-tauri/src/services/repo_index.rs) | 1121 | как есть |
| Индексеры языков | [infra/language_indexers/](../src-tauri/src/infra/language_indexers/) | 559 | с правкой |
| Чанкование: спаны, щели, splitting, breadcrumb | [domain/chunk_index.rs](../src-tauri/src/domain/chunk_index.rs) | 718 | как есть |
| Стратегии чанкования | [infra/chunk_strategies/](../src-tauri/src/infra/chunk_strategies/) | 208 | как есть |
| Сборка чанков и чтение текста по требованию | [services/chunk_builder.rs](../src-tauri/src/services/chunk_builder.rs), [services/chunk_text.rs](../src-tauri/src/services/chunk_text.rs) | 879 | как есть |
| Хранилище индекса (SQLite + FTS5 + BM25) | [infra/index_store.rs](../src-tauri/src/infra/index_store.rs) | 1384 | как есть |
| Идентичность репозитория | [infra/repository_identity.rs](../src-tauri/src/infra/repository_identity.rs) | 205 | как есть |
| Разбор поискового запроса, FTS5-выражение | [domain/search_query.rs](../src-tauri/src/domain/search_query.rs) | 641 | с правкой |
| Инкрементальная синхронизация | [services/embedding_sync.rs](../src-tauri/src/services/embedding_sync.rs) | 1730 | с правкой |
| Векторное хранилище (usearch) | [infra/vector_store.rs](../src-tauri/src/infra/vector_store.rs) | 271 | как есть |
| Эмбеддинги: провайдеры, индекс | [services/embedding_index.rs](../src-tauri/src/services/embedding_index.rs), infra/embedding_providers/ | 436+ | как есть |
| Обход файлов с учётом gitignore | [infra/workspace_scanner.rs](../src-tauri/src/infra/workspace_scanner.rs) | 490 | как есть |

Решения, ради которых это и берётся:

1. **Чанк не хранит текст** — только смещения и хэш; текст читается с диска по
   требованию, с определением устаревания. Репо на тысячи файлов не дублируется в памяти.
2. **Кэш вне репозитория, ключ — идентичность репо** (SHA-256 канонического remote URL,
   либо UUID в `.atlas/project.json` для репо без remote). Тот же репозиторий,
   склонированный второй раз или перемещённый, переиспользует индекс, а не строит заново.
3. **Два независимых номера версии.** `CHUNK_VERSION`/`INDEX_VERSION` — дорогой слой
   (пересборка стоит полного переэмбеддинга); `DERIVED_VERSION` — всё, что
   пересчитывается из исходников (FTS-строки, квалифицированные имена). Починка второго
   не сносит первый. Без этого разделения любая правка в токенизации стоит полного
   перестроения векторов.
4. **Инкрементальность по хэшу содержимого** плюс файловый watcher, поддерживающий
   индекс свежим между ручными синками, плюс приоритизация первой синхронизации по
   открытым файлам.
5. **Каскад поиска с пометкой источника**: символ (точный → стем → путь) → вектора →
   BM25. Скоры между тирами несравнимы, поэтому каждый хит несёт `source`.

Правки:

- **Главная — уже сделана в апстриме.** `detect_language` возвращал `None` для всего,
  кроме пяти форматов, и такой файл отсеивался на входе в обход
  (`RepositoryIndex::build_internal`) — то есть был невидим и для BM25, а не только для
  векторов. Теперь есть `Language::PlainText`: лексический тир покрывает все текстовые
  файлы, эмбеддинги остаются на структурных языках (`Language::is_lexical_only`).
  Переносится вместе с индексным слоем как есть; полный разбор, включая грабли, на
  которые нельзя наступить повторно, — в [05-gaps.md](05-gaps.md) §5.9.
- Реестр языков узок осознанно (Java, JSON, YAML, Markdown, AsciiDoc — из них
  tree-sitter только Java и AsciiDoc). Расширение дешёвое: вариант enum + индексер +
  стратегия + две строки регистрации, причём тесты в обоих `mod.rs` падают, если язык
  добавили и забыли зарегистрировать.
- `tree-sitter` тянет `links = "tree-sitter"` — **одна версия на весь граф сборки**.
  Kotlin из Atlas выкинут именно поэтому (`tree-sitter-kotlin` запинен на `<0.23`,
  `tree-sitter-asciidoc` требует 0.26). В новом проекте не тащить экзотические грамматики
  и пиниться на версию официального набора.
- Рукописный обход дерева (`walk` с `match node.kind()`) не масштабируется: ~200 строк
  на язык. Заменить на query-driven извлечение через `tags.scm` — см.
  [05-gaps.md](05-gaps.md).
- **Шрам, который надо унести с собой:** `JavaIndexer` сознательно не рекурсирует внутрь
  тел методов. Анонимный класс в теле даёт вложенные символы с диапазоном *внутри*
  родительского, а `spans_from_backward_gap_symbols` предполагает последовательные
  непересекающиеся якоря — и весь воркер синхронизации падал на `content[start..end]`
  при `start > end`. В новом проекте инвариант «якоря не пересекаются» стоит либо
  ассертить в хелпере, либо чинить сортировкой, а не держать дисциплиной вызывающего.

---

## 4.7 Skills, память, планы

| Что | Откуда | Объём | Вердикт |
|-----|--------|-------|---------|
| Формат `SKILL.md`, парсинг, поиск, слияние каталогов | [domain/agent_skills.rs](../src-tauri/src/domain/agent_skills.rs) | 505 | как есть |
| Каталог скиллов: встроенные + пользовательские, вкл/выкл | [services/agent_skills.rs](../src-tauri/src/services/agent_skills.rs) | 635 | как есть |
| Память: области видимости, wake-context, заметки | [services/agent_memory.rs](../src-tauri/src/services/agent_memory.rs) | 537 | как есть |
| Политика памяти: дедуп по близости, решения о записи | [domain/memory_policy.rs](../src-tauri/src/domain/memory_policy.rs) | 742 | как есть |
| Пайплайн извлечения фактов из turn'а | [services/memory_pipeline.rs](../src-tauri/src/services/memory_pipeline.rs) | 440 | как есть |
| Планы | [services/plans.rs](../src-tauri/src/services/plans.rs), [domain/plan.rs](../src-tauri/src/domain/plan.rs) | 400+ | с правкой |

Скиллы в Atlas — это ровно тот механизм, что нужен: каталог `SKILL.md` с фронтматтером,
валидация имени, поиск по каталогу, **ленивая загрузка тела в контекст по требованию
модели** (инструмент `skill`), переключатели включения/выключения на уровне настроек,
слияние встроенного и пользовательского каталогов. Переносится почти без правок.

Память стоит переносить целиком, но включать по умолчанию выключенной: в кодинг-агенте
автоматическое запоминание фактов из сессии — решение, которое пользователь должен
принять осознанно. `MemoryExtractGuard` (не запускать извлечение дважды на один чат) и
дедупликация новых фактов по близости к существующим — обе вещи неочевидные и уже
сделанные.

---

## 4.8 Наблюдаемость

| Что | Откуда | Объём | Вердикт |
|-----|--------|-------|---------|
| Лог вызовов инструментов с редактированием | [infra/tool_call_log.rs](../src-tauri/src/infra/tool_call_log.rs) | 613 | как есть |
| Типы фильтрации/страницы лога | [domain/tool_call_log.rs](../src-tauri/src/domain/tool_call_log.rs) | 58 | как есть |
| UI лога | [src/components/ToolLog/](../src/components/ToolLog/) | — | как есть |
| Телеметрия (очередь, батчи, opt-in) | [infra/metrics_queue.rs](../src-tauri/src/infra/metrics_queue.rs), [infra/metrics_store.rs](../src-tauri/src/infra/metrics_store.rs) | — | как образец |

`redact_args` / `redact_result` — вырезание содержимого файлов из записей лога (включая
контекстные строки grep'а, которые ровно такое же содержимое). Это то, что позволяет
держать лог включённым по умолчанию.

---

## 4.9 UI десктопа

Актуально только если десктоп делается на Tauri + React. Если стек другой — это раздел
«как образец» целиком.

| Что | Откуда | Объём | Вердикт |
|-----|--------|-------|---------|
| Редьюсер и `seq`-протокол событий turn'а | [src/lib/chatTurnReducer.ts](../src/lib/chatTurnReducer.ts) | 294 | как есть |
| Блок вызова инструмента (аргументы + результат) | [AssistantToolCallBlock.tsx](../src/components/RightDock/AssistantToolCallBlock.tsx) | 671 | с правкой |
| Группа подтверждений | [AssistantToolApprovalGroup.tsx](../src/components/RightDock/AssistantToolApprovalGroup.tsx) | 172 | как есть |
| Предпросмотр диффа до записи (Monaco DiffEditor) | [WriteFileDiffReview.tsx](../src/components/RightDock/WriteFileDiffReview.tsx), [EditFileDiffReview.tsx](../src/components/RightDock/EditFileDiffReview.tsx) | 191 | как есть |
| Карточки удаления файла/каталога | DeleteFileReview.tsx, DeleteDirectoryReview.tsx | — | как есть |
| Транскрипт и прокрутка | [AssistantConversation.tsx](../src/components/RightDock/AssistantConversation.tsx), useChatScrollFollow | 618+ | с правкой |
| Заметка о компакции, reasoning-блок, steering-блок | Assistant*.tsx | — | как есть |
| Виджеты todo/плана | TodoProgressWidget.tsx, PlanProgressWidget.tsx | — | как есть |
| Оркестрация чата | [src/hooks/useLlmChat.ts](../src/hooks/useLlmChat.ts) | 1619 | с правкой |
| Описания вызовов для человека | `describeToolActivity`/`describeToolResult` в assistantConfig.ts | — | с правкой |
| Карточки артефактов, диаграмм, тикетов | AssistantArtifactCard, AssistantVisualCard, AssistantTicketCard | — | не брать |

`useLlmChat.ts` требует правки самого большого объёма: в Atlas он тащит и то, что должно
уехать в ядро (компакция, сборка промпта). После выноса логики в ядро хук сильно
похудеет.

Из UI-конвенций стоит унести правило из [AGENTS.md](../AGENTS.md): не использовать
нативные контролы там, где приложение рисует свои, и все цвета/отступы/шрифты — только
через токены. Плюс само разделение `components` (только рендер) / `hooks` (состояние и
вызовы) / `lib` (обёртки команд и чистые хелперы).

---

## 4.10 Организационное

| Что | Откуда | Вердикт |
|-----|--------|---------|
| Правила слоёв, направление зависимостей, sink вместо `AppHandle` | [AGENTS.md](../AGENTS.md) | как есть |
| Политика ошибок: `thiserror` вглубь, `String` только на границе | AGENTS.md | как есть |
| Конвенция «одна команда — одна типизированная обёртка» | AGENTS.md | как есть |
| Формат документации фич (что реализовано и как себя ведёт) | [doc/implemented-features.md](../doc/implemented-features.md) | как образец |
| Формат документа об устройстве подсистемы | [AI_HARNESS.md](../AI_HARNESS.md) | как образец |

`AI_HARNESS.md` стоит отметить отдельно: это документ, который фиксирует не только «как
устроено», но и «что сознательно не построено». Такой раздел экономит больше времени,
чем любая диаграмма.

---

## 4.11 Сводка объёмов

| Область | Строк (Rust+TS, с тестами) | Доля переносимого |
|---------|---------------------------|-------------------|
| Агентный цикл + протокол | ~3 900 | высокая |
| Граница инструментов + файловые инструменты | ~7 000 | высокая (минус ~1 300 документационных) |
| Индекс и поиск | ~8 500 | высокая, с одной существенной правкой |
| Провайдеры, ключи, rate limit | ~5 400 | высокая |
| Сессии, контекст, компакция | ~2 800 | средняя (нужен перенос слоя TS → Rust) |
| Skills, память, планы | ~2 900 | высокая |
| Наблюдаемость | ~700 | высокая |
| UI-слой чата | ~4 000 | средняя, только при стеке Tauri+React |

Порядок величины: около **30 000 строк уже написанного и покрытого тестами кода**, из
которых переносится большая часть. Чего в этих 30 000 нет — в [05-gaps.md](05-gaps.md).
