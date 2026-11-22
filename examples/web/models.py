from fury import BaseModel


class Item(BaseModel):
    id: int
    title: str
    available: bool
    note: str | None
