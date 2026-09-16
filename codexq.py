#!/usr/bin/env python3
"""
codexq - local multi-account Codex quota manager & tracker

Features:
- AsyncIO engine for high-performance concurrent quota queries and JSON-RPC.
- Modular Python SDK (`CodexQ` class) for easy programmatic import and integration.
- Seamless auto-discovery: automatically detects and synchronizes ~/.codex/auth.json on any operation.
- Active account indicator: highlights the currently active Codex login in lists and JSON output.
- Deduplicate accounts by (chatgpt_user_id, chatgpt_account_id).
- Isolated profile sandboxes with CODEX_HOME and file-backed auth credentials.
- One-command account switching (`codexq switch <target>`).
- Account alias management (`codexq alias <target> <alias>`).
- Account deletion & cleanup (`codexq remove <target>`).
- Historical quota tracking & inspection (`codexq history <target>`).
- Built-in lightweight REST API server (`codexq serve`).
- Standard library only (Python 3.10+, zero external dependencies).
"""

from __future__ import annotations

__version__ = "1.0.1"

import argparse
import asyncio
import base64
import copy
import hashlib
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import time
import unicodedata
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Coroutine, Iterable

if sys.stdout and hasattr(sys.stdout, "reconfigure"):
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass
if sys.stderr and hasattr(sys.stderr, "reconfigure"):
    try:
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

APP_VERSION = "0.2.0"
DEFAULT_DATA_DIR = Path.home() / ".codexq"
DEFAULT_AUTH_PATH = Path.home() / ".codex" / "auth.json"
DEFAULT_CODEX_BIN = "codex"

DEFAULT_CONFIG: dict[str, Any] = {
    "$schema_version": 1,
    "general": {
        "locale": "auto",
    },
    "auto_refresh": {
        "enabled": True,
        "interval_minutes": 15,
        "refresh_on_startup": True,
        "dynamic_reset_enabled": True,
        "notify_on_quota_restored": True,
        "notify_on_update": False,
    },
    "trigger": {
        "default_model": "gpt-5.6-luna",
        "preset_models": ["gpt-5.6-luna", "o3-mini", "gpt-4o"],
        "prompt": "ping",
        "skip_if_active": True,
        "auto_rollover_on_restored": False,
        "auto_rollover_scope": "current",
        "auto_rollover_weekly_threshold": 100.0,
        "account_rollovers": {},
    },
}

SETTING_KEY_MAPPING: dict[str, tuple[str, str]] = {
    "general.locale": ("general", "locale"),
    "warmup.default_model": ("trigger", "default_model"),
    "warmup.preset_models": ("trigger", "preset_models"),
    "warmup.prompt": ("trigger", "prompt"),
    "warmup.skip_if_active": ("trigger", "skip_if_active"),
    "warmup.min_interval_hours": ("trigger", "min_interval_hours"),
    "warmup.auto_rollover_on_restored": ("trigger", "auto_rollover_on_restored"),
    "warmup.auto_rollover_scope": ("trigger", "auto_rollover_scope"),
    "warmup.auto_rollover_weekly_threshold": ("trigger", "auto_rollover_weekly_threshold"),
    "trigger.auto_rollover_on_restored": ("trigger", "auto_rollover_on_restored"),
    "trigger.auto_rollover_scope": ("trigger", "auto_rollover_scope"),
    "trigger.auto_rollover_weekly_threshold": ("trigger", "auto_rollover_weekly_threshold"),
    "auto_refresh.enabled": ("auto_refresh", "enabled"),
    "auto_refresh.interval_minutes": ("auto_refresh", "interval_minutes"),
    "auto_refresh.refresh_on_startup": ("auto_refresh", "refresh_on_startup"),
    "auto_refresh.dynamic_reset_enabled": ("auto_refresh", "dynamic_reset_enabled"),
    "auto_refresh.notify_on_quota_restored": ("auto_refresh", "notify_on_quota_restored"),
    "auto_refresh.notify_on_update": ("auto_refresh", "notify_on_update"),
}


class CodexQError(RuntimeError):
    pass


@dataclass(frozen=True)
class Identity:
    user_id: str
    account_id: str
    email: str | None
    plan: str | None
    org_title: str | None = None

    @property
    def key(self) -> str:
        return f"{self.user_id}\x1f{self.account_id}"

    @property
    def profile_id(self) -> str:
        return hashlib.sha256(self.key.encode("utf-8")).hexdigest()[:20]


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def epoch_to_local(ts: int | float | None) -> str:
    if ts is None:
        return "-"
    try:
        return datetime.fromtimestamp(float(ts), timezone.utc).astimezone().strftime("%m-%d %H:%M")
    except (ValueError, OSError, TypeError):
        return str(ts)


def safe_plan(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, str):
        return value
    try:
        return json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    except TypeError:
        return str(value)


def decode_jwt_payload(jwt: str) -> dict[str, Any]:
    parts = jwt.split(".")
    if len(parts) != 3 or not parts[1]:
        raise CodexQError("id_token is not a valid JWT")
    payload = parts[1]
    payload += "=" * (-len(payload) % 4)
    try:
        raw = base64.urlsafe_b64decode(payload.encode("ascii"))
        data = json.loads(raw.decode("utf-8"))
    except Exception as exc:
        raise CodexQError(f"cannot decode id_token payload: {exc}") from exc
    if not isinstance(data, dict):
        raise CodexQError("id_token payload is not a JSON object")
    return data


def read_json_stable(path: Path, retries: int = 6, delay: float = 0.15) -> tuple[dict[str, Any], bytes]:
    last_exc: Exception | None = None
    for _ in range(retries):
        try:
            raw = path.read_bytes()
            data = json.loads(raw.decode("utf-8"))
            if not isinstance(data, dict):
                raise CodexQError(f"{path} does not contain a JSON object")
            return data, raw
        except (OSError, UnicodeDecodeError, json.JSONDecodeError, CodexQError) as exc:
            last_exc = exc
            time.sleep(delay)
    raise CodexQError(f"cannot read a stable auth file from {path}: {last_exc}")


def extract_identity(auth: dict[str, Any]) -> Identity:
    tokens = auth.get("tokens")
    if not isinstance(tokens, dict):
        tokens = {}

    user_id: str | None = None
    account_id: str | None = None
    email: str | None = None
    plan: str | None = None
    org_title: str | None = None

    id_token = tokens.get("id_token")
    if isinstance(id_token, str) and id_token:
        claims = decode_jwt_payload(id_token)
        auth_claims = claims.get("https://api.openai.com/auth")
        profile_claims = claims.get("https://api.openai.com/profile")

        if not isinstance(auth_claims, dict):
            auth_claims = {}
        if not isinstance(profile_claims, dict):
            profile_claims = {}

        user_id = auth_claims.get("chatgpt_user_id") or auth_claims.get("user_id")
        account_id = tokens.get("account_id") or auth_claims.get("chatgpt_account_id")
        email = claims.get("email") or profile_claims.get("email")
        plan = safe_plan(auth_claims.get("chatgpt_plan_type"))

        orgs = auth_claims.get("organizations")
        if isinstance(orgs, list) and orgs:
            for o in orgs:
                if isinstance(o, dict) and o.get("is_default"):
                    org_title = o.get("title")
                    break
            if not org_title and isinstance(orgs[0], dict):
                org_title = orgs[0].get("title")

    agent_identity = auth.get("agent_identity")
    if isinstance(agent_identity, dict):
        user_id = user_id or agent_identity.get("chatgpt_user_id")
        account_id = account_id or agent_identity.get("account_id")
        email = email or agent_identity.get("email")
        plan = plan or safe_plan(agent_identity.get("plan_type"))
        org_title = org_title or agent_identity.get("org_title")

    if not isinstance(user_id, str) or not user_id.strip():
        raise CodexQError("cannot find chatgpt_user_id in auth.json")
    if not isinstance(account_id, str) or not account_id.strip():
        raise CodexQError("cannot find chatgpt_account_id/account_id in auth.json")

    return Identity(
        user_id=user_id.strip(),
        account_id=account_id.strip(),
        email=email.strip() if isinstance(email, str) and email.strip() else None,
        plan=plan,
        org_title=org_title.strip() if isinstance(org_title, str) and org_title.strip() else None,
    )


def is_auth_newer_or_equal(candidate: dict[str, Any], existing: dict[str, Any]) -> bool:
    """Check whether candidate credentials are newer than or equal to existing credentials.

    Compares last_refresh ISO timestamps first; falls back to JWT expiration timestamp.
    If timestamps cannot be determined, returns True (permissive).

    Args:
        candidate: Candidate auth.json dictionary.
        existing: Existing auth.json dictionary.

    Returns:
        True if candidate is newer or equal, False otherwise.
    """
    cand_lr = candidate.get("last_refresh")
    exist_lr = existing.get("last_refresh")

    if isinstance(cand_lr, str) and isinstance(exist_lr, str):
        try:
            c_dt = datetime.fromisoformat(cand_lr.replace("Z", "+00:00"))
            e_dt = datetime.fromisoformat(exist_lr.replace("Z", "+00:00"))
            return c_dt >= e_dt
        except Exception:
            pass

    def _get_exp(val: dict[str, Any]) -> int | None:
        tokens_obj = val.get("tokens")
        if isinstance(tokens_obj, dict):
            tok = tokens_obj.get("access_token")
            if isinstance(tok, str) and tok:
                try:
                    claims = decode_jwt_payload(tok)
                    exp = claims.get("exp")
                    if isinstance(exp, (int, float)):
                        return int(exp)
                except Exception:
                    pass
        return None

    c_exp = _get_exp(candidate)
    e_exp = _get_exp(existing)
    if c_exp is not None and e_exp is not None:
        return c_exp >= e_exp

    return True


def is_access_token_expired(auth: dict[str, Any], buffer_seconds: int = 300) -> bool:
    """Check whether an account's OAuth access_token is expired or will expire soon.

    Args:
        auth: Deserialized auth.json data dictionary.
        buffer_seconds: Safety buffer in seconds before nominal expiry (default 300s).

    Returns:
        True if access token is missing, unparseable, or expiring within buffer.
        False if valid or if using non-OAuth credentials (e.g. API key).
    """
    tokens = auth.get("tokens")
    if not isinstance(tokens, dict):
        return False
    access_token = tokens.get("access_token")
    if not isinstance(access_token, str) or not access_token.strip():
        return True
    try:
        claims = decode_jwt_payload(access_token)
        exp = claims.get("exp")
        if isinstance(exp, (int, float)):
            now = time.time()
            return (now + buffer_seconds) >= exp
    except Exception:
        return True
    return False


def refresh_oauth_token_sync(profile_dir: Path) -> bool:
    """Refresh OAuth access_token using refresh_token for a specific profile directory.

    Implements the official OpenAI OAuth 2.0 Refresh Token Rotation (RTR) flow
    using standard library urllib only.

    Args:
        profile_dir: Path to the profile sandbox directory containing auth.json.

    Returns:
        True if token was refreshed and saved successfully.

    Raises:
        CodexQError: If refresh token is missing, invalid, or network call fails.
    """
    auth_path = profile_dir / "auth.json"
    if not auth_path.is_file():
        raise CodexQError(f"auth.json not found in {profile_dir}")

    auth_val, _ = read_json_stable(auth_path)
    tokens = auth_val.get("tokens")
    if not isinstance(tokens, dict):
        raise CodexQError("No tokens object found in auth.json")

    refresh_token = tokens.get("refresh_token")
    if not isinstance(refresh_token, str) or not refresh_token.strip():
        raise CodexQError("No refresh_token found in auth.json")

    client_id = "app_EMoamEEZ73f0CkXaXp7hrann"
    access_token = tokens.get("access_token")
    if isinstance(access_token, str) and access_token:
        try:
            claims = decode_jwt_payload(access_token)
            cid = claims.get("client_id")
            if isinstance(cid, str) and cid.strip():
                client_id = cid.strip()
        except Exception:
            pass

    payload_bytes = json.dumps({
        "client_id": client_id,
        "grant_type": "refresh_token",
        "refresh_token": refresh_token.strip(),
    }).encode("utf-8")

    endpoints = [
        "https://auth.openai.com/oauth/token",
        "https://auth0.openai.com/oauth/token",
    ]

    last_error: Exception | None = None
    resp_data: dict[str, Any] | None = None

    for endpoint in endpoints:
        req = urllib.request.Request(
            endpoint,
            data=payload_bytes,
            headers={
                "Content-Type": "application/json",
                "User-Agent": "Codex/0.149.1",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=20.0) as resp:
                raw_body = resp.read()
                resp_data = json.loads(raw_body.decode("utf-8"))
                break
        except urllib.error.HTTPError as http_err:
            err_body = http_err.read().decode("utf-8", errors="replace")
            try:
                err_json = json.loads(err_body)
                err_msg = err_json.get("error", {})
                if isinstance(err_msg, dict):
                    msg = err_msg.get("message") or err_msg.get("code") or err_body
                else:
                    msg = err_json.get("error_description") or str(err_msg)
            except Exception:
                msg = err_body[:200]
            raise CodexQError(f"OAuth renewal rejected (HTTP {http_err.code}): {msg}") from http_err
        except Exception as exc:
            last_error = exc
            continue

    if resp_data is None:
        raise CodexQError(f"OAuth token renewal network failure: {last_error}")

    new_access_token = resp_data.get("access_token")
    if not isinstance(new_access_token, str) or not new_access_token:
        raise CodexQError("OAuth response missing access_token")

    new_refresh_token = resp_data.get("refresh_token") or refresh_token
    new_id_token = resp_data.get("id_token")

    tokens["access_token"] = new_access_token
    tokens["refresh_token"] = new_refresh_token
    if new_id_token:
        tokens["id_token"] = new_id_token

    auth_val["last_refresh"] = utc_now_iso()

    updated_bytes = json.dumps(auth_val, indent=2, ensure_ascii=False).encode("utf-8")
    atomic_write(auth_path, updated_bytes, mode=0o600)

    # If this profile is active in ~/.codex/auth.json, synchronize
    try:
        if DEFAULT_AUTH_PATH.is_file():
            active_auth, _ = read_json_stable(DEFAULT_AUTH_PATH, retries=1)
            active_id = extract_identity(active_auth)
            profile_id = extract_identity(auth_val)
            if active_id.key == profile_id.key:
                atomic_write(DEFAULT_AUTH_PATH, updated_bytes, mode=0o600)
    except Exception:
        pass

    return True


async def refresh_oauth_token_async(profile_dir: Path) -> bool:
    """Asynchronously refresh OAuth token in thread pool without blocking event loop."""
    return await asyncio.to_thread(refresh_oauth_token_sync, profile_dir)


def atomic_write(path: Path, data: bytes, mode: int = 0o600) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        with open(tmp, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        try:
            os.chmod(tmp, mode)
        except OSError:
            pass
        os.replace(tmp, path)
        try:
            os.chmod(path, mode)
        except OSError:
            pass
    finally:
        try:
            tmp.unlink(missing_ok=True)
        except OSError:
            pass


def ensure_profile_config(profile_dir: Path) -> None:
    config = profile_dir / "config.toml"
    desired = 'cli_auth_credentials_store = "file"\n'
    if not config.exists():
        atomic_write(config, desired.encode("utf-8"), mode=0o600)
        return

    try:
        text = config.read_text(encoding="utf-8")
    except OSError:
        return

    if "cli_auth_credentials_store" not in text:
        if text and not text.endswith("\n"):
            text += "\n"
        text += desired
        atomic_write(config, text.encode("utf-8"), mode=0o600)


def colorize(text: str, code: str, enabled: bool = True) -> str:
    if not enabled:
        return text
    return f"\033[{code}m{text}\033[0m"


def short_id(value: str | None, width: int = 10) -> str:
    if not value:
        return "-"
    if len(value) <= width:
        return value
    return value[: max(4, width - 3)] + "..."


def get_account_display_name(row: Any) -> str:
    alias = None
    email = None
    user_id = None

    if isinstance(row, sqlite3.Row):
        keys = row.keys()
        alias = row["alias"] if "alias" in keys else None
        email = row["email"] if "email" in keys else None
        user_id = row["user_id"] if "user_id" in keys else None
    elif isinstance(row, dict):
        alias = row.get("alias")
        email = row.get("email")
        user_id = row.get("user_id")

    if alias and str(alias).strip():
        return str(alias).strip()
    if email and str(email).strip():
        return str(email).strip()
    return short_id(user_id, 18)


class Store:
    def __init__(self, data_dir: Path):
        self.data_dir = data_dir
        self.db_path = data_dir / "codexq.db"
        self.config_path = data_dir / "config.json"
        self.profiles_dir = data_dir / "profiles"
        self.trash_dir = data_dir / "trash"
        data_dir.mkdir(parents=True, exist_ok=True)
        self.profiles_dir.mkdir(parents=True, exist_ok=True)
        self.trash_dir.mkdir(parents=True, exist_ok=True)
        try:
            os.chmod(data_dir, 0o700)
            os.chmod(self.profiles_dir, 0o700)
            os.chmod(self.trash_dir, 0o700)
        except OSError:
            pass
        self._load_config()
        self._init_db()

    def connect(self) -> sqlite3.Connection:
        con = sqlite3.connect(self.db_path)
        con.row_factory = sqlite3.Row
        con.execute("PRAGMA foreign_keys=ON")
        con.execute("PRAGMA journal_mode=WAL")
        return con

    def _init_db(self) -> None:
        with self.connect() as con:
            con.executescript(
                """
                CREATE TABLE IF NOT EXISTS accounts (
                    identity_key TEXT PRIMARY KEY,
                    profile_id TEXT NOT NULL UNIQUE,
                    user_id TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    email TEXT,
                    plan TEXT,
                    alias TEXT,
                    org_title TEXT,
                    first_seen_at TEXT NOT NULL,
                    last_seen_at TEXT NOT NULL,
                    last_credential_update TEXT NOT NULL,
                    credential_sha256 TEXT NOT NULL,
                    credential_path TEXT NOT NULL,
                    credential_status TEXT NOT NULL DEFAULT 'active',
                    reset_credits INTEGER DEFAULT 0,
                    last_error TEXT,
                    UNIQUE(user_id, account_id)
                );
                """
            )
            try:
                con.execute("ALTER TABLE accounts ADD COLUMN reset_credits INTEGER DEFAULT 0")
            except sqlite3.OperationalError:
                pass
            try:
                con.execute("ALTER TABLE accounts ADD COLUMN org_title TEXT")
            except sqlite3.OperationalError:
                pass
            con.executescript(
                """

                CREATE TABLE IF NOT EXISTS quota_latest (
                    identity_key TEXT NOT NULL,
                    limit_id TEXT NOT NULL,
                    fetched_at TEXT NOT NULL,
                    primary_used_percent REAL,
                    primary_window_minutes INTEGER,
                    primary_resets_at INTEGER,
                    secondary_used_percent REAL,
                    secondary_window_minutes INTEGER,
                    secondary_resets_at INTEGER,
                    raw_json TEXT NOT NULL,
                    PRIMARY KEY(identity_key, limit_id),
                    FOREIGN KEY(identity_key) REFERENCES accounts(identity_key) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS quota_snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    identity_key TEXT NOT NULL,
                    limit_id TEXT NOT NULL,
                    observed_at TEXT NOT NULL,
                    primary_used_percent REAL,
                    primary_window_minutes INTEGER,
                    primary_resets_at INTEGER,
                    secondary_used_percent REAL,
                    secondary_window_minutes INTEGER,
                    secondary_resets_at INTEGER,
                    raw_json TEXT NOT NULL,
                    FOREIGN KEY(identity_key) REFERENCES accounts(identity_key) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_quota_snapshots_identity_time
                    ON quota_snapshots(identity_key, observed_at);

                CREATE TABLE IF NOT EXISTS removed_accounts (
                    identity_key TEXT PRIMARY KEY,
                    profile_id TEXT NOT NULL,
                    email TEXT,
                    user_id TEXT,
                    plan TEXT,
                    display_name TEXT,
                    removed_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS account_alarms (
                    id TEXT PRIMARY KEY,
                    identity_key TEXT NOT NULL,
                    time_of_day TEXT NOT NULL,
                    days_of_week TEXT NOT NULL DEFAULT '1,2,3,4,5',
                    enabled INTEGER NOT NULL DEFAULT 1,
                    model_override TEXT,
                    prompt_override TEXT,
                    last_triggered_at TEXT,
                    last_status TEXT,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(identity_key) REFERENCES accounts(identity_key) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_account_alarms_identity
                    ON account_alarms(identity_key);
                """
            )
            try:
                con.execute("ALTER TABLE removed_accounts ADD COLUMN plan TEXT")
            except sqlite3.OperationalError:
                pass
            try:
                con.execute("ALTER TABLE removed_accounts ADD COLUMN display_name TEXT")
            except sqlite3.OperationalError:
                pass

            # Migrate any legacy SQLite app_settings rows into config.json
            try:
                rows = con.execute("SELECT key, value FROM app_settings").fetchall()
                if rows:
                    for r in rows:
                        self.set_setting(r["key"], r["value"])
                    con.execute("DROP TABLE IF EXISTS app_settings")
            except Exception:
                pass

    def profile_dir(self, identity: Identity) -> Path:
        return self.profiles_dir / identity.profile_id

    def ingest_auth(self, source_path: Path) -> tuple[Identity, bool, bool]:
        auth, raw = read_json_stable(source_path)
        identity = extract_identity(auth)
        profile_dir = self.profile_dir(identity)
        profile_dir.mkdir(parents=True, exist_ok=True)
        try:
            os.chmod(profile_dir, 0o700)
        except OSError:
            pass
        ensure_profile_config(profile_dir)

        auth_dst = profile_dir / "auth.json"
        new_sha = hashlib.sha256(raw).hexdigest()
        now = utc_now_iso()

        with self.connect() as con:
            existing = con.execute(
                "SELECT credential_sha256 FROM accounts WHERE identity_key=?",
                (identity.key,),
            ).fetchone()

            is_new = existing is None
            credential_changed = is_new or existing["credential_sha256"] != new_sha

            # Anti-downgrade guard: If profile already exists, do not overwrite if existing profile is newer
            if credential_changed and not is_new and auth_dst.is_file():
                try:
                    existing_auth, _ = read_json_stable(auth_dst, retries=1)
                    if not is_auth_newer_or_equal(auth, existing_auth):
                        credential_changed = False
                except Exception:
                    pass

            effective_sha = existing["credential_sha256"] if (not credential_changed and not is_new) else new_sha

            if credential_changed:
                atomic_write(auth_dst, raw, mode=0o600)

            con.execute("DELETE FROM removed_accounts WHERE identity_key=?", (identity.key,))

            con.execute(
                """
                INSERT INTO accounts (
                    identity_key, profile_id, user_id, account_id, email, plan, org_title,
                    first_seen_at, last_seen_at, last_credential_update,
                    credential_sha256, credential_path, credential_status, last_error
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', NULL)
                ON CONFLICT(identity_key) DO UPDATE SET
                    email=COALESCE(excluded.email, accounts.email),
                    plan=COALESCE(excluded.plan, accounts.plan),
                    org_title=COALESCE(excluded.org_title, accounts.org_title),
                    last_seen_at=excluded.last_seen_at,
                    last_credential_update=CASE
                        WHEN excluded.credential_sha256 <> accounts.credential_sha256
                        THEN excluded.last_credential_update
                        ELSE accounts.last_credential_update
                    END,
                    credential_sha256=excluded.credential_sha256,
                    credential_path=excluded.credential_path,
                    credential_status=CASE
                        WHEN excluded.credential_sha256 <> accounts.credential_sha256
                        THEN 'active'
                        ELSE accounts.credential_status
                    END,
                    last_error=CASE
                        WHEN excluded.credential_sha256 <> accounts.credential_sha256
                        THEN NULL
                        ELSE accounts.last_error
                    END
                """,
                (
                    identity.key,
                    identity.profile_id,
                    identity.user_id,
                    identity.account_id,
                    identity.email,
                    identity.plan,
                    identity.org_title,
                    now,
                    now,
                    now,
                    effective_sha,
                    str(auth_dst),
                ),
            )

        return identity, is_new, credential_changed

    def account_rows(self) -> list[sqlite3.Row]:
        with self.connect() as con:
            return list(
                con.execute(
                    """
                    SELECT a.*,
                           q.limit_id,
                           q.fetched_at,
                           q.primary_used_percent,
                           q.primary_window_minutes,
                           q.primary_resets_at,
                           q.secondary_used_percent,
                           q.secondary_window_minutes,
                           q.secondary_resets_at
                    FROM accounts a
                    LEFT JOIN quota_latest q
                       ON q.identity_key = a.identity_key
                      AND q.limit_id = COALESCE(
                          (
                            SELECT q2.limit_id
                            FROM quota_latest q2
                            WHERE q2.identity_key = a.identity_key
                            ORDER BY CASE WHEN q2.limit_id='codex' THEN 0 ELSE 1 END, q2.limit_id
                            LIMIT 1
                          ),
                          'codex'
                      )
                    ORDER BY COALESCE(a.alias, a.email, a.user_id)
                    """
                )
            )

    def accounts_for_refresh(self) -> list[sqlite3.Row]:
        with self.connect() as con:
            return list(
                con.execute(
                    "SELECT * FROM accounts ORDER BY COALESCE(alias, email, user_id)"
                )
            )

    def resolve_account(self, target: str) -> sqlite3.Row | None:
        target = target.strip()
        if not target:
            return None
        with self.connect() as con:
            row = con.execute(
                """
                SELECT * FROM accounts
                 WHERE identity_key = ?
                    OR alias = ?
                    OR email = ?
                    OR user_id = ?
                    OR profile_id = ?
                    OR profile_id LIKE ? || '%'
                 ORDER BY CASE
                     WHEN identity_key = ? THEN 0
                     WHEN alias = ? THEN 1
                     WHEN email = ? THEN 2
                     WHEN profile_id = ? THEN 3
                     ELSE 4
                 END
                 LIMIT 1
                """,
                (target, target, target, target, target, target, target, target, target, target),
            ).fetchone()
            if row:
                return row

            all_rows = con.execute("SELECT * FROM accounts").fetchall()
            target_lower = target.lower()
            matching_username = [
                r for r in all_rows
                if r["email"] and r["email"].split("@")[0].lower() == target_lower
            ]
            if len(matching_username) == 1:
                return matching_username[0]
            return None

    def set_alias(self, target: str, alias: str | None) -> bool:
        row = self.resolve_account(target)
        if not row:
            return False
        clean_alias = alias.strip() if (alias and alias.strip()) else None
        with self.connect() as con:
            con.execute(
                "UPDATE accounts SET alias=? WHERE identity_key=?",
                (clean_alias, row["identity_key"]),
            )
        return True

    def reset_all_aliases(self) -> int:
        with self.connect() as con:
            cur = con.execute("UPDATE accounts SET alias = NULL WHERE alias IS NOT NULL")
            return cur.rowcount

    def get_removed_identity_keys(self) -> set[str]:
        with self.connect() as con:
            rows = con.execute("SELECT identity_key FROM removed_accounts").fetchall()
            return {r["identity_key"] for r in rows}

    def remove_account(self, target: str) -> bool:
        row = self.resolve_account(target)
        if not row:
            return False
        identity_key = row["identity_key"]
        profile_dir = self.profiles_dir / row["profile_id"]
        now = utc_now_iso()
        display_name = get_account_display_name(row)
        plan = row["plan"]
        with self.connect() as con:
            con.execute(
                """
                INSERT OR REPLACE INTO removed_accounts (
                    identity_key, profile_id, email, user_id, plan, display_name, removed_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (identity_key, row["profile_id"], row["email"], row["user_id"], plan, display_name, now),
            )
            con.execute("DELETE FROM accounts WHERE identity_key=?", (identity_key,))
        if profile_dir.exists():
            self.trash_dir.mkdir(parents=True, exist_ok=True)
            trash_profile = self.trash_dir / row["profile_id"]
            if trash_profile.exists():
                shutil.rmtree(trash_profile, ignore_errors=True)
            try:
                shutil.move(str(profile_dir), str(trash_profile))
            except Exception:
                # Fallback to copy and delete if cross-device or permission issue
                shutil.copytree(profile_dir, trash_profile, dirs_exist_ok=True)
                shutil.rmtree(profile_dir, ignore_errors=True)
        return True

    def resolve_trash_account(self, target: str) -> sqlite3.Row | None:
        target = target.strip()
        if not target:
            return None
        with self.connect() as con:
            row = con.execute(
                """
                SELECT * FROM removed_accounts
                WHERE identity_key = ?
                   OR profile_id = ?
                   OR email = ?
                   OR display_name = ?
                """,
                (target, target, target, target),
            ).fetchone()
            if row:
                return row
            t_lower = target.lower()
            rows = con.execute("SELECT * FROM removed_accounts").fetchall()
            for r in rows:
                if (r["email"] and r["email"].lower() == t_lower) or \
                   (r["display_name"] and r["display_name"].lower() == t_lower) or \
                   (r["profile_id"] and r["profile_id"].lower().startswith(t_lower)) or \
                   (r["identity_key"] and r["identity_key"].lower().startswith(t_lower)):
                    return r
            return None

    def list_trash(self) -> list[dict[str, Any]]:
        with self.connect() as con:
            rows = con.execute("SELECT * FROM removed_accounts ORDER BY removed_at DESC").fetchall()
            result = []
            for r in rows:
                p_id = r["profile_id"]
                trash_auth = self.trash_dir / p_id / "auth.json"
                result.append({
                    "identity_key": r["identity_key"],
                    "profile_id": p_id,
                    "email": r["email"],
                    "user_id": r["user_id"],
                    "plan": r["plan"],
                    "display_name": r["display_name"] or r["email"] or p_id,
                    "removed_at": r["removed_at"],
                    "has_credentials": trash_auth.is_file(),
                })
            return result

    def restore_account(self, target: str) -> tuple[bool, str]:
        row = self.resolve_trash_account(target)
        if not row:
            return False, f"Account '{target}' not found in recycle bin"

        identity_key = row["identity_key"]
        profile_id = row["profile_id"]
        display_label = row["display_name"] or row["email"] or profile_id

        trash_auth = self.trash_dir / profile_id / "auth.json"
        auth_file_to_ingest: Path | None = None

        if trash_auth.is_file():
            auth_file_to_ingest = trash_auth
        else:
            # Fallback: check if ~/.codex/backups has matching credentials
            backup_dir = Path.home() / ".codex" / "backups"
            if backup_dir.exists():
                for cand in backup_dir.glob("*/auth.json"):
                    try:
                        data, _ = read_json_stable(cand, retries=1)
                        cand_id = extract_identity(data)
                        if cand_id.key == identity_key or (row["email"] and data.get("user", {}).get("email") == row["email"]):
                            auth_file_to_ingest = cand
                            break
                    except Exception:
                        continue

        if not auth_file_to_ingest:
            return False, f"Credentials for '{display_label}' not found in trash or backups. Please log in again via browser."

        try:
            self.ingest_auth(auth_file_to_ingest)
        except Exception as exc:
            return False, f"Failed to ingest credentials for '{display_label}': {exc}"

        trash_profile = self.trash_dir / profile_id
        if trash_profile.exists():
            shutil.rmtree(trash_profile, ignore_errors=True)

        with self.connect() as con:
            con.execute("DELETE FROM removed_accounts WHERE identity_key=?", (identity_key,))

        return True, f"Account '{display_label}' has been successfully restored."

    def purge_trash(self, target: str | None = None) -> int:
        count = 0
        if target:
            row = self.resolve_trash_account(target)
            if not row:
                return 0
            trash_profile = self.trash_dir / row["profile_id"]
            if trash_profile.exists():
                shutil.rmtree(trash_profile, ignore_errors=True)
            with self.connect() as con:
                con.execute("DELETE FROM removed_accounts WHERE identity_key=?", (row["identity_key"],))
            return 1
        else:
            with self.connect() as con:
                rows = con.execute("SELECT profile_id, identity_key FROM removed_accounts").fetchall()
                for r in rows:
                    p = self.trash_dir / r["profile_id"]
                    if p.exists():
                        shutil.rmtree(p, ignore_errors=True)
                    count += 1
                con.execute("DELETE FROM removed_accounts")
            if self.trash_dir.exists():
                for item in self.trash_dir.iterdir():
                    if item.is_dir():
                        shutil.rmtree(item, ignore_errors=True)
            return count

    def mark_status(self, identity_key: str, status: str, error: str | None = None) -> None:
        with self.connect() as con:
            con.execute(
                "UPDATE accounts SET credential_status=?, last_error=? WHERE identity_key=?",
                (status, error, identity_key),
            )

    def sync_profile_auth_metadata(self, identity_key: str, profile_dir: Path) -> None:
        path = profile_dir / "auth.json"
        if not path.exists():
            return
        try:
            auth, raw = read_json_stable(path)
            identity = extract_identity(auth)
        except CodexQError:
            return
        if identity.key != identity_key:
            raise CodexQError(
                "profile auth identity changed unexpectedly; refusing to update metadata"
            )
        sha = hashlib.sha256(raw).hexdigest()
        with self.connect() as con:
            old = con.execute(
                "SELECT credential_sha256 FROM accounts WHERE identity_key=?",
                (identity_key,),
            ).fetchone()
            if old and old["credential_sha256"] != sha:
                con.execute(
                    """
                    UPDATE accounts
                       SET credential_sha256=?,
                           last_credential_update=?,
                           last_seen_at=?,
                           email=COALESCE(?, email),
                           plan=COALESCE(?, plan),
                           org_title=COALESCE(?, org_title),
                           credential_status='active',
                           last_error=NULL
                     WHERE identity_key=?
                    """,
                    (sha, utc_now_iso(), utc_now_iso(), identity.email, identity.plan, identity.org_title, identity_key),
                )

    def save_quota(self, identity_key: str, result: dict[str, Any]) -> None:
        now = utc_now_iso()
        buckets = normalize_rate_limit_buckets(result)
        if not buckets:
            buckets = [("unknown", {}, result)]

        with self.connect() as con:
            for limit_id, snap, raw_bucket in buckets:
                primary = normalize_window(snap.get("primary"))
                secondary = normalize_window(snap.get("secondary"))
                raw_json = json.dumps(raw_bucket, ensure_ascii=False, separators=(",", ":"))

                values = (
                    identity_key,
                    limit_id,
                    now,
                    primary["used_percent"],
                    primary["window_minutes"],
                    primary["resets_at"],
                    secondary["used_percent"],
                    secondary["window_minutes"],
                    secondary["resets_at"],
                    raw_json,
                )

                con.execute(
                    """
                    INSERT INTO quota_latest (
                        identity_key, limit_id, fetched_at,
                        primary_used_percent, primary_window_minutes, primary_resets_at,
                        secondary_used_percent, secondary_window_minutes, secondary_resets_at,
                        raw_json
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(identity_key, limit_id) DO UPDATE SET
                        fetched_at=excluded.fetched_at,
                        primary_used_percent=excluded.primary_used_percent,
                        primary_window_minutes=excluded.primary_window_minutes,
                        primary_resets_at=excluded.primary_resets_at,
                        secondary_used_percent=excluded.secondary_used_percent,
                        secondary_window_minutes=excluded.secondary_window_minutes,
                        secondary_resets_at=excluded.secondary_resets_at,
                        raw_json=excluded.raw_json
                    """,
                    values,
                )

                con.execute(
                    """
                    INSERT INTO quota_snapshots (
                        identity_key, limit_id, observed_at,
                        primary_used_percent, primary_window_minutes, primary_resets_at,
                        secondary_used_percent, secondary_window_minutes, secondary_resets_at,
                        raw_json
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    values,
                )

            resets_info = get_any(result, "rateLimitResetCredits", "rate_limit_reset_credits")
            reset_count = None
            if isinstance(resets_info, dict):
                reset_count = resets_info.get("availableCount") or resets_info.get("available_count")

            plan = get_any(result, "planType", "plan_type")
            con.execute(
                "UPDATE accounts SET credential_status='active', last_error=NULL, reset_credits=COALESCE(?, reset_credits), plan=COALESCE(?, plan) WHERE identity_key=?",
                (reset_count, plan, identity_key),
            )

    def get_snapshots(self, identity_key: str, limit: int = 50) -> list[sqlite3.Row]:
        with self.connect() as con:
            return list(
                con.execute(
                    """
                    SELECT * FROM quota_snapshots
                     WHERE identity_key = ?
                     ORDER BY observed_at DESC
                     LIMIT ?
                    """,
                    (identity_key, limit),
                )
            )

    def prune_snapshots(self, keep_days: int = 30) -> int:
        cutoff = datetime.now(timezone.utc).timestamp() - (keep_days * 86400)
        cutoff_iso = datetime.fromtimestamp(cutoff, timezone.utc).isoformat(timespec="seconds")
        with self.connect() as con:
            cur = con.execute("DELETE FROM quota_snapshots WHERE observed_at < ?", (cutoff_iso,))
            return cur.rowcount

    def _load_config(self) -> dict[str, Any]:
        if not self.config_path.is_file():
            cfg = copy.deepcopy(DEFAULT_CONFIG)
            self._save_config_atomic(cfg)
            return cfg
        try:
            data, _ = read_json_stable(self.config_path, retries=2)
            if isinstance(data, dict):
                merged = copy.deepcopy(DEFAULT_CONFIG)
                for k, v in data.items():
                    if isinstance(v, dict) and k in merged and isinstance(merged[k], dict):
                        merged[k].update(v)
                    else:
                        merged[k] = v
                return merged
        except Exception:
            pass
        return copy.deepcopy(DEFAULT_CONFIG)

    def _save_config_atomic(self, cfg: dict[str, Any]) -> None:
        self.data_dir.mkdir(parents=True, exist_ok=True)
        tmp_path = self.data_dir / f"config.json.tmp.{os.getpid()}_{int(time.time() * 1000)}"
        text = json.dumps(cfg, indent=2, ensure_ascii=False) + "\n"
        try:
            with open(tmp_path, "w", encoding="utf-8") as f:
                f.write(text)
                f.flush()
                os.fsync(f.fileno())
            os.replace(tmp_path, self.config_path)
        except Exception:
            if tmp_path.exists():
                try:
                    tmp_path.unlink()
                except OSError:
                    pass
            raise

    def get_setting(self, key: str, default: str | None = None) -> str | None:
        cfg = self._load_config()
        if key in SETTING_KEY_MAPPING:
            section, subkey = SETTING_KEY_MAPPING[key]
            val = cfg.get(section, {}).get(subkey)
            if val is not None:
                if isinstance(val, bool):
                    return "true" if val else "false"
                if isinstance(val, (list, dict)):
                    return json.dumps(val, ensure_ascii=False)
                return str(val)
        if "." in key:
            parts = key.split(".", 1)
            if parts[0] in cfg and isinstance(cfg[parts[0]], dict):
                v = cfg[parts[0]].get(parts[1])
                if v is not None:
                    if isinstance(v, bool):
                        return "true" if v else "false"
                    if isinstance(v, (list, dict)):
                        return json.dumps(v, ensure_ascii=False)
                    return str(v)
        if key in cfg:
            v = cfg[key]
            if isinstance(v, bool):
                return "true" if v else "false"
            if isinstance(v, (list, dict)):
                return json.dumps(v, ensure_ascii=False)
            return str(v)
        return default

    def set_setting(self, key: str, value: Any) -> None:
        cfg = self._load_config()
        val = value
        if isinstance(value, str):
            v_lower = value.strip().lower()
            if v_lower == "true":
                val = True
            elif v_lower == "false":
                val = False
            else:
                try:
                    val = json.loads(value)
                except Exception:
                    val = value

        if key in SETTING_KEY_MAPPING:
            section, subkey = SETTING_KEY_MAPPING[key]
            if section not in cfg or not isinstance(cfg[section], dict):
                cfg[section] = {}
            cfg[section][subkey] = val
        elif "." in key:
            parts = key.split(".", 1)
            if parts[0] not in cfg or not isinstance(cfg[parts[0]], dict):
                cfg[parts[0]] = {}
            cfg[parts[0]][parts[1]] = val
        else:
            cfg[key] = val

        self._save_config_atomic(cfg)

    def get_account_rollover(self, identity_key: str) -> dict[str, Any]:
        cfg = self._load_config()
        rollovers = cfg.get("trigger", {}).get("account_rollovers", {})
        return rollovers.get(identity_key, {"enabled": False, "min_weekly_remaining": 0.0})

    def set_account_rollover(
        self,
        identity_key: str,
        enabled: bool,
        min_weekly_remaining: float = 0.0,
    ) -> None:
        cfg = self._load_config()
        trigger = cfg.setdefault("trigger", {})
        rollovers = trigger.setdefault("account_rollovers", {})
        rollovers[identity_key] = {
            "enabled": bool(enabled),
            "min_weekly_remaining": max(0.0, min(100.0, float(min_weekly_remaining))),
        }
        self._save_config_atomic(cfg)

    def get_all_settings(self) -> dict[str, str]:
        cfg = self._load_config()
        res: dict[str, str] = {}
        for dot_key, (section, subkey) in SETTING_KEY_MAPPING.items():
            v = cfg.get(section, {}).get(subkey)
            if v is not None:
                if isinstance(v, bool):
                    res[dot_key] = "true" if v else "false"
                elif isinstance(v, (list, dict)):
                    res[dot_key] = json.dumps(v, ensure_ascii=False)
                else:
                    res[dot_key] = str(v)
        return res

    def list_alarms(self, identity_key: str | None = None) -> list[sqlite3.Row]:
        with self.connect() as con:
            if identity_key:
                return list(
                    con.execute(
                        "SELECT * FROM account_alarms WHERE identity_key = ? ORDER BY time_of_day ASC",
                        (identity_key,),
                    )
                )
            return list(con.execute("SELECT * FROM account_alarms ORDER BY time_of_day ASC"))

    def get_alarm(self, alarm_id: str) -> sqlite3.Row | None:
        with self.connect() as con:
            return con.execute("SELECT * FROM account_alarms WHERE id = ?", (alarm_id,)).fetchone()

    def save_alarm(self, alarm_data: dict[str, Any]) -> sqlite3.Row:
        alarm_id = str(alarm_data.get("id") or f"alm_{int(time.time() * 1000)}")
        identity_key = str(alarm_data["identity_key"])
        time_of_day = str(alarm_data["time_of_day"]).strip()
        days_of_week = str(alarm_data.get("days_of_week") or "1,2,3,4,5").strip()
        enabled = 1 if alarm_data.get("enabled", True) else 0
        model_override = alarm_data.get("model_override") or None
        if model_override:
            model_override = str(model_override).strip() or None
        prompt_override = alarm_data.get("prompt_override") or None
        if prompt_override:
            prompt_override = str(prompt_override).strip() or None
        now_iso = utc_now_iso()

        with self.connect() as con:
            con.execute(
                """
                INSERT INTO account_alarms(
                    id, identity_key, time_of_day, days_of_week, enabled,
                    model_override, prompt_override, created_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(id) DO UPDATE SET
                    identity_key=excluded.identity_key,
                    time_of_day=excluded.time_of_day,
                    days_of_week=excluded.days_of_week,
                    enabled=excluded.enabled,
                    model_override=excluded.model_override,
                    prompt_override=excluded.prompt_override
                """,
                (alarm_id, identity_key, time_of_day, days_of_week, enabled, model_override, prompt_override, now_iso),
            )
            row = con.execute("SELECT * FROM account_alarms WHERE id = ?", (alarm_id,)).fetchone()
            assert row is not None
            return row

    def delete_alarm(self, alarm_id: str) -> bool:
        with self.connect() as con:
            cur = con.execute("DELETE FROM account_alarms WHERE id = ?", (alarm_id,))
            return cur.rowcount > 0

    def update_alarm_status(self, alarm_id: str, status: str, triggered_at: str | None = None) -> None:
        now_iso = triggered_at or utc_now_iso()
        with self.connect() as con:
            con.execute(
                "UPDATE account_alarms SET last_status = ?, last_triggered_at = ? WHERE id = ?",
                (status, now_iso, alarm_id),
            )


def validate_alarm_intervals(
    existing_alarms: Iterable[Any],
    candidate_time: str,
    exclude_id: str | None = None,
    min_interval_minutes: int = 300,
) -> tuple[bool, str | None]:
    """
    Validates that a candidate alarm time does not conflict (overlap within min_interval_minutes)
    with any other active alarm for the same account.
    """
    def parse_time_to_minutes(t_str: str) -> int:
        parts = t_str.strip().split(":")
        return int(parts[0]) * 60 + int(parts[1])

    try:
        cand_min = parse_time_to_minutes(candidate_time)
    except (ValueError, IndexError):
        return False, f"Invalid time format: '{candidate_time}'. Expected HH:MM."

    for alm in existing_alarms:
        if isinstance(alm, sqlite3.Row) or isinstance(alm, dict):
            alm_id = alm["id"]
            alm_enabled = bool(alm["enabled"])
            alm_time = alm["time_of_day"]
        else:
            continue

        if not alm_enabled or (exclude_id and alm_id == exclude_id):
            continue

        try:
            exist_min = parse_time_to_minutes(alm_time)
        except (ValueError, IndexError):
            continue

        diff = abs(cand_min - exist_min)
        if diff > 12 * 60:
            diff = 24 * 60 - diff

        if diff < min_interval_minutes:
            diff_h = diff / 60.0
            req_h = min_interval_minutes / 60.0
            return (
                False,
                f"与已有闹钟 {alm_time} 间隔仅 {diff_h:.1f} 小时，必须 >= {req_h:.1f} 小时（滑动窗口防重叠）",
            )

    return True, None


def get_any(d: dict[str, Any] | None, *keys: str) -> Any:
    if not isinstance(d, dict):
        return None
    for key in keys:
        if key in d:
            return d[key]
    return None


def normalize_window(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict):
        return {"used_percent": None, "window_minutes": None, "resets_at": None}
    return {
        "used_percent": get_any(value, "usedPercent", "used_percent"),
        "window_minutes": get_any(
            value, "windowDurationMins", "window_duration_mins", "windowMinutes", "window_minutes"
        ),
        "resets_at": get_any(value, "resetsAt", "resets_at", "resetAt", "reset_at"),
    }


def normalize_rate_limit_buckets(
    result: dict[str, Any],
) -> list[tuple[str, dict[str, Any], dict[str, Any]]]:
    """Normalize various rate limit JSON structures into standardized limit buckets.

    Args:
        result: Raw JSON-RPC result dictionary from account/rateLimits/read.

    Returns:
        List of tuples (limit_id, primary_window_dict, raw_bucket_dict).
    """
    if not isinstance(result, dict):
        return []

    # 1. Top-level bucket directly on result (e.g. {"limitId": "codex", "primary": {...}, ...})
    if result.get("primary") or result.get("secondary") or result.get("limitId"):
        limit_id = str(get_any(result, "limitId", "limit_id") or "codex")
        return [(limit_id, result, result)]

    # 2. Check rateLimitsByLimitId container
    by_id = get_any(result, "rateLimitsByLimitId", "rate_limits_by_limit_id")
    buckets: list[tuple[str, dict[str, Any], dict[str, Any]]] = []

    if isinstance(by_id, dict) and by_id:
        for key, snap in by_id.items():
            if not isinstance(snap, dict):
                continue
            limit_id = str(get_any(snap, "limitId", "limit_id") or key)
            buckets.append((limit_id, snap, snap))
        return buckets

    # 3. Check rateLimits / rate_limits container
    snap = get_any(result, "rateLimits", "rate_limits")
    if isinstance(snap, dict):
        if snap.get("primary") or snap.get("secondary") or snap.get("limitId"):
            limit_id = str(get_any(snap, "limitId", "limit_id") or "codex")
            buckets.append((limit_id, snap, snap))
            return buckets
        for key, val in snap.items():
            if isinstance(val, dict) and (val.get("primary") or val.get("secondary") or val.get("limitId")):
                limit_id = str(get_any(val, "limitId", "limit_id") or key)
                buckets.append((limit_id, val, val))
        if buckets:
            return buckets
    elif isinstance(snap, list):
        for item in snap:
            if isinstance(item, dict):
                limit_id = str(get_any(item, "limitId", "limit_id") or "codex")
                buckets.append((limit_id, item, item))
        if buckets:
            return buckets

    return buckets


async def _read_json_rpc(
    reader: asyncio.StreamReader,
    target_id: int,
    stderr_lines: list[str],
) -> dict[str, Any]:
    while True:
        line_bytes = await reader.readline()
        if not line_bytes:
            detail = f"; stderr: {' | '.join(stderr_lines[-3:])}" if stderr_lines else ""
            raise CodexQError(f"codex app-server closed connection prematurely{detail}")
        line = line_bytes.decode("utf-8", errors="replace").strip()
        if not line:
            continue
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(payload, dict):
            continue
        if payload.get("id") != target_id:
            continue
        if "error" in payload:
            raise CodexQError(f"app-server RPC error: {payload['error']}")
        result = payload.get("result")
        if not isinstance(result, dict):
            raise CodexQError(f"unexpected app-server response for id={target_id}: {payload}")
        return result


async def _drain_stderr(stderr_reader: asyncio.StreamReader, lines_buf: list[str]) -> None:
    try:
        while True:
            line_bytes = await stderr_reader.readline()
            if not line_bytes:
                break
            line = line_bytes.decode("utf-8", errors="replace").strip()
            if line:
                lines_buf.append(line)
                if len(lines_buf) > 30:
                    del lines_buf[:10]
    except (asyncio.CancelledError, OSError):
        pass


async def query_rate_limits_async(profile_dir: Path, codex_bin: str, timeout: float = 30.0) -> dict[str, Any]:
    if shutil.which(codex_bin) is None and not Path(codex_bin).exists():
        raise CodexQError(f"cannot find Codex executable: {codex_bin}")

    ensure_profile_config(profile_dir)

    env = os.environ.copy()
    env["CODEX_HOME"] = str(profile_dir)

    creationflags = 0
    if os.name == "nt" and hasattr(subprocess, "CREATE_NO_WINDOW"):
        creationflags = subprocess.CREATE_NO_WINDOW

    try:
        proc = await asyncio.create_subprocess_exec(
            codex_bin,
            "app-server",
            "--stdio",
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=env,
            creationflags=creationflags,
        )
    except OSError as exc:
        raise CodexQError(f"cannot start codex app-server: {exc}") from exc

    assert proc.stdin is not None
    assert proc.stdout is not None
    assert proc.stderr is not None

    stderr_lines: list[str] = []
    stderr_task = asyncio.create_task(_drain_stderr(proc.stderr, stderr_lines))

    async def send(msg: dict[str, Any]) -> None:
        raw = json.dumps(msg, separators=(",", ":")).encode("utf-8") + b"\n"
        proc.stdin.write(raw)
        await proc.stdin.drain()

    async def execute_handshake() -> dict[str, Any]:
        await send(
            {
                "method": "initialize",
                "id": 0,
                "params": {
                    "clientInfo": {
                        "name": "codexq",
                        "title": "Codex Quota Manager",
                        "version": APP_VERSION,
                    }
                },
            }
        )
        await _read_json_rpc(proc.stdout, 0, stderr_lines)

        await send({"method": "initialized", "params": {}})
        await send({"method": "account/rateLimits/read", "id": 1})
        return await _read_json_rpc(proc.stdout, 1, stderr_lines)

    try:
        return await asyncio.wait_for(execute_handshake(), timeout=timeout)
    except asyncio.TimeoutError as exc:
        detail = f"; stderr: {' | '.join(stderr_lines[-3:])}" if stderr_lines else ""
        raise CodexQError(f"codex app-server timed out after {timeout:g}s{detail}") from exc
    finally:
        stderr_task.cancel()
        try:
            proc.stdin.close()
            await proc.stdin.wait_closed()
        except Exception:
            pass
        if proc.returncode is None:
            try:
                proc.terminate()
                await asyncio.wait_for(proc.wait(), timeout=2.0)
            except (asyncio.TimeoutError, ProcessLookupError):
                try:
                    proc.kill()
                    await asyncio.wait_for(proc.wait(), timeout=1.0)
                except Exception:
                    pass


async def refresh_one_async(
    store: Store,
    row: sqlite3.Row,
    codex_bin: str,
    timeout: float,
    quiet: bool = False,
) -> bool:
    identity_key = row["identity_key"]
    profile_dir = store.profiles_dir / row["profile_id"]
    auth_file = profile_dir / "auth.json"
    label = get_account_display_name(row)

    if not auth_file.exists():
        store.mark_status(identity_key, "reauth_required", "saved auth.json is missing")
        if not quiet:
            print(f"[FAIL] {label}: saved auth.json is missing", file=sys.stderr)
        return False

    # Proactive renewal: if access_token is expired or will expire within 5 minutes, renew via OAuth
    try:
        auth_data, _ = read_json_stable(auth_file, retries=1)
        if is_access_token_expired(auth_data, buffer_seconds=300):
            try:
                await refresh_oauth_token_async(profile_dir)
                store.sync_profile_auth_metadata(identity_key, profile_dir)
            except Exception:
                pass
    except Exception:
        pass

    try:
        result = await query_rate_limits_async(profile_dir, codex_bin, timeout=timeout)
        store.sync_profile_auth_metadata(identity_key, profile_dir)
        store.save_quota(identity_key, result)
        if not quiet:
            print(f"[ OK ] {label}")
        return True
    except Exception as exc:
        msg = str(exc)
        lower = msg.lower()
        is_auth_error = any(s in lower for s in ("401", "token_expired", "token is expired", "unauthorized", "login required", "not logged in"))

        if is_auth_error:
            # Attempt OAuth refresh fallback and retry probe once
            try:
                await refresh_oauth_token_async(profile_dir)
                store.sync_profile_auth_metadata(identity_key, profile_dir)
                result = await query_rate_limits_async(profile_dir, codex_bin, timeout=timeout)
                store.sync_profile_auth_metadata(identity_key, profile_dir)
                store.save_quota(identity_key, result)
                if not quiet:
                    print(f"[ OK ] {label}")
                return True
            except Exception:
                pass

        if is_auth_error:
            status = "reauth_required"
        else:
            status = "error"
        if "could not parse your authentication token" in lower or "401" in lower:
            msg = "Authentication token expired (401 Unauthorized). Automatic renewal failed; please sign in again."
        elif "app-server rpc error:" in lower:
            m = re.search(r'["\']message["\']:\s*["\']([^"\'\n]+)', msg)
            if m:
                msg = m.group(1).strip()
            else:
                msg = msg.replace("app-server RPC error:", "").strip()
        msg = msg[:300]
        store.mark_status(identity_key, status, msg)
        if not quiet:
            print(f"[FAIL] {label}: {msg}", file=sys.stderr)
        return False


async def warmup_account_async(
    store: Store,
    row: sqlite3.Row,
    codex_bin: str = DEFAULT_CODEX_BIN,
    model: str | None = None,
    prompt: str | None = None,
    timeout: float = 30.0,
    force: bool = False,
    quiet: bool = False,
) -> dict[str, Any]:
    """
    Executes an ephemeral, lightweight warmup ping using the account's profile sandbox
    to trigger / advance the 5-hour rolling rate limit window.
    """
    identity_key = row["identity_key"]
    profile_dir = store.profiles_dir / row["profile_id"]
    auth_file = profile_dir / "auth.json"
    label = get_account_display_name(row)

    if not auth_file.exists():
        msg = "saved auth.json is missing"
        store.mark_status(identity_key, "reauth_required", msg)
        return {"status": "failed", "error": msg, "label": label}

    # Model resolution: explicit param -> global setting -> "gpt-5.6-luna"
    resolved_model = model or store.get_setting("warmup.default_model", "gpt-5.6-luna")
    # Prompt resolution: explicit param -> global setting -> "ping"
    resolved_prompt = prompt or store.get_setting("warmup.prompt", "ping")

    # Check if window is already active and skip if not forced
    skip_if_active = store.get_setting("warmup.skip_if_active", "true").lower() == "true"
    if not force and skip_if_active:
        with store.connect() as con:
            latest_q = con.execute(
                "SELECT primary_used_percent, primary_resets_at FROM quota_latest WHERE identity_key = ?",
                (identity_key,),
            ).fetchone()
        if latest_q:
            used = latest_q["primary_used_percent"]
            resets_at = latest_q["primary_resets_at"]
            now_epoch = time.time()
            if used is not None and used > 0 and resets_at and resets_at > now_epoch:
                msg = f"当前额度窗口已处于活跃状态（将于 {epoch_to_local(resets_at)} 重置），已自动跳过以避免消耗额度。(Quota window already active)"
                if not quiet:
                    print(f"[SKIP] {label}: {msg}")
                return {
                    "status": "skipped",
                    "label": label,
                    "message": msg,
                    "resets_at": resets_at,
                    "model": resolved_model,
                }

    ensure_profile_config(profile_dir)

    # Proactive renewal: if access_token is expired or will expire within 5 minutes, renew via OAuth
    try:
        auth_data, _ = read_json_stable(auth_file, retries=1)
        if is_access_token_expired(auth_data, buffer_seconds=300):
            try:
                await refresh_oauth_token_async(profile_dir)
                store.sync_profile_auth_metadata(identity_key, profile_dir)
            except Exception:
                pass
    except Exception:
        pass

    env = os.environ.copy()
    env["CODEX_HOME"] = str(profile_dir)

    creationflags = 0
    if os.name == "nt" and hasattr(subprocess, "CREATE_NO_WINDOW"):
        creationflags = subprocess.CREATE_NO_WINDOW

    cmd = [
        codex_bin,
        "exec",
        "--ephemeral",
        "--skip-git-repo-check",
        "-m",
        resolved_model,
        resolved_prompt,
    ]

    try:
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            stdin=asyncio.subprocess.DEVNULL,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=env,
            creationflags=creationflags,
        )
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=timeout)
        if proc.returncode == 0:
            if not quiet:
                print(f"[WARMUP OK] {label} (model: {resolved_model})")
            # Automatically trigger background refresh to capture newly activated quota
            try:
                await refresh_one_async(store, row, codex_bin, timeout=timeout, quiet=True)
            except Exception:
                pass
            return {
                "status": "success",
                "label": label,
                "model": resolved_model,
                "prompt": resolved_prompt,
                "message": f"Warmup request successfully executed with model '{resolved_model}'.",
            }
        else:
            err_msg = stderr.decode("utf-8", errors="replace").strip() or f"Process exited with code {proc.returncode}"
            err_msg = err_msg[:300]
            if not quiet:
                print(f"[WARMUP FAIL] {label}: {err_msg}", file=sys.stderr)
            return {
                "status": "failed",
                "label": label,
                "model": resolved_model,
                "error": err_msg,
            }
    except asyncio.TimeoutError:
        err_msg = f"Warmup timed out after {timeout:g}s"
        if not quiet:
            print(f"[WARMUP TIMEOUT] {label}: {err_msg}", file=sys.stderr)
        return {
            "status": "failed",
            "label": label,
            "model": resolved_model,
            "error": err_msg,
        }
    except Exception as exc:
        err_msg = str(exc)[:300]
        if not quiet:
            print(f"[WARMUP ERROR] {label}: {err_msg}", file=sys.stderr)
        return {
            "status": "failed",
            "label": label,
            "model": resolved_model,
            "error": err_msg,
        }


def to_remaining(used_value: Any) -> float | None:
    if used_value is None:
        return None
    try:
        v = float(used_value)
        return max(0.0, min(100.0, 100.0 - v))
    except (ValueError, TypeError):
        return None


def remaining_text(used_value: Any) -> str:
    rem = to_remaining(used_value)
    if rem is None:
        return "-"
    if rem.is_integer():
        return f"{int(rem)}%"
    return f"{rem:.1f}%"


def percent_text(value: Any) -> str:
    if value is None:
        return "-"
    try:
        v = float(value)
        if v.is_integer():
            return f"{int(v)}%"
        return f"{v:.1f}%"
    except (ValueError, TypeError):
        return str(value)


def find_window(row: sqlite3.Row, target_minutes: int) -> tuple[Any, Any]:
    candidates = [
        (
            row["primary_window_minutes"],
            row["primary_used_percent"],
            row["primary_resets_at"],
        ),
        (
            row["secondary_window_minutes"],
            row["secondary_used_percent"],
            row["secondary_resets_at"],
        ),
    ]
    for mins, used, reset in candidates:
        if mins is not None and int(mins) == target_minutes:
            return used, reset
    return None, None


def get_window_display(row: sqlite3.Row) -> tuple[str, Any, Any, str, Any, Any]:
    p_used, p_reset = find_window(row, 300)
    s_used, s_reset = find_window(row, 10080)

    p_label = "5H"
    s_label = "WEEK"

    if p_used is None:
        p_used, p_reset = row["primary_used_percent"], row["primary_resets_at"]
        if row["primary_window_minutes"] is not None:
            mins = int(row["primary_window_minutes"])
            p_label = f"{mins // 60}H" if mins % 60 == 0 else f"{mins}M"

    if s_used is None:
        s_used, s_reset = row["secondary_used_percent"], row["secondary_resets_at"]
        if row["secondary_window_minutes"] is not None:
            mins = int(row["secondary_window_minutes"])
            s_label = f"{mins // 1440}D" if mins % 1440 == 0 else (f"{mins // 60}H" if mins % 60 == 0 else f"{mins}M")

    return p_label, p_used, p_reset, s_label, s_used, s_reset


def plain_len(s: str) -> int:
    clean = re.sub(r"\033\[[0-9;]*m", "", s)
    w = 0
    for ch in clean:
        if unicodedata.east_asian_width(ch) in ("F", "W"):
            w += 2
        else:
            w += 1
    return w


def render_table(rows: list[sqlite3.Row], current_key: str | None = None, use_color: bool = True) -> str:
    if not rows:
        return "No accounts recorded."

    headers = ["ACCOUNT", "PLAN", "5H REMAIN", "5H RESET", "WEEK REMAIN", "WEEK RESET", "RESETS", "STATUS"]
    data: list[list[str]] = []

    for row in rows:
        is_cur = (current_key is not None and row["identity_key"] == current_key)
        base_label = get_account_display_name(row)

        if is_cur:
            label = ("* " + base_label) if not use_color else (colorize("*", "32;1") + " " + colorize(base_label, "1"))
        else:
            label = "  " + base_label

        _, p_used, p_reset, _, s_used, s_reset = get_window_display(row)

        p_rem = to_remaining(p_used)
        s_rem = to_remaining(s_used)
        p_str = remaining_text(p_used)
        s_str = remaining_text(s_used)
        status_str = str(row["credential_status"])

        row_keys = row.keys() if hasattr(row, "keys") else []
        resets_val = row["reset_credits"] if ("reset_credits" in row_keys and row["reset_credits"] is not None) else "-"
        resets_str = str(resets_val)

        if use_color:
            # Remaining quota: 0% is exhausted (Red), <= 20% is low (Yellow), > 20% is good (Green)
            if p_rem is not None:
                p_str = colorize(p_str, "31" if p_rem <= 0 else ("33" if p_rem <= 20 else "32"))
            if s_rem is not None:
                s_str = colorize(s_str, "31" if s_rem <= 0 else ("33" if s_rem <= 20 else "32"))
            if resets_val not in ("-", 0, "0"):
                resets_str = colorize(resets_str, "36;1")
            if status_str == "active":
                status_str = colorize(status_str, "32")
            elif status_str == "reauth_required":
                status_str = colorize(status_str, "31")
            else:
                status_str = colorize(status_str, "33")

        data.append(
            [
                str(label),
                str(row["plan"] or "-"),
                p_str,
                epoch_to_local(p_reset),
                s_str,
                epoch_to_local(s_reset),
                resets_str,
                status_str,
            ]
        )

    widths = [len(h) for h in headers]
    for r in data:
        for i, cell in enumerate(r):
            widths[i] = min(max(widths[i], plain_len(cell)), 36)

    def fmt_row(r: Iterable[str]) -> str:
        cells = []
        for i, cell in enumerate(r):
            text = str(cell)
            pad = max(0, widths[i] - plain_len(text))
            cells.append(text + " " * pad)
        return "  ".join(cells)

    lines = [fmt_row(headers), fmt_row("-" * w for w in widths)]
    lines.extend(fmt_row(r) for r in data)
    return "\n".join(lines)


def render_history_table(snapshots: list[sqlite3.Row], use_color: bool = True) -> str:
    if not snapshots:
        return "No history records found."

    headers = ["OBSERVED AT (UTC)", "LIMIT ID", "PRIMARY REMAIN", "WINDOW", "RESET AT", "SECONDARY REMAIN", "WINDOW"]
    data: list[list[str]] = []

    for s in snapshots:
        p_rem = to_remaining(s["primary_used_percent"])
        s_rem = to_remaining(s["secondary_used_percent"])
        p_str = remaining_text(s["primary_used_percent"])
        s_str = remaining_text(s["secondary_used_percent"])
        p_win = f"{s['primary_window_minutes']}m" if s["primary_window_minutes"] else "-"
        s_win = f"{s['secondary_window_minutes']}m" if s["secondary_window_minutes"] else "-"

        if use_color and p_rem is not None:
            p_str = colorize(p_str, "31" if p_rem <= 0 else ("33" if p_rem <= 20 else "32"))

        data.append([
            str(s["observed_at"]),
            str(s["limit_id"]),
            p_str,
            p_win,
            epoch_to_local(s["primary_resets_at"]),
            s_str,
            s_win,
        ])

    widths = [len(h) for h in headers]
    for r in data:
        for i, cell in enumerate(r):
            widths[i] = min(max(widths[i], plain_len(cell)), 36)

    def fmt_row(r: Iterable[str]) -> str:
        cells = []
        for i, cell in enumerate(r):
            text = str(cell)
            pad = max(0, widths[i] - plain_len(text))
            cells.append(text + " " * pad)
        return "  ".join(cells)

    lines = [fmt_row(headers), fmt_row("-" * w for w in widths)]
    lines.extend(fmt_row(r) for r in data)
    return "\n".join(lines)


def restart_codex_system(
    relaunch: bool = True,
    codex_bin: str = DEFAULT_CODEX_BIN,
    start_if_not_running: bool = True,
) -> tuple[bool, str]:
    """
    Terminates running Codex processes (codex CLI / app-server daemon,
    desktop apps like OpenAI.Codex/ChatGPT, Codex++), and optionally relaunches them.
    Ensures zero black console window popups across all platforms.
    """
    if sys.platform != "win32":
        try:
            # Check if Codex Desktop App was running (macOS / Linux)
            was_desktop = False
            if sys.platform == "darwin":
                chk = subprocess.run(["pgrep", "-f", "Codex.app"], capture_output=True, text=True)
                if chk.returncode == 0 and chk.stdout.strip():
                    was_desktop = True
            elif sys.platform.startswith("linux"):
                chk = subprocess.run(["pgrep", "-f", "codex.*app"], capture_output=True, text=True)
                if chk.returncode == 0 and chk.stdout.strip():
                    was_desktop = True

            # Terminate running daemons and host
            subprocess.run(["pkill", "-f", "codex.*app-server"], capture_output=True)
            subprocess.run(["pkill", "-f", "codex-code-mode-host"], capture_output=True)
            if was_desktop:
                subprocess.run(["pkill", "-f", "Codex.app"], capture_output=True)

            relaunched: list[str] = []
            if relaunch:
                should_launch_desktop = was_desktop or start_if_not_running
                if sys.platform == "darwin" and should_launch_desktop:
                    try:
                        subprocess.Popen(
                            ["open", "-a", "Codex"],
                            stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL,
                            start_new_session=True,
                        )
                        relaunched.append("Codex 桌面应用")
                    except Exception:
                        subprocess.Popen(
                            [codex_bin, "app"],
                            stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL,
                            start_new_session=True,
                        )
                        relaunched.append("Codex 桌面应用")
                elif should_launch_desktop:
                    subprocess.Popen(
                        [codex_bin, "app"],
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                        start_new_session=True,
                    )
                    relaunched.append("Codex 桌面应用")

            if relaunched:
                if was_desktop:
                    return True, f"Codex 进程已重启并重新拉起: {', '.join(relaunched)}"
                return True, "Codex 桌面应用已启动。"
            if not relaunch:
                return True, "已成功终止所有运行中的 Codex 进程。"
            return True, "Codex 后台进程已重启。"
        except Exception as e:
            return False, f"重启 Codex 失败: {e}"

    # --- Windows Implementation ---
    # Combine inspect and kill into a single hidden PowerShell script to minimize latency and guarantee 0 window flash.
    ps_combined = (
        "$procs = Get-Process | Where-Object { "
        "($_.Name -eq 'codex' -or $_.Name -eq 'codex-code-mode-host') -or "
        "($_.Name -eq 'ChatGPT' -and $_.Path -like '*OpenAI.Codex*') -or "
        "($_.Name -like 'codex-plus-plus*') "
        "}; "
        "$info = $procs | Select-Object Name, Path | ConvertTo-Json -Compress; "
        "$procs | Stop-Process -Force -ErrorAction SilentlyContinue; "
        "if ($info) { Write-Output $info }"
    )
    was_codex_app = False
    was_cpp = False
    cpp_path: Path | None = None

    try:
        proc = subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", ps_combined],
            capture_output=True,
            text=True,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0x08000000),
        )
        raw = proc.stdout.strip()
        if raw:
            try:
                data = json.loads(raw)
                if isinstance(data, dict):
                    data = [data]
                for item in data:
                    p_name = (item.get("Name") or "").lower()
                    p_path = item.get("Path") or ""
                    if "openai.codex" in p_path.lower() or (p_name == "chatgpt" and "codex" in p_path.lower()):
                        was_codex_app = True
                    if "codex-plus-plus" in p_name:
                        was_cpp = True
                        if p_path and Path(p_path).is_file():
                            cpp_path = Path(p_path)
            except Exception:
                pass
    except Exception as e:
        return False, f"终止 Codex 进程失败: {e}"

    time.sleep(0.5)

    relaunched: list[str] = []
    no_window = getattr(subprocess, "CREATE_NO_WINDOW", 0x08000000)

    if relaunch:
        if was_cpp:
            try:
                target_cpp = cpp_path or Path("E:/Applications/Codex++/codex-plus-plus.exe")
                if target_cpp.is_file():
                    if hasattr(os, "startfile"):
                        try:
                            os.startfile(str(target_cpp))
                            relaunched.append("Codex++")
                        except Exception:
                            subprocess.Popen(
                                [str(target_cpp)],
                                cwd=str(target_cpp.parent),
                                creationflags=no_window,
                                stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL,
                            )
                            relaunched.append("Codex++")
                    else:
                        subprocess.Popen(
                            [str(target_cpp)],
                            cwd=str(target_cpp.parent),
                            creationflags=no_window,
                            stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL,
                        )
                        relaunched.append("Codex++")
            except Exception:
                pass

        if was_codex_app or (not was_cpp and start_if_not_running):
            started = False
            # 1. Native Windows ShellExecute via os.startfile: 100% silent, 0 console window, instant
            if hasattr(os, "startfile"):
                try:
                    os.startfile("shell:AppsFolder\\OpenAI.Codex_2p2nqsd0c76g0!App")
                    started = True
                except Exception:
                    started = False

            # 2. Fallback: launch `codex app` silently using CREATE_NO_WINDOW with DEVNULL
            if not started:
                try:
                    subprocess.Popen(
                        [codex_bin, "app"],
                        creationflags=no_window,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                    )
                    started = True
                except Exception:
                    pass

            if started:
                relaunched.append("Codex 桌面应用")

    if relaunched:
        if was_codex_app or was_cpp:
            return True, f"Codex 进程已重启并重新拉起: {', '.join(relaunched)}"
        return True, "Codex 桌面应用已启动。"
    if not relaunch:
        return True, "已成功终止所有运行中的 Codex 进程。"
    return True, "已成功清理 Codex 后台服务，下次调用将直接加载最新凭证。"


# =====================================================================
# General Python SDK Interface
# =====================================================================

class CodexQ:
    """
    High-level asynchronous controller for multi-account Codex quota management.
    Seamlessly discovers and auto-syncs accounts without manual scanning.
    """

    def __init__(
        self,
        data_dir: Path | str | None = None,
        auth_path: Path | str | None = None,
        codex_bin: str = DEFAULT_CODEX_BIN,
    ):
        self.data_dir = Path(data_dir or DEFAULT_DATA_DIR).expanduser()
        self.auth_path = Path(auth_path or DEFAULT_AUTH_PATH).expanduser()
        self.codex_bin = codex_bin
        self.store = Store(self.data_dir)

    def get_current_identity_key(self) -> str | None:
        """Returns the identity_key of the currently active account in auth_path."""
        if not self.auth_path.exists():
            return None
        try:
            auth, _ = read_json_stable(self.auth_path)
            return extract_identity(auth).key
        except Exception:
            return None

    def auto_sync_backups(self) -> int:
        """
        Scans ~/.codex/backups/ for any accounts managed by tools like Codex++,
        automatically absorbing them into codexq profiles.
        Only ingests the newest backup file for each unique account, and updates
        existing accounts if the backup credentials are newer.
        """
        backup_dir = self.auth_path.parent / "backups"
        if not backup_dir.exists():
            return 0

        removed_keys = self.store.get_removed_identity_keys()

        latest_files: dict[str, tuple[Path, dict[str, Any], float]] = {}
        for auth_file in backup_dir.glob("*/auth.json"):
            try:
                data, _ = read_json_stable(auth_file, retries=1)
                ident = extract_identity(data)
                if ident.key in removed_keys:
                    continue
                mtime = auth_file.stat().st_mtime
                if ident.key not in latest_files:
                    latest_files[ident.key] = (auth_file, data, mtime)
                else:
                    _, prev_data, prev_mtime = latest_files[ident.key]
                    if is_auth_newer_or_equal(data, prev_data) and (mtime > prev_mtime or data != prev_data):
                        latest_files[ident.key] = (auth_file, data, mtime)
            except Exception:
                continue

        imported = 0
        for auth_file, _, _ in latest_files.values():
            try:
                _, is_new, changed = self.store.ingest_auth(auth_file)
                if is_new or changed:
                    imported += 1
            except Exception:
                pass
        return imported

    def auto_sync_current(self, silent: bool = True) -> tuple[Identity, bool, bool] | None:
        """
        Silently checks ~/.codex/auth.json. If it contains a new account or updated
        credentials, automatically ingests it into SQLite and the profile store.
        """
        if not self.auth_path.exists():
            return None
        try:
            identity, is_new, changed = self.store.ingest_auth(self.auth_path)
            if not silent and (is_new or changed):
                action = "Auto-discovered new account" if is_new else "Auto-synced credentials for"
                label = identity.email or short_id(identity.user_id, 18)
                print(f"[{action}: {label}]")
            return identity, is_new, changed
        except Exception:
            return None

    def list_accounts(self, auto_sync: bool = True, silent: bool = True) -> list[dict[str, Any]]:
        if auto_sync:
            self.auto_sync_backups()
            self.auto_sync_current(silent=silent)

        current_key = self.get_current_identity_key()
        rows = self.store.account_rows()
        results: list[dict[str, Any]] = []
        for r in rows:
            p_label, p_used, p_reset, s_label, s_used, s_reset = get_window_display(r)
            is_current = (r["identity_key"] == current_key)
            r_keys = r.keys() if hasattr(r, "keys") else []
            results.append({
                "identity_key": r["identity_key"],
                "profile_id": r["profile_id"],
                "user_id": r["user_id"],
                "account_id": r["account_id"],
                "email": r["email"],
                "plan": r["plan"],
                "org_title": r["org_title"] if "org_title" in r_keys else None,
                "alias": r["alias"],
                "display_name": get_account_display_name(r),
                "is_current": is_current,
                "credential_status": r["credential_status"],
                "last_seen_at": r["last_seen_at"],
                "primary": {
                    "label": p_label,
                    "used_percent": p_used,
                    "remaining_percent": to_remaining(p_used),
                    "resets_at": p_reset,
                },
                "secondary": {
                    "label": s_label,
                    "used_percent": s_used,
                    "remaining_percent": to_remaining(s_used),
                    "resets_at": s_reset,
                },
                "reset_credits": r["reset_credits"] if ("reset_credits" in r_keys and r["reset_credits"] is not None) else None,
                "last_error": r["last_error"],
            })
        return results

    def get_account(self, target: str) -> dict[str, Any] | None:
        row = self.store.resolve_account(target)
        if not row:
            return None
        all_accounts = {a["identity_key"]: a for a in self.list_accounts(auto_sync=False)}
        return all_accounts.get(row["identity_key"])

    async def refresh_one(self, target: str, timeout: float = 30.0, quiet: bool = False) -> bool:
        row = self.store.resolve_account(target)
        if not row:
            raise CodexQError(f"Account '{target}' not found.")
        return await refresh_one_async(self.store, row, self.codex_bin, timeout=timeout, quiet=quiet)

    async def refresh_all(
        self,
        concurrency: int = 5,
        timeout: float = 30.0,
        quiet: bool = False,
        auto_sync: bool = True,
    ) -> list[dict[str, Any]]:
        if auto_sync:
            self.auto_sync_current(silent=quiet)

        rows = self.store.accounts_for_refresh()
        if not rows:
            return []

        sem = asyncio.Semaphore(max(1, concurrency))

        async def worker(r: sqlite3.Row) -> dict[str, Any]:
            async with sem:
                ok = await refresh_one_async(self.store, r, self.codex_bin, timeout=timeout, quiet=quiet)
                return {
                    "identity_key": r["identity_key"],
                    "profile_id": r["profile_id"],
                    "label": get_account_display_name(r),
                    "success": ok,
                }

        return await asyncio.gather(*(worker(r) for r in rows))

    def switch_account(
        self,
        target: str,
        dest_auth_path: Path | None = None,
        restart: bool = False,
    ) -> tuple[bool, str]:
        # Always auto-sync active login credentials first so no refreshed token is lost!
        self.auto_sync_current(silent=True)

        row = self.store.resolve_account(target)
        if not row:
            return False, f"Account '{target}' not found."
        profile_auth = self.store.profiles_dir / row["profile_id"] / "auth.json"
        if not profile_auth.exists():
            return False, f"Saved auth credentials for '{target}' do not exist."

        # Proactively renew destination account if token is expired or expiring soon
        try:
            profile_auth_val, _ = read_json_stable(profile_auth, retries=1)
            if is_access_token_expired(profile_auth_val, buffer_seconds=300):
                refresh_oauth_token_sync(profile_auth.parent)
                self.store.sync_profile_auth_metadata(row["identity_key"], profile_auth.parent)
        except Exception:
            pass

        dest = dest_auth_path or self.auth_path
        dest.parent.mkdir(parents=True, exist_ok=True)
        atomic_write(dest, profile_auth.read_bytes(), mode=0o600)
        label = get_account_display_name(row)
        msg = f"Switched active Codex account to: {label}"
        if restart:
            r_ok, r_msg = self.restart_codex(relaunch=True)
            msg += f" ({r_msg})"
        return True, msg

    def restart_codex(self, relaunch: bool = True, start_if_not_running: bool = True) -> tuple[bool, str]:
        """Terminates and optionally relaunches running Codex desktop apps and background daemons."""
        return restart_codex_system(
            relaunch=relaunch,
            codex_bin=self.codex_bin,
            start_if_not_running=start_if_not_running,
        )

    def set_alias(self, target: str, alias: str | None) -> bool:
        return self.store.set_alias(target, alias)

    def reset_all_aliases(self) -> int:
        return self.store.reset_all_aliases()

    def remove_account(self, target: str) -> bool:
        row = self.store.resolve_account(target)
        if not row:
            return False
        ident_key = row["identity_key"]
        # Also clean up historical backup dirs in ~/.codex/backups to prevent resurrection
        backup_dir = self.auth_path.parent / "backups"
        if backup_dir.exists():
            for auth_file in list(backup_dir.glob("*/auth.json")):
                try:
                    data, _ = read_json_stable(auth_file, retries=1)
                    if extract_identity(data).key == ident_key:
                        shutil.rmtree(auth_file.parent, ignore_errors=True)
                except Exception:
                    pass
        return self.store.remove_account(target)

    def list_trash(self) -> list[dict[str, Any]]:
        return self.store.list_trash()

    def restore_account(self, target: str) -> tuple[bool, str]:
        return self.store.restore_account(target)

    def purge_trash(self, target: str | None = None) -> int:
        return self.store.purge_trash(target)

    def get_history(self, target: str, limit: int = 50) -> list[dict[str, Any]]:
        row = self.store.resolve_account(target)
        if not row:
            raise CodexQError(f"Account '{target}' not found.")
        snaps = self.store.get_snapshots(row["identity_key"], limit=limit)
        return [dict(s) for s in snaps]

    def get_settings(self) -> dict[str, str]:
        return self.store.get_all_settings()

    def set_setting(self, key: str, value: str) -> None:
        self.store.set_setting(key, value)

    def list_alarms(self, target: str | None = None) -> list[dict[str, Any]]:
        ident_key = None
        if target:
            row = self.store.resolve_account(target)
            if not row:
                raise CodexQError(f"Account '{target}' not found.")
            ident_key = row["identity_key"]
        rows = self.store.list_alarms(ident_key)
        return [dict(r) for r in rows]

    def save_alarm(self, alarm_data: dict[str, Any]) -> dict[str, Any]:
        ident_key = alarm_data.get("identity_key")
        if not ident_key:
            target = alarm_data.get("target")
            if target:
                row = self.store.resolve_account(target)
                if row:
                    ident_key = row["identity_key"]
                    alarm_data["identity_key"] = ident_key
        if not ident_key:
            raise CodexQError("Alarm missing required 'identity_key'.")

        existing = self.store.list_alarms(ident_key)
        alarm_id = alarm_data.get("id")
        t_str = str(alarm_data.get("time_of_day", ""))
        min_int_hours = float(self.store.get_setting("warmup.min_interval_hours", "5") or 5)
        min_int_mins = int(min_int_hours * 60)
        ok, err = validate_alarm_intervals(existing, t_str, exclude_id=alarm_id, min_interval_minutes=min_int_mins)
        if not ok:
            raise CodexQError(err or "Alarm interval conflict")
        saved = self.store.save_alarm(alarm_data)
        return dict(saved)

    def delete_alarm(self, alarm_id: str) -> bool:
        return self.store.delete_alarm(alarm_id)

    async def warmup(
        self,
        target: str | None = None,
        model: str | None = None,
        prompt: str | None = None,
        timeout: float = 30.0,
        force: bool = False,
        quiet: bool = False,
    ) -> list[dict[str, Any]]:
        self.auto_sync_current(silent=True)
        if target:
            row = self.store.resolve_account(target)
            if not row:
                raise CodexQError(f"Account '{target}' not found.")
            targets = [row]
        else:
            current_key = self.get_current_identity_key()
            if current_key:
                row = self.store.resolve_account(current_key)
                targets = [row] if row else []
            else:
                rows = self.store.account_rows()
                targets = [rows[0]] if rows else []
        if not targets:
            raise CodexQError("No active account available for warmup.")

        results = []
        for r in targets:
            res = await warmup_account_async(
                self.store,
                r,
                codex_bin=self.codex_bin,
                model=model,
                prompt=prompt,
                timeout=timeout,
                force=force,
                quiet=quiet,
            )
            results.append(res)
        return results

    async def check_and_fire_alarms(
        self,
        now_dt: datetime | None = None,
    ) -> list[dict[str, Any]]:
        """Check all active alarms and trigger warmup if current time matches.

        Args:
            now_dt: Optional datetime to evaluate against (defaults to local now).

        Returns:
            A list of execution result dictionaries for any triggered alarms.
        """
        now = now_dt or datetime.now()
        current_hm = now.strftime("%H:%M")
        weekday = now.isoweekday()
        now_utc = datetime.now(timezone.utc)

        all_alarms = self.store.list_alarms()
        triggered_results = []

        for alm in all_alarms:
            if not alm["enabled"]:
                continue

            time_of_day = str(alm["time_of_day"]).strip()
            if time_of_day != current_hm:
                continue

            last_trig = alm["last_triggered_at"]
            if last_trig:
                try:
                    cleaned_ts = str(last_trig).replace("Z", "+00:00")
                    last_dt = datetime.fromisoformat(cleaned_ts)
                    if last_dt.tzinfo is None:
                        last_dt = last_dt.replace(tzinfo=timezone.utc)
                    if (now_utc - last_dt).total_seconds() < 75:
                        continue
                except Exception:
                    pass

            days_of_week = str(alm["days_of_week"] or "1,2,3,4,5").strip().lower()
            if days_of_week == "once":
                matches_day = True
            else:
                try:
                    allowed_days = [int(x.strip()) for x in days_of_week.split(",") if x.strip().isdigit()]
                    matches_day = weekday in allowed_days
                except Exception:
                    matches_day = False

            if not matches_day:
                continue

            ident_key = alm["identity_key"]
            model_override = alm["model_override"]
            prompt_override = alm["prompt_override"]

            try:
                res = await self.warmup(
                    target=ident_key,
                    model=model_override,
                    prompt=prompt_override,
                    force=False,
                    quiet=True,
                )
                item = res[0] if isinstance(res, list) and res else {}
                status = item.get("status", "success")
            except Exception as exc:
                status = "failed"
                item = {"status": "failed", "error": str(exc)}

            self.store.update_alarm_status(alm["id"], status=status)

            if days_of_week == "once":
                alm_dict = dict(alm)
                alm_dict["enabled"] = 0
                self.store.save_alarm(alm_dict)

            triggered_results.append({
                "alarm_id": alm["id"],
                "identity_key": ident_key,
                "status": status,
                "result": item,
                "auto_disabled": days_of_week == "once",
            })

        return triggered_results

    async def check_and_fire_auto_rollover(
        self,
        now_ts: float | None = None,
    ) -> list[dict[str, Any]]:
        """Check all eligible accounts for 5h quota restoration and trigger auto-rollover if weekly quota is available.

        Args:
            now_ts: Optional epoch timestamp to evaluate against (defaults to now).

        Returns:
            A list of execution result dictionaries for triggered auto-rollovers.
        """
        now = now_ts or time.time()
        raw_cfg = self.store._load_config()
        account_rollovers = raw_cfg.get("trigger", {}).get("account_rollovers", {})
        legacy_enabled = self.store.get_setting("warmup.auto_rollover_on_restored", "false").lower() == "true"

        if not account_rollovers and not legacy_enabled:
            return []

        legacy_scope = self.store.get_setting("warmup.auto_rollover_scope", "current").lower()
        try:
            legacy_threshold = float(self.store.get_setting("warmup.auto_rollover_weekly_threshold", "100.0") or 100.0)
        except (ValueError, TypeError):
            legacy_threshold = 100.0

        all_rows = self.store.account_rows()
        curr_key = self.get_current_identity_key()

        if not hasattr(self, "_auto_rollover_cooldown"):
            self._auto_rollover_cooldown = {}

        results = []
        for r in all_rows:
            ident_key = r["identity_key"]
            if r["credential_status"] == "reauth_required":
                continue

            # Resolve account-level config or legacy fallback
            if ident_key in account_rollovers:
                acc_cfg = account_rollovers[ident_key]
                if not acc_cfg.get("enabled", False):
                    continue
                min_remaining = float(acc_cfg.get("min_weekly_remaining", 0.0))
            elif legacy_enabled:
                if legacy_scope != "all" and ident_key != curr_key:
                    continue
                min_remaining = max(0.0, 100.0 - legacy_threshold)
            else:
                continue

            p_used = r["primary_used_percent"]
            p_resets = r["primary_resets_at"]

            if p_used is None and p_resets is None:
                continue

            is_active = (p_used is not None and p_resets is not None and p_used > 0 and p_resets > now)
            if is_active:
                continue

            s_used = r["secondary_used_percent"]
            if s_used is not None:
                s_rem = max(0.0, min(100.0, 100.0 - s_used))
                if min_remaining <= 0.0:
                    if s_rem <= 0.0:
                        continue
                else:
                    if s_rem < min_remaining:
                        continue

            last_attempt = self._auto_rollover_cooldown.get(ident_key, 0)
            if now - last_attempt < 900:
                continue

            self._auto_rollover_cooldown[ident_key] = now

            try:
                res = await self.warmup(
                    target=ident_key,
                    force=False,
                    quiet=True,
                )
                item = res[0] if isinstance(res, list) and res else {}
                status = item.get("status", "success")
                if status == "success":
                    try:
                        await self.refresh_one(ident_key, quiet=True)
                    except Exception:
                        pass
            except Exception as exc:
                status = "failed"
                item = {"status": "failed", "error": str(exc)}

            results.append({
                "identity_key": ident_key,
                "status": status,
                "result": item,
            })

        return results

    def get_account_rollover(self, identity_key: str) -> dict[str, Any]:
        """Get auto-rollover configuration for an account."""
        return self.store.get_account_rollover(identity_key)

    def set_account_rollover(
        self,
        identity_key: str,
        enabled: bool,
        min_weekly_remaining: float = 0.0,
    ) -> None:
        """Set auto-rollover configuration for an account."""
        self.store.set_account_rollover(identity_key, enabled, min_weekly_remaining)


# =====================================================================
# CLI Commands
# =====================================================================

def cmd_import(args: argparse.Namespace, client: CodexQ) -> int:
    """Import an auth.json file or profile directory into the account pool.

    Args:
        args: Parsed command-line arguments containing target path and formatting flags.
        client: Active CodexQ client instance.

    Returns:
        0 on success, or non-zero integer on failure.
    """
    target_path = Path(args.path).expanduser().resolve()
    if not target_path.exists():
        print(f"Error: Path '{args.path}' does not exist.", file=sys.stderr)
        return 1

    auth_file = target_path
    if target_path.is_dir():
        cand = target_path / "auth.json"
        if not cand.is_file():
            print(f"Error: No auth.json found in directory '{args.path}'.", file=sys.stderr)
            return 1
        auth_file = cand

    try:
        ident, is_new, cred_changed = client.store.ingest_auth(auth_file)
    except Exception as exc:
        print(f"Error importing auth file: {exc}", file=sys.stderr)
        return 1

    if getattr(args, "json", False):
        res = {
            "success": True,
            "identity_key": ident.key,
            "profile_id": ident.profile_id,
            "email": ident.email,
            "plan": ident.plan,
            "is_new": is_new,
            "credential_changed": cred_changed,
        }
        print(json.dumps(res, indent=2, ensure_ascii=False))
        return 0

    action = "Imported new account" if is_new else ("Updated credentials for" if cred_changed else "Account already up to date for")
    label = ident.email or ident.profile_id
    plan_str = f" [{ident.plan.upper()}]" if ident.plan else ""
    print(f"[OK] {action} '{label}'{plan_str} (Profile ID: {ident.profile_id})")
    return 0


def cmd_list(args: argparse.Namespace, client: CodexQ) -> int:
    is_json = getattr(args, "json", False)
    client.auto_sync_backups()
    client.auto_sync_current(silent=is_json)

    if is_json:
        print(json.dumps(client.list_accounts(auto_sync=False), indent=2, ensure_ascii=False))
        return 0

    rows = client.store.account_rows()
    if not rows:
        print("No accounts discovered yet. Log in to Codex CLI, then run: codexq list")
        return 0
    use_color = not getattr(args, "no_color", False) and sys.stdout.isatty()
    current_key = client.get_current_identity_key()
    print(render_table(rows, current_key=current_key, use_color=use_color))
    return 0


async def cmd_refresh(args: argparse.Namespace, client: CodexQ) -> int:
    concurrency = getattr(args, "concurrency", 5)
    is_json = getattr(args, "json", False)

    results = await client.refresh_all(
        concurrency=concurrency,
        timeout=args.timeout,
        quiet=is_json,
        auto_sync=True,
    )
    if not results:
        if is_json:
            print("[]")
        else:
            print("No accounts discovered yet. Log in to Codex CLI, then run: codexq list")
        return 0

    failed = sum(1 for r in results if not r["success"])
    if is_json:
        print(json.dumps(client.list_accounts(auto_sync=False), indent=2, ensure_ascii=False))
        return 0
    else:
        print()
        use_color = not getattr(args, "no_color", False) and sys.stdout.isatty()
        current_key = client.get_current_identity_key()
        print(render_table(client.store.account_rows(), current_key=current_key, use_color=use_color))
        return 0 if failed == 0 else 2


def cmd_switch(args: argparse.Namespace, client: CodexQ) -> int:
    ok, msg = client.switch_account(
        args.target,
        dest_auth_path=Path(args.auth_path).expanduser(),
        restart=getattr(args, "restart", False),
    )
    if ok:
        print(f"[OK] {msg}")
        return 0
    print(f"[FAIL] {msg}", file=sys.stderr)
    return 1


def cmd_restart(args: argparse.Namespace, client: CodexQ) -> int:
    relaunch = not getattr(args, "no_launch", False)
    ok, msg = client.restart_codex(relaunch=relaunch)
    if ok:
        print(f"[OK] {msg}")
        return 0
    print(f"[FAIL] {msg}", file=sys.stderr)
    return 1


def cmd_alias(args: argparse.Namespace, client: CodexQ) -> int:
    client.auto_sync_current(silent=True)
    target = (args.target or "").strip()
    is_reset = getattr(args, "reset", False)
    is_reset_all = getattr(args, "reset_all", False) or (target.lower() == "reset-all")

    if is_reset_all:
        count = client.reset_all_aliases()
        print(f"[OK] Reset all account aliases ({count} alias(es) restored to default).")
        return 0

    if not target:
        if is_reset:
            current_key = client.get_current_identity_key()
            if current_key:
                row = client.store.resolve_account(current_key)
                if row:
                    client.set_alias(row["identity_key"], None)
                    print(f"[OK] Alias for active account '{row['email'] or row['profile_id']}' reset to default email.")
                    return 0
        print("Error: account target is required (e.g. codexq alias <target> [new_alias] or codexq alias <target> --reset)", file=sys.stderr)
        return 1

    if is_reset:
        alias_val = None
    else:
        alias_val = args.alias.strip() if (args.alias and args.alias.strip()) else None

    ok = client.set_alias(target, alias_val)
    if ok:
        if alias_val:
            print(f"[OK] Alias for '{target}' set to '{alias_val}'.")
        else:
            print(f"[OK] Alias for '{target}' reset to default email.")
        return 0
    print(f"[FAIL] Account '{target}' not found.", file=sys.stderr)
    return 1


def cmd_remove(args: argparse.Namespace, client: CodexQ) -> int:
    client.auto_sync_current(silent=True)
    row = client.store.resolve_account(args.target)
    if not row:
        print(f"[FAIL] Account '{args.target}' not found.", file=sys.stderr)
        return 1

    label = row["alias"] or row["email"] or row["profile_id"]
    if not args.yes:
        confirm = input(f"Are you sure you want to remove account '{label}'? [y/N]: ").strip().lower()
        if confirm != "y":
            print("Cancelled.")
            return 0

    ok = client.remove_account(args.target)
    if ok:
        print(f"[OK] Account '{label}' moved to recycle bin.")
        return 0
    print(f"[FAIL] Failed to remove account '{label}'.", file=sys.stderr)
    return 1


def cmd_trash(args: argparse.Namespace, client: CodexQ) -> int:
    is_purge = getattr(args, "purge", False)
    is_json = getattr(args, "json", False)
    target = getattr(args, "target", None)

    if is_purge:
        count = client.purge_trash(target)
        if is_json:
            print(json.dumps({"success": True, "purged": count}))
        else:
            if target:
                if count > 0:
                    print(f"[OK] Permanently removed '{target}' from recycle bin.")
                else:
                    print(f"[FAIL] Account '{target}' not found in recycle bin.", file=sys.stderr)
                    return 1
            else:
                print(f"[OK] Emptied recycle bin ({count} account(s) permanently removed).")
        return 0

    trash_items = client.list_trash()
    if is_json:
        print(json.dumps(trash_items, indent=2, ensure_ascii=False))
        return 0

    if not trash_items:
        print("Recycle bin is empty. Removed accounts are kept here for safe recovery.")
        return 0

    print(f"Recycle Bin ({len(trash_items)} removed account(s)):")
    headers = ["#", "Display Name / Email", "Profile ID", "Plan", "Removed At"]
    rows = []
    for i, item in enumerate(trash_items, 1):
        name = item["display_name"] or item["email"] or item["profile_id"]
        p_id = item["profile_id"][:12] + "..." if len(item["profile_id"]) > 12 else item["profile_id"]
        plan = (item.get("plan") or "unknown").upper()
        rem_at = item["removed_at"]
        if "T" in rem_at:
            rem_at = rem_at.split(".")[0].replace("T", " ")
        rows.append([str(i), name, p_id, plan, rem_at])

    col_widths = [len(h) for h in headers]
    for r in rows:
        for i, val in enumerate(r):
            col_widths[i] = max(col_widths[i], len(val))

    fmt = "  ".join(f"{{:<{w}}}" for w in col_widths)
    print(fmt.format(*headers))
    print("  ".join("-" * w for w in col_widths))
    for r in rows:
        print(fmt.format(*r))
    print()
    print("Hint: Run 'codexq restore <email/target>' to restore an account.")
    return 0


def cmd_restore(args: argparse.Namespace, client: CodexQ) -> int:
    target = args.target.strip()
    if not target:
        print("Error: account target is required (e.g. codexq restore user@example.com)", file=sys.stderr)
        return 1

    ok, msg = client.restore_account(target)
    if ok:
        print(f"[OK] {msg}")
        return 0
    else:
        print(f"[FAIL] {msg}", file=sys.stderr)
        return 1



def cmd_history(args: argparse.Namespace, client: CodexQ) -> int:
    client.auto_sync_current(silent=True)
    target = (args.target or "").strip()
    if not target:
        current_key = client.get_current_identity_key()
        if current_key:
            row = client.store.resolve_account(current_key)
        else:
            rows = client.store.account_rows()
            row = rows[0] if rows else None
    else:
        row = client.store.resolve_account(target)

    if not row:
        target_name = target if target else "active account"
        print(f"Error: Account '{target_name}' not found.", file=sys.stderr)
        return 1

    snaps = client.store.get_snapshots(row["identity_key"], limit=args.limit)
    if getattr(args, "json", False):
        print(json.dumps([dict(s) for s in snaps], indent=2, ensure_ascii=False))
        return 0

    label = row["alias"] or row["email"] or row["profile_id"]
    print(f"Quota history for {label} (last {args.limit} records):")
    use_color = not getattr(args, "no_color", False) and sys.stdout.isatty()
    print(render_history_table(snaps, use_color=use_color))
    return 0


async def cmd_warmup(args: argparse.Namespace, client: CodexQ) -> int:
    is_json = getattr(args, "json", False)
    client.auto_sync_current(silent=True)

    if getattr(args, "all", False):
        rows = client.store.account_rows()
    elif args.target:
        row = client.store.resolve_account(args.target)
        if not row:
            print(f"codexq: account '{args.target}' not found", file=sys.stderr)
            return 2
        rows = [row]
    else:
        current_key = client.get_current_identity_key()
        if current_key:
            row = client.store.resolve_account(current_key)
            rows = [row] if row else []
        else:
            rows = client.store.account_rows()[:1]

    if not rows:
        print("codexq: no accounts available to warmup", file=sys.stderr)
        return 1

    results = []
    for r in rows:
        res = await warmup_account_async(
            client.store,
            r,
            codex_bin=client.codex_bin,
            model=args.model,
            prompt=args.prompt,
            force=getattr(args, "force", False),
            quiet=is_json,
        )
        results.append(res)

    if is_json:
        print(json.dumps(results, indent=2, ensure_ascii=False))
    return 0 if any(r.get("status") in ("success", "skipped") for r in results) else 1


def cmd_alarm(args: argparse.Namespace, client: CodexQ) -> int:
    is_json = getattr(args, "json", False)
    action = args.action or "list"

    if action == "list":
        target = getattr(args, "target", "")
        alarms = client.list_alarms(target=target if target else None)
        if is_json:
            print(json.dumps(alarms, indent=2, ensure_ascii=False))
            return 0
        if not alarms:
            print("No scheduled warmup alarms configured.")
            return 0
        print(f"{'ID':<16} {'ACCOUNT KEY':<22} {'TIME':<8} {'DAYS':<12} {'STATUS':<8} {'MODEL'}")
        print("-" * 75)
        for a in alarms:
            enabled_str = "ENABLED" if a["enabled"] else "DISABLED"
            model_str = a["model_override"] or "(default)"
            days_str = "ONCE" if a["days_of_week"] == "once" else a["days_of_week"]
            print(f"{a['id']:<16} {a['identity_key'][:20]:<22} {a['time_of_day']:<8} {days_str:<12} {enabled_str:<8} {model_str}")
        return 0

    elif action == "add":
        target = getattr(args, "target", "")
        time_str = getattr(args, "time", None)
        if not target or not time_str:
            print("codexq alarm add requires target and --time (HH:MM)", file=sys.stderr)
            return 2
        row = client.store.resolve_account(target)
        if not row:
            print(f"codexq: account '{target}' not found", file=sys.stderr)
            return 2
        alarm_data = {
            "identity_key": row["identity_key"],
            "time_of_day": time_str,
            "days_of_week": getattr(args, "days", "1,2,3,4,5"),
            "model_override": getattr(args, "model", None),
            "enabled": True,
        }
        try:
            saved = client.save_alarm(alarm_data)
            if is_json:
                print(json.dumps(saved, indent=2, ensure_ascii=False))
            else:
                label = get_account_display_name(row)
                print(f"[OK] Added alarm {saved['id']} for {label} at {time_str}")
            return 0
        except Exception as exc:
            print(f"codexq: {exc}", file=sys.stderr)
            return 1

    elif action == "remove":
        alarm_id = getattr(args, "id", None) or getattr(args, "target", None)
        if not alarm_id:
            print("codexq alarm remove requires --id or alarm id as target", file=sys.stderr)
            return 2
        ok = client.delete_alarm(alarm_id)
        if ok:
            print(f"[OK] Alarm '{alarm_id}' removed.")
            return 0
        else:
            print(f"codexq: alarm '{alarm_id}' not found", file=sys.stderr)
            return 1

    elif action == "toggle":
        alarm_id = getattr(args, "id", None) or getattr(args, "target", None)
        if not alarm_id:
            print("codexq alarm toggle requires --id or alarm id as target", file=sys.stderr)
            return 2
        alm = client.store.get_alarm(alarm_id)
        if not alm:
            print(f"codexq: alarm '{alarm_id}' not found", file=sys.stderr)
            return 1
        new_enabled = not bool(alm["enabled"])
        d = dict(alm)
        d["enabled"] = new_enabled
        client.store.save_alarm(d)
        print(f"[OK] Alarm '{alarm_id}' is now {'ENABLED' if new_enabled else 'DISABLED'}")
        return 0

    return 0


def file_fingerprint(path: Path) -> str | None:
    try:
        raw = path.read_bytes()
    except OSError:
        return None
    return hashlib.sha256(raw).hexdigest()


async def cmd_watch(args: argparse.Namespace, client: CodexQ) -> int:
    auth_path = Path(args.auth_path).expanduser()
    interval = max(0.5, float(args.interval))
    refresh_every = max(0.0, float(args.refresh_every))

    print(f"Watching {auth_path}")
    print(f"Poll interval: {interval:g}s")
    if refresh_every:
        print(f"Refresh all accounts every: {refresh_every:g}s")
    print("Press Ctrl+C to stop.")

    last_fp: str | None = None
    last_refresh_all = 0.0

    try:
        while True:
            fp = file_fingerprint(auth_path)
            if fp and fp != last_fp:
                await asyncio.sleep(min(0.35, interval))
                try:
                    res = client.auto_sync_current(silent=False)
                    if res:
                        identity, is_new, changed = res
                        if args.refresh_current:
                            row = client.store.resolve_account(identity.key)
                            if row:
                                await refresh_one_async(client.store, row, client.codex_bin, args.timeout)
                except Exception as exc:
                    print(f"[WARN] auto sync failed: {exc}", file=sys.stderr)
                last_fp = file_fingerprint(auth_path) or fp

            now = time.monotonic()
            if refresh_every and now - last_refresh_all >= refresh_every:
                await client.refresh_all(concurrency=5, timeout=args.timeout, quiet=True)
                last_refresh_all = now
                print(f"[{datetime.now().strftime('%H:%M:%S')}] refreshed all saved accounts")

            await asyncio.sleep(interval)
    except (asyncio.CancelledError, KeyboardInterrupt):
        print("\nStopped.")
        return 0


# =====================================================================
# Lightweight Native HTTP Server
# =====================================================================

async def handle_http_request(reader: asyncio.StreamReader, writer: asyncio.StreamWriter, client: CodexQ) -> None:
    try:
        req_line = await reader.readline()
        if not req_line:
            writer.close()
            return
        parts = req_line.decode("utf-8", errors="replace").strip().split()
        if len(parts) < 2:
            writer.close()
            return
        method, path = parts[0].upper(), parts[1]

        headers: dict[str, str] = {}
        while True:
            line = await reader.readline()
            if line in (b"\r\n", b"\n", b""):
                break
            h_parts = line.decode("utf-8", errors="replace").strip().split(":", 1)
            if len(h_parts) == 2:
                headers[h_parts[0].strip().lower()] = h_parts[1].strip()

        body = b""
        content_length = int(headers.get("content-length", 0))
        if content_length > 0:
            body = await reader.readexactly(content_length)

        def reply(status: int, data: Any, content_type: str = "application/json") -> None:
            resp_body = (json.dumps(data, ensure_ascii=False) if content_type == "application/json" else str(data)).encode("utf-8")
            status_text = {200: "OK", 400: "Bad Request", 404: "Not Found", 500: "Internal Server Error"}.get(status, "OK")
            head = (
                f"HTTP/1.1 {status} {status_text}\r\n"
                f"Content-Type: {content_type}; charset=utf-8\r\n"
                f"Content-Length: {len(resp_body)}\r\n"
                f"Access-Control-Allow-Origin: *\r\n"
                f"Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n"
                f"Access-Control-Allow-Headers: Content-Type\r\n"
                f"Connection: close\r\n\r\n"
            )
            writer.write(head.encode("ascii") + resp_body)

        if method == "OPTIONS":
            reply(200, {})
            await writer.drain()
            return

        pure_path = path.split("?")[0]

        if pure_path in ("/", "/api/accounts"):
            client.auto_sync_current(silent=True)
            reply(200, client.list_accounts(auto_sync=False))
        elif pure_path == "/api/refresh" and method == "POST":
            results = await client.refresh_all(concurrency=5, quiet=True, auto_sync=True)
            reply(200, {"results": results, "accounts": client.list_accounts(auto_sync=False)})
        elif pure_path == "/api/switch" and method == "POST":
            payload = json.loads(body.decode("utf-8")) if body else {}
            target = payload.get("target", "")
            restart = bool(payload.get("restart", False))
            ok, msg = client.switch_account(target, restart=restart)
            reply(200 if ok else 400, {"success": ok, "message": msg})
        elif pure_path == "/api/restart" and method == "POST":
            payload = json.loads(body.decode("utf-8")) if body else {}
            relaunch = bool(payload.get("relaunch", True))
            start_if_not_running = bool(payload.get("start_if_not_running", True))
            ok, msg = client.restart_codex(relaunch=relaunch, start_if_not_running=start_if_not_running)
            reply(200 if ok else 400, {"success": ok, "message": msg})
        elif pure_path == "/api/alias" and method == "POST":
            payload = json.loads(body.decode("utf-8")) if body else {}
            if payload.get("reset_all"):
                count = client.reset_all_aliases()
                reply(200, {"success": True, "reset_count": count})
            else:
                ok = client.set_alias(payload.get("target", ""), payload.get("alias", ""))
                reply(200 if ok else 400, {"success": ok})
        elif pure_path == "/api/trash" and method == "GET":
            reply(200, client.list_trash())
        elif pure_path == "/api/trash/restore" and method == "POST":
            payload = json.loads(body.decode("utf-8")) if body else {}
            target = payload.get("target", "")
            ok, msg = client.restore_account(target)
            reply(200 if ok else 400, {"success": ok, "message": msg})
        elif pure_path == "/api/trash/purge" and method == "POST":
            payload = json.loads(body.decode("utf-8")) if body else {}
            target = payload.get("target")
            count = client.purge_trash(target)
            reply(200, {"success": True, "purged": count})
        elif pure_path == "/api/health":
            reply(200, {"status": "ok", "version": APP_VERSION})
        else:
            reply(404, {"error": "not found"})

        await writer.drain()
    except Exception as exc:
        try:
            err_body = json.dumps({"error": str(exc)}).encode("utf-8")
            writer.write(b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\n\r\n" + err_body)
            await writer.drain()
        except Exception:
            pass
    finally:
        try:
            writer.close()
            await writer.wait_closed()
        except Exception:
            pass


async def cmd_serve(args: argparse.Namespace, client: CodexQ) -> int:
    host = getattr(args, "host", "127.0.0.1")
    port = getattr(args, "port", 8765)

    server = await asyncio.start_server(
        lambda r, w: handle_http_request(r, w, client),
        host,
        port,
    )
    addrs = ", ".join(str(sock.getsockname()) for sock in server.sockets)
    print(f"CodexQ API server running on http://{host}:{port} ({addrs})")
    print(f"- GET  /api/accounts")
    print(f"- POST /api/refresh")
    print(f"- POST /api/switch   (body: {{\"target\": \"<id/email/alias>\"}})")
    print(f"- POST /api/alias    (body: {{\"target\": \"...\", \"alias\": \"...\"}})")
    print("Press Ctrl+C to stop.")

    try:
        async with server:
            await server.serve_forever()
    except (asyncio.CancelledError, KeyboardInterrupt):
        print("\nServer stopped.")
        return 0


async def cmd_rpc(args: argparse.Namespace, client: CodexQ) -> int:
    """Run persistent JSON-RPC over stdio for GUI / host applications."""
    if sys.platform == "win32":
        try:
            sys.stdin.reconfigure(encoding="utf-8")
            sys.stdout.reconfigure(encoding="utf-8")
            sys.stderr.reconfigure(encoding="utf-8")
        except Exception:
            pass

    # Stream Isolation: Reserve real stdout exclusively for JSON-RPC messages.
    # Redirect global sys.stdout to sys.stderr so any stray print(...) from
    # libraries or methods won't pollute the RPC stream.
    rpc_stdout = sys.stdout
    sys.stdout = sys.stderr

    write_lock = asyncio.Lock()
    state_lock = asyncio.Lock()  # Serializes mutating operations
    active_tasks: set[asyncio.Task] = set()
    shutdown_requested = asyncio.Event()

    def read_line() -> str:
        return sys.stdin.readline()

    async def handle_request(raw_line: str) -> None:
        try:
            req = json.loads(raw_line)
        except Exception as exc:
            err_resp = {"jsonrpc": "2.0", "id": None, "error": f"Invalid JSON: {exc}"}
            async with write_lock:
                rpc_stdout.write(json.dumps(err_resp, ensure_ascii=False) + "\n")
                rpc_stdout.flush()
            return

        req_id = req.get("id")
        method = req.get("method")
        params = req.get("params") or {}

        try:
            if method == "ping":
                result = "pong"
            elif method == "shutdown":
                result = "ok"
                async def do_shutdown() -> None:
                    await asyncio.sleep(0.05)
                    # wait for any lingering active tasks up to 2.0s
                    lingering = [t for t in active_tasks if t is not asyncio.current_task()]
                    if lingering:
                        try:
                            await asyncio.wait(lingering, timeout=2.0)
                        except Exception:
                            pass
                    try:
                        rpc_stdout.flush()
                        sys.stderr.flush()
                    except Exception:
                        pass
                    os._exit(0)
                asyncio.create_task(do_shutdown())
            elif method == "list":
                client.auto_sync_current(silent=True)
                result = client.list_accounts(auto_sync=False)
            elif method == "refresh":
                concurrency = int(params.get("concurrency", 5))
                await client.refresh_all(
                    concurrency=concurrency,
                    timeout=getattr(args, "timeout", 20.0),
                    quiet=True,
                    auto_sync=True,
                )
                result = client.list_accounts(auto_sync=False)
            elif method == "switch":
                async with state_lock:
                    target = str(params.get("target", "")).strip()
                    restart = bool(params.get("restart", False))
                    dest_auth_path = Path(getattr(args, "auth_path", DEFAULT_AUTH_PATH)).expanduser()
                    ok, msg = client.switch_account(target, dest_auth_path=dest_auth_path, restart=restart)
                    if not ok:
                        raise CodexQError(msg)
                    result = msg
            elif method == "restart":
                async with state_lock:
                    relaunch = bool(params.get("relaunch", True))
                    start_if_not_running = bool(params.get("start_if_not_running", True))
                    ok, msg = client.restart_codex(relaunch=relaunch, start_if_not_running=start_if_not_running)
                    if not ok:
                        raise CodexQError(msg)
                    result = msg
            elif method == "set_alias":
                async with state_lock:
                    client.auto_sync_current(silent=True)
                    target = str(params.get("target", "")).strip()
                    alias = params.get("alias")
                    if alias is not None:
                        alias = str(alias).strip()
                        if not alias:
                            alias = None
                    ok = client.set_alias(target, alias)
                    if not ok:
                        raise CodexQError(f"Account '{target}' not found.")
                    result = f"Alias for '{target}' set to '{alias}'." if alias else f"Alias for '{target}' reset."
            elif method == "reset_all_aliases":
                async with state_lock:
                    count = client.reset_all_aliases()
                    result = f"Reset all account aliases ({count} restored)."
            elif method == "history":
                client.auto_sync_current(silent=True)
                target = str(params.get("target", "")).strip()
                limit = int(params.get("limit", 30))
                if not target:
                    current_key = client.get_current_identity_key()
                    if current_key:
                        row = client.store.resolve_account(current_key)
                    else:
                        rows = client.store.account_rows()
                        row = rows[0] if rows else None
                else:
                    row = client.store.resolve_account(target)

                if not row:
                    target_name = target if target else "active account"
                    raise CodexQError(f"Account '{target_name}' not found.")

                snaps = client.store.get_snapshots(row["identity_key"], limit=limit)
                result = [dict(s) for s in snaps]
            elif method == "remove":
                async with state_lock:
                    client.auto_sync_current(silent=True)
                    target = str(params.get("target", "")).strip()
                    ok = client.remove_account(target)
                    if not ok:
                        raise CodexQError(f"Failed to remove account '{target}'.")
                    result = f"Account '{target}' moved to recycle bin."
            elif method == "trash_list":
                result = client.list_trash()
            elif method == "trash_restore":
                async with state_lock:
                    target = str(params.get("target", "")).strip()
                    ok, msg = client.restore_account(target)
                    if not ok:
                        raise CodexQError(msg)
                    result = msg
            elif method == "trash_purge":
                async with state_lock:
                    target = params.get("target")
                    if target:
                        target = str(target).strip()
                    count = client.purge_trash(target)
                    result = f"Purged {count} account(s)."
            elif method == "get_app_settings":
                result = client.get_settings()
            elif method == "set_app_setting":
                async with state_lock:
                    key = str(params.get("key", "")).strip()
                    val = str(params.get("value", ""))
                    if not key:
                        raise CodexQError("Setting key cannot be empty.")
                    client.set_setting(key, val)
                    result = "ok"
            elif method == "list_account_alarms":
                target = params.get("target")
                if target:
                    target = str(target).strip()
                result = client.list_alarms(target=target)
            elif method == "save_account_alarm":
                async with state_lock:
                    alarm_data = dict(params.get("alarm") or params)
                    result = client.save_alarm(alarm_data)
            elif method == "delete_account_alarm":
                async with state_lock:
                    alarm_id = str(params.get("id") or params.get("alarm_id") or "").strip()
                    if not alarm_id:
                        raise CodexQError("Alarm id cannot be empty.")
                    ok = client.delete_alarm(alarm_id)
                    if not ok:
                        raise CodexQError(f"Alarm '{alarm_id}' not found.")
                    result = f"Alarm '{alarm_id}' deleted."
            elif method == "trigger_warmup":
                target = params.get("target")
                if target:
                    target = str(target).strip() or None
                model = params.get("model")
                if model:
                    model = str(model).strip() or None
                prompt = params.get("prompt")
                if prompt:
                    prompt = str(prompt).strip() or None
                force = bool(params.get("force", False))
                warmup_timeout = float(params.get("timeout", 90.0))
                result = await client.warmup(target=target, model=model, prompt=prompt, force=force, timeout=warmup_timeout)
            else:
                raise CodexQError(f"Unknown method: {method}")

            resp = {"jsonrpc": "2.0", "id": req_id, "result": result}
        except Exception as exc:
            resp = {"jsonrpc": "2.0", "id": req_id, "error": str(exc)}

        line_out = json.dumps(resp, ensure_ascii=False) + "\n"
        async with write_lock:
            rpc_stdout.write(line_out)
            rpc_stdout.flush()

    # Pre-warm accounts cache in the background so initial GUI queries are <0.5ms
    async def prewarm():
        try:
            client.auto_sync_current(silent=True)
            client.list_accounts(auto_sync=False)
        except Exception:
            pass
    asyncio.create_task(prewarm())

    # Background alarm scheduler loop: checks every 15s to trigger scheduled alarms
    async def alarm_scheduler():
        while not shutdown_requested.is_set():
            try:
                await client.check_and_fire_alarms()
            except Exception:
                pass
            try:
                await asyncio.sleep(15)
            except asyncio.CancelledError:
                break

    alarm_task = asyncio.create_task(alarm_scheduler())
    active_tasks.add(alarm_task)
    alarm_task.add_done_callback(active_tasks.discard)

    while not shutdown_requested.is_set():
        line = await asyncio.to_thread(read_line)
        if not line:
            # EOF reached (parent closed stdin) -> break to clean exit
            shutdown_requested.set()
            alarm_task.cancel()
            break

        line = line.strip()
        if not line:
            continue

        t = asyncio.create_task(handle_request(line))
        active_tasks.add(t)
        t.add_done_callback(active_tasks.discard)

    # Graceful shutdown: give in-flight active tasks up to 2.5s to finish
    if active_tasks:
        try:
            await asyncio.wait(active_tasks, timeout=2.5)
        except Exception:
            pass

    try:
        rpc_stdout.flush()
        sys.stderr.flush()
    except Exception:
        pass

    return 0


# =====================================================================
# Argument Parser & Entry Point
# =====================================================================

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="codexq",
        description="Universal multi-account Codex quota tracker and manager (asyncio powered).",
    )
    parser.add_argument(
        "-v",
        "--version",
        action="version",
        version=f"%(prog)s {__version__}",
    )
    parser.add_argument(
        "--data-dir",
        default=str(DEFAULT_DATA_DIR),
        help=f"storage directory (default: {DEFAULT_DATA_DIR})",
    )
    parser.add_argument(
        "--auth-path",
        default=str(DEFAULT_AUTH_PATH),
        help=f"current Codex auth.json (default: {DEFAULT_AUTH_PATH})",
    )
    parser.add_argument(
        "--codex-bin",
        default=DEFAULT_CODEX_BIN,
        help="Codex executable name/path (default: codex)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=30.0,
        help="app-server request timeout in seconds (default: 30)",
    )

    sub = parser.add_subparsers(dest="command", required=True)

    # list
    p_list = sub.add_parser("list", help="show all discovered accounts and latest quota (auto-syncs current)")
    p_list.add_argument("--json", action="store_true", help="output as JSON array")
    p_list.add_argument("--no-color", action="store_true", help="disable ANSI color highlights")

    # import
    p_import = sub.add_parser("import", help="import an auth.json file or folder into the account pool")
    p_import.add_argument("path", help="path to auth.json file or backup directory")
    p_import.add_argument("--json", action="store_true", help="output result as JSON")

    # refresh
    p_refresh = sub.add_parser("refresh", help="refresh quota for all saved accounts concurrently")
    p_refresh.add_argument("-c", "--concurrency", type=int, default=5, help="max concurrent refreshes (default: 5)")
    p_refresh.add_argument("--json", action="store_true", help="output updated accounts as JSON")
    p_refresh.add_argument("--no-color", action="store_true", help="disable ANSI color highlights")

    # switch
    p_switch = sub.add_parser("switch", help="switch active Codex login to target account")
    p_switch.add_argument("target", help="alias, email, or profile_id")
    p_switch.add_argument("-r", "--restart", action="store_true", help="restart running Codex desktop app and daemons after switching")

    # restart
    p_restart = sub.add_parser("restart", help="restart running Codex desktop app and background daemons")
    p_restart.add_argument("--no-launch", action="store_true", help="terminate processes without relaunching desktop app")

    # alias
    p_alias = sub.add_parser("alias", help="set or reset account aliases")
    p_alias.add_argument("target", nargs="?", default="", help="email, alias, or profile_id")
    p_alias.add_argument("alias", nargs="?", default="", help="friendly nickname/alias (omit or pass --reset to restore default)")
    p_alias.add_argument("--reset", action="store_true", help="reset this target account's alias back to default email")
    p_alias.add_argument("--reset-all", action="store_true", help="reset all accounts' aliases back to default email")

    # remove
    p_remove = sub.add_parser("remove", help="remove an account and move credentials to recycle bin")
    p_remove.add_argument("target", help="alias, email, or profile_id")
    p_remove.add_argument("-y", "--yes", action="store_true", help="skip confirmation prompt")

    # restore
    p_restore = sub.add_parser("restore", help="restore a previously removed account from recycle bin")
    p_restore.add_argument("target", help="alias, email, or profile_id")

    # trash
    p_trash = sub.add_parser("trash", help="view or empty the recycle bin")
    p_trash.add_argument("target", nargs="?", default=None, help="specific account to inspect/purge (optional)")
    p_trash.add_argument("--purge", action="store_true", help="permanently delete accounts in recycle bin")
    p_trash.add_argument("--json", action="store_true", help="output recycle bin items as JSON")

    # history
    p_history = sub.add_parser("history", help="inspect quota snapshots for an account (defaults to active account)")
    p_history.add_argument("target", nargs="?", default="", help="alias, email, or profile_id (defaults to current active account)")
    p_history.add_argument("-n", "--limit", type=int, default=30, help="number of snapshots to show (default: 30)")
    p_history.add_argument("--json", action="store_true", help="output history as JSON")
    p_history.add_argument("--no-color", action="store_true", help="disable ANSI color highlights")

    # watch
    p_watch = sub.add_parser("watch", help="watch current auth.json and automatically absorb credential changes")
    p_watch.add_argument("--interval", type=float, default=2.0, help="auth.json polling interval in seconds (default: 2)")
    p_watch.add_argument("--refresh-current", action="store_true", help="refresh quota immediately on change")
    p_watch.add_argument("--refresh-every", type=float, default=0.0, help="periodically refresh all accounts (seconds)")

    # warmup
    p_warmup = sub.add_parser("warmup", help="trigger lightweight warmup ping to advance 5h quota window")
    p_warmup.add_argument("target", nargs="?", default="", help="alias, email, or profile_id (defaults to current active account)")
    p_warmup.add_argument("-m", "--model", default=None, help="override model for warmup (defaults to stored default_model)")
    p_warmup.add_argument("-p", "--prompt", default=None, help="override prompt for warmup (default: ping)")
    p_warmup.add_argument("-f", "--force", action="store_true", help="force warmup even if account quota window is already active")
    p_warmup.add_argument("--all", action="store_true", help="trigger warmup across all saved accounts")
    p_warmup.add_argument("--json", action="store_true", help="output warmup results as JSON")

    # alarm
    p_alarm = sub.add_parser("alarm", help="manage scheduled warmup alarms for accounts")
    p_alarm.add_argument("action", choices=["list", "add", "remove", "toggle"], nargs="?", default="list", help="alarm action")
    p_alarm.add_argument("target", nargs="?", default="", help="target account alias, email, or profile_id")
    p_alarm.add_argument("--time", help="alarm time HH:MM (e.g. 08:00)")
    p_alarm.add_argument("--days", default="1,2,3,4,5", help="repeat frequency: 'once', '1,2,3,4,5', or '1,2,3,4,5,6,7' (default: 1,2,3,4,5)")
    p_alarm.add_argument("--model", default=None, help="model override for this alarm")
    p_alarm.add_argument("--id", help="alarm id to remove or toggle")
    p_alarm.add_argument("--json", action="store_true", help="output alarms as JSON")

    # serve
    p_serve = sub.add_parser("serve", help="run a local lightweight HTTP REST API daemon")
    p_serve.add_argument("--host", default="127.0.0.1", help="server bind host (default: 127.0.0.1)")
    p_serve.add_argument("-p", "--port", type=int, default=8765, help="server bind port (default: 8765)")

    # rpc
    sub.add_parser("rpc", help="run as a persistent JSON-RPC process over stdio")

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    client = CodexQ(
        data_dir=args.data_dir,
        auth_path=args.auth_path,
        codex_bin=args.codex_bin,
    )

    try:
        if args.command == "list":
            return cmd_list(args, client)
        if args.command == "import":
            return cmd_import(args, client)
        if args.command == "refresh":
            return asyncio.run(cmd_refresh(args, client))
        if args.command == "switch":
            return cmd_switch(args, client)
        if args.command == "restart":
            return cmd_restart(args, client)
        if args.command == "alias":
            return cmd_alias(args, client)
        if args.command == "remove":
            return cmd_remove(args, client)
        if args.command == "restore":
            return cmd_restore(args, client)
        if args.command == "trash":
            return cmd_trash(args, client)
        if args.command == "history":
            return cmd_history(args, client)
        if args.command == "warmup":
            return asyncio.run(cmd_warmup(args, client))
        if args.command == "alarm":
            return cmd_alarm(args, client)
        if args.command == "watch":
            return asyncio.run(cmd_watch(args, client))
        if args.command == "serve":
            return asyncio.run(cmd_serve(args, client))
        if args.command == "rpc":
            return asyncio.run(cmd_rpc(args, client))
        parser.error(f"unknown command: {args.command}")
        return 2
    except CodexQError as exc:
        print(f"codexq: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
