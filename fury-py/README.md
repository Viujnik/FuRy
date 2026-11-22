# fury-py

Import `BaseModel` and `FuryDB` from `fury`. Follow the [workspace README](../README.md) to build the Python extension and run the complete browser demo.

## From a query to a frame

```python
from fury import BaseModel, FuryDB

class Item(BaseModel):
    id: int
    title: str

db = FuryDB.connect(database_url)

async def item_frame(item_id: int) -> bytes:
    item = await db.fetch_one(
        Item, "SELECT id, title FROM items WHERE id = $1", [item_id]
    )
    return item.to_bytes()
```

Supply a PostgreSQL connection URL and a matching table. Give selected columns names and SQL types that match the model. `fetch_one` requires exactly one result. `fetch_all` materializes a list, so bound result size in the query. `execute` returns the number of affected rows. The application handles authorization, HTTP responses and transaction boundaries. This API exposes no transaction or streaming cursor object.

Use runtime Python annotations in schema classes. `int` maps to signed 64-bit, `float` to Float64, and `T | None` makes a field nullable. Other scalar annotations include `str`, `bool`, `bytes`, `date`, `datetime`, and `UUID`. Use timezone-naive values with PostgreSQL `timestamp`; project `timestamptz` explicitly when timezone semantics matter. UUID fields currently read back as strings. The SQL path supports scalar columns; exact `Decimal` and nested SQL results need an explicit application-level projection. `BaseModel()` starts empty. Query results provide the encoded byte buffer.

Defining a model registers its schema and attempts to update `.fury_registry.bin` in the working directory. For TypeScript generation, use the [CLI](../fury-cli/) with trusted model source and publish the generated registry with the matching client.
