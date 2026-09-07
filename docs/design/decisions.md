# Решения по мере реализации

## 2026-09-07 — M0: исполняемый синхронный backend

Исходный документ — источник идей, а не неизменяемая спецификация. Здесь фиксируем
реализованные решения, отличия и вопросы для следующих итераций. При расхождении
с документом текущую реализованную семантику описывают код, тесты и этот журнал.

### Что принято для первой итерации

- Один library crate `flatbt`, Rust edition 2024, без внешних зависимостей.
  Проверено на Rust 1.98.1; минимальная поддерживаемая версия пока не определена.
- Определение дерева неизменно (`&self`); mutable application data передаётся через
  `&mut C`. Одно определение можно последовательно выполнять с разными context.
- `BtNode::update(&self, &mut C) -> NodeResult` пока синхронный. Не вводим фиктивные
  `ExecutionCursor`, `EntryMode` или persistent state до исполняемого сценария M1.
  Это временная сигнатура, а не решение отказаться от resumable protocol.
- `BtControl<C>` — отдельная policy, не node. `ControlNode` владеет циклом исполнения;
  policy выбирает следующий индекс. Callback вызывается после terminal result child.
  `State: Default` создаётся на каждую invocation; `Send` пока не требуется, поскольку
  state не переживает update. Ограничения persistent state обсудим в M1.
- Children — обычные tuples 0–32. Macro генерирует прямой вызов concrete child в
  каждом `match` arm. Trait objects, unsafe и frame storage в этом срезе отсутствуют.
- Sequence идёт до первой failure, selector — до первого success. Пустые controls
  имеют identity results: sequence success, selector failure.
- Custom policy может снова выбрать terminal child в том же update. Ответственность
  за конечность цикла лежит на policy; неверный индекс приводит к понятному panic.
- Context не откатывается после failure. Будущая speculative execution также требует
  отдельного обсуждения side effects; здесь пока нет commit или tick guarantee.

### Подтверждение кодом

`tests/synchronous.rs` проверяет порядок и short-circuit, вложенную разнородную
композицию, повторный update, разные context, custom policy со свежим state,
нулевое число повторов, неверные индексы и dispatch всех 32 позиций.
`examples/synchronous.rs` исполняет combat/patrol/idle и включает собственную `Repeat`.

### Следующая итерация — M1

Реализовать `Running` без обязательного tick, boxed active-path storage и normal
resume. Исполняемый сценарий: condition вызывается один раз, wait приостанавливается,
а fire выполняется в том же update, в котором wait завершился.

До закрепления API проверить кодом:

1. Как safe child-entry API разделяет execution traversal и typed frame storage.
2. Как связываются definition и runtime state, чтобы state не достался другой node.
3. Как заканчивается invocation и освобождается state, включая terminal result и Drop.
4. Как fresh entry получает Evaluate, а сохранённая continuation — Resume.

Memoryful sequence при root revalidation, reactive selector и сохранение старой
ветки во время проверки альтернатив относятся к M2. Синхронный M0 их ещё не проверяет.
