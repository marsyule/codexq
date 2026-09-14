//! Account alarm scheduling, 5-hour interval validation, and background runner.

use std::time::Duration;
use chrono::{Datelike, Local, Timelike};
use rusqlite::params;
use serde_json::Value;

use super::db::{get_connection, resolve_account, AccountAlarm};
use super::warmup::trigger_warmup;

/// Parses `"HH:MM"` into minutes from midnight (0..1439).
fn parse_time_to_minutes(t_str: &str) -> Result<i64, String> {
    let parts: Vec<&str> = t_str.trim().split(':').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid time format: '{t_str}'. Expected HH:MM."));
    }
    let h: i64 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid hour: {}", parts[0]))?;
    let m: i64 = parts[1]
        .parse()
        .map_err(|_| format!("Invalid minute: {}", parts[1]))?;
    if !(0..=23).contains(&h) || !(0..=59).contains(&m) {
        return Err(format!("Time out of range: {h:02}:{m:02}"));
    }
    Ok(h * 60 + m)
}

/// Validates that a candidate alarm time does not overlap within `min_interval_minutes`
/// with any other active alarm for the same account.
pub fn validate_alarm_intervals(
    existing_alarms: &[AccountAlarm],
    candidate_time: &str,
    exclude_id: Option<&str>,
    min_interval_minutes: i64,
) -> Result<(), String> {
    let cand_min = parse_time_to_minutes(candidate_time)?;

    for alm in existing_alarms {
        if !alm.enabled || exclude_id == Some(&alm.id) {
            continue;
        }

        if let Ok(exist_min) = parse_time_to_minutes(&alm.time_of_day) {
            let mut diff = (cand_min - exist_min).abs();
            if diff > 12 * 60 {
                diff = 24 * 60 - diff;
            }

            if diff < min_interval_minutes {
                let diff_h = diff as f64 / 60.0;
                let req_h = min_interval_minutes as f64 / 60.0;
                return Err(format!(
                    "与已有闹钟 {} 间隔仅 {:.1} 小时，必须 >= {:.1} 小时（滑动窗口防重叠）",
                    alm.time_of_day, diff_h, req_h
                ));
            }
        }
    }

    Ok(())
}

/// Lists alarms for a specific account or for all accounts.
pub fn list_account_alarms(target: Option<&str>) -> Result<Vec<AccountAlarm>, String> {
    let conn = get_connection()?;
    let identity_key = if let Some(t) = target.filter(|s| !s.trim().is_empty()) {
        resolve_account(t)?
            .map(|i| i.key())
    } else {
        None
    };

    let mut stmt = if let Some(ref _key) = identity_key {
        conn.prepare(
            "SELECT id, identity_key, time_of_day, days_of_week, enabled,
                    model_override, prompt_override, last_triggered_at, last_status, created_at
             FROM account_alarms
             WHERE identity_key = ?1
             ORDER BY time_of_day ASC",
        )
        .map_err(|e| e.to_string())?
    } else {
        conn.prepare(
            "SELECT id, identity_key, time_of_day, days_of_week, enabled,
                    model_override, prompt_override, last_triggered_at, last_status, created_at
             FROM account_alarms
             ORDER BY time_of_day ASC",
        )
        .map_err(|e| e.to_string())?
    };

    let map_row = |row: &rusqlite::Row| {
        let enabled_val: i64 = row.get(4)?;
        Ok(AccountAlarm {
            id: row.get(0)?,
            identity_key: row.get(1)?,
            time_of_day: row.get(2)?,
            days_of_week: row.get(3)?,
            enabled: enabled_val != 0,
            model_override: row.get(5)?,
            prompt_override: row.get(6)?,
            last_triggered_at: row.get(7)?,
            last_status: row.get(8)?,
            created_at: row.get(9)?,
        })
    };

    let rows: Vec<AccountAlarm> = if let Some(ref key) = identity_key {
        stmt.query_map(params![key], map_row)
            .map_err(|e| e.to_string())?
            .flatten()
            .collect()
    } else {
        stmt.query_map([], map_row)
            .map_err(|e| e.to_string())?
            .flatten()
            .collect()
    };

    Ok(rows)
}

/// Saves or updates an account alarm after enforcing non-overlapping constraints.
pub fn save_account_alarm(alarm_val: Value) -> Result<AccountAlarm, String> {
    let identity_key = alarm_val
        .get("identity_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing identity_key".to_string())?
        .trim()
        .to_string();

    let time_of_day = alarm_val
        .get("time_of_day")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing time_of_day".to_string())?
        .trim()
        .to_string();

    let id = alarm_val
        .get("id")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("alm_{}", chrono::Utc::now().timestamp_millis()));

    let days_of_week = alarm_val
        .get("days_of_week")
        .and_then(|v| v.as_str())
        .unwrap_or("1,2,3,4,5")
        .trim()
        .to_string();

    let enabled = match alarm_val.get("enabled") {
        Some(v) if v.is_boolean() => v.as_bool().unwrap_or(true),
        Some(v) if v.is_number() => v.as_i64().map(|n| n != 0).unwrap_or(true),
        _ => true,
    };

    let model_override = alarm_val
        .get("model_override")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let prompt_override = alarm_val
        .get("prompt_override")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // 5-hour constraint check if enabled
    if enabled {
        let existing = list_account_alarms(Some(&identity_key))?;
        validate_alarm_intervals(&existing, &time_of_day, Some(&id), 300)?;
    }

    let conn = get_connection()?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    conn.execute(
        "INSERT INTO account_alarms (
             id, identity_key, time_of_day, days_of_week, enabled,
             model_override, prompt_override, created_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
             identity_key = excluded.identity_key,
             time_of_day = excluded.time_of_day,
             days_of_week = excluded.days_of_week,
             enabled = excluded.enabled,
             model_override = excluded.model_override,
             prompt_override = excluded.prompt_override",
        params![
            id,
            identity_key,
            time_of_day,
            days_of_week,
            if enabled { 1 } else { 0 },
            model_override,
            prompt_override,
            now
        ],
    )
    .map_err(|e| format!("Failed to save alarm: {e}"))?;

    let row = conn
        .query_row(
            "SELECT id, identity_key, time_of_day, days_of_week, enabled,
                    model_override, prompt_override, last_triggered_at, last_status, created_at
             FROM account_alarms WHERE id = ?1",
            params![id],
            |r| {
                let en: i64 = r.get(4)?;
                Ok(AccountAlarm {
                    id: r.get(0)?,
                    identity_key: r.get(1)?,
                    time_of_day: r.get(2)?,
                    days_of_week: r.get(3)?,
                    enabled: en != 0,
                    model_override: r.get(5)?,
                    prompt_override: r.get(6)?,
                    last_triggered_at: r.get(7)?,
                    last_status: r.get(8)?,
                    created_at: r.get(9)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;

    Ok(row)
}

/// Deletes an account alarm by ID.
pub fn delete_account_alarm(id: &str) -> Result<bool, String> {
    let conn = get_connection()?;
    let affected = conn
        .execute("DELETE FROM account_alarms WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(affected > 0)
}

/// Background ticker checking and firing due alarms.
pub fn start_alarm_scheduler() {
    tauri::async_runtime::spawn(async {
        log::info!("Started background account alarm scheduler ticker");
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;

            let now = Local::now();
            let current_hh_mm = format!("{:02}:{:02}", now.hour(), now.minute());
            // ISO weekday: 1 (Mon) .. 7 (Sun)
            let current_weekday = now.weekday().number_from_monday().to_string();

            let alarms = match list_account_alarms(None) {
                Ok(a) => a,
                Err(_) => continue,
            };

            for alm in alarms {
                if !alm.enabled || alm.time_of_day != current_hh_mm {
                    continue;
                }

                // Check day pattern match
                let is_once = alm.days_of_week == "once";
                let days: Vec<&str> = alm.days_of_week.split(',').map(|s| s.trim()).collect();
                let matches_day = is_once || days.contains(&current_weekday.as_str());

                if !matches_day {
                    continue;
                }

                // Guard: don't trigger more than once per minute (timezone-safe timestamp diff)
                if let Some(ref last) = alm.last_triggered_at {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(last) {
                        if (now.timestamp() - dt.timestamp()).abs() < 60 {
                            continue;
                        }
                    } else if last.starts_with(&now.format("%Y-%m-%d").to_string()) && last.contains(&current_hh_mm) {
                        continue;
                    }
                }

                log::info!("Alarm {} for {} triggered at {}", alm.id, alm.identity_key, current_hh_mm);

                let res = trigger_warmup(
                    Some(alm.identity_key.clone()),
                    alm.model_override.clone(),
                    alm.prompt_override.clone(),
                    false,
                    60.0,
                )
                .await;

                let status = match res {
                    Ok(val) => val
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("success")
                        .to_string(),
                    Err(_) => "failed".to_string(),
                };

                let triggered_iso = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

                if let Ok(conn) = get_connection() {
                    let next_enabled = if is_once { 0 } else { 1 };
                    let _ = conn.execute(
                        "UPDATE account_alarms SET last_status = ?1, last_triggered_at = ?2, enabled = ?3 WHERE id = ?4",
                        params![status, triggered_iso, next_enabled, alm.id],
                    );
                }
            }
        }
    });
}
