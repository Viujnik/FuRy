-- Deterministic seed for the FuRy benchmark suite.
--
-- Design goals:
--   1. Same corpus on every machine. We use `setseed` + a linear sequence,
--      never `random()`, never `clock_timestamp()`.
--   2. Realistic shape: 12 columns, a mix of small and large fields, some
--      NULLs. The two "fat" columns (bio, avatar) are deliberately 1–2 KB and
--      8 KB so a wide query produces a payload that is meaningfully expensive
--      to serialize and meaningfully wasteful to serialize if unused.
--   3. Fast seed. 10 000 rows × ~10 KB ≈ 100 MB. Fits in shared_buffers.

SET
client_min_messages = WARNING;

DROP TABLE IF EXISTS users;

CREATE TABLE users
(
    id           BIGSERIAL PRIMARY KEY,
    username     TEXT      NOT NULL,
    email        TEXT      NOT NULL,
    display_name TEXT, -- nullable, ~1/3 NULL
    is_active    BOOLEAN   NOT NULL,
    reputation   BIGINT    NOT NULL,
    flags        BIGINT    NOT NULL,
    external_id  UUID      NOT NULL,
    created_at   TIMESTAMP NOT NULL,
    updated_at   TIMESTAMP NOT NULL,
    bio          TEXT, -- 1–2 KB
    avatar       BYTEA -- 8 KB
);

SELECT setseed(0.42);

INSERT INTO users (username, email, display_name, is_active, reputation, flags,
                   external_id, created_at, updated_at, bio, avatar)
SELECT 'user_' || g,
       'user_' || g || '@example.com',
       CASE WHEN g % 3 = 0 THEN NULL ELSE 'User ' || g END,
       (g % 2 = 0),
       (g * 7919) % 100000,
    (g * 104729) % 256,
    (
        lpad(to_hex(g), 8, '0') || '-' ||
        '0000-0000-0000-' ||
        lpad(to_hex(g), 12, '0')
    )::uuid,
    TIMESTAMP '2024-01-01 00:00:00' + (g || ' seconds')::interval,
    TIMESTAMP '2024-06-01 00:00:00' + (g || ' seconds')::interval,
    repeat(md5(g::text), 32 + (g % 32)),
    decode(repeat(md5(g::text), 256), 'hex')
FROM generate_series(1, 10000) AS g;

ANALYZE
users;

-- Prewarm: load the whole table into shared_buffers so the first query of
-- each variant doesn't pay for disk I/O.
SELECT count(*) FROM users;
