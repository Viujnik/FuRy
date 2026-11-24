"""Deterministic order-feed workload for the disposable benchmark database."""

from sqlalchemy import BigInteger, Text, select
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column

from models import OrderEntry


SEED_ORDERS = """
CREATE TABLE bench_customers AS
SELECT n AS customer_id,
       (ARRAY['Ирина Волкова','Haruto Tanaka','Sofía García','Noah Smith',
              'Layla Haddad','Léa Martin','Lucas Silva','Min-jun Park'])[1 + n % 8]
           || ' ' || (n / 8 + 1)::text AS buyer_name,
       (ARRAY['Москва','東京','Madrid','Toronto','دبي','Paris','São Paulo','서울'])[1 + n % 8] AS city
FROM generate_series(1::bigint, 20000) AS n;
ALTER TABLE bench_customers ADD PRIMARY KEY (customer_id);

CREATE TABLE bench_orders AS
SELECT n AS order_id,
       CASE WHEN n % 5 = 0 THEN 1 ELSE 2 + (n * 53) % 63 END AS shop_id,
       1 + (n * 137) % 20000 AS customer_id,
       1735689600000 + n * 1273 + (n * 31) % 1000 AS placed_at_ms,
       CASE WHEN n % 37 = 0 THEN 'refunded' WHEN n % 11 = 0 THEN 'cancelled'
            WHEN n % 7 = 0 THEN 'packing' WHEN n % 5 = 0 THEN 'shipped'
            ELSE 'delivered' END AS status,
       (ARRAY['EUR','USD','CAD','BRL'])[1 + ((n * 53) % 63) % 4] AS currency,
       350 + ((n * 7919) % 70000) + CASE WHEN n % 17 = 0 THEN 120000 ELSE 0 END AS total_cents,
       CASE WHEN n % 37 = 0 THEN -350 - ((n * 7919) % 70000)
            WHEN n % 9 = 0 THEN -500 ELSE 0 END AS adjustment_cents,
       1 + CASE WHEN n % 23 = 0 THEN 8 + n % 12 ELSE n % 4 END AS item_count,
       CASE WHEN n % 9 = 0 THEN
           (ARRAY['WELCOME10','SPRING25','LOYALTY'])[1 + (n / 9) % 3] END AS coupon,
       CASE WHEN n % 17 = 0 THEN
           'Please deliver after 18:00. Use the side entrance next to the pharmacy. '
           || 'Call if the entrance is locked. The parcel can be left with reception if nobody answers. Ref '
           || n::text
           WHEN n % 7 = 0 THEN 'Позвоните перед доставкой. Пункт выдачи № ' || (1 + n % 137)::text
       END AS note
FROM generate_series(1::bigint, 250000) AS n;
ALTER TABLE bench_orders ADD PRIMARY KEY (order_id);
ALTER TABLE bench_orders ADD FOREIGN KEY (customer_id) REFERENCES bench_customers (customer_id);
CREATE INDEX bench_orders_shop_feed ON bench_orders (shop_id, order_id DESC);
ANALYZE bench_customers;
ANALYZE bench_orders;
"""

ORDERS_QUERY = """
SELECT o.order_id, o.placed_at_ms, c.buyer_name, c.city, o.status, o.currency,
       o.total_cents, o.adjustment_cents, o.item_count, o.coupon, o.note
FROM bench_orders o JOIN bench_customers c USING (customer_id)
WHERE o.shop_id = $2::bigint
ORDER BY o.order_id DESC LIMIT $1::bigint
"""


class Base(DeclarativeBase):
    pass


class Order(Base):
    __tablename__ = "bench_orders"

    order_id: Mapped[int] = mapped_column(BigInteger, primary_key=True)
    shop_id: Mapped[int] = mapped_column(BigInteger)
    customer_id: Mapped[int] = mapped_column(BigInteger)
    placed_at_ms: Mapped[int] = mapped_column(BigInteger)
    status: Mapped[str] = mapped_column(Text)
    currency: Mapped[str] = mapped_column(Text)
    total_cents: Mapped[int] = mapped_column(BigInteger)
    adjustment_cents: Mapped[int] = mapped_column(BigInteger)
    item_count: Mapped[int] = mapped_column(BigInteger)
    coupon: Mapped[str | None] = mapped_column(Text)
    note: Mapped[str | None] = mapped_column(Text)


class Customer(Base):
    __tablename__ = "bench_customers"

    customer_id: Mapped[int] = mapped_column(BigInteger, primary_key=True)
    buyer_name: Mapped[str] = mapped_column(Text)
    city: Mapped[str] = mapped_column(Text)


async def read_order_entities(session, shop_id, size):
    result = await session.execute(
        select(Order, Customer)
        .join(Customer, Order.customer_id == Customer.customer_id)
        .where(Order.shop_id == shop_id)
        .order_by(Order.order_id.desc())
        .limit(size)
    )
    return [
        {name: getattr(customer if name in ("buyer_name", "city") else order, name)
         for name in OrderEntry.__annotations__}
        for order, customer in result
    ]
