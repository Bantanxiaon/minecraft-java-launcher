use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    time::Duration,
};
use url::Url;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MicrosoftProfile {
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

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct XboxTokenResponse {
    token: String,
    #[serde(default)]
    display_claims: DisplayClaims,
}

#[derive(Debug, Default, Deserialize)]
struct DisplayClaims {
    xui: Vec<XuiClaim>,
}

#[derive(Debug, Default, Deserialize)]
struct XuiClaim {
    uhs: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MinecraftTokenResponse {
    access_token: String,
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

pub(crate) async fn login(client_id: &str) -> Result<LoginResult, String> {
    if client_id.len() < 10 || client_id.contains(char::is_whitespace) {
        return Err("这个版本还没有配置 Microsoft 登录。请联系 SH 启动器发布者更新安装包。".into());
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("无法创建 Microsoft 登录回调：{error}"))?;
    listener
        .set_nonblocking(false)
        .map_err(|error| format!("无法准备登录回调：{error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("无法读取登录回调端口：{error}"))?
        .port();
    // Microsoft recommends the localhost loopback URI for desktop apps using
    // the system browser. The listener remains bound to the local machine only.
    let redirect_uri = format!("http://localhost:{port}/callback");
    let mut verifier_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut verifier_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut state_bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut state_bytes);
    let state = URL_SAFE_NO_PAD.encode(state_bytes);
    let mut authorize =
        Url::parse("https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize")
            .map_err(|error| format!("Microsoft 登录地址无效：{error}"))?;
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
        .map_err(|error| format!("无法打开系统浏览器：{error}"))?;

    let callback = tokio::time::timeout(
        Duration::from_secs(300),
        tokio::task::spawn_blocking(move || receive_callback(listener)),
    )
    .await
    .map_err(|_| "Microsoft 登录等待超时，请重新点击登录。".to_string())?
    .map_err(|error| format!("登录回调线程失败：{error}"))?
    .map_err(|error| format!("Microsoft 登录失败：{error}"))?;
    if callback.state != state {
        return Err("Microsoft 登录状态校验失败，已拒绝这次回调。".into());
    }
    let code = callback
        .code
        .ok_or_else(|| "Microsoft 登录已取消或没有返回授权码。".to_string())?;
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("SHLauncher/0.1.1")
        .build()
        .map_err(|error| format!("创建登录网络连接失败：{error}"))?;
    let token = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|error| format!("Microsoft Token 请求失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("Microsoft Token 返回错误：{error}"))?
        .json::<TokenResponse>()
        .await
        .map_err(|error| format!("Microsoft Token 返回内容无效：{error}"))?;
    let xbox_user = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&serde_json::json!({
            "Properties": {"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={}", token.access_token)},
            "RelyingParty":"http://auth.xboxlive.com",
            "TokenType":"JWT"
        }))
        .send().await
        .map_err(|error| format!("Xbox 账户验证失败：{error}"))?
        .error_for_status().map_err(|error| format!("Xbox 账户验证被拒绝：{error}"))?
        .json::<XboxTokenResponse>().await
        .map_err(|error| format!("Xbox 账户验证返回内容无效：{error}"))?;
    let uhs = xbox_user
        .display_claims
        .xui
        .first()
        .and_then(|claim| claim.uhs.clone())
        .ok_or_else(|| "Xbox 账户缺少用户标识，无法继续 Minecraft 验证。".to_string())?;
    let xsts = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&serde_json::json!({
            "Properties": {"SandboxId":"RETAIL","UserTokens":[xbox_user.token]},
            "RelyingParty":"rp://api.minecraftservices.com/",
            "TokenType":"JWT"
        }))
        .send()
        .await
        .map_err(|error| format!("Xbox XSTS 验证失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("Xbox XSTS 返回错误：{error}"))?
        .json::<XboxTokenResponse>()
        .await
        .map_err(|error| format!("Xbox XSTS 返回内容无效：{error}"))?;
    let minecraft = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&serde_json::json!({"identityToken":format!("XBL3.0 x={};{}", uhs, xsts.token)}))
        .send()
        .await
        .map_err(|error| format!("Minecraft Services 验证失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("Minecraft 资格验证失败：{error}"))?
        .json::<MinecraftTokenResponse>()
        .await
        .map_err(|error| format!("Minecraft Services 返回内容无效：{error}"))?;
    let profile = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&minecraft.access_token)
        .send()
        .await
        .map_err(|error| format!("读取 Minecraft Profile 失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("没有找到 Minecraft 资格或 Profile：{error}"))?
        .json::<ProfileResponse>()
        .await
        .map_err(|error| format!("Minecraft Profile 返回内容无效：{error}"))?;
    Ok(LoginResult {
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

pub(crate) async fn refresh(client_id: &str, refresh_token: &str) -> Result<LoginResult, String> {
    if client_id.len() < 10 || client_id.contains(char::is_whitespace) {
        return Err("这个版本的 Microsoft 登录配置无效，请联系 SH 启动器发布者更新安装包。".into());
    }
    if refresh_token.trim().is_empty() {
        return Err("Microsoft 登录凭据缺少刷新令牌，请重新登录。".into());
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("SHLauncher/0.1.1")
        .build()
        .map_err(|error| format!("创建登录网络连接失败：{error}"))?;
    let token = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
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
    let xbox_user = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&serde_json::json!({
            "Properties": {"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={}", token.access_token)},
            "RelyingParty":"http://auth.xboxlive.com",
            "TokenType":"JWT"
        }))
        .send().await
        .map_err(|error| format!("Xbox 账户验证失败：{error}"))?
        .error_for_status().map_err(|error| format!("Xbox 账户验证被拒绝：{error}"))?
        .json::<XboxTokenResponse>().await
        .map_err(|error| format!("Xbox 账户验证返回内容无效：{error}"))?;
    let uhs = xbox_user
        .display_claims
        .xui
        .first()
        .and_then(|claim| claim.uhs.clone())
        .ok_or_else(|| "Xbox 账户缺少用户标识。".to_string())?;
    let xsts = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&serde_json::json!({
            "Properties": {"SandboxId":"RETAIL","UserTokens":[xbox_user.token]},
            "RelyingParty":"rp://api.minecraftservices.com/",
            "TokenType":"JWT"
        }))
        .send()
        .await
        .map_err(|error| format!("Xbox XSTS 验证失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("Xbox XSTS 返回错误：{error}"))?
        .json::<XboxTokenResponse>()
        .await
        .map_err(|error| format!("Xbox XSTS 返回内容无效：{error}"))?;
    let minecraft = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&serde_json::json!({"identityToken":format!("XBL3.0 x={};{}", uhs, xsts.token)}))
        .send()
        .await
        .map_err(|error| format!("Minecraft Services 验证失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("Minecraft 资格验证失败：{error}"))?
        .json::<MinecraftTokenResponse>()
        .await
        .map_err(|error| format!("Minecraft Services 返回内容无效：{error}"))?;
    let profile = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&minecraft.access_token)
        .send()
        .await
        .map_err(|error| format!("读取 Minecraft Profile 失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("没有找到 Minecraft 资格或 Profile：{error}"))?
        .json::<ProfileResponse>()
        .await
        .map_err(|error| format!("Minecraft Profile 返回内容无效：{error}"))?;
    Ok(LoginResult {
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

struct Callback {
    code: Option<String>,
    state: String,
}

fn receive_callback(listener: TcpListener) -> Result<Callback, String> {
    let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 8192];
    let count = stream
        .read(&mut buffer)
        .map_err(|error| error.to_string())?;
    let request = String::from_utf8_lossy(&buffer[..count]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| "登录回调请求格式无效".to_string())?;
    let url = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|error| format!("登录回调地址无效：{error}"))?;
    let code = url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned());
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default();
    let body = if code.is_some() {
        "登录完成，可以回到 SH 启动器。"
    } else {
        "登录未完成，请关闭此页面后重试。"
    };
    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = stream.write_all(response.as_bytes());
    Ok(Callback { code, state })
}
