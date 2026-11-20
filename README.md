# FuRy

> **FuRy — это бинарный формат записей, который живёт от драйвера базы данных до вкладки браузера, не останавливаясь по
пути на JSON.**

FuRy описывает модель **один раз** — на уровне схемы — и передаёт её как типизированные бинарные записи через всю
цепочку: от строки, которую вернул драйвер БД, до поля, которое прочитал фронтенд. Ни промежуточного словаря, ни
`JSON.stringify`, ни `JSON.parse`.

Формат **языко-агностичен**. `fury-core` знает раскладку байтов; всё остальное — тонкие адаптеры к нему. Python и
TypeScript — это первые привязки, но не конечная точка.

[Проблема](#проблема) · [Почему FuRy](#почему-fury-а-не-msgpack--protobuf--flatbuffers) · [Как это работает](#как-это-работает) · [Бенчмарки](#бенчмарки) · [Быстрый старт](#быстрый-старт) · [English](#english)

---

## Проблема

Большинство стеков делает так:

```
строка из Postgres → драйвер → dict → JSON.stringify → HTTP → JSON.parse → JS-объект → рендер
```

Каждая стрелка на этом пути аллоцирует. `JSON.stringify` обходит всё дерево. HTTP-слой копирует строку ещё раз.
`JSON.parse` строит свежий граф в V8. Если фронтенд рендерит три поля из тридцати — он всё равно распарсил все тридцать.

Для дашбордов, админок, live-обновляющихся таблиц и всего, где запрос разворачивается в тысячи строк, это доминирующая
стоимость — больше, чем сам запрос.

FuRy убирает шаг с JSON:

```
строка из Postgres → fury-core → BinaryRecord (типизированные байты) → HTTP → WasmRecord → поле по требованию
```

Байты никогда не были словарём Python. Они никогда не были объектом JavaScript. Это один плоский буфер с самоописываемой
раскладкой, и поле декодируется **только когда вы к нему обращаетесь**.

---

## Почему FuRy, а не msgpack / protobuf / FlatBuffers

Каждый новый бинарный формат должен ответить на этот вопрос. Вот честный ответ:

- **msgpack, CBOR, bincode** — это *сериализаторы*. Они дают вам байты и кодек. Чего они не дают — так это *контракта
  схемы* между продюсером и консьюмером. FuRy даёт: тот же реестр схем, который продюсер регистрирует при объявлении
  модели, читает кодогенератор на другой стороне и валидирует рантайм.
- **protobuf** требует `.proto`-файла и шага кодогенерации вне вашего языка. FuRy строит схему **из аннотаций, которые
  вы уже написали** — тот же эргономический ход, который Pydantic сделал для валидации, применённый к wire-формату.
- **FlatBuffers** — близкая идея (zero-copy, схема-driven), и мы перед ним в долгу. Но FuRy — **не** FlatBuffers: нет
  vtables, нет обязательных `.fbs`. Мы используем самоописываемый TLV-формат, который обменивает несколько байт на поле
  на forward-совместимость с эволюцией схемы, которой у чистого FlatBuffers нет.
- **Apache Arrow** целится в колоночную аналитику. FuRy целится в строковую форму: достать пользователя, отрендерить
  строку таблицы, обновить поле.

Если вам нужен стабильный, языко-агностичный IDL в стиле protobuf для большой команды с формальным ревью схем — protobuf
правильный инструмент, а FuRy нет. FuRy — для случая, когда **схема уже в вашем исходнике, а фронтенд уже написан на
другом языке**.

---

## Как это работает

```
┌───────────────┐  fetch_one   ┌──────────────────┐  as_bytes   ┌───────────────┐
│   Postgres    │ ───────────► │   BinaryRecord   │ ──────────► │  HTTP body    │
│               │  (sqlx)      │  (fury-core TLV) │             │  (raw bytes)  │
└───────────────┘              └──────────────────┘             └───────┬───────┘
                                                                       │
                                                         fetch → arrayBuffer
                                                                       │
                                                                       ▼
                                                              ┌─────────────────┐
                                                              │   WasmRecord    │
                                                              │ (fury-ts, wasm) │
                                                              └────────┬────────┘
                                                                       │
                                                        user.name ─────┘  ленивое декодирование
```

**Wire-формат.** Запись — это шестибайтовый заголовок (4 байта общего размера, magic `'F'`, версия `2`), за которым идут
TLV-записи — по одной на поле:

```
[1B длина имени][имя UTF-8][4B длина значения][байты значения]
```

Блок `байты значения` — то, что производит `ValueType::write_to`: один байт-флаг null, затем либо фиксированный
payload (`Int32` = 4 байта LE, `DateTime` = 8 байт LE), либо длина + blob (`String`, `Bytes`, `List`, `Map`). Раскладка
стабильна, и байт версии её охраняет.

**Чтение поля — O(1).** При парсинге `BinaryRecord` один раз обходит буфер и запоминает `(offset, length)` для каждого
известного поля. Чтение `user.name` — это срез `&buffer[offset..offset+len]`, за которым идёт декодирование по типу поля
из схемы.

При открытии кодек индексирует поля и проверяет значения по схеме. Типизированный геттер материализует значение,
а Rust-доступ к сырым байтам заимствует срез. Переход через границу WASM или Python может копировать буфер.

**Изменение.** Сеттер WASM обновляет запись, а следующий `finish()` компактит буфер. Метод `toBytes()` сгенерированной
модели финализирует изменения и возвращает JS-буфер.

---

## Языки и привязки

`fury-core` — единственный крейт, который знает раскладку байтов. Всё остальное — адаптеры. Формат — это контракт, а не
библиотека для конкретного языка.

Что есть сейчас:

| Привязка   | Что даёт                                                   | Собирается в             |
|------------|------------------------------------------------------------|--------------------------|
| `fury-py`  | Объявление моделей классами Python, async-драйвер Postgres | `cdylib` (`import fury`) |
| `fury-ts`  | Ленивое чтение полей в браузере, `WasmRecord`              | wasm + JS-глу            |
| `fury-cli` | Извлечение схем из исходников, кодогенерация               | нативный бинарник        |

Что это значит на практике: если вы хотите привязку для Go, Rust (нативно), Java, Ruby, C# — вам нужно реализовать
только адаптер к `fury-core` (или к его wire-формату, если вы не пишете на Rust). Ядро не переписывается.

Схема описывается **на языке продюсера** — сейчас это Python-аннотации, но концептуально это может быть любой язык, где
аннотации/декораторы/макросы доступны. Кодогенератор `fury-cli` читает реестр схем и выдаёт модели на языке консьюмера.

---

## Бенчмарки

Подход и команды описаны в [методике бенчмарков](benches/README.md). Поддерживаемые сценарии и открытые проверки перечислены в [отчёте о готовности](docs/READINESS.md).

## Быстрый старт

Пример ниже — с Python на входе и TypeScript на выходе, потому что это текущие привязки. Формат не меняется, если одна
из сторон другая.

**1. Поставьте CLI.**

```sh
cargo install --locked --path fury-cli
python3 -m venv .venv
source .venv/bin/activate
python -m pip install maturin==1.14.1
maturin develop --locked --manifest-path fury-py/Cargo.toml --features extension-module
wasm-pack build fury-ts --target web --out-dir "$PWD/fury-ts/pkg" --dev -- --locked
```

**2. Опишите модели.** Схема регистрируется в момент объявления класса.

```python
# models.py
from datetime import datetime
from fury import BaseModel

class User(BaseModel):
    id: int
    name: str
    email: str | None
    created_at: datetime
```

**3. Сгенерируйте модели для консьюмера из реестра схем.**

```sh
fury generate --lang-from python --lang-to ts \
              --src models.py \
              --dst ./frontend/src/models
```

Рядом с `user.ts` появится копия `.fury_registry.bin`. Сгенерированный класс — тонкая обёртка над `WasmRecord` с
геттерами и сеттерами по примитивным полям.

**4. Читайте из БД, отдавайте байты на клиент.**

Из каталога `frontend` установите рантайм командой `npm install @fury/wasm-core@file:../fury-ts/pkg`. Положите
сгенерированный реестр в статический каталог под именем `fury-registry.bin`.

```python
# server.py
from fury import FuryDB
from models import User

db = FuryDB.connect("postgresql://localhost/app")

async def load_user(user_id: int) -> bytes:
    user = await db.fetch_one(
        User,
        "SELECT id, name, email, created_at FROM users WHERE id = $1",
        [user_id],
    )
    return user.to_bytes()
```

```typescript
// frontend/main.ts
import init, {loadRegistry} from '@fury/wasm-core';
import {User} from './models/user';
await init();
const registry = await fetch('/fury-registry.bin');
loadRegistry(new Uint8Array(await registry.arrayBuffer()));

const bytes = await fetch(`/api/users/1`).then(r => r.arrayBuffer());
const user = new User(new Uint8Array(bytes));

try {
    console.log(user.name, user.created_at);
} finally {
    user.$record.free();
}
```

Объект `user` держится на сырых байтах, которые пришли из Python. JSON не парсился. Объектный граф в JS не строился для
тех полей, которых вы не читали.

---

## Что FuRy **не** делает

Мы предпочитаем сказать это сразу, а не давать вам выяснить это тяжёлым путём.

- **Не мульти-БД.** Пока только PostgreSQL. `sqlx::PgPool` используется напрямую, потому что драйвер `Any` не реализует
  `Decode`/`Type` для `chrono`/`uuid`. Добавить другую БД — значит научить executor её системе типов.
- **Не «Python → что угодно».** CLI сейчас поддерживает `--lang-from python --lang-to ts`. Rust→TS, Java→TS и прочее не
  реализованы. Архитектура к этому готова, кода пока нет.
- **Не Python 3.9 и ниже.** Разбор `Union` опирается на `types.UnionType` (PEP 604, `int | None`), доступный с 3.10.
- **Не замена protobuf/gRPC для больших команд с формальным процессом ревью IDL.** Нет `.proto`, нет `.fbs`, нет `buf` —
  схема живёт в исходнике продюсера. Для маленьких команд это фича, для больших — баг.
- **Не умеет вложенные модели (пока).** `List<Object>` и `Map<…, Object>` поддерживаются кодеком и методами
  `WasmRecord.getObjectList`/`setObjectList`, но кодогенератор пропускает составные поля, а десериализация в Python
  кидает `NotImplementedError`. Это самая большая дыра между «работает» и «пригодно для настоящего приложения».
- **Не стабилен.** Wire-формат версионируется и самоописывается, но публичный API (`BaseModel` и флаги `fury` CLI) может
  меняться без мажорного бампа до `1.0`.
- **Не FlatBuffers.** Несмотря на ранние формулировки в README, FuRy — кастомный TLV, а не Google FlatBuffers. Другая
  раскладка, другие компромиссы.

---

## Структура workspace

| Крейт       | Ответственность                                    | Собирается в           |
|-------------|----------------------------------------------------|------------------------|
| `fury-core` | Бинарный формат, реестр схем, Postgres-executor    | rlib                   |
| `fury-py`   | Python-привязка: `BaseModel`, `FuryDB`             | cdylib (`import fury`) |
| `fury-ts`   | WebAssembly-привязка: `WasmRecord`, `loadRegistry` | wasm + JS-глу          |
| `fury-cli`  | Извлечение схем, кодогенерация                     | нативный бинарник      |
| `benches/`  | Кросс-язычные бенчмарки (Python / Node / Rust)     | —                      |

`fury-core` — единственный крейт, который знает раскладку байтов. Остальные крейты — тонкие адаптеры. Если портируете
FuRy на новый язык, вы реализуете только адаптер.

---

## Статус проекта

**Экспериментальный.** Структура workspace стабильна, публичный API — нет. Wire-формат версионируется (`v2` на момент
написания) и forward-совместим с эволюцией схемы, но до `1.0` ожидайте ломающих изменений в `BaseModel`-подклассах и
CLI.

- [x] Самоописываемый wire-формат v2
- [x] Postgres → `BinaryRecord` через sqlx
- [x] Python-привязка с регистрацией схем из аннотаций
- [x] Async `FuryDB` в Python (`fetch_one`, `fetch_all`, `execute`)
- [x] Кодогенерация моделей из реестра
- [x] WASM-привязка с ленивым чтением полей
- [x] Кросс-язычный набор бенчмарков
- [ ] Вложенные модели в кодогенерации
- [ ] Вложенные модели в десериализации Python
- [ ] `Uuid` как настоящий `uuid.UUID` в Python (сейчас строка)
- [x] Точные 64-битные целые в JS через `$record.getBigInt` и `$record.setBigInt`
- [ ] Мульти-БД

---

## Стоит на плечах гигантов

FuRy — небольшой оркестрирующий слой. Тяжёлую работу делают:

- [`sqlx`](https://github.com/launchbadge/sqlx) — драйвер Postgres, подготовленные запросы, пул соединений. Executor
  FuRy — тонкий адаптер над `PgPool`.
- [`PyO3`](https://github.com/PyO3/pyo3) — мост Rust ↔ CPython. Всё в `fury-py` идёт через него.
- [`pyo3-async-runtimes`](https://github.com/PyO3/pyo3-async-runtimes) — интеграция Tokio для `Future` в PyO3.
- [`wasm-bindgen`](https://github.com/rustwasm/wasm-bindgen) — мост Rust ↔ JS в браузере.
- [`serde`](https://serde.rs) + [`bincode`](https://github.com/bincode-org/bincode) — формат файла реестра схем.

Без них FuRy был бы исследовательским проектом, а не рабочим pipeline'ом.

---

## Участие

См. [требования к PR и команды](CONTRIBUTING.md) и [порядок сообщения об уязвимостях](SECURITY.md).

## Лицензия

Apache-2.0. См. [LICENSE](./LICENSE).

---

# English

> **FuRy is a binary record format that lives from your database driver to your browser tab, without stopping at JSON in
between.**

FuRy describes a model **once** — at the schema level — and carries it as typed binary records through the whole chain:
from the row the database driver returned, to the field your frontend read. No intermediate dict, no `JSON.stringify`,
no `JSON.parse`.

The format is **language-agnostic**. `fury-core` knows the byte layout; everything else is a thin adapter. Python and
TypeScript are the first bindings, not the destination.

[Problem](#problem) · [Why FuRy](#why-fury-and-not-msgpack--protobuf--flatbuffers) · [How it works](#how-it-works) · [Benchmarks](#benchmarks) · [Quick start](#quick-start)

---

## Problem

Most stacks do this:

```
Postgres row → driver → dict → JSON.stringify → HTTP → JSON.parse → JS object → render
```

Every arrow on that path allocates. `JSON.stringify` walks the entire object graph. The HTTP layer copies the string
again. `JSON.parse` allocates a fresh graph in V8. If your frontend renders three fields out of thirty, you parsed all
thirty anyway.

For dashboards, admin panels, live-updating tables, and anything where a query fans out to thousands of rows, this is
the dominant cost — bigger than the query itself.

FuRy removes the JSON step:

```
Postgres row → fury-core → BinaryRecord (typed bytes) → HTTP → WasmRecord → field on demand
```

The bytes are never a Python dict. They are never a JavaScript object. They are one flat buffer with a self-describing
layout, and a field is decoded **only when you read it**.

---

## Why FuRy and not msgpack / protobuf / FlatBuffers

Every new binary format should answer this question. Here is the honest one:

- **msgpack, CBOR, bincode** are *serializers*. They give you bytes and a codec. What they don't give you is a *schema
  contract* between producer and consumer. FuRy does: the same schema registry the producer registers at
  class-definition time is the one the codegen reads and the runtime validates against.
- **protobuf** requires a `.proto` file and a codegen step outside your language. FuRy builds the schema from *
  *annotations you already wrote** — the same ergonomic move Pydantic made for validation, applied to the wire format.
- **FlatBuffers** is a similar idea (zero-copy, schema-driven), and we owe it a debt. But FuRy is **not** FlatBuffers:
  no vtables, no mandatory `.fbs`. We use a self-describing TLV layout, which trades a few bytes per field for
  forward-compatibility with schema evolution that pure FlatBuffers does not give you.
- **Apache Arrow** targets columnar analytics. FuRy targets the row-shaped case: fetch a user, render a table row,
  update a field.

If you need a stable, language-agnostic, protobuf-style IDL for a large team with a formal schema review process,
protobuf is the right tool and FuRy is not. FuRy is for the case where **the schema is already in your source and the
frontend is already written in another language**.

---

## How it works

```
┌───────────────┐  fetch_one   ┌──────────────────┐  as_bytes   ┌───────────────┐
│   Postgres    │ ───────────► │   BinaryRecord   │ ──────────► │  HTTP body    │
│               │  (sqlx)      │  (fury-core TLV) │             │  (raw bytes)  │
└───────────────┘              └──────────────────┘             └───────┬───────┘
                                                                       │
                                                         fetch → arrayBuffer
                                                                       │
                                                                       ▼
                                                              ┌─────────────────┐
                                                              │   WasmRecord    │
                                                              │ (fury-ts, wasm) │
                                                              └────────┬────────┘
                                                                       │
                                                        user.name ─────┘  lazy decode
```

**The wire format.** A record is a six-byte header (4-byte total size, magic `'F'`, version `2`) followed by TLV
entries — one per field:

```
[1B name-len][name UTF-8][4B value-len][value bytes]
```

The value bytes block is what `ValueType::write_to` produces: a 1-byte null flag, then a fixed-size payload (`Int32` = 4
bytes LE, `DateTime` = 8 bytes LE) or a length-prefixed blob (`String`, `Bytes`, `List`, `Map`). The layout is stable,
and the version byte guards it.

**Reading a field is O(1).** At parse time, `BinaryRecord` walks the buffer once and stores `(offset, length)` per known
field. Reading `user.name` is a slice `&buffer[offset..offset+len]`, followed by a decode using the field's type from
the schema.

Opening a frame indexes the fields and validates known values. A typed getter materializes its value and Rust's
raw access borrows a slice. Crossing a WASM or Python boundary can copy the buffer.

**Edits.** A WASM setter updates the record and the next `finish()` compacts the buffer. The generated model's
`toBytes()` finalizes edits and returns a JavaScript buffer.

---

## Languages and bindings

`fury-core` is the only crate that knows the byte layout. Everything else is an adapter. The format is a contract, not a
library for a specific language.

What exists today:

| Binding    | Provides                                                    | Builds to                |
|------------|-------------------------------------------------------------|--------------------------|
| `fury-py`  | Model declaration via Python classes, async Postgres driver | `cdylib` (`import fury`) |
| `fury-ts`  | Lazy field access in the browser, `WasmRecord`              | wasm + JS glue           |
| `fury-cli` | Schema extraction from source, codegen                      | native binary            |

What that means in practice: if you want a binding for Go, Rust (native), Java, Ruby, C# — you implement only the
adapter to `fury-core` (or to its wire format, if you are not writing Rust). The core does not get rewritten.

The schema is described **in the producer's language** — today that is Python annotations, but conceptually any language
with annotations/decorators/macros works. The `fury-cli` generator reads the schema registry and emits models in the
consumer's language.

---

## Benchmarks

Use the [benchmark methodology](benches/README.md). The [readiness review](docs/READINESS.md) records the supported scenarios and outstanding checks.

## Quick start

The example below goes Python-in, TypeScript-out because those are the current bindings. The format does not change if
one side is something else.

**1. Install the CLI.**

```sh
cargo install --locked --path fury-cli
python3 -m venv .venv
source .venv/bin/activate
python -m pip install maturin==1.14.1
maturin develop --locked --manifest-path fury-py/Cargo.toml --features extension-module
wasm-pack build fury-ts --target web --out-dir "$PWD/fury-ts/pkg" --dev -- --locked
```

**2. Describe models.** The schema registers the moment the class is defined.

```python
# models.py
from datetime import datetime
from fury import BaseModel

class User(BaseModel):
    id: int
    name: str
    email: str | None
    created_at: datetime
```

**3. Generate consumer-side models from the schema registry.**

```sh
fury generate --lang-from python --lang-to ts \
              --src models.py \
              --dst ./frontend/src/models
```

You get a `user.ts` next to a copy of `.fury_registry.bin`. The generated class is a thin wrapper over `WasmRecord` with
getters and setters for each primitive field.

**4. Read from the database and send the bytes to the client.**

From `frontend`, run `npm install @fury/wasm-core@file:../fury-ts/pkg`. Copy the generated registry to your static
directory as `fury-registry.bin`.

```python
# server.py
from fury import FuryDB
from models import User

db = FuryDB.connect("postgresql://localhost/app")

async def load_user(user_id: int) -> bytes:
    user = await db.fetch_one(
        User,
        "SELECT id, name, email, created_at FROM users WHERE id = $1",
        [user_id],
    )
    return user.to_bytes()
```

```typescript
// frontend/main.ts
import init, {loadRegistry} from '@fury/wasm-core';
import {User} from './models/user';
await init();
const registry = await fetch('/fury-registry.bin');
loadRegistry(new Uint8Array(await registry.arrayBuffer()));

const bytes = await fetch(`/api/users/1`).then(r => r.arrayBuffer());
const user = new User(new Uint8Array(bytes));

try {
    console.log(user.name, user.created_at);
} finally {
    user.$record.free();
}
```

The `user` object is backed by the raw bytes Python sent. No JSON was parsed. No JavaScript object graph was allocated
for the fields you did not read.

---

## What FuRy is **not**

We would rather say this up front than let you find out the hard way.

- **Not multi-database.** PostgreSQL only, for now. `sqlx::PgPool` is used directly because the `Any` driver does not
  implement `Decode`/`Type` for `chrono`/`uuid`. Adding another database means teaching the executor its type system.
- **Not "Python → anything".** The CLI currently supports `--lang-from python --lang-to ts`. Rust→TS, Java→TS, and so on
  are not implemented. The architecture is ready for them; the code is not.
- **Not Python 3.9 or below.** Union parsing relies on `types.UnionType` (PEP 604, `int | None`), available from 3.10.
- **Not a protobuf/gRPC replacement for large teams with a formal IDL review process.** No `.proto`, no `.fbs`, no
  `buf` — the schema lives in the producer's source. For small teams that is a feature; for large ones, a bug.
- **Not nested-model aware yet.** `List<Object>` and `Map<…, Object>` are supported by the codec and by
  `WasmRecord.getObjectList`/`setObjectList`, but the code generator skips composite fields and Python deserialization
  raises `NotImplementedError`. This is the biggest gap between "it works" and "it is usable in a real app".
- **Not stable.** The wire format is versioned and self-describing, but the public API (`BaseModel` behavior, `fury` CLI
  flags) may change without a major bump until `1.0`.
- **Not FlatBuffers.** Despite early README wording, FuRy is a custom TLV format, not Google FlatBuffers. Different
  layout, different tradeoffs.

---

## Workspace layout

| Crate       | Responsibility                                    | Builds to              |
|-------------|---------------------------------------------------|------------------------|
| `fury-core` | Binary format, schema registry, Postgres executor | rlib                   |
| `fury-py`   | Python binding: `BaseModel`, `FuryDB`             | cdylib (`import fury`) |
| `fury-ts`   | WebAssembly binding: `WasmRecord`, `loadRegistry` | wasm + JS glue         |
| `fury-cli`  | Schema extraction, code generation                | native binary          |
| `benches/`  | Cross-language benchmarks (Python / Node / Rust)  | —                      |

`fury-core` is the only crate that knows the byte layout. The rest are thin adapters. If you port FuRy to a new
language, you implement only the adapter.

---

## Project status

**Experimental.** The workspace structure is stable; the public API is not. The wire format is versioned (`v2` as of
this writing) and forward-compatible with schema evolution, but expect breaking changes to `BaseModel` subclasses and
the CLI until `1.0`.

- [x] Self-describing wire format v2
- [x] Postgres → `BinaryRecord` via sqlx
- [x] Python binding with annotation-based schema registration
- [x] Async `FuryDB` in Python (`fetch_one`, `fetch_all`, `execute`)
- [x] Model code generation from the registry
- [x] WASM binding with lazy field decoding
- [x] Cross-language benchmark suite
- [ ] Nested models in code generation
- [ ] Nested models in Python deserialization
- [ ] `Uuid` as a real `uuid.UUID` in Python (currently a string)
- [x] Exact 64-bit integers in JS through `$record.getBigInt` and `$record.setBigInt`
- [ ] Multi-database support

---

## Built on the shoulders of giants

FuRy is a small orchestration layer. The heavy lifting is done by:

- [`sqlx`](https://github.com/launchbadge/sqlx) — Postgres driver, prepared statements, connection pooling. FuRy's
  executor is a thin adapter over `PgPool`.
- [`PyO3`](https://github.com/PyO3/pyo3) — Rust ↔ CPython bridge. Everything in `fury-py` goes through it.
- [`pyo3-async-runtimes`](https://github.com/PyO3/pyo3-async-runtimes) — Tokio integration for PyO3 `Future`s.
- [`wasm-bindgen`](https://github.com/rustwasm/wasm-bindgen) — Rust ↔ JS bridge in the browser.
- [`serde`](https://serde.rs) + [`bincode`](https://github.com/bincode-org/bincode) — schema registry file format.

Without them, FuRy would be a research project instead of a working pipeline.

---

## Contributing

See the [PR checks and commands](CONTRIBUTING.md) and [private reporting procedure](SECURITY.md).

## License

Apache-2.0. See [LICENSE](./LICENSE).
