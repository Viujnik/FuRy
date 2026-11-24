from html import escape
from http.cookiejar import CookieJar
import json
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from urllib.error import HTTPError
from urllib.request import HTTPCookieProcessor, Request, build_opener


from questions import QUESTIONS

origin = sys.argv[1]
client = build_opener(HTTPCookieProcessor(CookieJar()))


def call(path, data=None):
    body = None if data is None else json.dumps(data).encode()
    request = Request(origin + path, body, {"Content-Type": "application/json"} if body else {})
    try:
        result = client.open(request, timeout=5)
    except HTTPError as error:
        result = error
    with result:
        return result.code, json.load(result)


answers = [q.answer for q in QUESTIONS]
credentials = {"handle": "quiz-tester", "password": "sample-passphrase-42"}
assert call("/api/attempts", {"answers": answers})[0] == 401
assert call("/api/register", credentials) == (201, {"handle": "quiz-tester"})
assert call("/api/register", credentials)[0] == 409
assert call("/api/me") == (200, {"handle": "quiz-tester"})
assert call("/api/attempts", {"answers": answers}) == (201, {"score": len(answers), "total": len(answers)})
assert call("/api/attempts", {"answers": [-1] * len(answers)})[0] == 422
assert call("/api/logout", {}) == (200, {"handle": None})
assert call("/api/me") == (200, {"handle": None})
assert call("/api/login", {**credentials, "password": "another-password"})[0] == 401
assert call("/api/login", credentials) == (200, {"handle": "quiz-tester"})
assert {"handle": "quiz-tester", "score": len(answers)} in call("/api/leaderboard.json")[1]

with client.open(origin + "/api/leaderboard.fury", timeout=5) as response:
    page = response.read()
    assert page[4:6] == b"F\x03"
with client.open(origin + "/api/leaderboard.fury-models", timeout=5) as response:
    assert response.read() == page

if browser := os.environ.get("QUIZ_BROWSER"):
    with TemporaryDirectory() as profile:
        command = [
            browser, "--headless", "--no-first-run", f"--user-data-dir={profile}",
            "--virtual-time-budget=8000", "--dump-dom", origin,
        ]
        page = subprocess.run(command, check=True, text=True, capture_output=True, timeout=30).stdout
        assert escape(QUESTIONS[0].prompt, quote=False) in page and "<td>demo-" in page, page
        if screenshot := os.environ.get("QUIZ_SCREENSHOT"):
            subprocess.run([
                browser, "--headless", "--no-first-run", f"--user-data-dir={profile}",
                "--virtual-time-budget=8000", "--window-size=1280,1000",
                f"--screenshot={Path(screenshot).resolve()}", origin,
            ], check=True, capture_output=True, timeout=30)
        print("Chrome rendered the question and FuRy leaderboard")

print("Quiz registration, sign-in, submission and sign-out passed")
