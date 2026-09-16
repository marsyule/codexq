import asyncio
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import AsyncMock, patch

import gc
import warnings
from datetime import datetime

from codexq import (
    CodexQ,
    CodexQError,
    Store,
    validate_alarm_intervals,
    warmup_account_async,
)


class TestWarmupScheduler(unittest.TestCase):
    def setUp(self):
        warnings.filterwarnings("ignore", category=ResourceWarning)
        self.tmp_dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.data_dir = Path(self.tmp_dir.name) / ".codexq"
        self.auth_path = Path(self.tmp_dir.name) / ".codex" / "auth.json"
        self.store = Store(self.data_dir)
        self.client = CodexQ(data_dir=self.data_dir, auth_path=self.auth_path)

    def tearDown(self):
        del self.store
        del self.client
        gc.collect()
        self.tmp_dir.cleanup()

    def test_default_settings_initialized(self):
        settings = self.store.get_all_settings()
        self.assertEqual(settings.get("general.locale"), "auto")
        self.assertEqual(settings.get("warmup.default_model"), "gpt-5.6-luna")
        presets = json.loads(settings.get("warmup.preset_models", "[]"))
        self.assertIn("gpt-5.6-luna", presets)
        self.assertIn("o3-mini", presets)
        self.assertEqual(settings.get("warmup.skip_if_active"), "true")

    def test_set_and_get_settings(self):
        self.store.set_setting("warmup.default_model", "o1-preview")
        self.assertEqual(self.store.get_setting("warmup.default_model"), "o1-preview")
        self.store.set_setting("general.locale", "zh-CN")
        self.assertEqual(self.store.get_setting("general.locale"), "zh-CN")

        # Update preset models
        new_presets = ["gpt-5.6-luna", "o1-preview", "custom-model"]
        self.store.set_setting("warmup.preset_models", json.dumps(new_presets))
        retrieved = json.loads(self.store.get_setting("warmup.preset_models"))
        self.assertEqual(retrieved, new_presets)

    def test_config_json_file_persistence_and_human_edit(self):
        # 1. Verify config.json exists and is valid JSON with indentation
        self.assertTrue(self.store.config_path.is_file())
        raw_text = self.store.config_path.read_text(encoding="utf-8")
        self.assertIn("  \"trigger\": {", raw_text)
        data = json.loads(raw_text)
        self.assertEqual(data["$schema_version"], 1)
        self.assertEqual(data["general"]["locale"], "auto")
        self.assertIn("auto_refresh", data)
        self.assertIn("trigger", data)

        # 2. Simulate a human editing config.json manually with Notepad
        data["trigger"]["default_model"] = "gpt-custom-human-edit"
        self.store.config_path.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")

        # 3. Verify store immediately reads the human edit without SQLite dependency
        self.assertEqual(self.store.get_setting("warmup.default_model"), "gpt-custom-human-edit")


    def test_alarm_crud_and_non_overlapping_validation(self):
        # Create dummy account
        now_iso = "2026-09-12T00:00:00Z"
        with self.store.connect() as con:
            con.execute(
                """
                INSERT INTO accounts(
                    identity_key, profile_id, user_id, account_id, email, first_seen_at,
                    last_seen_at, last_credential_update, credential_sha256, credential_path
                )
                VALUES ('user1\x1facc1', 'prof1', 'user1', 'acc1', 'test@example.com', ?, ?, ?, 'sha', 'path')
                """,
                (now_iso, now_iso, now_iso),
            )

        # 1. Add Alarm 1: 08:00
        alm1 = self.client.save_alarm({
            "identity_key": "user1\x1facc1",
            "time_of_day": "08:00",
            "days_of_week": "1,2,3,4,5",
            "enabled": True,
        })
        self.assertEqual(alm1["time_of_day"], "08:00")
        self.assertTrue(alm1["enabled"])

        # 2. Try adding Alarm 2 with conflict: 09:30 (only 1.5h apart, should fail < 5h)
        with self.assertRaises(CodexQError) as ctx:
            self.client.save_alarm({
                "identity_key": "user1\x1facc1",
                "time_of_day": "09:30",
                "enabled": True,
            })
        self.assertIn("间隔仅 1.5 小时", str(ctx.exception))

        # 3. Add Alarm 2 without conflict: 13:30 (5.5h apart, should succeed >= 5h)
        alm2 = self.client.save_alarm({
            "identity_key": "user1\x1facc1",
            "time_of_day": "13:30",
            "days_of_week": "1,2,3,4,5",
            "enabled": True,
            "model_override": "o3-mini",
        })
        self.assertEqual(alm2["time_of_day"], "13:30")
        self.assertEqual(alm2["model_override"], "o3-mini")

        # 4. List alarms for this account
        alarms = self.client.list_alarms("test@example.com")
        self.assertEqual(len(alarms), 2)
        times = [a["time_of_day"] for a in alarms]
        self.assertEqual(times, ["08:00", "13:30"])

        # 5. Delete Alarm
        ok = self.client.delete_alarm(alm1["id"])
        self.assertTrue(ok)
        remaining = self.client.list_alarms("test@example.com")
        self.assertEqual(len(remaining), 1)
        self.assertEqual(remaining[0]["id"], alm2["id"])

    def test_validate_alarm_intervals_helper(self):
        existing = [
            {"id": "a1", "time_of_day": "08:00", "enabled": True},
            {"id": "a2", "time_of_day": "14:00", "enabled": True},
        ]
        # 12:30 is 4.5h from 08:00 -> conflict
        ok, err = validate_alarm_intervals(existing, "12:30", min_interval_minutes=300)
        self.assertFalse(ok)

        # 13:00 is exactly 5.0h from 08:00, but 1.0h from 14:00 -> conflict with a2
        ok, err = validate_alarm_intervals(existing, "13:00", min_interval_minutes=300)
        self.assertFalse(ok)

        # 20:00 is 6.0h from 14:00 and 12h from 08:00 -> no conflict
        ok, err = validate_alarm_intervals(existing, "20:00", min_interval_minutes=300)
        self.assertTrue(ok)
        self.assertIsNone(err)

    def test_skip_if_active_warmup(self):
        # Setup account with active quota
        now_iso = "2026-09-12T00:00:00Z"
        ident_key = "user2\x1facc2"
        prof_dir = self.store.profiles_dir / "prof2"
        prof_dir.mkdir(parents=True, exist_ok=True)
        (prof_dir / "auth.json").write_text("{}", encoding="utf-8")

        with self.store.connect() as con:
            con.execute(
                """
                INSERT INTO accounts(
                    identity_key, profile_id, user_id, account_id, email, first_seen_at,
                    last_seen_at, last_credential_update, credential_sha256, credential_path
                )
                VALUES (?, 'prof2', 'user2', 'acc2', 'active@example.com', ?, ?, ?, 'sha', 'path')
                """,
                (ident_key, now_iso, now_iso, now_iso),
            )
            # Insert active quota that resets in 3 hours
            import time
            future_reset = int(time.time() + 3 * 3600)
            con.execute(
                """
                INSERT INTO quota_latest(
                    identity_key, limit_id, fetched_at, primary_used_percent,
                    primary_window_minutes, primary_resets_at, raw_json
                )
                VALUES (?, 'codex', ?, 45.0, 300, ?, '{}')
                """,
                (ident_key, now_iso, future_reset),
            )
            row = con.execute("SELECT * FROM accounts WHERE identity_key = ?", (ident_key,)).fetchone()

        # Run warmup without force -> should be skipped!
        res = asyncio.run(warmup_account_async(self.store, row, force=False))
        self.assertEqual(res.get("status"), "skipped")
        self.assertIn("already active", res.get("message", ""))

    def test_once_alarm_creation_and_auto_disable(self):
        # 1. Create dummy account
        now_iso = "2026-09-12T00:00:00Z"
        with self.store.connect() as con:
            con.execute(
                """
                INSERT INTO accounts(
                    identity_key, profile_id, user_id, account_id, email, first_seen_at,
                    last_seen_at, last_credential_update, credential_sha256, credential_path
                )
                VALUES ('user3\x1facc3', 'prof3', 'user3', 'acc3', 'once@example.com', ?, ?, ?, 'sha', 'path')
                """,
                (now_iso, now_iso, now_iso),
            )

        # 2. Add 'once' alarm at 09:00
        alm = self.client.save_alarm({
            "identity_key": "user3\x1facc3",
            "time_of_day": "09:00",
            "days_of_week": "once",
            "enabled": True,
        })
        self.assertEqual(alm["days_of_week"], "once")
        self.assertEqual(alm["enabled"], 1)

        # 3. Simulate scheduler tick at 08:59 (should not trigger)
        triggered_early = asyncio.run(
            self.client.check_and_fire_alarms(now_dt=datetime(2026, 9, 12, 8, 59))
        )
        self.assertEqual(len(triggered_early), 0)
        alm_saved = self.store.get_alarm(alm["id"])
        self.assertEqual(alm_saved["enabled"], 1)

        # 4. Simulate scheduler tick at 09:00 with mocked warmup
        mock_result = [{"status": "skipped", "message": "already active"}]
        with patch.object(self.client, "warmup", new=AsyncMock(return_value=mock_result)):
            triggered = asyncio.run(
                self.client.check_and_fire_alarms(now_dt=datetime(2026, 9, 12, 9, 0))
            )

        self.assertEqual(len(triggered), 1)
        self.assertEqual(triggered[0]["alarm_id"], alm["id"])
        self.assertTrue(triggered[0]["auto_disabled"])
        self.assertEqual(triggered[0]["status"], "skipped")

        # 5. Verify that in database, alarm is now automatically disabled (enabled = 0)
        alm_after = self.store.get_alarm(alm["id"])
        self.assertEqual(alm_after["enabled"], 0)
        self.assertEqual(alm_after["last_status"], "skipped")
        self.assertIsNotNone(alm_after["last_triggered_at"])

        # 6. Simulate scheduler tick again at 09:00 (should not fire because enabled = 0)
        triggered_again = asyncio.run(
            self.client.check_and_fire_alarms(now_dt=datetime(2026, 9, 12, 9, 0))
        )
        self.assertEqual(len(triggered_again), 0)

    def test_auto_rollover_on_restored(self):
        # 1. Verify settings read/write
        self.store.set_setting("warmup.auto_rollover_on_restored", "true")
        self.store.set_setting("warmup.auto_rollover_scope", "all")
        self.store.set_setting("warmup.auto_rollover_weekly_threshold", "98.0")
        self.assertEqual(self.store.get_setting("warmup.auto_rollover_on_restored"), "true")
        self.assertEqual(self.store.get_setting("warmup.auto_rollover_scope"), "all")
        self.assertEqual(self.store.get_setting("warmup.auto_rollover_weekly_threshold"), "98.0")

        # 2. Insert test account with 5h restored (used=0 or resets_at in the past) and weekly has remaining quota (used=20%)
        now_epoch = int(datetime.now().timestamp())
        now_iso = datetime.now().isoformat()
        with self.store.connect() as con:
            con.execute(
                """
                INSERT INTO accounts (
                    identity_key, profile_id, user_id, account_id, email,
                    first_seen_at, last_seen_at, last_credential_update, credential_sha256, credential_path
                )
                VALUES ('user4\x1facc4', 'prof4', 'user4', 'acc4', 'rollover@example.com', ?, ?, ?, 'sha', 'path')
                """,
                (now_iso, now_iso, now_iso),
            )
            con.execute(
                """
                INSERT INTO quota_latest (
                    identity_key, limit_id, fetched_at,
                    primary_used_percent, primary_window_minutes, primary_resets_at,
                    secondary_used_percent, secondary_window_minutes, secondary_resets_at, raw_json
                )
                VALUES ('user4\x1facc4', 'codex', ?, 0.0, 300, ?, 25.0, 10080, ?, '{}')
                """,
                (now_iso, now_epoch - 60, now_epoch + 86400 * 5),
            )

        # 3. Trigger auto-rollover
        mock_result = [{"status": "success", "message": "warmup sent"}]
        with patch.object(self.client, "warmup", new=AsyncMock(return_value=mock_result)), \
             patch.object(self.client, "refresh_one", new=AsyncMock(return_value=True)):
            results = asyncio.run(self.client.check_and_fire_auto_rollover(now_ts=now_epoch))

        self.assertEqual(len(results), 1)
        self.assertEqual(results[0]["identity_key"], "user4\x1facc4")
        self.assertEqual(results[0]["status"], "success")

        # 4. Anti-spam check: immediate second run within 900s should be debounced
        results2 = asyncio.run(self.client.check_and_fire_auto_rollover(now_ts=now_epoch + 10))
        self.assertEqual(len(results2), 0)

    def test_account_level_rollover(self):
        # Test get and set account rollover config
        self.client.set_account_rollover("user5\x1facc5", enabled=True, min_weekly_remaining=15.0)
        cfg = self.client.get_account_rollover("user5\x1facc5")
        self.assertTrue(cfg["enabled"])
        self.assertEqual(cfg["min_weekly_remaining"], 15.0)

        # Unconfigured account should return defaults
        def_cfg = self.client.get_account_rollover("user_unconfigured")
        self.assertFalse(def_cfg["enabled"])
        self.assertEqual(def_cfg["min_weekly_remaining"], 0.0)


if __name__ == "__main__":
    unittest.main()
