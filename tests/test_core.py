import base64
import gc
import json
import os
import shutil
import tempfile
import time
import unittest
import warnings
from pathlib import Path

import codexq
from codexq import CodexQ, CodexQError, Identity, Store, decode_jwt_payload, extract_identity


class TestIdentityAndJWT(unittest.TestCase):
    def test_identity_key_and_profile_id(self):
        ident = Identity(
            user_id="user-12345",
            account_id="acc-67890",
            email="dev@example.com",
            plan="plus",
        )
        self.assertEqual(ident.key, "user-12345\x1facc-67890")
        self.assertEqual(len(ident.profile_id), 20)
        # Check determinism
        ident2 = Identity(
            user_id="user-12345",
            account_id="acc-67890",
            email="another@example.com",
            plan="free",
        )
        self.assertEqual(ident.profile_id, ident2.profile_id)

    def test_decode_jwt_payload_valid(self):
        payload = {"sub": "123", "email": "test@example.com"}
        encoded = base64.urlsafe_b64encode(json.dumps(payload).encode("utf-8")).decode("ascii").rstrip("=")
        jwt_token = f"header.{encoded}.signature"
        decoded = decode_jwt_payload(jwt_token)
        self.assertEqual(decoded["sub"], "123")
        self.assertEqual(decoded["email"], "test@example.com")

    def test_decode_jwt_payload_invalid(self):
        with self.assertRaises(CodexQError):
            decode_jwt_payload("invalid_token_without_dots")
        with self.assertRaises(CodexQError):
            decode_jwt_payload("part1.part2")

    def test_extract_identity_from_agent_identity(self):
        mock_auth = {
            "agent_identity": {
                "chatgpt_user_id": "u-abc",
                "account_id": "a-xyz",
                "email": "agent@test.com",
                "plan_type": "team",
            }
        }
        ident = extract_identity(mock_auth)
        self.assertEqual(ident.user_id, "u-abc")
        self.assertEqual(ident.account_id, "a-xyz")
        self.assertEqual(ident.email, "agent@test.com")
        self.assertEqual(ident.plan, "team")

    def test_extract_identity_missing_ids(self):
        with self.assertRaises(CodexQError):
            extract_identity({"agent_identity": {"email": "no_ids@test.com"}})


class TestStoreSandbox(unittest.TestCase):
    def setUp(self):
        warnings.filterwarnings("ignore", category=ResourceWarning)
        self.tmp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.data_dir = Path(self.tmp_dir.name) / ".codexq"
        self.store = Store(self.data_dir)

    def tearDown(self):
        del self.store
        gc.collect()
        self.tmp_dir.cleanup()

    def _create_mock_auth_file(self, user_id: str, account_id: str, email: str, plan: str = "plus") -> Path:
        mock_file = Path(self.tmp_dir.name) / f"auth_{user_id}.json"
        data = {
            "tokens": {
                "access_token": "secret_access_token_should_never_leak_to_sqlite",
                "refresh_token": "secret_refresh_token",
            },
            "agent_identity": {
                "chatgpt_user_id": user_id,
                "account_id": account_id,
                "email": email,
                "plan_type": plan,
            },
        }
        mock_file.write_text(json.dumps(data), encoding="utf-8")
        return mock_file

    def test_schema_initialization(self):
        with self.store.connect() as con:
            tables = [r[0] for r in con.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall()]
            self.assertIn("accounts", tables)
            self.assertIn("quota_latest", tables)
            self.assertIn("quota_snapshots", tables)
            self.assertIn("removed_accounts", tables)

    def test_ingest_auth_and_token_safety(self):
        auth_file = self._create_mock_auth_file("user-1", "acc-1", "user1@example.com")
        ident, is_new, cred_changed = self.store.ingest_auth(auth_file)
        self.assertTrue(is_new)
        self.assertTrue(cred_changed)

        profile_dir = self.store.profile_dir(ident)
        self.assertTrue(profile_dir.exists())
        self.assertTrue((profile_dir / "auth.json").exists())
        self.assertTrue((profile_dir / "config.toml").exists())

        # Verify config.toml forces file store
        config_content = (profile_dir / "config.toml").read_text(encoding="utf-8")
        self.assertIn('cli_auth_credentials_store = "file"', config_content)

        # Invariant check: SQLite must NOT store secret plaintext token!
        with self.store.connect() as con:
            row = con.execute("SELECT * FROM accounts WHERE identity_key=?", (ident.key,)).fetchone()
            self.assertIsNotNone(row)
            row_dict = dict(row)
            # Ensure none of the columns contain the secret token
            for col, val in row_dict.items():
                self.assertNotIn("secret_access_token", str(val))

    def test_alias_management(self):
        auth_file = self._create_mock_auth_file("user-2", "acc-2", "user2@example.com")
        ident, _, _ = self.store.ingest_auth(auth_file)

        # Set alias
        self.store.set_alias(ident.key, "primary-work")
        row = self.store.resolve_account(ident.key)
        self.assertIsNotNone(row)
        self.assertEqual(row["alias"], "primary-work")

        # Reset alias
        self.store.reset_all_aliases()
        row_after = self.store.resolve_account(ident.key)
        self.assertIsNone(row_after["alias"])

    def test_soft_delete_and_restore(self):
        auth_file = self._create_mock_auth_file("user-3", "acc-3", "user3@example.com")
        ident, _, _ = self.store.ingest_auth(auth_file)

        # Move to trash
        self.store.remove_account(ident.key)
        self.assertIsNone(self.store.resolve_account(ident.key))

        trash = self.store.list_trash()
        self.assertTrue(any(t["identity_key"] == ident.key for t in trash))

        # Restore from trash
        ok, msg = self.store.restore_account(ident.key)
        self.assertTrue(ok)
        self.assertIsNotNone(self.store.resolve_account(ident.key))

        # Purge
        self.store.remove_account(ident.key)
        self.store.purge_trash(ident.key)
        trash_after = self.store.list_trash()
        self.assertFalse(any(t["identity_key"] == ident.key for t in trash_after))


class TestCodexQController(unittest.TestCase):
    def setUp(self):
        warnings.filterwarnings("ignore", category=ResourceWarning)
        self.tmp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.data_dir = Path(self.tmp_dir.name) / ".codexq"
        self.auth_path = Path(self.tmp_dir.name) / ".codex" / "auth.json"
        self.auth_path.parent.mkdir(parents=True, exist_ok=True)
        self.client = CodexQ(data_dir=self.data_dir, auth_path=self.auth_path)

    def tearDown(self):
        del self.client
        gc.collect()
        self.tmp_dir.cleanup()

    def test_auto_sync_on_empty(self):
        accounts = self.client.list_accounts(auto_sync=True)
        self.assertEqual(len(accounts), 0)

    def test_auto_sync_with_active_login(self):
        # Place a mock active auth.json
        active_auth = {
            "tokens": {"access_token": "active_token"},
            "agent_identity": {
                "chatgpt_user_id": "active-user",
                "account_id": "active-acc",
                "email": "active@domain.com",
                "plan_type": "pro",
            },
        }
        self.auth_path.write_text(json.dumps(active_auth), encoding="utf-8")

        accounts = self.client.list_accounts(auto_sync=True)
        self.assertEqual(len(accounts), 1)
        self.assertEqual(accounts[0]["email"], "active@domain.com")
        self.assertTrue(accounts[0]["is_current"])
        self.assertEqual(accounts[0]["plan"], "pro")


    def test_switch_account_without_restart(self):
        from unittest.mock import patch
        # Setup an account
        auth_data = {
            "tokens": {"access_token": "token-1"},
            "agent_identity": {
                "chatgpt_user_id": "u-test-switch",
                "account_id": "a-test-switch",
                "email": "switch@domain.com",
                "plan_type": "plus",
            },
        }
        self.auth_path.write_text(json.dumps(auth_data), encoding="utf-8")
        self.client.auto_sync_current(silent=True)

        with patch.object(self.client, "restart_codex") as mock_restart:
            ok, msg = self.client.switch_account("switch@domain.com", restart=False)
            self.assertTrue(ok)
            mock_restart.assert_not_called()

    def test_restart_codex_not_running_start_if_not_running(self):
        from unittest.mock import patch, MagicMock
        with patch("codexq.subprocess.run") as mock_run, \
             patch("codexq.os.startfile", create=True) as mock_startfile:
            # Simulate powershell returns empty (no processes found)
            mock_res = MagicMock()
            mock_res.stdout = ""
            mock_run.return_value = mock_res

            # When start_if_not_running=True (default)
            ok, msg = codexq.restart_codex_system(relaunch=True, start_if_not_running=True)
            self.assertTrue(ok)
            self.assertIn("已启动", msg)
            mock_startfile.assert_called_once()

    def test_restart_codex_not_running_no_start(self):
        from unittest.mock import patch, MagicMock
        with patch("codexq.subprocess.run") as mock_run, \
             patch("codexq.os.startfile", create=True) as mock_startfile:
            mock_res = MagicMock()
            mock_res.stdout = ""
            mock_run.return_value = mock_res

            # When start_if_not_running=False
            ok, msg = codexq.restart_codex_system(relaunch=True, start_if_not_running=False)
            self.assertTrue(ok)
            self.assertIn("后台服务", msg)
            mock_startfile.assert_not_called()

    def test_auto_sync_backups_absorbing_newer_credentials(self):
        # 1. Start with initial active login
        initial_auth = {
            "tokens": {"access_token": "token-v1"},
            "agent_identity": {
                "chatgpt_user_id": "u-backup-test",
                "account_id": "a-backup-test",
                "email": "backup@domain.com",
                "plan_type": "plus",
            },
            "last_refresh": "2026-09-01T00:00:00Z",
        }
        self.auth_path.write_text(json.dumps(initial_auth), encoding="utf-8")
        self.client.auto_sync_current(silent=True)

        accs = self.client.list_accounts(auto_sync=False)
        self.assertEqual(len(accs), 1)

        # 2. Add an older backup - should NOT overwrite
        backups_dir = self.auth_path.parent / "backups"
        old_b = backups_dir / "backup_old"
        old_b.mkdir(parents=True, exist_ok=True)
        old_auth = {
            "tokens": {"access_token": "token-v0"},
            "agent_identity": {
                "chatgpt_user_id": "u-backup-test",
                "account_id": "a-backup-test",
                "email": "backup@domain.com",
                "plan_type": "plus",
            },
            "last_refresh": "2026-08-01T00:00:00Z",
        }
        (old_b / "auth.json").write_text(json.dumps(old_auth), encoding="utf-8")

        imported = self.client.auto_sync_backups()
        self.assertEqual(imported, 0)

        # 3. Add a newer backup - MUST overwrite and update credentials
        new_b = backups_dir / "backup_new"
        new_b.mkdir(parents=True, exist_ok=True)
        new_auth = {
            "tokens": {"access_token": "token-v2-renewed"},
            "agent_identity": {
                "chatgpt_user_id": "u-backup-test",
                "account_id": "a-backup-test",
                "email": "backup@domain.com",
                "plan_type": "team",
            },
            "last_refresh": "2026-09-14T10:00:00Z",
        }
        (new_b / "auth.json").write_text(json.dumps(new_auth), encoding="utf-8")

        imported = self.client.auto_sync_backups()
        self.assertEqual(imported, 1)

        accs_after = self.client.list_accounts(auto_sync=False)
        self.assertEqual(accs_after[0]["plan"], "team")

    def test_cmd_import_auth_file(self):
        # Create an external auth file in a separate directory
        ext_dir = Path(self.tmp_dir.name) / "external_import"
        ext_dir.mkdir(parents=True, exist_ok=True)
        ext_auth = {
            "tokens": {"access_token": "token-imported"},
            "agent_identity": {
                "chatgpt_user_id": "u-imported",
                "account_id": "a-imported",
                "email": "imported@domain.com",
                "plan_type": "pro",
            },
            "last_refresh": "2026-09-14T12:00:00Z",
        }
        auth_file = ext_dir / "auth.json"
        auth_file.write_text(json.dumps(ext_auth), encoding="utf-8")

        # Test importing via CLI function
        import argparse
        args = argparse.Namespace(path=str(auth_file), json=True)
        rc = codexq.cmd_import(args, self.client)
        self.assertEqual(rc, 0)

        # Verify account was ingested into account store
        accs = self.client.list_accounts(auto_sync=False)
        found = next((a for a in accs if a["email"] == "imported@domain.com"), None)
        self.assertIsNotNone(found)
        self.assertEqual(found["plan"], "pro")


class TestAuthValidationAndExpiry(unittest.TestCase):
    def test_is_auth_newer_or_equal(self):
        older = {"last_refresh": "2026-09-01T08:00:00Z"}
        newer = {"last_refresh": "2026-09-14T10:00:00Z"}
        self.assertTrue(codexq.is_auth_newer_or_equal(newer, older))
        self.assertFalse(codexq.is_auth_newer_or_equal(older, newer))
        self.assertTrue(codexq.is_auth_newer_or_equal(newer, newer))

    def test_is_access_token_expired(self):
        now = time.time()
        import base64
        # Expired token
        p_expired = base64.urlsafe_b64encode(json.dumps({"exp": now - 100}).encode("utf-8")).decode("ascii").rstrip("=")
        jwt_expired = f"eyJhbGciOiJub25lIn0.{p_expired}.sig"
        self.assertTrue(codexq.is_access_token_expired({"tokens": {"access_token": jwt_expired}}, 300))

        # Valid token
        p_valid = base64.urlsafe_b64encode(json.dumps({"exp": now + 50000}).encode("utf-8")).decode("ascii").rstrip("=")
        jwt_valid = f"eyJhbGciOiJub25lIn0.{p_valid}.sig"
        self.assertFalse(codexq.is_access_token_expired({"tokens": {"access_token": jwt_valid}}, 300))

        # Expiring within buffer (200s < 300s)
        p_buf = base64.urlsafe_b64encode(json.dumps({"exp": now + 200}).encode("utf-8")).decode("ascii").rstrip("=")
        jwt_buf = f"eyJhbGciOiJub25lIn0.{p_buf}.sig"
        self.assertTrue(codexq.is_access_token_expired({"tokens": {"access_token": jwt_buf}}, 300))

        # API key mode
        self.assertFalse(codexq.is_access_token_expired({"auth_mode": "api_key"}, 300))


if __name__ == "__main__":
    unittest.main()
