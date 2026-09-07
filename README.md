# FlatBT

Экспериментальный Behavior Tree runtime на Rust. Развиваем небольшими рабочими
итерациями: сначала проверяем семантику в generic runtime, затем добавляем storage
и compiled frontend.

Сейчас реализован **M0 — синхронная композиция**:

- `BtNode` и результаты `Success` / `Failure`;
- `seq`, `select`, `check` и `leaf`;
- `ControlNode<P, Children>` и открытый `BtControl` для custom policies;
- разнородные tuple-children размером 0–32 со статическим dispatch.

```rust
use flatbt::{BtNode, NodeResult, check, leaf, select, seq};

let tree = select((
    seq((
        check(|ammo: &usize| *ammo > 0),
        leaf(|ammo: &mut usize| {
            *ammo -= 1;
            NodeResult::Success
        }),
    )),
    leaf(|_: &mut usize| NodeResult::Failure),
));

let mut ammo = 1;
assert_eq!(tree.update(&mut ammo), NodeResult::Success);
assert_eq!(tree.update(&mut ammo), NodeResult::Failure);
```

Запуск примера с combat/patrol/idle и собственной policy `Repeat`:

```sh
cargo run --offline --example synchronous
```

Проверки:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo fmt --check
```

Каждый `update` пока выполняется до terminal result и начинает новую invocation.
Пустая sequence возвращает `Success`, пустой selector — `Failure`. Изменения context
применяются сразу и сохраняются даже после неуспешной ветки. Пример `fire` демонстрирует
только синхронное выполнение; post-commit гарантия для эффектов появится с `BtTick`.
Custom policy отвечает за завершение своего цикла: execution budget пока отсутствует.

API экспериментальный и будет меняться. `Running`, resume/revalidation, frame storage,
`BtTick`, dynamic behaviors и `bt!` ещё не реализованы. Следующий срез — M1:
`check → wait_frames(3) → fire` через несколько updates.

[Журнал решений](docs/design/decisions.md) описывает текущее состояние реализации.
[Исходный архитектурный документ](docs/design/original-architecture.md) сохранён как
материал для обсуждения; его положения не являются обязательным контрактом.
