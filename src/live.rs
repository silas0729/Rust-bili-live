use std::io::{Cursor, Read};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use brotli::Decompressor;
use flate2::read::ZlibDecoder;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_util::sync::CancellationToken;

use crate::bilibili::{AuthContext, BiliClient, HostInfo};

const HEADER_LENGTH: usize = 16;
const PROTO_RAW: u16 = 0;
const PROTO_INT: u16 = 1;
const PROTO_ZLIB: u16 = 2;
const PROTO_BROTLI: u16 = 3;
const OP_HEARTBEAT: u32 = 2;
const OP_HEARTBEAT_REPLY: u32 = 3;
const OP_SEND_MSG_REPLY: u32 = 5;
const OP_AUTH: u32 = 7;
const OP_AUTH_REPLY: u32 = 8;

#[derive(Debug, Clone)]
pub enum LiveEvent {
    Danmu(DanmuMsg),
    Popularity(PopularityMsg),
    Gift(GiftData),
    ComboSend(ComboSendData),
    SysMsg(String),
    Error(String),
    SuperChat(SuperChatMsgData),
    Interaction(InteractMsg),
    Toast(ToastMsgData),
    GiftStarProcess(GiftStarProcessData),
    OnlineRankCount(OnlineRankCountData),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DanmuMsg {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub user_id: i64,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub medal_name: String,
    #[serde(default)]
    pub medal_level: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PopularityMsg {
    pub popularity: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GiftData {
    #[serde(default)]
    pub uid: i64,
    #[serde(default)]
    pub uname: String,
    #[serde(default)]
    pub face: String,
    #[serde(default, alias = "giftName")]
    pub gift_name: String,
    #[serde(default, alias = "num")]
    pub gift_num: i32,
    #[serde(default)]
    pub price: f64,
    #[serde(default)]
    pub combo_total_coin: i32,
    #[serde(default)]
    pub total_coin: i32,
    #[serde(default)]
    pub coin_type: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub gift_info: GiftInfo,
    #[serde(default)]
    pub medal_info: MedalInfo,
    #[serde(default)]
    pub combo_send: ComboSend,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GiftInfo {
    #[serde(default)]
    pub img_basic: String,
    #[serde(default)]
    pub gif: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MedalInfo {
    #[serde(default)]
    pub medal_name: String,
    #[serde(default)]
    pub medal_level: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComboSend {
    #[serde(default)]
    pub combo_id: String,
    #[serde(default)]
    pub combo_num: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuperChatMsgData {
    #[serde(default)]
    pub medal_info: MedalInfo,
    #[serde(default)]
    pub message: String,
    #[serde(default, rename = "message_font_color")]
    pub font_color: String,
    #[serde(default)]
    pub price: i32,
    #[serde(default)]
    pub user_info: UserInfo,
    #[serde(default)]
    pub start_time: i64,
    #[serde(default)]
    pub end_time: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserInfo {
    #[serde(default)]
    pub face: String,
    #[serde(default, alias = "uname")]
    pub uname: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InteractMsg {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub status: i32,
    #[serde(default, rename = "type")]
    pub kind: i32,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToastMsgData {
    #[serde(default)]
    pub guard_level: i32,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub price: i32,
    #[serde(default)]
    pub uid: i64,
    #[serde(default)]
    pub num: i32,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub role_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GiftStarProcessData {
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineRankCountData {
    #[serde(default)]
    pub count: i32,
    #[serde(default)]
    pub count_text: String,
    #[serde(default)]
    pub online_count: i32,
    #[serde(default)]
    pub online_count_text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComboSendData {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub batch_combo_id: String,
    #[serde(default)]
    pub batch_combo_num: i32,
    #[serde(default)]
    pub combo_id: String,
    #[serde(default)]
    pub combo_num: i32,
    #[serde(default)]
    pub combo_total_coin: i32,
    #[serde(default)]
    pub dmscore: i32,
    #[serde(default)]
    pub gift_id: i32,
    #[serde(default)]
    pub gift_name: String,
    #[serde(default)]
    pub gift_num: i32,
    #[serde(default)]
    pub is_join_receiver: bool,
    #[serde(default)]
    pub is_naming: bool,
    #[serde(default)]
    pub is_show: i32,
    #[serde(default)]
    pub medal_info: MedalInfo,
    #[serde(default)]
    pub name_color: String,
    #[serde(default, rename = "r_uname")]
    pub r_uname: String,
    #[serde(default)]
    pub receive_user_info: ReceiveUserInfo,
    #[serde(default)]
    pub ruid: i64,
    #[serde(default)]
    pub total_num: i32,
    #[serde(default)]
    pub uid: i64,
    #[serde(default)]
    pub uname: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReceiveUserInfo {
    #[serde(default)]
    pub uid: i64,
    #[serde(default)]
    pub uname: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InteractData102 {
    #[serde(default)]
    pub combo: Vec<InteractCombo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InteractCombo {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub cnt: i32,
    #[serde(default)]
    pub guide: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InteractDataNotice {
    #[serde(default)]
    pub cnt: i32,
    #[serde(default)]
    pub suffix_text: String,
    #[serde(default)]
    pub gift_id: i32,
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub room_id: u64,
    pub cookie: String,
}

#[derive(Debug, Clone, Copy)]
struct PacketHeader {
    packet_len: usize,
    proto_ver: u16,
    operation: u32,
}

#[derive(Debug, Deserialize)]
struct BaseCmd {
    cmd: String,
}

#[derive(Debug, Serialize)]
struct AuthPayload {
    uid: u64,
    roomid: u64,
    protover: i32,
    platform: String,
    #[serde(rename = "type")]
    kind: i32,
    key: String,
    buvid: String,
}

pub async fn run_session(
    api: BiliClient,
    config: SessionConfig,
    event_tx: UnboundedSender<LiveEvent>,
    cancel: CancellationToken,
) -> Result<()> {
    let auth_context = api.prepare_auth(&config.cookie).await?;
    let real_room_id = api.get_real_room_id(config.room_id, &config.cookie).await?;
    let danmu_info = api
        .get_danmu_info_data(real_room_id, &config.cookie)
        .await?;

    connect_loop(
        real_room_id,
        &danmu_info.host_list,
        &danmu_info.token,
        &auth_context,
        event_tx,
        cancel,
    )
    .await;

    Ok(())
}

async fn connect_loop(
    room_id: u64,
    hosts: &[HostInfo],
    token: &str,
    auth_context: &AuthContext,
    event_tx: UnboundedSender<LiveEvent>,
    cancel: CancellationToken,
) {
    for (index, host) in hosts.iter().enumerate() {
        if cancel.is_cancelled() {
            return;
        }

        let _ = event_tx.send(LiveEvent::SysMsg(format!(
            "[Yuuna-Danmu] 正在连接线路 [{}/{}]：{}:{}",
            index + 1,
            hosts.len(),
            host.host,
            host.wss_port
        )));

        let result = run_client(
            room_id,
            host,
            token,
            auth_context,
            &event_tx,
            cancel.clone(),
        )
        .await;
        if cancel.is_cancelled() {
            return;
        }

        if let Err(err) = result {
            let _ = event_tx.send(LiveEvent::Error(format!(
                "[Yuuna-Danmu] 与 {}:{} 断开连接：{err}",
                host.host, host.wss_port
            )));
            let _ = event_tx.send(LiveEvent::SysMsg(
                "[Yuuna-Danmu] 正在切换到下一条线路…".to_owned(),
            ));
            sleep(Duration::from_secs(3)).await;
        } else {
            sleep(Duration::from_secs(1)).await;
        }
    }

    let _ = event_tx.send(LiveEvent::Error(
        "[Yuuna-Danmu] 所有线路均已尝试，直播连接已停止。".to_owned(),
    ));
}

async fn run_client(
    room_id: u64,
    host: &HostInfo,
    token: &str,
    auth_context: &AuthContext,
    event_tx: &UnboundedSender<LiveEvent>,
    cancel: CancellationToken,
) -> Result<()> {
    let uri = format!("wss://{}:{}/sub", host.host, host.wss_port);
    let mut request = uri
        .into_client_request()
        .context("构建 WebSocket 请求失败")?;
    request.headers_mut().insert(
        "User-Agent",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            .parse()
            .context("设置 WebSocket User-Agent 失败")?,
    );
    request.headers_mut().insert(
        "Origin",
        "https://live.bilibili.com"
            .parse()
            .context("设置 WebSocket Origin 失败")?,
    );
    request.headers_mut().insert(
        "Referer",
        "https://live.bilibili.com/"
            .parse()
            .context("设置 WebSocket Referer 失败")?,
    );
    if !auth_context.cookie.trim().is_empty() {
        request.headers_mut().insert(
            "Cookie",
            auth_context
                .cookie
                .parse()
                .context("设置 WebSocket Cookie 失败")?,
        );
    }

    let (stream, _) = connect_async(request)
        .await
        .context("连接 WebSocket 失败")?;
    let (mut write, mut read) = stream.split();

    let auth_payload = AuthPayload {
        uid: auth_context.uid,
        roomid: room_id,
        protover: PROTO_BROTLI as i32,
        platform: "web".to_owned(),
        kind: 2,
        key: token.to_owned(),
        buvid: auth_context.buvid3.clone(),
    };

    let auth_packet = pack_packet(OP_AUTH, &serde_json::to_vec(&auth_payload)?);
    write
        .send(Message::Binary(auth_packet.into()))
        .await
        .context("发送鉴权数据失败")?;

    let heartbeat_cancel = cancel.child_token();
    let heartbeat_stop = heartbeat_cancel.clone();
    let heartbeat_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = heartbeat_stop.cancelled() => break,
                _ = interval.tick() => {
                    let packet = pack_packet(OP_HEARTBEAT, b"[object Object]");
                    if write.send(Message::Binary(packet.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            message = read.next() => {
                let Some(message) = message else {
                    bail!("WebSocket 已关闭");
                };

                let message = message.context("读取 WebSocket 消息失败")?;
                match message {
                    Message::Binary(bytes) => handle_packets(&bytes, event_tx)?,
                    Message::Close(_) => bail!("直播服务器主动关闭了连接"),
                    _ => {}
                }
            }
        }
    }

    heartbeat_cancel.cancel();
    heartbeat_task.abort();
    Ok(())
}

fn handle_packets(bytes: &[u8], event_tx: &UnboundedSender<LiveEvent>) -> Result<()> {
    let mut offset = 0;
    while offset + HEADER_LENGTH <= bytes.len() {
        let header = parse_header(&bytes[offset..offset + HEADER_LENGTH])?;
        if offset + header.packet_len > bytes.len() {
            break;
        }

        let body = &bytes[offset + HEADER_LENGTH..offset + header.packet_len];
        match header.proto_ver {
            PROTO_RAW | PROTO_INT => route_operation(header.operation, body, event_tx)?,
            PROTO_BROTLI => {
                let decompressed = decompress_brotli(body)?;
                handle_packets(&decompressed, event_tx)?;
            }
            PROTO_ZLIB => {
                let decompressed = decompress_zlib(body)?;
                handle_packets(&decompressed, event_tx)?;
            }
            version => {
                let _ = event_tx.send(LiveEvent::Error(format!(
                    "[Yuuna-Danmu] 不支持的协议版本 {version}"
                )));
            }
        }

        offset += header.packet_len;
    }

    Ok(())
}

fn route_operation(
    operation: u32,
    body: &[u8],
    event_tx: &UnboundedSender<LiveEvent>,
) -> Result<()> {
    match operation {
        OP_HEARTBEAT_REPLY => {
            if body.len() >= 4 {
                let popularity = i32::from_be_bytes(body[0..4].try_into().unwrap_or([0; 4]));
                let _ = event_tx.send(LiveEvent::Popularity(PopularityMsg { popularity }));
            }
        }
        OP_SEND_MSG_REPLY => dispatch_command(body, event_tx)?,
        OP_AUTH_REPLY => {
            let _ = event_tx.send(LiveEvent::SysMsg("已连接到直播间".to_owned()));
        }
        _ => {}
    }

    Ok(())
}

fn dispatch_command(body: &[u8], event_tx: &UnboundedSender<LiveEvent>) -> Result<()> {
    let base = serde_json::from_slice::<BaseCmd>(body).context("解析直播消息类型失败")?;
    let command = base.cmd.split(':').next().unwrap_or(base.cmd.as_str());

    match command {
        "DANMU_MSG" => {
            if let Some(data) = parse_danmu(body) {
                let _ = event_tx.send(LiveEvent::Danmu(data));
            }
        }
        "SEND_GIFT" => {
            if let Some(data) = parse_json_data::<GiftData>(body) {
                let _ = event_tx.send(LiveEvent::Gift(data));
            }
        }
        "COMBO_SEND" => {
            if let Some(data) = parse_json_data::<ComboSendData>(body) {
                let _ = event_tx.send(LiveEvent::ComboSend(data));
            }
        }
        "SUPER_CHAT_MESSAGE" => {
            if let Some(data) = parse_json_data::<SuperChatMsgData>(body) {
                let _ = event_tx.send(LiveEvent::SuperChat(data));
            }
        }
        "DM_INTERACTION" => {
            if let Some(data) = parse_json_data::<InteractMsg>(body) {
                let _ = event_tx.send(LiveEvent::Interaction(data));
            }
        }
        "USER_TOAST_MSG" => {
            if let Some(data) = parse_json_data::<ToastMsgData>(body) {
                let _ = event_tx.send(LiveEvent::Toast(data));
            }
        }
        "GIFT_STAR_PROCESS" => {
            if let Some(data) = parse_json_data::<GiftStarProcessData>(body) {
                let _ = event_tx.send(LiveEvent::GiftStarProcess(data));
            }
        }
        "ONLINE_RANK_COUNT" => {
            if let Some(data) = parse_json_data::<OnlineRankCountData>(body) {
                let _ = event_tx.send(LiveEvent::OnlineRankCount(data));
            }
        }
        _ => {}
    }

    Ok(())
}

fn parse_json_data<T>(body: &[u8]) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    #[derive(Deserialize)]
    struct Envelope<T> {
        data: T,
    }

    serde_json::from_slice::<Envelope<T>>(body)
        .ok()
        .map(|raw| raw.data)
}

fn parse_danmu(body: &[u8]) -> Option<DanmuMsg> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let info = value.get("info")?.as_array()?;
    let content = info.get(1)?.as_str()?.to_owned();

    let user = info.get(2)?.as_array()?;
    let user_id = user
        .first()?
        .as_i64()
        .or_else(|| user.first()?.as_u64().map(|v| v as i64))?;
    let nickname = user.get(1)?.as_str()?.to_owned();

    let (medal_name, medal_level) = info
        .get(3)
        .and_then(|value| value.as_array())
        .and_then(|medal| {
            let level = medal
                .first()?
                .as_i64()
                .or_else(|| medal.first()?.as_u64().map(|v| v as i64))?
                as i32;
            let name = medal.get(1)?.as_str()?.to_owned();
            Some((name, level))
        })
        .unwrap_or_default();

    Some(DanmuMsg {
        content,
        user_id,
        nickname,
        medal_name,
        medal_level,
    })
}

fn parse_header(data: &[u8]) -> Result<PacketHeader> {
    if data.len() < HEADER_LENGTH {
        bail!("消息包长度小于协议头长度");
    }

    Ok(PacketHeader {
        packet_len: u32::from_be_bytes(data[0..4].try_into()?) as usize,
        proto_ver: u16::from_be_bytes(data[6..8].try_into()?),
        operation: u32::from_be_bytes(data[8..12].try_into()?),
    })
}

fn pack_packet(operation: u32, body: &[u8]) -> Vec<u8> {
    let packet_len = (HEADER_LENGTH + body.len()) as u32;
    let mut output = Vec::with_capacity(HEADER_LENGTH + body.len());
    output.extend_from_slice(&packet_len.to_be_bytes());
    output.extend_from_slice(&(HEADER_LENGTH as u16).to_be_bytes());
    output.extend_from_slice(&PROTO_INT.to_be_bytes());
    output.extend_from_slice(&operation.to_be_bytes());
    output.extend_from_slice(&1_u32.to_be_bytes());
    output.extend_from_slice(body);
    output
}

fn decompress_brotli(body: &[u8]) -> Result<Vec<u8>> {
    let mut decompressor = Decompressor::new(Cursor::new(body), 4096);
    let mut output = Vec::new();
    decompressor
        .read_to_end(&mut output)
        .context("解压 Brotli 消息失败")?;
    Ok(output)
}

fn decompress_zlib(body: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(body);
    let mut output = Vec::new();
    decoder
        .read_to_end(&mut output)
        .context("解压 zlib 消息失败")?;
    Ok(output)
}
