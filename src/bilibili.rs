use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use cookie::Cookie;
use rand::thread_rng;
use reqwest::header::{
    ACCEPT, COOKIE, HeaderMap, HeaderValue, ORIGIN, REFERER, SET_COOKIE, USER_AGENT,
};
use rsa::{Oaep, RsaPublicKey, pkcs8::DecodePublicKey};
use serde::Deserialize;
use sha2::Sha256;

const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

const PUBLIC_KEY_PEM: &str = r#"
-----BEGIN PUBLIC KEY-----
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDLgd2OAkcGVtoE3ThUREbio0Eg
Uc/prcajMKXvkCKFCWhJYJcLkcM2DKKcSeFpD/j6Boy538YXnR6VhcuUJOhH2x71
nzPjfdTcqMz7djHum0qSZA0AyCBDABUqCrfNgCiJ00Ra7GmRj+YCK1NJEuewlb40
JNrRuoEUXpabUzGB8QIDAQAB
-----END PUBLIC KEY-----
"#;

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub uid: u64,
    pub buvid3: String,
    pub cookie: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct HostInfo {
    pub host: String,
    pub port: u16,
    pub wss_port: u16,
    pub ws_port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DanmuInfoResponse {
    pub code: i32,
    pub message: String,
    pub data: DanmuInfoData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DanmuInfoData {
    pub token: String,
    pub host_list: Vec<HostInfo>,
}

#[derive(Debug, Clone)]
pub struct BiliClient {
    http: reqwest::Client,
}

impl BiliClient {
    pub fn new() -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_USER_AGENT));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://www.bilibili.com/"),
        );
        headers.insert(ORIGIN, HeaderValue::from_static("https://www.bilibili.com"));

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(10))
            .build()
            .context("创建 HTTP 客户端失败")?;

        Ok(Self { http })
    }

    pub fn get_cookie_value(cookie: &str, name: &str) -> Option<String> {
        parse_cookie_string(cookie).remove(name)
    }

    pub async fn prepare_auth(&self, cookie: &str) -> Result<AuthContext> {
        let uid = Self::get_cookie_value(cookie, "DedeUserID")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_default();

        let buvid3 = if let Some(buvid3) = Self::get_cookie_value(cookie, "buvid3") {
            buvid3
        } else if cookie.trim().is_empty() {
            self.get_guest_buvid3().await?
        } else {
            bail!("buvid3 cookie not found")
        };

        Ok(AuthContext {
            uid,
            buvid3,
            cookie: cookie.to_owned(),
        })
    }

    pub async fn get_guest_buvid3(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct GuestBuvidResponse {
            code: i32,
            data: GuestBuvidData,
        }

        #[derive(Deserialize)]
        struct GuestBuvidData {
            buvid: String,
        }

        let response = self
            .http
            .get("https://api.bilibili.com/x/web-frontend/getbuvid")
            .send()
            .await
            .context("请求游客 buvid 失败")?
            .error_for_status()
            .context("guest buvid api returned error status")?;

        let payload = response
            .json::<GuestBuvidResponse>()
            .await
            .context("解析游客 buvid 响应失败")?;

        if payload.code != 0 {
            bail!("guest buvid api returned code {}", payload.code);
        }

        Ok(payload.data.buvid)
    }

    pub async fn get_real_room_id(&self, room_id: u64, cookie: &str) -> Result<u64> {
        #[derive(Deserialize)]
        struct RoomInitResponse {
            code: i32,
            message: String,
            data: RoomInitData,
        }

        #[derive(Deserialize)]
        struct RoomInitData {
            room_id: u64,
        }

        let response = self
            .request(
                self.http.get(format!(
                    "https://api.live.bilibili.com/room/v1/Room/room_init?id={room_id}"
                )),
                cookie,
            )
            .send()
            .await
            .context("请求 room_init 失败")?
            .error_for_status()
            .context("room_init returned error status")?;

        let payload = response
            .json::<RoomInitResponse>()
            .await
            .context("解析 room_init 响应失败")?;

        if payload.code != 0 {
            bail!(
                "room_init api returned code {}: {}",
                payload.code,
                payload.message
            );
        }

        Ok(payload.data.room_id)
    }

    pub async fn get_danmu_info(&self, room_id: u64, cookie: &str) -> Result<DanmuInfoResponse> {
        let query = self
            .sign_query(&[
                ("id", room_id.to_string()),
                ("type", "0".to_owned()),
                ("wts", Utc::now().timestamp().to_string()),
            ])
            .await?;

        let response = self
            .request(
                self.http.get(format!(
                    "https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?{query}"
                )),
                cookie,
            )
            .send()
            .await
            .context("请求 getDanmuInfo 失败")?
            .error_for_status()
            .context("getDanmuInfo returned error status")?;

        let payload = response
            .json::<DanmuInfoResponse>()
            .await
            .context("解析 getDanmuInfo 响应失败")?;

        if payload.code != 0 {
            bail!(
                "getDanmuInfo api returned code {}: {}",
                payload.code,
                payload.message
            );
        }

        Ok(payload)
    }

    pub async fn check_and_refresh_cookie(
        &self,
        refresh_token: &str,
        cookie: &str,
    ) -> Result<Option<(String, String)>> {
        if refresh_token.trim().is_empty() || cookie.trim().is_empty() {
            return Ok(None);
        }

        let mut cookie_map = parse_cookie_string(cookie);
        let mut cookie_string = build_cookie_string(&cookie_map);

        if !self.need_refresh(&cookie_string).await? {
            return Ok(None);
        }

        let timestamp = Utc::now().timestamp_millis();
        let correspond_path = build_correspond_path(timestamp)?;
        let refresh_csrf = self
            .get_refresh_csrf(&correspond_path, &cookie_string)
            .await?;

        let csrf = cookie_map
            .get("bili_jct")
            .cloned()
            .ok_or_else(|| anyhow!("csrf token bili_jct not found in cookie"))?;

        let new_refresh_token = self
            .refresh_cookie(&csrf, &refresh_csrf, refresh_token, &mut cookie_map)
            .await?;
        cookie_string = build_cookie_string(&cookie_map);

        let new_csrf = cookie_map
            .get("bili_jct")
            .cloned()
            .ok_or_else(|| anyhow!("csrf token bili_jct missing after refresh"))?;

        self.confirm_refresh(&new_csrf, refresh_token, &cookie_string)
            .await?;

        Ok(Some((build_cookie_string(&cookie_map), new_refresh_token)))
    }

    async fn need_refresh(&self, cookie: &str) -> Result<bool> {
        #[derive(Deserialize)]
        struct CookieInfoResponse {
            code: i32,
            data: CookieInfoData,
        }

        #[derive(Deserialize)]
        struct CookieInfoData {
            refresh: bool,
        }

        let response = self
            .request(
                self.http
                    .get("https://passport.bilibili.com/x/passport-login/web/cookie/info"),
                cookie,
            )
            .send()
            .await
            .context("请求 cookie/info 失败")?
            .error_for_status()
            .context("cookie/info returned error status")?;

        let payload = response
            .json::<CookieInfoResponse>()
            .await
            .context("解析 cookie/info 响应失败")?;

        if payload.code != 0 {
            bail!("cookie/info api returned code {}", payload.code);
        }

        Ok(payload.data.refresh)
    }

    async fn get_refresh_csrf(&self, correspond_path: &str, cookie: &str) -> Result<String> {
        let response = self
            .request(
                self.http.get(format!(
                    "https://www.bilibili.com/correspond/1/{correspond_path}"
                )),
                cookie,
            )
            .send()
            .await
            .context("请求 correspond 页面失败")?
            .error_for_status()
            .context("correspond page returned error status")?;

        let html = response.text().await.context("读取 correspond 页面失败")?;

        let marker = r#"<div id="1-name">"#;
        let start = html
            .find(marker)
            .ok_or_else(|| anyhow!("refresh_csrf marker not found"))?
            + marker.len();
        let end = html[start..]
            .find("</div>")
            .ok_or_else(|| anyhow!("refresh_csrf end marker not found"))?
            + start;

        Ok(html[start..end].to_owned())
    }

    async fn refresh_cookie(
        &self,
        csrf: &str,
        refresh_csrf: &str,
        refresh_token: &str,
        cookie_map: &mut BTreeMap<String, String>,
    ) -> Result<String> {
        #[derive(Deserialize)]
        struct RefreshCookieResponse {
            code: i32,
            message: String,
            data: RefreshCookieData,
        }

        #[derive(Deserialize)]
        struct RefreshCookieData {
            refresh_token: String,
        }

        let cookie_string = build_cookie_string(cookie_map);
        let response = self
            .request(
                self.http
                    .post("https://passport.bilibili.com/x/passport-login/web/cookie/refresh")
                    .form(&[
                        ("csrf", csrf),
                        ("refresh_csrf", refresh_csrf),
                        ("source", "main_web"),
                        ("refresh_token", refresh_token),
                    ]),
                &cookie_string,
            )
            .send()
            .await
            .context("请求 Cookie 刷新失败")?
            .error_for_status()
            .context("cookie refresh returned error status")?;

        let headers = response.headers().clone();
        let payload = response
            .json::<RefreshCookieResponse>()
            .await
            .context("解析 Cookie 刷新响应失败")?;

        if payload.code != 0 {
            bail!(
                "cookie refresh api returned code {}: {}",
                payload.code,
                payload.message
            );
        }

        apply_set_cookie_headers(cookie_map, &headers);
        Ok(payload.data.refresh_token)
    }

    async fn confirm_refresh(&self, csrf: &str, refresh_token: &str, cookie: &str) -> Result<()> {
        #[derive(Deserialize)]
        struct ConfirmRefreshResponse {
            code: i32,
            message: String,
        }

        let response = self
            .request(
                self.http
                    .post("https://passport.bilibili.com/x/passport-login/web/confirm/refresh")
                    .form(&[("csrf", csrf), ("refresh_token", refresh_token)]),
                cookie,
            )
            .send()
            .await
            .context("请求确认刷新失败")?
            .error_for_status()
            .context("confirm refresh returned error status")?;

        let payload = response
            .json::<ConfirmRefreshResponse>()
            .await
            .context("解析确认刷新响应失败")?;

        if payload.code != 0 {
            bail!(
                "confirm refresh api returned code {}: {}",
                payload.code,
                payload.message
            );
        }

        Ok(())
    }

    async fn sign_query(&self, params: &[(&str, String)]) -> Result<String> {
        let keys = self.fetch_wbi_keys().await?;
        let mixin = mixin_key(&keys.0, &keys.1)?;

        let mut values = BTreeMap::new();
        for (key, value) in params {
            values.insert((*key).to_owned(), sanitize_value(value));
        }

        let encoded = encode_query(&values);
        let digest = format!("{:x}", md5::compute(format!("{encoded}{mixin}")));
        values.insert("w_rid".to_owned(), digest);
        Ok(encode_query(&values))
    }

    async fn fetch_wbi_keys(&self) -> Result<(String, String)> {
        #[derive(Deserialize)]
        struct NavResponse {
            code: i32,
            message: String,
            data: NavData,
        }

        #[derive(Deserialize)]
        struct NavData {
            wbi_img: WbiImg,
        }

        #[derive(Deserialize)]
        struct WbiImg {
            img_url: String,
            sub_url: String,
        }

        let response = self
            .http
            .get("https://api.bilibili.com/x/web-interface/nav")
            .send()
            .await
            .context("请求 nav 接口失败")?
            .error_for_status()
            .context("nav returned error status")?;

        let payload = response
            .json::<NavResponse>()
            .await
            .context("解析 nav 响应失败")?;

        if payload.code != 0 && payload.code != -101 {
            bail!(
                "nav api returned code {}: {}",
                payload.code,
                payload.message
            );
        }

        let img = payload
            .data
            .wbi_img
            .img_url
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".png")
            .to_owned();
        let sub = payload
            .data
            .wbi_img
            .sub_url
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".png")
            .to_owned();

        if img.is_empty() || sub.is_empty() {
            bail!("wbi keys are empty");
        }

        Ok((img, sub))
    }

    fn request(&self, request: reqwest::RequestBuilder, cookie: &str) -> reqwest::RequestBuilder {
        if cookie.trim().is_empty() {
            request
        } else {
            request.header(COOKIE, cookie.to_owned())
        }
    }
}

fn parse_cookie_string(cookie: &str) -> BTreeMap<String, String> {
    cookie
        .split(';')
        .filter_map(|segment| {
            let trimmed = segment.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (name, value) = trimmed.split_once('=')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn build_cookie_string(cookie_map: &BTreeMap<String, String>) -> String {
    cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn apply_set_cookie_headers(cookie_map: &mut BTreeMap<String, String>, headers: &HeaderMap) {
    for value in headers.get_all(SET_COOKIE) {
        let Ok(header_value) = value.to_str() else {
            continue;
        };

        if let Ok(parsed) = Cookie::parse(header_value.to_owned()) {
            cookie_map.insert(parsed.name().to_owned(), parsed.value().to_owned());
            continue;
        }

        if let Some((name, value)) = header_value
            .split(';')
            .next()
            .and_then(|raw| raw.split_once('='))
        {
            cookie_map.insert(name.trim().to_owned(), value.trim().to_owned());
        }
    }
}

fn sanitize_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '!' | '\'' | '(' | ')' | '*'))
        .collect()
}

fn encode_query(values: &BTreeMap<String, String>) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in values {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn mixin_key(img: &str, sub: &str) -> Result<String> {
    const MIXIN_KEY_ENC_TAB: [usize; 64] = [
        46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19,
        29, 28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4,
        22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
    ];

    let mixed_source = format!("{img}{sub}");
    let bytes = mixed_source.as_bytes();
    if bytes.len() <= MIXIN_KEY_ENC_TAB.iter().copied().max().unwrap_or_default() {
        bail!("wbi key source is shorter than expected");
    }

    let mixed = MIXIN_KEY_ENC_TAB
        .iter()
        .map(|&index| bytes[index] as char)
        .take(32)
        .collect::<String>();

    Ok(mixed)
}

fn build_correspond_path(timestamp: i64) -> Result<String> {
    let public_key = RsaPublicKey::from_public_key_pem(PUBLIC_KEY_PEM).context("解析公钥失败")?;
    let message = format!("refresh_{timestamp}");
    let encrypted = public_key
        .encrypt(&mut thread_rng(), Oaep::new::<Sha256>(), message.as_bytes())
        .context("加密 correspond 路径失败")?;

    Ok(hex::encode(encrypted))
}
