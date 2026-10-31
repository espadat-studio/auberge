"""The role's DAV user convergence, run against the SQL the role ships.

The statement is read out of `tasks/main.yml` rather than restated, and the
fixture carries Baikal 0.13.0's real `users` table verbatim: the upsert leans
on its `UNIQUE(username)`.
"""

import re
import sqlite3
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


def converge(db, username, digest):
    body = re.search(
        r'"(INSERT INTO users .*?SELECT changes\(\);)"', TASKS, re.DOTALL
    ).group(1)
    sql = (
        body.replace("\\\n", " ")
        .replace("{{ admin_user_name }}", username)
        .replace("{{ baikal_dav_user_password_hash }}", digest)
    )
    upsert, select_changes, _ = sql.split(";")
    db.execute(upsert)
    return db.execute(select_changes).fetchone()[0]


def digests(db):
    return dict(db.execute("SELECT username, digesta1 FROM users"))


@pytest.fixture
def db():
    connection = sqlite3.connect(":memory:", isolation_level=None)
    connection.executescript(USERS_SCHEMA)
    yield connection
    connection.close()


def test_fresh_database_creates_the_dav_user(db):
    assert converge(db, "alice", "digest") == 1
    assert digests(db) == {"alice": "digest"}


def test_changed_password_rewrites_only_the_dav_user(db):
    db.execute("INSERT INTO users (username, digesta1) VALUES ('bob', 'bob-digest')")
    converge(db, "alice", "old")
    assert converge(db, "alice", "new") == 1
    assert digests(db) == {"alice": "new", "bob": "bob-digest"}


def test_unchanged_password_reports_no_change(db):
    converge(db, "alice", "digest")
    assert converge(db, "alice", "digest") == 0
    assert digests(db) == {"alice": "digest"}


def test_changed_when_reads_the_changes_count():
    assert 'changed_when: baikal_dav_user_upsert.stdout != "0"' in TASKS


def test_each_login_hashes_its_own_password():
    admin = task("Generate Baikal admin web interface password hash")
    dav = task("Generate Baikal DAV user password hash")
    assert "baikal_admin_password" in admin
    assert "baikal_dav_password" not in admin
    assert "baikal_dav_password" in dav
    assert "baikal_admin_password" not in dav
