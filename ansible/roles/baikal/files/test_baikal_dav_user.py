"""The role's DAV user convergence, run against the input the role ships.

The stdin is read out of `tasks/main.yml` rather than restated and goes through
the sqlite3 CLI as on the host, so the assertions see the stdout `changed_when`
reads. The fixture carries Baikal 0.13.0's real `users` table verbatim: the
upsert leans on its `UNIQUE(username)`.
"""

import sqlite3
import subprocess
import textwrap
from pathlib import Path

import pytest

TASKS = (Path(__file__).parent.parent / "tasks" / "main.yml").read_text()
USERS_SCHEMA = """
CREATE TABLE users (
    id integer primary key asc NOT NULL,
    username TEXT NOT NULL,
    digesta1 TEXT NOT NULL,
    UNIQUE(username)
);
"""


def task(name):
    start = TASKS.index(f"- name: {name}\n")
    end = TASKS.find("\n    - name: ", start)
    return TASKS[start:end]


def converge_stdin():
    body = task("Converge DAV user digest in database")
    return textwrap.dedent(body.split("stdin: |\n")[1].split("\n      register:")[0])


def converge(db, username, digest):
    sql = converge_stdin().replace("{{ admin_user_name }}", username).replace(
        "{{ baikal_dav_user_password_hash }}", digest
    )
    return subprocess.run(
        ["sqlite3", db], input=sql, capture_output=True, text=True, check=True
    ).stdout.strip()


def digests(db):
    connection = sqlite3.connect(db)
    rows = dict(connection.execute("SELECT username, digesta1 FROM users"))
    connection.close()
    return rows


@pytest.fixture
def db(tmp_path):
    path = str(tmp_path / "db.sqlite")
    connection = sqlite3.connect(path)
    connection.executescript(USERS_SCHEMA)
    connection.close()
    return path


def test_fresh_database_creates_the_dav_user(db):
    assert converge(db, "alice", "digest") == "1"
    assert digests(db) == {"alice": "digest"}


def test_changed_password_rewrites_only_the_dav_user(db):
    converge(db, "bob", "bob-digest")
    converge(db, "alice", "old")
    assert converge(db, "alice", "new") == "1"
    assert digests(db) == {"alice": "new", "bob": "bob-digest"}


def test_unchanged_password_reports_no_change(db):
    converge(db, "alice", "digest")
    assert converge(db, "alice", "digest") == "0"
    assert digests(db) == {"alice": "digest"}


def test_a_locked_database_fails_the_task(db):
    holder = sqlite3.connect(db, isolation_level=None)
    holder.execute("BEGIN EXCLUSIVE")
    stdin = converge_stdin()
    fast = stdin.replace(".timeout 30000", ".timeout 1")
    assert fast != stdin
    result = subprocess.run(["sqlite3", db], input=fast, capture_output=True, text=True)
    holder.rollback()
    holder.close()
    assert result.returncode != 0


def test_each_login_hashes_its_own_password():
    admin = task("Generate Baikal admin web interface password hash")
    dav = task("Generate Baikal DAV user password hash")
    assert "baikal_admin_password" in admin
    assert "baikal_dav_password" not in admin
    assert "baikal_dav_password" in dav
    assert "baikal_admin_password" not in dav
