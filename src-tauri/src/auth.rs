//! Microsoft → Xbox → XSTS → Minecraft Services → Entitlement → Profile 正版认证。
//!
//! 协议依据（本机当前实现，不复制陈旧博客 URL）：
//! - OAuth2 Authorization Code + PKCE(S256)，consumers（个人 Microsoft 账户）authority：
//!   authorize = https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize
//!   token     = https://login.microsoftonline.com/consumers/oauth2/v2.0/token
//!   scope     = XboxLive.signin offline_access（最小权限，不申请 Mail/Graph 等无关权限）
//! - Xbox user auth：POST https://user.auth.xboxlive.com/user/authenticate（AuthMethod=RPS）
//! - XSTS：POST https://xsts.auth.xboxlive.com/xsts/authorize
//!   （SandboxId=RETAIL，RelyingParty=rp://api.minecraftservices.com/）
//! - Minecraft：POST https://api.minecraftservices.com/authentication/login_with_xbox
//! - 资格：GET https://api.minecraftservices.com/entitlements/mcstore
//!   （items 中须含 product_minecraft / game_minecraft）
//! - 档案：GET https://api.minecraftservices.com/minecraft/profile

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    time::Duration,
};
use url::Url;
use uuid::Uuid;

const OAUTH_AUTHORIZE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const OAUTH_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBOX_USER_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTHORIZE: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN_WITH_XBOX: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
const MINECRAFT_ENTITLEMENTS: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MINECRAFT_PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";
const XSTS_RELYING_PARTY: &str = "rp://api.minecraftservices.com/";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftProfile {
    pub name: String,
    pub uuid: String,
    pub skin_url: Option<String>,
    pub xuid: Option<String>,
}

#[derive(Debug)]
pub(crate) struct LoginResult {
    pub profile: MicrosoftProfile,
    pub refresh_token: String,
    pub access_token: String,
}

/// 认证阶段（PoC / 生产 UI 共用的阶段划分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthStageKind {
    MicrosoftOauth,
    XboxUserAuth,
    Xsts,
    MinecraftServices,
    MinecraftEntitlement,
    MinecraftProfile,
}

impl fmt::Display for AuthStageKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            AuthStageKind::MicrosoftOauth => "Microsoft OAuth",
            AuthStageKind::XboxUserAuth => "Xbox User Auth",
            AuthStageKind::Xsts => "XSTS",
            AuthStageKind::MinecraftServices => "Minecraft Services",
            AuthStageKind::MinecraftEntitlement => "Minecraft Entitlement",
            AuthStageKind::MinecraftProfile => "Minecraft Profile",
        };
        formatter.write_str(label)
    }
}

/// 单阶段结果：只含非敏感信息（status / error code / 产品化文案），绝不携带 token。
#[derive(Debug, Clone)]
pub struct StageOutcome {
    pub stage: AuthStageKind,
    pub passed: bool,
    pub http_status: Option<u16>,
    pub error_code: Option<String>,
    pub message: Option<String>,
}

/// 完整的阶段化认证结果。token 仅存在于内存；由调用方决定是否写入系统凭据。
#[derive(Debug)]
pub struct AuthenticatedAccount {
    pub profile: MicrosoftProfile,
    pub refresh_token: String,
    pub access_token: String,
}

/// 认证失败：结构化分类，UI/PoC 可直接展示产品化文案。
#[derive(Debug, Clone)]
pub enum AuthFailure {
    Cancelled,
    Timeout,
    StateMismatch,
    NetworkTimeout,
    InvalidResponse {
        stage: AuthStageKind,
        detail: String,
    },
    Xsts {
        code: Option<i64>,
        category: XstsCategory,
        message: String,
    },
    MinecraftServices {
        status: Option<u16>,
        code: Option<String>,
        detail: String,
        app_registration_blocked: bool,
    },
    EntitlementMissing,
    InvalidProfile {
        detail: String,
    },
    Http {
        stage: AuthStageKind,
        status: Option<u16>,
        code: Option<String>,
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XstsCategory {
    XboxProfileMissing,
    ChildOrFamilyRestricted,
    RegionRestricted,
    AccountRestricted,
    AuthorizationDenied,
    Unknown,
}

impl fmt::Display for AuthFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            AuthFailure::Cancelled => "Microsoft 登录已取消。".to_string(),
            AuthFailure::Timeout => "Microsoft 登录等待超时，请重新登录。".to_string(),
            AuthFailure::StateMismatch => "登录回调状态校验失败，已拒绝这次回调。".to_string(),
            AuthFailure::NetworkTimeout => "网络超时，请检查网络后重试。".to_string(),
            AuthFailure::InvalidResponse { stage, detail } => {
                format!("{stage} 返回内容无效：{detail}")
            }
            AuthFailure::Xsts {
                code,
                category,
                message,
            } => {
                let code = code.map(|value| format!("（{value}）")).unwrap_or_default();
                let category = match category {
                    XstsCategory::XboxProfileMissing => "没有 Xbox 档案",
                    XstsCategory::ChildOrFamilyRestricted => "未成年/家庭限制",
                    XstsCategory::RegionRestricted => "地区限制",
                    XstsCategory::AccountRestricted => "账户受限",
                    XstsCategory::AuthorizationDenied => "授权被拒绝",
                    XstsCategory::Unknown => "XSTS 错误",
                };
                format!("{category}{code}：{message}")
            }
            AuthFailure::MinecraftServices {
                status,
                code,
                detail,
                app_registration_blocked,
            } => {
                if *app_registration_blocked {
                    format!(
                        "Minecraft Services 拒绝了当前应用注册（HTTP {status:?}，错误码 {code:?}）：{detail}"
                    )
                } else {
                    format!("Minecraft Services 验证失败（HTTP {status:?}）：{detail}")
                }
            }
            AuthFailure::EntitlementMissing => {
                "此 Microsoft 账户当前没有可用的 Minecraft Java Edition 游戏资格。".to_string()
            }
            AuthFailure::InvalidProfile { detail } => format!("Minecraft 档案无效：{detail}"),
            AuthFailure::Http {
                stage,
                status,
                code,
                detail,
            } => format!("{stage} 请求失败（HTTP {status:?}，错误码 {code:?}）：{detail}"),
        };
        formatter.write_str(&message)
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxTokenResponse {
    token: String,
    #[serde(default)]
    display_claims: DisplayClaims,
}

#[derive(Debug, Default, Deserialize)]
struct DisplayClaims {
    #[serde(default)]
    xui: Vec<XuiClaim>,
}

#[derive(Debug, Default, Deserialize)]
struct XuiClaim {
    uhs: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XstsErrorResponse {
    #[serde(rename = "XErr")]
    xerr: Option<i64>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MinecraftTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct MinecraftErrorResponse {
    error: Option<String>,
    error_description: Option<String>,
    #[serde(rename = "errorMessage")]
    error_message: Option<String>,
    #[serde(rename = "errorType")]
    error_type: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EntitlementsResponse {
    #[serde(default)]
    items: Vec<EntitlementItem>,
}

#[derive(Debug, Deserialize)]
struct EntitlementItem {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProfileResponse {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<ProfileSkin>,
}

#[derive(Debug, Deserialize)]
struct ProfileSkin {
    url: String,
}

/// 认证网络客户端：连接/整体超时、UA。
fn auth_client() -> Result<Client, AuthFailure> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .user_agent("SHLauncher/0.9.0")
        .build()
        .map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: error.to_string(),
        })
}

/// 只对明确的瞬态失败重试（网络错误/429/5xx，遵守 Retry-After），
/// 401/403/invalid_grant/app registration 等身份错误绝不盲目重试。
fn retryable_status(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// 发送 JSON POST，只对瞬态失败（网络错误/429/5xx，遵守 Retry-After）做有限指数退避；
/// 401/403/invalid_grant/app registration 等身份错误绝不重试。
async fn post_json_with_retry(
    client: &Client,
    stage: AuthStageKind,
    url: &str,
    body: serde_json::Value,
) -> Result<reqwest::Response, AuthFailure> {
    let mut attempt = 0u32;
    loop {
        let result = client.post(url).json(&body).send().await;
        match result {
            Ok(response) => {
                let status = response.status().as_u16();
                if retryable_status(status) && attempt < 3 {
                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(1)
                        .min(10);
                    attempt += 1;
                    tokio::time::sleep(Duration::from_secs(retry_after * u64::from(attempt))).await;
                    continue;
                }
                return Ok(response);
            }
            Err(error)
                if (error.is_timeout() || error.is_connect() || error.is_request())
                    && attempt < 2 =>
            {
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(u64::from(attempt))).await;
            }
            Err(error) => {
                return Err(if error.is_timeout() || error.is_connect() {
                    AuthFailure::NetworkTimeout
                } else {
                    AuthFailure::Http {
                        stage,
                        status: None,
                        code: None,
                        detail: format!("网络错误：{error}"),
                    }
                });
            }
        }
    }
}

async fn read_text_safe(response: reqwest::Response) -> Result<(u16, String), AuthFailure> {
    let status = response.status().as_u16();
    let body = response.text().await.map_err(|error| AuthFailure::Http {
        stage: AuthStageKind::MicrosoftOauth,
        status: None,
        code: None,
        detail: format!("读取响应失败：{error}"),
    })?;
    Ok((status, body))
}

fn classify_xsts(status: u16, body: &str) -> AuthFailure {
    let parsed = serde_json::from_str::<XstsErrorResponse>(body)
        .ok()
        .unwrap_or(XstsErrorResponse {
            xerr: None,
            message: None,
        });
    let (category, message) = match parsed.xerr {
        Some(2148916233) => (
            XstsCategory::XboxProfileMissing,
            "该 Microsoft 账户还没有 Xbox 档案，请先登录 Xbox 完成创建。".to_string(),
        ),
        Some(2148916235) => (
            XstsCategory::RegionRestricted,
            "所在地区不可用 Xbox Live，无法继续 Minecraft 验证。".to_string(),
        ),
        Some(2148916236) => (
            XstsCategory::ChildOrFamilyRestricted,
            "该账户是未成年人且未加入家庭组，需要成年监护人授权。".to_string(),
        ),
        Some(2148916238) => (
            XstsCategory::ChildOrFamilyRestricted,
            "该账户是儿童账户，需要在 Microsoft 家庭设置中允许。".to_string(),
        ),
        Some(2148916227) => (
            XstsCategory::AccountRestricted,
            "该 Xbox 账户已被限制，无法继续。".to_string(),
        ),
        Some(2148916234) => (
            XstsCategory::AuthorizationDenied,
            "账户授权被拒绝，请检查账户安全设置。".to_string(),
        ),
        Some(code) => (
            XstsCategory::Unknown,
            parsed
                .message
                .clone()
                .unwrap_or_else(|| format!("XSTS 拒绝了请求（HTTP {status}，XErr {code}）。")),
        ),
        None => (
            XstsCategory::Unknown,
            format!("XSTS 返回错误（HTTP {status}），且响应中没有 XErr。"),
        ),
    };
    AuthFailure::Xsts {
        code: parsed.xerr,
        category,
        message: message.to_string(),
    }
}

/// SecretRedactor：任何诊断文本在输出前都必须经过这里。
/// 测试必须证明 Bearer/XBL3.0/RpsTicket/code/token 不会泄漏。
pub struct SecretRedactor;

impl SecretRedactor {
    pub fn redact(text: &str) -> String {
        let mut output = text.to_string();
        for pattern in [
            "Bearer ",
            "bearer ",
            "XBL3.0 x=",
            "RpsTicket\":\"d=",
            "access_token",
            "refresh_token",
            "id_token",
            "code=",
        ] {
            output = redact_after_pattern(&output, pattern);
        }
        output
    }
}

fn redact_after_pattern(text: &str, pattern: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    // XBL3.0 x=<uhs>;<xsts>：整个字符串是凭据，分号后也是秘密，不能提前截断。
    let semicolon_is_delimiter = pattern != "XBL3.0 x=";
    while let Some(index) = rest.find(pattern) {
        result.push_str(&rest[..index]);
        result.push_str(pattern);
        let after = &rest[index + pattern.len()..];
        let token_end = after
            .find(|character: char| {
                character.is_whitespace()
                    || character == '&'
                    || character == '"'
                    || character == '\''
                    || (semicolon_is_delimiter && character == ';')
            })
            .unwrap_or(after.len());
        result.push_str("<REDACTED>");
        rest = &after[token_end..];
    }
    result.push_str(rest);
    result
}

struct CallbackOutcome {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn receive_callback(listener: TcpListener) -> Result<CallbackOutcome, AuthFailure> {
    let (mut stream, _) = listener
        .accept()
        .map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: error.to_string(),
        })?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut buffer = [0u8; 8192];
    let count = stream
        .read(&mut buffer)
        .map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: error.to_string(),
        })?;
    let request = String::from_utf8_lossy(&buffer[..count]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: "回调请求格式无效".to_string(),
        })?;
    let url = Url::parse(&format!("http://127.0.0.1{target}")).map_err(|error| {
        AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: error.to_string(),
        }
    })?;
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut error_description = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => error_description = Some(value.into_owned()),
            _ => {}
        }
    }
    let page = if code.is_some() {
        "<!doctype html><html lang=\"zh\"><meta charset=\"utf-8\"><title>SH Launcher</title>\
         <body style=\"font-family:system-ui;display:flex;align-items:center;justify-content:center;height:100vh;margin:0\">\
         <div style=\"text-align:center\"><h1>SH Launcher</h1>\
         <p>Microsoft 登录完成。<br>可以关闭此页面并返回启动器。</p></div></body></html>"
    } else {
        "<!doctype html><html lang=\"zh\"><meta charset=\"utf-8\"><title>SH Launcher</title>\
         <body>登录未完成，请关闭此页面后重试。</body></html>"
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(response.as_bytes());
    Ok(CallbackOutcome {
        code,
        state,
        error,
        error_description,
    })
}

fn build_pkce() -> (String, String) {
    let mut verifier_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut verifier_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn build_state() -> String {
    let mut state_bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut state_bytes);
    URL_SAFE_NO_PAD.encode(state_bytes)
}

/// 完整阶段化认证。阶段结果收集在 `AuthenticatedAccount.stages`（只含非敏感信息）。
pub async fn authenticate(
    client_id: &str,
    events: tokio::sync::mpsc::UnboundedSender<StageOutcome>,
) -> Result<AuthenticatedAccount, AuthFailure> {
    let client_id = client_id.trim();
    if client_id.len() < 10 || client_id.contains(char::is_whitespace) {
        return Err(AuthFailure::Http {
            stage: AuthStageKind::MicrosoftOauth,
            status: None,
            code: Some("INVALID_CLIENT_ID".to_string()),
            detail: "客户端 ID 未配置或无效。".to_string(),
        });
    }

    // 1. OAuth：loopback listener + PKCE + state，系统默认浏览器登录。
    // 双栈 loopback：Windows 下 localhost 会优先解析到 ::1，若只绑定 IPv4，
    // 浏览器回调可能落到 IPv6 端口上导致收不到。绑定 [::]:0（Windows 默认
    // 接受 IPv4-mapped 连接），失败时回退到 127.0.0.1:0。
    let listener = TcpListener::bind("[::]:0")
        .or_else(|_| TcpListener::bind("127.0.0.1:0"))
        .map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: error.to_string(),
        })?;
    listener.set_nonblocking(false).ok();
    let port = listener
        .local_addr()
        .map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: error.to_string(),
        })?
        .port();
    // 注册的 redirect_uri 为 http://localhost（无路径）。
    // 按 RFC 8252 §7.3，loopback URI 匹配时忽略端口，但路径必须与注册值完全一致：
    // 因此这里不能带 /callback 路径；保留随机端口是为了让浏览器回调回到本监听器。
    let redirect_uri = format!("http://localhost:{port}");
    let (verifier, challenge) = build_pkce();
    let state = build_state();
    let mut authorize = Url::parse(OAUTH_AUTHORIZE).expect("authorize endpoint is constant");
    authorize
        .query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_mode", "query")
        .append_pair("scope", "XboxLive.signin offline_access")
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", authorize.as_str()])
        .spawn()
        .map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: format!("无法打开系统浏览器：{error}"),
        })?;
    let callback = tokio::time::timeout(
        Duration::from_secs(600),
        tokio::task::spawn_blocking(move || receive_callback(listener)),
    )
    .await
    .map_err(|_| AuthFailure::Timeout)?
    .map_err(|error| AuthFailure::InvalidResponse {
        stage: AuthStageKind::MicrosoftOauth,
        detail: error.to_string(),
    })??;
    if let Some(oauth_error) = callback.error {
        let failure = if oauth_error == "access_denied" {
            AuthFailure::Cancelled
        } else {
            AuthFailure::Http {
                stage: AuthStageKind::MicrosoftOauth,
                status: None,
                code: Some(oauth_error),
                detail: callback.error_description.unwrap_or_default(),
            }
        };
        emit_failure(&events, AuthStageKind::MicrosoftOauth, None, &failure);
        return Err(failure);
    }
    if callback.state.as_deref() != Some(state.as_str()) {
        emit_failure(
            &events,
            AuthStageKind::MicrosoftOauth,
            None,
            &AuthFailure::StateMismatch,
        );
        return Err(AuthFailure::StateMismatch);
    }
    let code = match callback.code {
        Some(code) => code,
        None => {
            emit_failure(
                &events,
                AuthStageKind::MicrosoftOauth,
                None,
                &AuthFailure::Cancelled,
            );
            return Err(AuthFailure::Cancelled);
        }
    };

    let client = auth_client()?;
    let token_response = client
        .post(OAUTH_TOKEN)
        .form(&[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|error| AuthFailure::Http {
            stage: AuthStageKind::MicrosoftOauth,
            status: None,
            code: None,
            detail: format!("Token 请求失败：{error}"),
        })?;
    let token_status = token_response.status().as_u16();
    let token_body = token_response
        .text()
        .await
        .map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: error.to_string(),
        })?;
    if !(200..300).contains(&token_status) {
        let oauth_error: OAuthErrorResponse =
            serde_json::from_str(&token_body).unwrap_or(OAuthErrorResponse {
                error: None,
                error_description: None,
            });
        let failure = AuthFailure::Http {
            stage: AuthStageKind::MicrosoftOauth,
            status: Some(token_status),
            code: oauth_error.error,
            detail: oauth_error.error_description.unwrap_or_default(),
        };
        emit_failure(
            &events,
            AuthStageKind::MicrosoftOauth,
            Some(token_status),
            &failure,
        );
        return Err(failure);
    }
    let token: TokenResponse =
        serde_json::from_str(&token_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MicrosoftOauth,
            detail: error.to_string(),
        })?;
    emit_stage(
        &events,
        AuthStageKind::MicrosoftOauth,
        true,
        Some(token_status),
        None,
        None,
    );

    // 2. Xbox user authentication。
    let xbox_response = post_json_with_retry(
        &client,
        AuthStageKind::XboxUserAuth,
        XBOX_USER_AUTH,
        serde_json::json!({
            "Properties": {"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={}", token.access_token)},
            "RelyingParty":"http://auth.xboxlive.com",
            "TokenType":"JWT"
        }),
    )
    .await?;
    let (xbox_status, xbox_body) = read_text_safe(xbox_response).await?;
    if !(200..300).contains(&xbox_status) {
        let failure = AuthFailure::Http {
            stage: AuthStageKind::XboxUserAuth,
            status: Some(xbox_status),
            code: None,
            detail: non_sensitive_error_body(&xbox_body),
        };
        emit_failure(
            &events,
            AuthStageKind::XboxUserAuth,
            Some(xbox_status),
            &failure,
        );
        return Err(failure);
    }
    let xbox_user: XboxTokenResponse =
        serde_json::from_str(&xbox_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::XboxUserAuth,
            detail: error.to_string(),
        })?;
    let uhs = match xbox_user
        .display_claims
        .xui
        .first()
        .and_then(|claim| claim.uhs.as_deref())
        .filter(|uhs| !uhs.is_empty())
    {
        Some(uhs) => uhs.to_string(),
        None => {
            let failure = AuthFailure::InvalidResponse {
                stage: AuthStageKind::XboxUserAuth,
                detail: "XBOX_USER_AUTH_INVALID_RESPONSE：缺少 xui/uhs 用户哈希。".to_string(),
            };
            emit_failure(
                &events,
                AuthStageKind::XboxUserAuth,
                Some(xbox_status),
                &failure,
            );
            return Err(failure);
        }
    };
    emit_stage(
        &events,
        AuthStageKind::XboxUserAuth,
        true,
        Some(xbox_status),
        None,
        None,
    );

    // 3. XSTS。
    let xsts_response = post_json_with_retry(
        &client,
        AuthStageKind::Xsts,
        XSTS_AUTHORIZE,
        serde_json::json!({
            "Properties": {"SandboxId":"RETAIL","UserTokens":[xbox_user.token]},
            "RelyingParty": XSTS_RELYING_PARTY,
            "TokenType":"JWT"
        }),
    )
    .await?;
    let (xsts_status, xsts_body) = read_text_safe(xsts_response).await?;
    if !(200..300).contains(&xsts_status) {
        let failure = classify_xsts(xsts_status, &xsts_body);
        emit_failure(&events, AuthStageKind::Xsts, Some(xsts_status), &failure);
        return Err(failure);
    }
    let xsts: XboxTokenResponse =
        serde_json::from_str(&xsts_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::Xsts,
            detail: error.to_string(),
        })?;
    emit_stage(
        &events,
        AuthStageKind::Xsts,
        true,
        Some(xsts_status),
        None,
        None,
    );

    // 4. Minecraft Services。
    let minecraft_response = post_json_with_retry(
        &client,
        AuthStageKind::MinecraftServices,
        MINECRAFT_LOGIN_WITH_XBOX,
        serde_json::json!({"identityToken": format!("XBL3.0 x={};{}", uhs, xsts.token)}),
    )
    .await?;
    let (minecraft_status, minecraft_body) = read_text_safe(minecraft_response).await?;
    if !(200..300).contains(&minecraft_status) {
        let parsed: MinecraftErrorResponse =
            serde_json::from_str(&minecraft_body).unwrap_or(MinecraftErrorResponse {
                error: None,
                error_description: None,
                error_message: None,
                error_type: None,
                path: None,
            });
        let error_code = parsed
            .error
            .or_else(|| parsed.path.clone())
            .or_else(|| parsed.error_type.clone());
        let detail = parsed
            .error_description
            .or(parsed.error_message)
            .or_else(|| error_code.clone())
            .unwrap_or_else(|| non_sensitive_error_body(&minecraft_body));
        // Minecraft Services 对第三方 App Registration 的典型拒绝：403
        // / ForbiddenOperationException / "does not have permission to use this endpoint"。
        let combined = format!("{} {}", detail, error_code.as_deref().unwrap_or_default())
            .to_ascii_lowercase();
        let app_registration_blocked = minecraft_status == 403
            || combined.contains("permission")
            || combined.contains("client")
            || combined.contains("app registration")
            || combined.contains("unauthorized")
            || combined.contains("forbidden");
        let failure = AuthFailure::MinecraftServices {
            status: Some(minecraft_status),
            code: error_code,
            detail,
            app_registration_blocked,
        };
        emit_failure(
            &events,
            AuthStageKind::MinecraftServices,
            Some(minecraft_status),
            &failure,
        );
        return Err(failure);
    }
    let minecraft: MinecraftTokenResponse =
        serde_json::from_str(&minecraft_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MinecraftServices,
            detail: error.to_string(),
        })?;
    emit_stage(
        &events,
        AuthStageKind::MinecraftServices,
        true,
        Some(minecraft_status),
        None,
        None,
    );

    // 5. Entitlement（真实购买资格，不把登录成功当正版）。
    let entitlement_response = client
        .get(MINECRAFT_ENTITLEMENTS)
        .bearer_auth(&minecraft.access_token)
        .send()
        .await
        .map_err(|error| AuthFailure::Http {
            stage: AuthStageKind::MinecraftEntitlement,
            status: None,
            code: None,
            detail: error.to_string(),
        })?;
    let (entitlement_status, entitlement_body) = read_text_safe(entitlement_response).await?;
    if !(200..300).contains(&entitlement_status) {
        let failure = AuthFailure::Http {
            stage: AuthStageKind::MinecraftEntitlement,
            status: Some(entitlement_status),
            code: None,
            detail: non_sensitive_error_body(&entitlement_body),
        };
        emit_failure(
            &events,
            AuthStageKind::MinecraftEntitlement,
            Some(entitlement_status),
            &failure,
        );
        return Err(failure);
    }
    let entitlements: EntitlementsResponse =
        serde_json::from_str(&entitlement_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MinecraftEntitlement,
            detail: error.to_string(),
        })?;
    let owned = entitlements.items.iter().any(|item| {
        matches!(
            item.name.as_deref(),
            Some("product_minecraft" | "game_minecraft")
        )
    });
    if !owned {
        emit_failure(
            &events,
            AuthStageKind::MinecraftEntitlement,
            Some(entitlement_status),
            &AuthFailure::EntitlementMissing,
        );
        return Err(AuthFailure::EntitlementMissing);
    }
    emit_stage(
        &events,
        AuthStageKind::MinecraftEntitlement,
        true,
        Some(entitlement_status),
        None,
        None,
    );

    // 6. Minecraft Profile。
    let profile_response = client
        .get(MINECRAFT_PROFILE)
        .bearer_auth(&minecraft.access_token)
        .send()
        .await
        .map_err(|error| AuthFailure::Http {
            stage: AuthStageKind::MinecraftProfile,
            status: None,
            code: None,
            detail: error.to_string(),
        })?;
    let (profile_status, profile_body) = read_text_safe(profile_response).await?;
    if !(200..300).contains(&profile_status) {
        let failure = AuthFailure::Http {
            stage: AuthStageKind::MinecraftProfile,
            status: Some(profile_status),
            code: None,
            detail: non_sensitive_error_body(&profile_body),
        };
        emit_failure(
            &events,
            AuthStageKind::MinecraftProfile,
            Some(profile_status),
            &failure,
        );
        return Err(failure);
    }
    let profile: ProfileResponse =
        serde_json::from_str(&profile_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MinecraftProfile,
            detail: error.to_string(),
        })?;
    if profile.name.trim().is_empty() {
        let failure = AuthFailure::InvalidProfile {
            detail: "MINECRAFT_PROFILE_VALID：username 为空。".to_string(),
        };
        emit_failure(
            &events,
            AuthStageKind::MinecraftProfile,
            Some(profile_status),
            &failure,
        );
        return Err(failure);
    }
    if Uuid::parse_str(&profile.id).is_err() {
        let failure = AuthFailure::InvalidProfile {
            detail: "MINECRAFT_PROFILE_VALID：UUID 格式无效。".to_string(),
        };
        emit_failure(
            &events,
            AuthStageKind::MinecraftProfile,
            Some(profile_status),
            &failure,
        );
        return Err(failure);
    }
    emit_stage(
        &events,
        AuthStageKind::MinecraftProfile,
        true,
        Some(profile_status),
        None,
        None,
    );

    Ok(AuthenticatedAccount {
        profile: MicrosoftProfile {
            name: profile.name,
            uuid: profile.id,
            skin_url: profile.skins.first().map(|skin| skin.url.clone()),
            xuid: None,
        },
        refresh_token: token.refresh_token,
        access_token: minecraft.access_token,
    })
}

fn emit_stage(
    events: &tokio::sync::mpsc::UnboundedSender<StageOutcome>,
    stage: AuthStageKind,
    passed: bool,
    http_status: Option<u16>,
    error_code: Option<String>,
    message: Option<String>,
) {
    let _ = events.send(StageOutcome {
        stage,
        passed,
        http_status,
        error_code,
        message,
    });
}

fn emit_failure(
    events: &tokio::sync::mpsc::UnboundedSender<StageOutcome>,
    stage: AuthStageKind,
    http_status: Option<u16>,
    failure: &AuthFailure,
) {
    let code = match failure {
        AuthFailure::Xsts { code, .. } => code.map(|value| value.to_string()),
        AuthFailure::MinecraftServices { code, .. } => code.clone(),
        AuthFailure::Http { code, .. } => code.clone(),
        other => Some(match other {
            AuthFailure::Cancelled => "AUTH_CANCELLED".to_string(),
            AuthFailure::Timeout => "AUTH_TIMEOUT".to_string(),
            AuthFailure::StateMismatch => "AUTH_STATE_MISMATCH".to_string(),
            AuthFailure::EntitlementMissing => "MINECRAFT_JAVA_NOT_ENTITLED".to_string(),
            _ => "AUTH_ERROR".to_string(),
        }),
    };
    let _ = events.send(StageOutcome {
        stage,
        passed: false,
        http_status,
        error_code: code,
        message: Some(failure.to_string()),
    });
}

/// 提取响应体里可能包含 token 的错误信息时，只返回安全的截断片段，绝不打印完整 body。
fn non_sensitive_error_body(body: &str) -> String {
    let cleaned = SecretRedactor::redact(body);
    let compact: String = cleaned.chars().take(240).collect();
    if compact.is_empty() {
        "（无错误详情）".to_string()
    } else {
        compact
    }
}

/// 兼容旧生产路径：login() = 完整阶段认证，但把阶段结果折叠成 LoginResult。
pub(crate) async fn login(client_id: &str) -> Result<LoginResult, String> {
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let account = authenticate(client_id, events)
        .await
        .map_err(|error| error.to_string())?;
    Ok(LoginResult {
        profile: account.profile,
        refresh_token: account.refresh_token,
        access_token: account.access_token,
    })
}

pub(crate) async fn refresh(client_id: &str, refresh_token: &str) -> Result<LoginResult, String> {
    if client_id.trim().len() < 10 || client_id.contains(char::is_whitespace) {
        return Err("这个版本的 Microsoft 登录配置无效，请联系 SH 启动器发布者更新安装包。".into());
    }
    if refresh_token.trim().is_empty() {
        return Err("Microsoft 登录凭据缺少刷新令牌，请重新登录。".into());
    }
    let client = auth_client().map_err(|error| error.to_string())?;
    let response = client
        .post(OAUTH_TOKEN)
        .form(&[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(|error| format!("Microsoft 登录刷新失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("Microsoft 登录已过期，请重新登录：{error}"))?
        .json::<TokenResponse>()
        .await
        .map_err(|error| format!("Microsoft 刷新结果无效：{error}"))?;
    // 重新走 Xbox/XSTS/Minecraft 链，返回真实 Minecraft token。
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let account = authenticate_refreshed(client_id, &response.access_token, events)
        .await
        .map_err(|error| error.to_string())?;
    Ok(LoginResult {
        profile: account.profile,
        refresh_token: response.refresh_token,
        access_token: account.access_token,
    })
}

async fn authenticate_refreshed(
    _client_id: &str,
    access_token: &str,
    events: tokio::sync::mpsc::UnboundedSender<StageOutcome>,
) -> Result<AuthenticatedAccount, AuthFailure> {
    // 与 authenticate 后半段相同的 Xbox→XSTS→Minecraft→Entitlement→Profile 链。
    let client = auth_client()?;
    let xbox_response = post_json_with_retry(
        &client,
        AuthStageKind::XboxUserAuth,
        XBOX_USER_AUTH,
        serde_json::json!({
            "Properties": {"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={access_token}")},
            "RelyingParty":"http://auth.xboxlive.com",
            "TokenType":"JWT"
        }),
    )
    .await?;
    let (xbox_status, xbox_body) = read_text_safe(xbox_response).await?;
    if !(200..300).contains(&xbox_status) {
        return Err(AuthFailure::Http {
            stage: AuthStageKind::XboxUserAuth,
            status: Some(xbox_status),
            code: None,
            detail: non_sensitive_error_body(&xbox_body),
        });
    }
    let xbox_user: XboxTokenResponse =
        serde_json::from_str(&xbox_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::XboxUserAuth,
            detail: error.to_string(),
        })?;
    let uhs = xbox_user
        .display_claims
        .xui
        .first()
        .and_then(|claim| claim.uhs.as_deref())
        .filter(|uhs| !uhs.is_empty())
        .ok_or_else(|| AuthFailure::InvalidResponse {
            stage: AuthStageKind::XboxUserAuth,
            detail: "XBOX_USER_AUTH_INVALID_RESPONSE：缺少 xui/uhs 用户哈希。".to_string(),
        })?
        .to_string();
    emit_stage(
        &events,
        AuthStageKind::XboxUserAuth,
        true,
        Some(xbox_status),
        None,
        None,
    );
    let xsts_response = post_json_with_retry(
        &client,
        AuthStageKind::Xsts,
        XSTS_AUTHORIZE,
        serde_json::json!({
            "Properties": {"SandboxId":"RETAIL","UserTokens":[xbox_user.token]},
            "RelyingParty": XSTS_RELYING_PARTY,
            "TokenType":"JWT"
        }),
    )
    .await?;
    let (xsts_status, xsts_body) = read_text_safe(xsts_response).await?;
    if !(200..300).contains(&xsts_status) {
        return Err(classify_xsts(xsts_status, &xsts_body));
    }
    let xsts: XboxTokenResponse =
        serde_json::from_str(&xsts_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::Xsts,
            detail: error.to_string(),
        })?;
    emit_stage(
        &events,
        AuthStageKind::Xsts,
        true,
        Some(xsts_status),
        None,
        None,
    );
    let minecraft_response = post_json_with_retry(
        &client,
        AuthStageKind::MinecraftServices,
        MINECRAFT_LOGIN_WITH_XBOX,
        serde_json::json!({"identityToken": format!("XBL3.0 x={};{}", uhs, xsts.token)}),
    )
    .await?;
    let (minecraft_status, minecraft_body) = read_text_safe(minecraft_response).await?;
    if !(200..300).contains(&minecraft_status) {
        let parsed: MinecraftErrorResponse =
            serde_json::from_str(&minecraft_body).unwrap_or(MinecraftErrorResponse {
                error: None,
                error_description: None,
                error_message: None,
                error_type: None,
                path: None,
            });
        return Err(AuthFailure::MinecraftServices {
            status: Some(minecraft_status),
            code: parsed.error.or(parsed.path),
            detail: parsed
                .error_description
                .unwrap_or_else(|| non_sensitive_error_body(&minecraft_body)),
            app_registration_blocked: false,
        });
    }
    let minecraft: MinecraftTokenResponse =
        serde_json::from_str(&minecraft_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MinecraftServices,
            detail: error.to_string(),
        })?;
    emit_stage(
        &events,
        AuthStageKind::MinecraftServices,
        true,
        Some(minecraft_status),
        None,
        None,
    );
    let entitlement_response = client
        .get(MINECRAFT_ENTITLEMENTS)
        .bearer_auth(&minecraft.access_token)
        .send()
        .await
        .map_err(|error| AuthFailure::Http {
            stage: AuthStageKind::MinecraftEntitlement,
            status: None,
            code: None,
            detail: error.to_string(),
        })?;
    let (entitlement_status, entitlement_body) = read_text_safe(entitlement_response).await?;
    if !(200..300).contains(&entitlement_status) {
        return Err(AuthFailure::Http {
            stage: AuthStageKind::MinecraftEntitlement,
            status: Some(entitlement_status),
            code: None,
            detail: non_sensitive_error_body(&entitlement_body),
        });
    }
    let entitlements: EntitlementsResponse =
        serde_json::from_str(&entitlement_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MinecraftEntitlement,
            detail: error.to_string(),
        })?;
    if !entitlements.items.iter().any(|item| {
        matches!(
            item.name.as_deref(),
            Some("product_minecraft" | "game_minecraft")
        )
    }) {
        return Err(AuthFailure::EntitlementMissing);
    }
    emit_stage(
        &events,
        AuthStageKind::MinecraftEntitlement,
        true,
        Some(entitlement_status),
        None,
        None,
    );
    let profile_response = client
        .get(MINECRAFT_PROFILE)
        .bearer_auth(&minecraft.access_token)
        .send()
        .await
        .map_err(|error| AuthFailure::Http {
            stage: AuthStageKind::MinecraftProfile,
            status: None,
            code: None,
            detail: error.to_string(),
        })?;
    let (profile_status, profile_body) = read_text_safe(profile_response).await?;
    if !(200..300).contains(&profile_status) {
        return Err(AuthFailure::Http {
            stage: AuthStageKind::MinecraftProfile,
            status: Some(profile_status),
            code: None,
            detail: non_sensitive_error_body(&profile_body),
        });
    }
    let profile: ProfileResponse =
        serde_json::from_str(&profile_body).map_err(|error| AuthFailure::InvalidResponse {
            stage: AuthStageKind::MinecraftProfile,
            detail: error.to_string(),
        })?;
    if profile.name.trim().is_empty() || Uuid::parse_str(&profile.id).is_err() {
        return Err(AuthFailure::InvalidProfile {
            detail: "MINECRAFT_PROFILE_VALID：档案无效。".to_string(),
        });
    }
    emit_stage(
        &events,
        AuthStageKind::MinecraftProfile,
        true,
        Some(profile_status),
        None,
        None,
    );
    Ok(AuthenticatedAccount {
        profile: MicrosoftProfile {
            name: profile.name,
            uuid: profile.id,
            skin_url: profile.skins.first().map(|skin| skin.url.clone()),
            xuid: None,
        },
        refresh_token: String::new(),
        access_token: minecraft.access_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_generates_valid_s256_pair() {
        let (verifier, challenge) = build_pkce();
        assert_eq!(
            verifier.len(),
            43,
            "32 字节随机数的 base64url 无填充长度为 43"
        );
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
        assert_ne!(verifier, build_pkce().0, "verifier 必须随机");
    }

    #[test]
    fn state_is_random_and_unique() {
        assert_ne!(build_state(), build_state());
        assert_eq!(build_state().len(), 32);
    }

    #[test]
    fn xsts_classification_maps_known_codes() {
        let failure = classify_xsts(
            401,
            r#"{"Identity":"0","XErr":2148916233,"Message":"","Redirect":""}"#,
        );
        assert!(matches!(
            failure,
            AuthFailure::Xsts {
                code: Some(2148916233),
                category: XstsCategory::XboxProfileMissing,
                ..
            }
        ));
        let failure = classify_xsts(401, r#"{"XErr":2148916235}"#);
        assert!(matches!(
            failure,
            AuthFailure::Xsts {
                category: XstsCategory::RegionRestricted,
                ..
            }
        ));
        let failure = classify_xsts(401, r#"{"XErr":2148916238}"#);
        assert!(matches!(
            failure,
            AuthFailure::Xsts {
                category: XstsCategory::ChildOrFamilyRestricted,
                ..
            }
        ));
        // 恶意/畸形 body 不 panic。
        let _ = classify_xsts(500, "{not json");
        let _ = classify_xsts(401, "");
    }

    #[test]
    fn secret_redactor_removes_tokens_and_codes() {
        let sample = concat!(
            "Authorization: Bearer eyJhbGciOi.abc.xyz\n",
            "identityToken=XBL3.0 x=uhs123;xsts-token-value\n",
            "\"RpsTicket\":\"d=ms-access-token-123\"\n",
            "code=oauth-code-123&state=ok"
        );
        let redacted = SecretRedactor::redact(sample);
        for secret in [
            "eyJhbGciOi.abc.xyz",
            "xsts-token-value",
            "ms-access-token-123",
            "oauth-code-123",
        ] {
            assert!(
                !redacted.contains(secret),
                "秘密泄漏：{secret} 出现在 {redacted}"
            );
        }
        assert!(redacted.contains("<REDACTED>"));
    }

    #[test]
    fn non_sensitive_error_body_truncates_and_redacts() {
        let body = "Bearer secret-token-value goes here";
        let cleaned = non_sensitive_error_body(body);
        assert!(!cleaned.contains("secret-token-value"));
    }

    #[test]
    fn retry_policy_only_retries_transient_statuses() {
        assert!(retryable_status(429));
        assert!(retryable_status(500));
        assert!(retryable_status(503));
        assert!(!retryable_status(401));
        assert!(!retryable_status(403));
        assert!(!retryable_status(404));
    }

    #[test]
    fn entitlement_requires_minecraft_products() {
        let owned: EntitlementsResponse = serde_json::from_str(
            r#"{"items":[{"name":"product_minecraft"},{"name":"game_minecraft"}]}"#,
        )
        .unwrap();
        assert!(owned.items.iter().any(|item| matches!(
            item.name.as_deref(),
            Some("product_minecraft" | "game_minecraft")
        )));
        let not_owned: EntitlementsResponse =
            serde_json::from_str(r#"{"items":[{"name":"product_dungeons"}]}"#).unwrap();
        assert!(!not_owned.items.iter().any(|item| matches!(
            item.name.as_deref(),
            Some("product_minecraft" | "game_minecraft")
        )));
    }

    #[test]
    fn malformed_server_responses_do_not_panic() {
        // 不可信输入：畸形 JSON、缺字段、超长字符串，都必须是结构化错误。
        let broken: Result<XboxTokenResponse, _> = serde_json::from_str("{oops");
        assert!(broken.is_err());
        let missing: Result<MinecraftTokenResponse, _> = serde_json::from_str("{}");
        assert!(missing.is_err());
        let empty_profile: Result<ProfileResponse, _> = serde_json::from_str("{}");
        assert!(empty_profile.is_err());
    }
}
