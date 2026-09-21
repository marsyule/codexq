import gc
import json
import os
import shutil
import tempfile
import time
import unittest
from pathlib import Path

from codexq import (
    CodexQ,
    Store,
    DEFAULT_MIN_CONTEXT_WINDOW,
    build_parser,
    cmd_provider,
    mask_api_key,
    generate_model_catalog,
    update_codex_config_for_provider,
    lift_codex_config_provider,
    read_codex_config_active_provider,
)


class TestProviderIntegration(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.mkdtemp(prefix="codexq_test_prov_")
        self.data_dir = Path(self.temp_dir) / "data"
        self.auth_path = Path(self.temp_dir) / "codex" / "auth.json"
        self.auth_path.parent.mkdir(parents=True, exist_ok=True)
        self.client = CodexQ(data_dir=self.data_dir, auth_path=self.auth_path)

    def tearDown(self):
        gc.collect()
        time.sleep(0.05)
        shutil.rmtree(self.temp_dir, ignore_errors=True)

    def test_key_masking(self):
        """Verify API key masking retains recognizable edges without leaking the secret."""
        self.assertEqual(mask_api_key("sk-1234567890abcdef"), "sk-12****cdef")
        self.assertEqual(mask_api_key("short"), "****")
        self.assertEqual(mask_api_key(""), "")
        self.assertEqual(mask_api_key("Bearer_long_secret_key_1234"), "Bea****1234")

    def test_context_window_default_and_clamping(self):
        """Verify default minimum is 256k and values below 256k are clamped to 256k."""
        self.assertEqual(DEFAULT_MIN_CONTEXT_WINDOW, 256_000)
        prov_dir = self.data_dir / "providers" / "test_clamp"
        host_home = self.auth_path.parent

        # 1. Default when unspecified: 256,000
        p1 = generate_model_catalog(prov_dir, "test_clamp", ["m1"], codex_home=host_home)
        d1 = json.loads(p1.read_text(encoding="utf-8"))
        self.assertEqual(d1["models"][0]["context_window"], 256_000)
        self.assertEqual(d1["models"][0]["auto_compact_token_limit"], 217_600)

        # 2. Clamped when set below 256k (e.g. 128k -> 256k)
        p2 = generate_model_catalog(prov_dir, "test_clamp", ["m2"], context_window=128_000, codex_home=host_home)
        d2 = json.loads(p2.read_text(encoding="utf-8"))
        self.assertEqual(d2["models"][0]["context_window"], 256_000)

        # 3. Explicit larger value allowed (e.g. 1M)
        p3 = generate_model_catalog(prov_dir, "test_clamp", ["m3"], context_window=1_000_000, codex_home=host_home)
        d3 = json.loads(p3.read_text(encoding="utf-8"))
        self.assertEqual(d3["models"][0]["context_window"], 1_000_000)

        # 4. Host mirror must land under the provided codex home (sandbox), not the real ~/.codex
        self.assertTrue((host_home / "model-catalogs" / "codexq-test_clamp.json").is_file())

    def test_generate_model_catalog(self):
        """Verify model catalog is generated with per-model and provider context windows."""
        prov_dir = self.data_dir / "providers" / "test_p"
        catalog_path = generate_model_catalog(
            prov_dir,
            "test_p",
            ["deepseek-v4-flash", "step-5-preview"],
            context_window=512_000,
            model_context_windows={"deepseek-v4-flash": 1_000_000},
            codex_home=self.auth_path.parent,
        )
        self.assertTrue(catalog_path.is_file())
        data = json.loads(catalog_path.read_text(encoding="utf-8"))
        self.assertIn("models", data)
        self.assertEqual(len(data["models"]), 2)
        self.assertEqual(data["models"][0]["slug"], "deepseek-v4-flash")
        self.assertEqual(data["models"][0]["context_window"], 1_000_000)
        self.assertEqual(data["models"][0]["auto_compact_token_limit"], 850_000)
        self.assertEqual(data["models"][1]["slug"], "step-5-preview")
        self.assertEqual(data["models"][1]["context_window"], 512_000)
        self.assertEqual(data["models"][1]["auto_compact_token_limit"], 435_200)

    def test_store_provider_crud_and_key_safety(self):
        """Verify provider metadata in SQLite and keys isolated in sandbox files."""
        prov = self.client.store.upsert_provider(
            name="StepFun",
            base_url="https://api.stepfun.com/step_plan/v1",
            active_model="step-5-preview",
            models=["step-5-preview", "step-2-16k"],
            api_key="sk-stepfun-secret-token-123456",
            notes="Primary StepFun provider",
        )
        self.assertEqual(prov["id"], "stepfun")
        self.assertEqual(prov["name"], "StepFun")
        self.assertEqual(prov["active_model"], "step-5-preview")
        self.assertEqual(len(prov["models"]), 2)
        self.assertTrue(prov["key_masked"].startswith("sk-"))
        self.assertIn("****", prov["key_masked"])

        # Verify SQLite NEVER stores plaintext key
        with self.client.store.connect() as con:
            row = con.execute("SELECT * FROM providers WHERE id = 'stepfun'").fetchone()
            self.assertNotIn("sk-stepfun-secret", str(dict(row)))

        # Verify plaintext key is in file
        key = self.client.store.read_provider_key("stepfun")
        self.assertEqual(key, "sk-stepfun-secret-token-123456")

        # Test listing
        provs = self.client.store.list_providers()
        self.assertEqual(len(provs), 1)
        self.assertEqual(provs[0]["name"], "StepFun")

        # Test delete
        deleted = self.client.store.delete_provider("stepfun")
        self.assertTrue(deleted)
        self.assertIsNone(self.client.store.get_provider("stepfun"))
        self.assertFalse((self.data_dir / "providers" / "stepfun").exists())

    def test_lossless_toml_editing_preserves_comments_and_mcp(self):
        """Verify config.toml editing preserves existing settings, comments, and MCP blocks."""
        config_path = self.auth_path.parent / "config.toml"
        initial_toml = (
            "# Global User Configuration\n"
            'cli_auth_credentials_store = "file"\n'
            "\n"
            "# MCP Server Setup\n"
            "[mcp_servers.local_filesystem]\n"
            'command = "npx"\n'
            'args = ["-y", "@modelcontextprotocol/server-filesystem"]\n'
        )
        config_path.write_text(initial_toml, encoding="utf-8")

        catalog_path = self.data_dir / "providers" / "deepseek" / "models.json"
        update_codex_config_for_provider(
            config_path=config_path,
            provider_id="deepseek",
            name="DeepSeek",
            base_url="https://api.deepseek.com/v1",
            wire_api="responses",
            api_key="sk-ds-secret-key-9999",
            active_model="deepseek-chat",
            catalog_path=catalog_path,
        )

        updated_text = config_path.read_text(encoding="utf-8")
        self.assertIn('model = "deepseek-chat"', updated_text)
        self.assertIn('model_provider = "deepseek"', updated_text)
        self.assertIn("[model_providers.deepseek]", updated_text)
        self.assertIn('experimental_bearer_token = "sk-ds-secret-key-9999"', updated_text)
        # Verify MCP server and comments are preserved!
        self.assertIn("# Global User Configuration", updated_text)
        self.assertIn("[mcp_servers.local_filesystem]", updated_text)
        self.assertIn('cli_auth_credentials_store = "file"', updated_text)

        # Now test lifting provider
        lift_codex_config_provider(config_path)
        lifted_text = config_path.read_text(encoding="utf-8")
        self.assertNotIn('model_provider = "deepseek"', lifted_text)
        self.assertNotIn('model = "deepseek-chat"', lifted_text)
        self.assertIn("# Global User Configuration", lifted_text)
        self.assertIn("[mcp_servers.local_filesystem]", lifted_text)
        self.assertIn('cli_auth_credentials_store = "file"', lifted_text)

    def test_switch_to_provider_and_active_mode(self):
        """Verify full switch_to_provider and runtime mode reporting."""
        # Setup provider
        self.client.store.upsert_provider(
            name="StepFun",
            base_url="https://api.stepfun.com/step_plan/v1",
            active_model="step-5-preview",
            models=["step-5-preview"],
            api_key="sk-stepfun-key-1234",
        )

        ok, msg = self.client.switch_to_provider("stepfun", restart=False)
        self.assertTrue(ok)
        self.assertIn("StepFun", msg)

        # Check active runtime mode
        mode = self.client.get_active_runtime_mode()
        self.assertEqual(mode["mode"], "provider")
        self.assertEqual(mode["provider_id"], "stepfun")
        self.assertEqual(mode["active_model"], "step-5-preview")

        # Provider auth flows through config.toml (experimental_bearer_token); the active
        # auth.json must be left untouched so the official account switch stays lossless.
        config_path = self.auth_path.parent / "config.toml"
        config_text = config_path.read_text(encoding="utf-8")
        self.assertIn('model_provider = "stepfun"', config_text)
        self.assertIn("[model_providers.stepfun]", config_text)
        self.assertIn('experimental_bearer_token = "sk-stepfun-key-1234"', config_text)
        # The unsupported root-level catalog key must never be written.
        self.assertNotIn("model_catalog_json", config_text)
        self.assertFalse(self.auth_path.exists())

        # Catalog mirror must be isolated under the configured codex home.
        self.assertTrue((self.auth_path.parent / "model-catalogs" / "codexq-stepfun.json").is_file())

    def test_cli_provider_add_supports_context_window(self):
        """Verify `codexq provider add --context-window` plumbs and clamps the override."""
        import contextlib
        import io

        def run_add(name: str, context_window: str) -> dict:
            args = build_parser().parse_args([
                "provider", "add", name,
                "--base-url", "https://api.example.com/v1",
                "--key", "sk-cli-context-1234",
                "--model", "cli-model",
                "--context-window", context_window,
            ])
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(cmd_provider(args, self.client), 0)
            return self.client.store.get_provider(name.lower().replace(" ", "_"))

        prov = run_add("CLI Context", "512000")
        self.assertEqual(prov["context_window"], 512_000)

        # Values below the 256k floor must be clamped upward.
        clamped = run_add("CLI Clamp", "128000")
        self.assertEqual(clamped["context_window"], 256_000)

    def test_lift_third_party_provider_purges_table_and_secret(self):
        """Case 1: switching a third-party provider back to official removes table and key."""
        config_path = self.auth_path.parent / "config.toml"
        config_path.write_text(
            "# user config\n"
            'cli_auth_credentials_store = "file"\n'
            "\n"
            'model = "deepseek-chat"\n'
            'model_provider = "deepseek"\n'
            "\n"
            "[model_providers.deepseek]\n"
            'name = "DeepSeek"\n'
            'base_url = "https://api.deepseek.com/v1"\n'
            'wire_api = "responses"\n'
            'experimental_bearer_token = "sk-secret"\n'
            "\n"
            "[mcp_servers.local_filesystem]\n"
            'command = "npx"\n',
            encoding="utf-8",
        )

        lift_codex_config_provider(config_path)

        text = config_path.read_text(encoding="utf-8")
        self.assertNotIn('model_provider = "deepseek"', text)
        self.assertNotIn('model = "deepseek-chat"', text)
        self.assertNotIn("[model_providers.deepseek]", text)
        self.assertNotIn("experimental_bearer_token", text)
        self.assertNotIn("sk-secret", text)
        # Unrelated user configuration must survive intact.
        self.assertIn('cli_auth_credentials_store = "file"', text)
        self.assertIn("[mcp_servers.local_filesystem]", text)
        self.assertIn("# user config", text)

    def test_lift_preserves_other_unrelated_providers(self):
        """Case 2: only the active provider table is removed; siblings are preserved."""
        config_path = self.auth_path.parent / "config.toml"
        config_path.write_text(
            'model = "deepseek-chat"\n'
            'model_provider = "deepseek"\n'
            "\n"
            "[model_providers.deepseek]\n"
            'experimental_bearer_token = "sk-deepseek"\n'
            "\n"
            "[model_providers.openrouter]\n"
            'experimental_bearer_token = "sk-openrouter"\n',
            encoding="utf-8",
        )

        lift_codex_config_provider(config_path)

        text = config_path.read_text(encoding="utf-8")
        self.assertNotIn("[model_providers.deepseek]", text)
        self.assertNotIn("sk-deepseek", text)
        # The unrelated provider must be preserved; never delete the whole [model_providers].
        self.assertIn("[model_providers.openrouter]", text)
        self.assertIn('experimental_bearer_token = "sk-openrouter"', text)

    def test_lift_keeps_official_openai_config(self):
        """Case 3: user-authored official routing must not be mis-pruned."""
        config_path = self.auth_path.parent / "config.toml"
        config_path.write_text(
            'model = "gpt-5.6-codex"\n'
            'model_provider = "openai"\n',
            encoding="utf-8",
        )

        lift_codex_config_provider(config_path)

        text = config_path.read_text(encoding="utf-8")
        self.assertIn('model = "gpt-5.6-codex"', text)
        self.assertIn('model_provider = "openai"', text)

    def test_lift_always_purges_legacy_model_catalog_json(self):
        """Case 4: the unsupported root-level model_catalog_json is always removed."""
        provider_config = self.auth_path.parent / "provider.toml"
        provider_config.write_text(
            'model = "deepseek-chat"\n'
            'model_provider = "deepseek"\n'
            'model_catalog_json = "/tmp/catalog.json"\n'
            "\n"
            "[model_providers.deepseek]\n"
            'experimental_bearer_token = "sk-deepseek"\n',
            encoding="utf-8",
        )
        lift_codex_config_provider(provider_config)
        self.assertNotIn("model_catalog_json", provider_config.read_text(encoding="utf-8"))

        official_config = self.auth_path.parent / "official.toml"
        official_config.write_text(
            'model = "gpt-5.6-codex"\n'
            'model_catalog_json = "/tmp/legacy.json"\n',
            encoding="utf-8",
        )
        lift_codex_config_provider(official_config)
        official_text = official_config.read_text(encoding="utf-8")
        self.assertNotIn("model_catalog_json", official_text)
        self.assertIn('model = "gpt-5.6-codex"', official_text)


if __name__ == "__main__":
    unittest.main()
