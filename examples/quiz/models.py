from fury import BaseModel


class LeaderboardEntry(BaseModel):
    handle: str
    score: int


class OrderEntry(BaseModel):
    order_id: int
    placed_at_ms: int
    buyer_name: str
    city: str
    status: str
    currency: str
    total_cents: int
    adjustment_cents: int
    item_count: int
    coupon: str | None
    note: str | None
