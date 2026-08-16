//! 第一阶段：Microsoft → Xbox → XSTS → Minecraft → Entitlement → Profile 真实认证 PoC。
//! 用法：microsoft_auth_poc.exe [client_id]
//! 不打印任何 token / authorization code / 完整响应体。
//! 退出码：0=PASS；1=CANCELLED/TIMEOUT；2=AUTH_POC_FAIL；3=AUTH_POC_BLOCKED_BY_APP_REGISTRATION

use app_lib::auth::{authenticate, AuthFailure, SecretRedactor, StageOutcome};

fn main() {
    tauri::async_runtime::block_on(run());
}

async fn run() {
    let client_id = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("SH_MICROSOFT_CLIENT_ID").ok())
        .unwrap_or_else(|| "15d02331-9d3c-4e74-8a93-c771cf1b1c28".to_string());

    println!("========================================");
    println!("SH Microsoft Authentication PoC");
    println!("========================================");
    println!("正在打开系统浏览器完成 Microsoft 登录（如有 MFA 请完成验证）。");
    println!("完成后浏览器会显示“Microsoft 登录完成”，可关闭页面。");
    println!();

    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel::<StageOutcome>();
    let printer = tokio::spawn(async move {
        let mut collected = Vec::new();
        while let Some(stage) = receiver.recv().await {
            println!(
                "{:<24} {}",
                stage.stage.to_string(),
                if stage.passed { "PASS" } else { "FAIL" }
            );
            collected.push(stage);
        }
        collected
    });
    let result = authenticate(&client_id, events).await;
    let stages = printer.await.unwrap_or_default();

    match result {
        Ok(account) => {
            println!();
            println!("========================================");
            println!("SH Microsoft Authentication PoC");
            println!("========================================");
            for stage in &stages {
                println!(
                    "{:<24} {}",
                    stage.stage.to_string(),
                    if stage.passed { "PASS" } else { "FAIL" }
                );
            }
            println!();
            println!("Username: {}", account.profile.name);
            println!("UUID: {}", account.profile.uuid);
            println!();
            println!("AUTH_POC_RESULT=PASS");
            println!("========================================");
            std::process::exit(0);
        }
        Err(failure) => {
            let result_code = match &failure {
                AuthFailure::Cancelled => "AUTH_CANCELLED",
                AuthFailure::Timeout => "AUTH_TIMEOUT",
                AuthFailure::MinecraftServices {
                    app_registration_blocked: true,
                    ..
                } => "AUTH_POC_BLOCKED_BY_APP_REGISTRATION",
                _ => "AUTH_POC_FAIL",
            };
            println!();
            println!("========================================");
            println!("SH Microsoft Authentication PoC");
            println!("========================================");
            for stage in &stages {
                println!(
                    "{:<24} {}",
                    stage.stage.to_string(),
                    if stage.passed { "PASS" } else { "FAIL" }
                );
                if !stage.passed {
                    if let Some(http_status) = stage.http_status {
                        println!("  http_status: {http_status}");
                    }
                    if let Some(error_code) = &stage.error_code {
                        println!("  error_code: {error_code}");
                    }
                    if let Some(message) = &stage.message {
                        println!("  message: {}", SecretRedactor::redact(message));
                    }
                }
            }
            println!();
            println!("failure: {}", SecretRedactor::redact(&failure.to_string()));
            println!();
            println!("AUTH_POC_RESULT={result_code}");
            println!("========================================");
            let exit_code = match result_code {
                "AUTH_POC_BLOCKED_BY_APP_REGISTRATION" => 3,
                "AUTH_CANCELLED" | "AUTH_TIMEOUT" => 1,
                _ => 2,
            };
            std::process::exit(exit_code);
        }
    }
}
