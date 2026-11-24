from dataclasses import dataclass

import pyfory

from leaderboard_pb2 import Leaderboard


@dataclass
class ForyEntry:
    handle: str
    score: pyfory.Int64


@dataclass
class ForyLeaderboard:
    rows: list[ForyEntry]


fory = pyfory.Fory(xlang=True, ref=False, strict=True, compatible=True)
fory.register(ForyEntry, name="quiz.Entry")
fory.register(ForyLeaderboard, name="quiz.Leaderboard")


def encode_protobuf(rows):
    return Leaderboard(rows=rows).SerializeToString()


def encode_apache_fory(rows):
    return fory.serialize(ForyLeaderboard([ForyEntry(**row) for row in rows]))
