from __future__ import annotations

import asyncio
from contextlib import asynccontextmanager
import hashlib
import hmac
import os
from pathlib import Path
import secrets

import asyncpg
from fastapi import Cookie, FastAPI, HTTPException, Response
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse
from fastapi.staticfiles import StaticFiles
from fury import FuryDB
from pydantic import BaseModel, Field
from sqlalchemy import BigInteger, Text, select
from sqlalchemy.engine import make_url
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column

from models import LeaderboardEntry, OrderEntry
from orders import ORDERS_QUERY, SEED_ORDERS, read_order_entities

from questions import QUESTIONS, score_attempt


CITIES = os.environ.get("QUIZ_DATASET") == "cities"
MIXED = CITIES or os.environ.get("QUIZ_DATASET") == "mixed"
ORDERS = os.environ.get("QUIZ_DATASET") == "orders"
RESULT_MODEL = OrderEntry if ORDERS else LeaderboardEntry
RESULT_PATH = "/api/orders" if ORDERS else "/api/leaderboard"
DEFAULT_CURSOR = 1 if ORDERS else 100000
PAGE_SIZE = int(os.environ.get("QUIZ_PAGE_SIZE", "128"))
if not 1 <= PAGE_SIZE <= (3000 if ORDERS else 4096 if MIXED else 128):
    raise ValueError("Use 1–128 quiz rows / 1–4096 mixed rows / 1–3000 order rows")
SESSION_TTL = 86_400
COOKIE_NAME = "fury_quiz_session"
RANKING = """
SELECT handle, score FROM quiz_scores
ORDER BY score DESC, id ASC
LIMIT $1::bigint
"""
if MIXED:
    RANKING = "SELECT handle, score FROM bench_scores WHERE id <= $2::bigint ORDER BY id DESC LIMIT $1::bigint"
if ORDERS:
    RANKING = ORDERS_QUERY
SCHEMA = """
CREATE TABLE IF NOT EXISTS quiz_users (
    handle text PRIMARY KEY,
    salt bytea NOT NULL,
    password_key bytea NOT NULL
);
CREATE TABLE IF NOT EXISTS quiz_sessions (
    token_key bytea PRIMARY KEY,
    handle text NOT NULL REFERENCES quiz_users(handle),
    expires_at timestamptz NOT NULL
);
CREATE TABLE IF NOT EXISTS quiz_scores (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    handle text NOT NULL,
    score bigint NOT NULL CHECK (score >= 0)
);
"""


class Credentials(BaseModel):
    handle: str = Field(min_length=3, max_length=24, pattern=r"^[a-z][a-z0-9_-]*$")
    password: str = Field(min_length=10, max_length=128)


class Submission(BaseModel):
    answers: list[int] = Field(min_length=len(QUESTIONS), max_length=len(QUESTIONS))


class OrmBase(DeclarativeBase):
    pass


class QuizScore(OrmBase):
    __tablename__ = "bench_scores" if MIXED else "quiz_scores"

    id: Mapped[int] = mapped_column(BigInteger, primary_key=True)
    handle: Mapped[str] = mapped_column(Text)
    score: Mapped[int] = mapped_column(BigInteger)


@asynccontextmanager
async def lifespan(app: FastAPI):
    dsn = os.environ["QUIZ_DATABASE_URL"]
    pool = await asyncpg.create_pool(dsn, host=os.environ["QUIZ_PG_SOCKET"], min_size=1, max_size=4)
    orm = create_async_engine(
        make_url(dsn).set(drivername="postgresql+asyncpg"),
        connect_args={"host": os.environ["QUIZ_PG_SOCKET"]},
        pool_size=4,
        max_overflow=0,
    )
    try:
        async with pool.acquire() as conn:
            async with conn.transaction():
                await conn.execute(SCHEMA)
                if ORDERS:
                    await conn.execute(SEED_ORDERS)
                if CITIES:
                    from cities import import_cities

                    await import_cities(conn, Path(os.environ["QUIZ_CITIES_ZIP"]))
                elif MIXED:
                    await conn.execute("""
                        CREATE TABLE bench_scores AS
                        SELECT n AS id,
                            CASE n % 3 WHEN 0 THEN 'имя-' ELSE 'user-' END ||
                            left(md5(n::text), 2 + n % 31) AS handle,
                            CASE n % 4 WHEN 0 THEN n % 8 WHEN 1 THEN -n
                                WHEN 2 THEN n::bigint * 1000000007
                                ELSE -4503599627370496 + n END AS score
                        FROM generate_series(1, 100000) AS n;
                        ALTER TABLE bench_scores ADD PRIMARY KEY (id);
                        ANALYZE bench_scores;
                    """)
                if os.environ.get("QUIZ_SEED") == "1":
                    await conn.execute(
                        """
                        INSERT INTO quiz_scores(handle, score)
                        SELECT 'demo-' || lpad(n::text, 3, '0'), n % $1::integer
                        FROM generate_series(1, 128) AS n
                        WHERE NOT EXISTS (SELECT FROM quiz_scores)
                        """,
                        len(QUESTIONS) + 1,
                    )
        app.state.pool = pool
        app.state.orm_session = async_sessionmaker(orm)
        app.state.fury = await asyncio.to_thread(FuryDB.connect, dsn, max_connections=4)
        yield
    finally:
        await orm.dispose()
        await pool.close()


app = FastAPI(title="Quick Quiz", lifespan=lifespan)


@app.exception_handler(RequestValidationError)
async def validation_error(_request, _error):
    return JSONResponse({"detail": "Check the submitted fields."}, status_code=422)


def password_key(password: str, salt: bytes) -> bytes:
    return hashlib.scrypt(password.encode(), salt=salt, n=2**14, r=8, p=1)


def token_key(token: str) -> bytes:
    return hashlib.sha256(token.encode()).digest()


async def set_session(response: Response, handle: str) -> None:
    token = secrets.token_urlsafe(32)
    await app.state.pool.execute(
        """INSERT INTO quiz_sessions (token_key, handle, expires_at)
           VALUES ($1, $2, now() + make_interval(secs => $3))""",
        token_key(token), handle, SESSION_TTL,
    )
    response.set_cookie(
        COOKIE_NAME, token, max_age=SESSION_TTL, httponly=True, samesite="strict",
        secure=os.environ.get("QUIZ_HTTPS") == "1",
    )


@app.post("/api/register", status_code=201)
async def register(body: Credentials, response: Response):
    salt = secrets.token_bytes(16)
    key = await asyncio.to_thread(password_key, body.password, salt)
    added = await app.state.pool.fetchval(
        """INSERT INTO quiz_users VALUES ($1, $2, $3)
           ON CONFLICT DO NOTHING RETURNING handle""",
        body.handle, salt, key,
    )
    if added is None:
        raise HTTPException(409, "That username is taken.")
    await set_session(response, body.handle)
    return {"handle": body.handle}


@app.post("/api/login")
async def login(body: Credentials, response: Response):
    user = await app.state.pool.fetchrow(
        "SELECT salt, password_key FROM quiz_users WHERE handle=$1", body.handle
    )
    candidate = await asyncio.to_thread(password_key, body.password, user["salt"] if user else bytes(16))
    if user is None or not hmac.compare_digest(candidate, user["password_key"]):
        raise HTTPException(401, "Invalid username or password.")
    await set_session(response, body.handle)
    return {"handle": body.handle}


@app.post("/api/logout")
async def logout(response: Response, fury_quiz_session: str | None = Cookie(default=None)):
    if fury_quiz_session:
        await app.state.pool.execute(
            "DELETE FROM quiz_sessions WHERE token_key=$1", token_key(fury_quiz_session)
        )
    response.delete_cookie(COOKIE_NAME)
    return {"handle": None}


async def current_handle(session: str | None):
    if session:
        return await app.state.pool.fetchval(
            "SELECT handle FROM quiz_sessions WHERE token_key=$1 AND expires_at > now()",
            token_key(session),
        )
    return None


@app.get("/api/me")
async def me(fury_quiz_session: str | None = Cookie(default=None)):
    return {"handle": await current_handle(fury_quiz_session)}


@app.get("/api/questions")
async def questions():
    return [{"prompt": q.prompt, "choices": q.choices} for q in QUESTIONS]


@app.post("/api/attempts", status_code=201)
async def attempt(body: Submission, fury_quiz_session: str | None = Cookie(default=None)):
    handle = await current_handle(fury_quiz_session)
    if handle is None:
        raise HTTPException(401, "Sign in to save a score.")
    try:
        score = score_attempt(body.answers)
    except ValueError as error:
        raise HTTPException(422, str(error)) from error
    await app.state.pool.execute("INSERT INTO quiz_scores (handle, score) VALUES ($1, $2)", handle, score)
    return {"score": score, "total": len(QUESTIONS)}


def ranking_args(cursor: int = DEFAULT_CURSOR):
    return [PAGE_SIZE, cursor] if MIXED or ORDERS else [PAGE_SIZE]


async def ranking_rows(cursor: int):
    rows = await app.state.pool.fetch(RANKING, *ranking_args(cursor))
    return [dict(row) for row in rows]


@app.get(RESULT_PATH + ".json")
async def leaderboard_json(cursor: int = DEFAULT_CURSOR):
    return await ranking_rows(cursor)


@app.get(RESULT_PATH + ".json-direct")
async def leaderboard_json_direct(cursor: int = DEFAULT_CURSOR):
    return JSONResponse(await ranking_rows(cursor))


@app.get(RESULT_PATH + ".fury-models")
async def leaderboard_fury(cursor: int = DEFAULT_CURSOR):
    rows = await app.state.fury.fetch_all(RESULT_MODEL, RANKING, ranking_args(cursor))
    return Response(b"".join(row.to_bytes() for row in rows), media_type="application/octet-stream")


@app.get(RESULT_PATH + ".orm.json")
async def leaderboard_orm(cursor: int = DEFAULT_CURSOR):
    async with app.state.orm_session() as session:
        if ORDERS:
            return await read_order_entities(session, cursor, PAGE_SIZE)
        query = select(QuizScore).limit(PAGE_SIZE)
        if MIXED:
            query = query.where(QuizScore.id <= cursor).order_by(QuizScore.id.desc())
        else:
            query = query.order_by(QuizScore.score.desc(), QuizScore.id)
        rows = await session.scalars(query)
        return [{"handle": row.handle, "score": row.score} for row in rows]


@app.get(RESULT_PATH + ".fury")
async def leaderboard_page(cursor: int = DEFAULT_CURSOR):
    data = await app.state.fury.fetch_page(
        RESULT_MODEL, RANKING, ranking_args(cursor), max_rows=PAGE_SIZE
    )
    return Response(data, media_type="application/octet-stream")


if os.environ.get("QUIZ_COMPARE_CODECS") == "1":
    if ORDERS:
        from orders_codecs import encode_apache_fory, encode_protobuf
    else:
        from codecs_bench_encode import encode_apache_fory, encode_protobuf

    @app.get(RESULT_PATH + ".protobuf")
    async def leaderboard_protobuf(cursor: int = DEFAULT_CURSOR):
        return Response(encode_protobuf(await ranking_rows(cursor)), media_type="application/x-protobuf")

    @app.get(RESULT_PATH + ".apache-fory")
    async def leaderboard_apache_fory(cursor: int = DEFAULT_CURSOR):
        return Response(encode_apache_fory(await ranking_rows(cursor)), media_type="application/octet-stream")


app.mount("/", StaticFiles(directory=Path(os.environ["QUIZ_SITE"]), html=True), name="site")
