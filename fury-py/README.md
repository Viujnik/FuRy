# FuRy-py workspace

## За что отвечает

`fury-py` - Python-биндинги FuRy. Это тонкая обёртка над `fury-core`: она ничего
не сериализует и не ходит в базу, он только переводит питоновские объекты
в типы ядра и обратно. Конкретно крейт даёт три вещи:

1. Объявление моделей обычными Python-классами с аннотациями: схема собирается
   и регистрируется в общем реестре автоматически в момент объявления класса
   (модуль `models`);
2. Чтение полей модели без распаковки всей записи: `user.name` парсит свой кусок
   байтов из общего буфера в момент обращения (тот же модуль `models`);
3. Async-доступ к PostgreSQL: `FuryDB` с `fetch_one`/`fetch_all`/`execute`,
   где строки БД превращаются в готовые Python-объекты моделей (модуль `db`).

Сборка устроена иначе, чем у остальных крейтов, в `Cargo.toml`:
`crate-type = ["cdylib"]` и секция `[lib] name = "fury"` означают, что на выходе
получается не исполняемый файл и не rust-библиотека, а shared library, которую
Python может импортировать как `import fury`. Фича `extension-module` у pyo3 говорит
«не линкуй libpython»: символы интерпретатора придут в момент импорта модуля,
поэтому собирать wheel нужно через `maturin`, а не через обычный `cargo build`.

Структура файлов:

```text
fury-py/src
├── lib.rs         - точка входа PyO3: что вообще видно из Python
├── models.rs      - BaseModel, регистрация схем, zero-copy чтение полей
├── db.rs          - FuryDB: connect, fetch_one, fetch_all, execute
├── conversion.rs  - Python-объект → SqlValue (аргумент SQL-запроса)
└── errors.rs      - FuryError → PyErr (точка маппинга ошибок)
```

---

## Точка входа - src/lib.rs

Файл делает одну вещь: объявляет Python-модуль и регистрирует в нём
классы.

`#[pymodule] fn fury(m: &Bound<'_, PyModule>) -> PyResult<()>` - атрибут
`#[pymodule]` говорит pyo3 «это точка входа модуля»: компилятор сгенерирует
C-символ `PyInit_fury`, который интерпретатор вызовет при `import fury`. Имя
функции обязано совпадать с именем модуля из `[lib] name = "fury"` в
`Cargo.toml`, иначе Python не найдёт точку входа.

Внутри - только `m.add_class::<BaseModel>()?` и `m.add_class::<FuryDB>()?`:
после этого в Python доступны `from fury import BaseModel, FuryDB`. Функций на
уровне модуля нет: всё поведение живёт в классах, а реестр схем
доступен неявно (через регистрацию в `__init_subclass__` и чтение в `FuryDB`),
чтобы не плодить глобальное API, которое питонист должен помнить.

---

## Models. BaseModel - src/models.rs

Самый содержательный файл крейта: здесь Python-класс становится схемой и меняет свое поведение под капотом.

### Класс BaseModel и его поля

`#[pyclass(subclass, skip_from_py_object)] pub struct BaseModel` с тремя
полями: `buffer: Vec<u8>`, `field_offsets: HashMap<String, (usize, usize)>`,
`schema: Arc<ModelSchema>`.

- `subclass` - от класса можно наследоваться в Python, это основной способ
  использования;
- `skip_from_py_object` - нельзя построить `BaseModel` из произвольного
  питоновского объекта; экземпляр создаётся либо пустым конструктором, либо
  внутренней функцией `create_py_model_instance`;
- `buffer` - копия готового байтового буфера записи;
- `field_offsets` - таблица «имя поля → (начало, длина)» для O(1)-доступа;
- `schema` - `Arc` на схему: одна на все экземпляры и все запросы.

Поля приватные и без `#[pyo3(get)]`: пользователь не лазит в буфер руками,
только через `__getattr__` и `to_bytes`. Пустой `#[new]`-конструктор создаёт
«заготовку» (пустой буфер, пустые offsets, схема-заглушка `"Unknown"`) - он
нужен не пользователю, а `create_py_model_instance`, которая сразу после
`call0()` заполняет всё настоящее через `set_data`. Ручной `impl Debug`
печатает `schema_name`/`buffer_len`/`fields_count`, а не байты.

### Регистрация схемы - __init_subclass__

`#[classmethod] __init_subclass__` - хук Python: вызывается интерпретатором в
момент объявления каждого класса-наследника `BaseModel`. Объявил класс - схема уже в реестре.
Шаги метода следующие: получение имя класса из `__name__` (имена
`BaseModel` и `FuryBaseModel` пропускаются жёстко зашитой проверкой - это
служебные имена, не модели); обход `__mro()` в обратном порядке (от базовых к
более сложным) и сбор `__annotations__` каждого уровня в упорядоченный список:
поле, впервые встреченное в базе наследования, сохраняет позицию, а
переопределение в наследнике заменяет тип на месте; каждое имя прогоняется
через `parse_python_type`; из списка `FieldSchema` собирается `ModelSchema` и
кладётся в глобальный реестр (`register` - замена по имени + атомарная
перезапись файла через tmp+rename); если в окружении стоит `FURY_EMIT_MODELS`
(её ставит только CLI-субпроцесс), печатается строка `__FURY_MODEL__:{имя}` в
stdout - служебный канал для кодогенерации, обычный сервер его не включает.

### Разбор аннотаций - parse_python_type

Принимает объект-аннотацию и возвращает пару `(FieldType, nullable)`. Работает
через `typing.get_origin`/`get_args`:

+ origin = `list` → рекурсия в первый аргумент и `FieldType::List`;
+ origin = `dict` → рекурсия в ключ и значение и `FieldType::Map`;
+ origin = `Union` или `UnionType`
  (синтаксис `Type | None` из Python 3.10+) → проход по аргументам: `NoneType`
  выставляет nullable. Допускается ровно один тип значения.
  `Optional[list[int]]` даёт nullable `List(Int64)`, а union нескольких
  типов значений возвращает `TypeError`.
+ Для обычного класса (origin = None) сверяется пара
  (`__module__`, `__name__`) с таблицей точных соответствий: `builtins.int` →
  Int64, `builtins.str` → String, `builtins.bool` → Bool, `builtins.float` →
  Float64, `builtins.bytes`/`bytearray` → Bytes, `datetime.datetime` → DateTime,
  `datetime.date` → Date, `uuid.UUID` → Uuid.
+ Всё остальное - `TypeError` «Unsupported Fury annotation» на этапе объявления класса, а не
  тихая порча данных потом. Проверка пары модуль+имя защищает от ложных
  срабатываний: пользовательский класс `Printing` не станет Int64 из-за
  подстроки «int».

### Zero-copy чтение - __getattr__

Python вызывает `__getattr__` при обращении к атрибуту, которого у объекта нет
физически (`user.name`). Шаги метода следующие: получаются координаты из `field_offsets` (если их нет -
`AttributeError`, чтобы опечатки и `hasattr` вели себя по-питоновски); поле из
схемы (если их нет - тоже `AttributeError`). Берём срез `&buffer[offset..offset+len]` - байты не копируются;
`ValueType::from_bytes` по типу поля
(если не сошлось - `ValueError` «Failed to parse field»); далее конвертация с помощью `value_to_python`. То
есть пока поле не прочитали, оно лежит сырыми байтами; прочитали - распарсился
только его кусок, а не вся запись.

### to_bytes

`to_bytes(py)` отдаёт буфер как `PyBytes`; `PyBytes` неизменяемый и владеет
своими байтами, поэтому это копия, а не view. Если дан пустой буфер - `RuntimeError` «Buffer is uninitialized».

### Конвертация в Python - value_to_python и кэш классов datetime

Match по всем вариантам `ValueType`:

+ `None` → `py.None()`;
+ числа и
  float'ы → нативные `int`/`float`;
+ `Bool` → `bool`;
+ `String` → `str`;
+ `Bytes` → `PyBytes`;
+ `Date` → `datetime.date`, `DateTime` → `datetime.datetime` с семью аргументами
  (год..секунды + микросекунды = `nanosecond / 1000`);
+ `Uuid` → строка (не
  `uuid.UUID` - точка роста на будущее);
+ `List` → `PyList` и `Map` → `PyDict` с рекурсивной конвертацией элементов;
+ `Object` → `NotImplementedError` («Nested models
  serialization not implemented») - вложенные модели в Python пока не
  десериализуются. Классы `date`/`datetime` не импортируются на каждое поле:
  ссылки кэшируются в `DATE_CLS`/`DATETIME_CLS` (`OnceLock`) при первом
  использовании, потому что чтение моделей - горячий путь.

### Сборка модели из записи - create_py_model_instance

Служебная функция (не метод Python), которой пользуется db.rs:
`model_class.call0()` создаёт пустой экземпляр и для каждого поля схемы
копируются координаты из `builder.get_field_offset`; буфер копируется целиком
(`builder.as_bytes().to_vec()`); через `PyRefMut` вызывается
`set_data(buffer, offsets, Arc::clone(schema))`; `PyRefMut` дропится,
экземпляр возвращается. Копия буфера и offsets берутся из одного и того же
finish-состояния, поэтому не могут разъехаться; последующие мутации
оригинального builder'а (reopen/компакция) на модель не влияют - в питоновском
флоу builder сразу после этого умирает.

---

## DB. FuryDB - src/db.rs

`#[pyclass] pub struct FuryDB { executor: Arc<QueryExecutor> }` - обёртка над
executor'ом ядра; `Arc`, потому что один `FuryDB` живёт на всё приложение, а каждый async-вызов забирает дешёвый клон
`Arc` в свой future.

### connect

`#[staticmethod] connect(connection_string)`: внутри
`pyo3_async_runtimes::tokio::get_runtime().block_on(DatabasePool::connect(...))` - синхронный блок на tokio-рантайме,
затем клон глобального реестра и
`Arc::new(QueryExecutor::new(pool, registry))`. Ошибка коннекта приходит в
Python как `RuntimeError`.

### fetch_one

`fetch_one(model_class, query, args)`: до входа в future получает имя модели
(`__name__`), конвертируются аргументы (`parse_sql_args`), клонируется `Arc`
executor'а, а класс уносится в future как `Py<PyAny>` (`Bound` жить через
await не может). Внутри `future_into_py`: `execute_raw`; если строк не ровно одна -
`QueryResultRowMismatch` (в Python `ValueError`); схема берётся из глобального
реестра по имени (если её нет - `SchemaNotFound` → `ValueError`); создаётся `buffer` с помощью `row_to_buffer` и
`finish`; затем `Python::attach` - повторный захват GIL уже внутри
async-блока - и `create_py_model_instance`. Возвращается готовый экземпляр
модели.

### fetch_all

Аналогично `fetch_one`, но без проверки «ровно одна строка»: все строки превращаются в буферы
(`row_to_buffer` + `finish` на каждую), список буферов уносится в
`Python::attach` и там разворачивается в `PyList` экземпляров. Схему берёт из
`executor.schema_registry()` - это тот же глобальный реестр (клон `Arc`),
расхождения с `fetch_one` нет.

### execute

`execute(query, args)`: `parse_sql_args`, `execute_affected` внутри future,
результат (`rows_affected`) возвращается питоновским `int` через
`Python::attach`. Этот метод для INSERT/UPDATE/DELETE.

### parse_sql_args

`Option<PyList>` → `None` даёт пустой вектор (запрос без параметров); `Some` -
каждый элемент прогоняется через `python_to_sql_value`; ошибка конвертации
любого аргумента (`TypeError`) вылетает до запроса, а не после.

---

## Conversion. python_to_sql_value - src/conversion.rs

Единственная точка, где питоновский объект становится аргументом SQL. Порядок
проверок важен и объясним:

+ `None` → `Null`;
+ затем `PyBool` - обязательно
  раньше `PyInt`, потому что в Python `bool` это подкласс `int`;
+ `PyString` → `String`;
+ `PyInt` → `Int64` (любой питоновский`int` становится BIGINT-аргументом);
+ `PyFloat` → `Float64`; `PyBytes` →
  `Bytes`.
+ Дальше для типов, которые downcast'ом не поймать, сверяется пара
  (`__module__`, имя типа):
    + `datetime.datetime` → `SqlValue::DateTime`
      (поддерживается datetime без часового пояса, часовой пояс вызывает
      `ValueError`);
    + `datetime.date` → `SqlValue::Date`;
    + `uuid.UUID` →
      `SqlValue::Uuid` (через `str()` и `parse_str`, кривой UUID - `ValueError`).
+ Всё остальное - `TypeError` со списком поддерживаемых типов.

---

## Errors. fury_error_to_pyerr - src/errors.rs

Все `#[pymethods]` возвращают `PyResult` и прогоняют ошибки ядра через эту функцию. Маппинг
нарочно не «всё в одну кучу»:

+ `Database`, `BufferNotFinished`,
  `BufferAlreadyFinished` → `RuntimeError`;
+ `Encoding`, `SchemaNotFound`, `FieldNotFound`,
  `QueryResultRowMismatch`, `InvalidSchema` → `ValueError`;
+ `TypeMismatch` → `TypeError`.

Благодаря этому пользовательский код может
писать `except`, а не ловить «какую-то ошибку».

---

## Дата-флоу: три пути

**1. Регистрация (в момент импорта модуля с моделями):**
`class User(BaseModel)` → интерпретатор вызывает `__init_subclass__` → обход
MRO в обратном порядке, сбор `__annotations__` в упорядоченный список →
`parse_python_type` на каждое поле (`typing.get_origin`/`get_args` или
таблица модуль+имя) → `ModelSchema` → `get_global_registry().register`
(замена по имени + атомарная перезапись `.fury_registry.bin` через
tmp+rename) → при `FURY_EMIT_MODELS` дополнительно строка
`__FURY_MODEL__:User` в stdout для CLI.

**2. Чтение (SELECT → объект модели → поля):**
`await db.fetch_one(User, "SELECT ... WHERE id = $1", [1])` →
`parse_sql_args` (Python → SqlValue) → `future_into_py` (GIL отпущен) →
`executor.execute_raw` (sqlx, `PgRow`) → `executor.row_to_buffer`
(`BinaryRecord::new` + `set_field` на каждое поле схемы) → `buffer.finish()`
(заголовок) → `Python::attach` (GIL снова взят) → `create_py_model_instance`
(копия буфера + offsets + `Arc`-схема) → экземпляр у пользователя. Дальше
`user.name`: `__getattr__` → срез `buffer[offset..offset+len]` без
копирования → `ValueType::from_bytes` → `value_to_python` → питоновский
объект. Никакой распаковки всей записи ради одного поля.

**3. Запись (INSERT/UPDATE/DELETE → affected):**
`await db.execute("UPDATE users SET name = $1 WHERE id = $2", ["Alice", 1])`
→ `parse_sql_args`: `str` → String, `int` → Int64 (bool проверится раньше int
и станет Bool) → `SqlValue::bind_to` внутри `prepare_query` (unsigned касты к
signed, UInt64 через `try_from` с ошибкой переполнения) → `execute_affected`
→ `rows_affected` → питоновский `int`. Объекты моделей в этом пути не
участвуют вовсе: мутации в Python-мире идут через SQL, а не через буфер.

---

## Подводные камни

- `connect` синхронный (`block_on`): зови один раз на старте (lifespan), а не
  на каждый запрос - иначе блокируешь event loop.
- `fetch_one`/`fetch_all`/`execute` асинхронные через `future_into_py`: во
  время await GIL отпущен, поэтому питоновские объекты трогать внутри future
  нельзя - всё взаимодействие с Python собрано в `Python::attach`; класс
  модели уносится в future как `Py<PyAny>`, потому что `Bound<'py>` через
  await не живёт.
- В conversion `bool` проверяется раньше `int` сознательно (`bool` - подкласс
  `int`); поменять порядок и `True` станет числом 1 в запросе.
- Любой питоновский `int` становится Int64/BIGINT; попросить Int32 из
  аннотации пока нельзя.
- Порядок полей схемы = порядок первого появления в MRO (базовые раньше);
  переопределение в наследнике меняет тип, но не позицию; свои классы с
  именами `BaseModel`/`FuryBaseModel` не объявлять - они пропускаются служебно.
- `fetch_one` берёт схему из `get_global_registry()`, `fetch_all` - из
  `executor.schema_registry()`: это один и тот же реестр (клон `Arc`),
  расхождения нет.
- Модель - снимок: буфер копируется в момент `create_py_model_instance` из
  одного finish-состояния вместе с offsets; последующие reopen/компакции
  оригинального builder'а на неё не влияют (builder в питоновском флоу сразу
  умирает).
- Модели read-only: сеттеров и reopen из Python нет сознательно; мутации -
  только через `db.execute`/SQL; `to_bytes` отдаёт копию (`PyBytes`), а не
  view. Хоть механизм reopen и есть в core(см. fury-ts).
- `__getattr__` срабатывает только для атрибутов, которых нет: реальные
  атрибуты/property подкласса перекрывают поля буфера; опечатка в имени поля
  даёт `AttributeError` (`hasattr` работает как ожидается).
- `value_to_python`: `Uuid` приходит строкой, а не `uuid.UUID`; микросекунды =
  `nanosecond / 1000` (суб-микросекундный остаток отбрасывается); `Object`
  (вложенные модели) - `NotImplementedError`, `List`/`Map` примитивов
  работают.
- Кросс-процессные байты: парсинг (`from_raw_bytes`) в wire v2 идёт по
  TLV-записям с именами полей, поэтому любой продьюсер байтов для чужого
  процесса обязан писать имена (в core за это отвечает `write_entry`);.
- wheel собирается только `maturin`'ом: обычный `cargo build` на fury-py
  падает линкером из-за `extension-module` - это ожидаемое поведение, а не
  поломка.
- `FURY_EMIT_MODELS` ставит только CLI-субпроцесс; в обычном сервере строки
  `__FURY_MODEL__:` не печатаются.

---

## Примеры

```python
from datetime import datetime
from uuid import UUID
from fury import BaseModel, FuryDB

class User(BaseModel):          # схема зарегистрировалась прямо здесь
    id: int
    name: str
    email: str | None           # nullable
    created_at: datetime
    external_id: UUID

async def main():
    db = FuryDB.connect("postgresql://postgres@localhost:5432/fury_bench")

    user = await db.fetch_one(
        User,
        "SELECT id, name, email, created_at, external_id FROM users WHERE id = $1",
        [1],
    )
    print(user.name, user.created_at)   # парсится в момент обращения

    users = await db.fetch_all(
        User,
        "SELECT id, name, email FROM users LIMIT $1",
        [10],
    )

    affected = await db.execute(
        "UPDATE users SET name = $1 WHERE id = $2",
        ["Alice", 1],
    )
    print(affected)

    raw: bytes = user.to_bytes()        # копия буфера как bytes
```

---

## Что можно добавить (на будущее)

1. Точные числовые типы в аннотациях: `Annotated[int, FuryType(Int32)]` вместо
   всегдашнего Int64.
2. Возврат настоящего `uuid.UUID` из `value_to_python` вместо строки.
3. Десериализация `Object` и вложенных моделей в Python (сейчас
   `NotImplementedError`).
4. Мутации моделей из Python: сеттеры + reopen/finish из core (ORM-подобный
   dirty-tracking + save) - механизм в ядре уже есть, в fury-py его просто
   никто не использует.
5. Async-версия `connect` через `future_into_py`, чтобы не блокировать loop
   даже на старте из async-контекста.
6. Поддержка `decimal.Decimal` и прочих типов стандартной библиотеки в
   `python_to_sql_value`.
