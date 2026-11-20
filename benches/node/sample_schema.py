from fury import BaseModel


class BenchUser(BaseModel):
    id: int
    username: str
    is_active: bool
    bio: str
