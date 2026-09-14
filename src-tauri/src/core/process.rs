//! Process inspection, termination, and relaunching of Codex desktop and daemon processes.

use std::time::Duration;
use tokio::process::Command;

use super::probe::find_codex_bin;

/// Terminates running Codex processes and optionally relaunches the desktop application.
///
/// # Arguments
///
/// * `relaunch` - Whether to relaunch the Codex app if it was previously running or requested.
/// * `start_if_not_running` - Whether to launch Codex if it wasn't already running.
///
/// # Errors
///
/// Returns `Err` if process inspection or command execution fails.
pub async fn restart_codex(relaunch: bool, start_if_not_running: bool) -> Result<String, String> {
    #[cfg(windows)]
    {
        restart_codex_windows(relaunch, start_if_not_running).await
    }

    #[cfg(not(windows))]
    {
        restart_codex_unix(relaunch, start_if_not_running).await
    }
}

#[cfg(windows)]
async fn restart_codex_windows(relaunch: bool, start_if_not_running: bool) -> Result<String, String> {
    let ps_cmd = r#"
        $procs = Get-Process | Where-Object {
            ($_.Name -eq 'codex' -or $_.Name -eq 'codex-code-mode-host') -or
            ($_.Name -eq 'ChatGPT' -and $_.Path -like '*OpenAI.Codex*') -or
            ($_.Name -like 'codex-plus-plus*')
        };
        $info = $procs | Select-Object Name, Path | ConvertTo-Json -Compress;
        $procs | Stop-Process -Force -ErrorAction SilentlyContinue;
        if ($info) { Write-Output $info }
    "#;

    let mut cmd = Command::new("powershell");
    cmd.arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-WindowStyle")
        .arg("Hidden")
        .arg("-Command")
        .arg(ps_cmd)
        .stdin(std::process::Stdio::null());

    // CREATE_NO_WINDOW = 0x08000000
    cmd.creation_flags(0x08000000);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to inspect/kill Codex processes: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let mut was_codex_app = false;
    let mut was_cpp = false;

    if !stdout_str.is_empty() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
            let items = if val.is_array() {
                val.as_array().unwrap().clone()
            } else {
                vec![val]
            };

            for item in items {
                let name = item.get("Name").and_then(|n| n.as_str()).unwrap_or("").to_lowercase();
                let path = item.get("Path").and_then(|p| p.as_str()).unwrap_or("").to_lowercase();
                if path.contains("openai.codex") || (name == "chatgpt" && path.contains("codex")) {
                    was_codex_app = true;
                }
                if name.contains("codex-plus-plus") {
                    was_cpp = true;
                }
            }
        }
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut relaunched: Vec<String> = Vec::new();

    if relaunch {
        if was_codex_app || (!was_cpp && start_if_not_running) {
            let mut started = false;

            // 1. Try launching the Windows Store / AppX package directly
            let mut launch_appx = Command::new("explorer.exe");
            launch_appx.arg("shell:AppsFolder\\OpenAI.Codex_2p2nqsd0c76g0!App");
            launch_appx.creation_flags(0x08000000);
            if launch_appx.spawn().is_ok() {
                started = true;
            }

            // 2. Fallback: launch via `codex app`
            if !started {
                let codex_bin = find_codex_bin();
                let mut launch_bin = Command::new(codex_bin);
                launch_bin.arg("app");
                launch_bin.stdin(std::process::Stdio::null());
                launch_bin.creation_flags(0x08000000);
                if launch_bin.spawn().is_ok() {
                    started = true;
                }
            }

            if started {
                relaunched.push("Codex 桌面应用".to_string());
            }
        }
    }

    if !relaunched.is_empty() {
        if was_codex_app || was_cpp {
            Ok(format!("Codex 进程已重启并重新拉起: {}", relaunched.join(", ")))
        } else {
            Ok("Codex 桌面应用已启动。".to_string())
        }
    } else if !relaunch {
        Ok("已成功终止所有运行中的 Codex 进程。".to_string())
    } else {
        Ok("已成功清理 Codex 后台服务，下次调用将直接加载最新凭证。".to_string())
    }
}

#[cfg(not(windows))]
async fn restart_codex_unix(relaunch: bool, start_if_not_running: bool) -> Result<String, String> {
    // 1. Check if Codex desktop GUI was running (macOS / Linux)
    let was_desktop = {
        #[cfg(target_os = "macos")]
        {
            let chk = Command::new("pgrep").arg("-f").arg("Codex.app").output().await;
            chk.map(|o| o.status.success() && !o.stdout.is_empty()).unwrap_or(false)
        }
        #[cfg(not(target_os = "macos"))]
        {
            // On Linux, match official codex desktop app, ensuring CodexQ is excluded
            let chk = Command::new("pgrep").arg("-f").arg("codex.*app").output().await;
            chk.map(|o| o.status.success() && !o.stdout.is_empty()).unwrap_or(false)
        }
    };

    // 2. Kill only official codex app-server daemons, code mode host, and desktop app (NEVER match CodexQ!)
    let _ = Command::new("pkill").arg("-f").arg("codex.*app-server").output().await;
    let _ = Command::new("pkill").arg("-f").arg("codex-code-mode-host").output().await;
    if was_desktop {
        #[cfg(target_os = "macos")]
        let _ = Command::new("pkill").arg("-f").arg("Codex.app").output().await;
        #[cfg(not(target_os = "macos"))]
        let _ = Command::new("pkill").arg("-f").arg("codex.*app").output().await;
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    let should_launch = was_desktop || start_if_not_running;

    if relaunch && should_launch {
        #[cfg(target_os = "macos")]
        {
            if Command::new("open").arg("-a").arg("Codex").spawn().is_err() {
                let codex_bin = find_codex_bin();
                let _ = Command::new(codex_bin).arg("app").spawn();
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let codex_bin = find_codex_bin();
            let _ = Command::new(codex_bin)
                .arg("app")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        Ok("Codex 桌面应用已启动。".to_string())
    } else if !relaunch {
        Ok("已成功终止所有运行中的 Codex 进程。".to_string())
    } else {
        Ok("已成功清理 Codex 后台服务，下次调用将直接加载最新凭证。".to_string())
    }
}
