from dataclasses import dataclass
from typing import Optional

import pyfory

from orders_pb2 import OrderFeed


@dataclass
class ForyOrder:
    order_id: pyfory.Int64
    placed_at_ms: pyfory.Int64
    buyer_name: str
    city: str
    status: str
    currency: str
    total_cents: pyfory.Int64
    adjustment_cents: pyfory.Int64
    item_count: pyfory.Int64
    coupon: Optional[str] = pyfory.field(nullable=True, default=None)
    note: Optional[str] = pyfory.field(nullable=True, default=None)


@dataclass
class ForyOrderFeed:
    rows: list[ForyOrder]


fory = pyfory.Fory(xlang=True, ref=False, strict=True, compatible=True)
fory.register(ForyOrder, name="shop.Order")
fory.register(ForyOrderFeed, name="shop.OrderFeed")


def encode_protobuf(rows):
    return OrderFeed(rows=rows).SerializeToString()


def encode_apache_fory(rows):
    return fory.serialize(ForyOrderFeed([ForyOrder(**row) for row in rows]))
