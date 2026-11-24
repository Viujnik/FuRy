# Query PostgreSQL from Python

Define a model for the columns an endpoint returns. For example, an account list can return a stable ID and a name.

```python
from fury import BaseModel, FuryDB


class User(BaseModel):
    id: int
    display_name: str


db = FuryDB.connect(database_url, max_connections=4)


async def first_page(org_id: int) -> bytes:
    return await db.fetch_page(
        User,
        "SELECT id, display_name FROM users WHERE org_id = $1 ORDER BY id LIMIT $2::bigint",
        [org_id, 50],
        max_rows=50,
    )
```

Create the pool at startup. Pass an organization the caller may access. The SQL limit bounds the query and `max_rows` caps the number of frames written into the Python bytes object.

Use `fetch_one` when you need exactly one row as a model. Use `fetch_all` for a bounded list of models, for example when Python needs to inspect each row. `execute` returns the number of affected rows. Coordinate multi-statement transactions in your database layer.

## Describe the type you actually query

Python `int` means signed 64-bit. A field declared `str | None` can hold SQL null. FuRy also supports booleans, bytes, dates, timestamps, floats, and UUID values. Project decimal amounts to exact integer cents when that matches the domain. Use a timezone-naive value for PostgreSQL `timestamp` and choose an explicit conversion for `timestamptz`.

Queries use the descriptor attached to the supplied Python class. The default wire schema name is the class name. If billing and shipping publish different orders in one registry, use distinct names such as `BillingOrder` and `ShippingOrder`. [Generate the client](../fury-cli/README.md)