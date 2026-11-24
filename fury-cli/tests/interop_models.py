from typing import Optional
from fury import BaseModel


class Message(BaseModel):
    id: int
    text: str
    active: bool
    note: str | None


class Summary(BaseModel):
    count: int
    label: Optional[str]
