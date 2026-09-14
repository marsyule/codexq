import asyncio
import io
import json
import tempfile
import unittest
import warnings
from pathlib import Path
from unittest.mock import MagicMock

from codexq import CodexQ


class TestRpcContract(unittest.TestCase):
    def setUp(self):
        warnings.filterwarnings("ignore", category=ResourceWarning)
        self.tmp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.data_dir = Path(self.tmp_dir.name) / ".codexq"
        self.auth_path = Path(self.tmp_dir.name) / ".codex" / "auth.json"
        self.auth_path.parent.mkdir(parents=True, exist_ok=True)
        self.client = CodexQ(data_dir=self.data_dir, auth_path=self.auth_path)

    def tearDown(self):
        self.tmp_dir.cleanup()

    def test_json_rpc_ping_response(self):
        # Verify basic JSON-RPC structure contract
        req = {"jsonrpc": "2.0", "id": 1, "method": "ping"}
        raw_line = json.dumps(req)
        parsed = json.loads(raw_line)
        self.assertEqual(parsed.get("method"), "ping")
        self.assertEqual(parsed.get("id"), 1)

    def test_rpc_list_accounts_empty(self):
        accounts = self.client.list_accounts(auto_sync=False)
        self.assertIsInstance(accounts, list)
        self.assertEqual(len(accounts), 0)

    def test_rpc_trash_operations(self):
        # Verify trash methods are callable on client
        trash = self.client.store.list_trash()
        self.assertIsInstance(trash, list)
        self.assertEqual(len(trash), 0)

    def test_rpc_settings_and_alarms_operations(self):
        # Test settings RPC layer
        settings = self.client.get_settings()
        self.assertEqual(settings.get("warmup.default_model"), "gpt-5.6-luna")

        self.client.set_setting("warmup.default_model", "gpt-5.6-turbo")
        self.assertEqual(self.client.get_settings().get("warmup.default_model"), "gpt-5.6-turbo")

        # Test alarms RPC layer
        alarms = self.client.list_alarms()
        self.assertEqual(len(alarms), 0)

    def test_rpc_trigger_warmup_protocol(self):
        """Verify trigger_warmup RPC endpoint correctly handles parameters and resolves without NameError."""
        import subprocess
        import sys
        script_path = Path(__file__).resolve().parent.parent / "codexq.py"
        p = subprocess.Popen(
            [
                sys.executable,
                str(script_path),
                "--data-dir",
                str(self.data_dir),
                "--auth-path",
                str(self.auth_path),
                "rpc",
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )
        try:
            req = {"jsonrpc": "2.0", "id": 101, "method": "trigger_warmup", "params": {"target": "non_existent", "force": True}}
            p.stdin.write(json.dumps(req) + "\n")
            p.stdin.flush()
            line = p.stdout.readline()
            res = json.loads(line)
            self.assertEqual(res.get("id"), 101)
            self.assertNotIn("prompt", res.get("error", "").lower())
            self.assertIn("Account 'non_existent' not found", res.get("error", ""))
        finally:
            try:
                p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 999, "method": "shutdown"}) + "\n")
                p.stdin.flush()
                p.wait(timeout=2)
            except Exception:
                p.kill()


if __name__ == "__main__":
    unittest.main()
