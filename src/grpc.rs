use std::net::SocketAddr;
use std::pin::Pin;

use anyhow::{Context, Result};
use futures_util::{Stream, StreamExt};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};

use crate::live::{
    ComboSendData, DanmuMsg, GiftData, GiftStarProcessData, InteractMsg, LiveEvent,
    OnlineRankCountData, PopularityMsg, SuperChatMsgData, ToastMsgData,
};

#[allow(dead_code)]
pub mod pb {
    tonic::include_proto!("live");
}

#[derive(Clone)]
struct LiveServiceImpl {
    tx: broadcast::Sender<pb::LiveEvent>,
}

#[tonic::async_trait]
impl pb::live_service_server::LiveService for LiveServiceImpl {
    type SubscribeStream =
        Pin<Box<dyn Stream<Item = Result<pb::LiveEvent, Status>> + Send + 'static>>;

    async fn subscribe(
        &self,
        _request: Request<pb::Empty>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let stream = BroadcastStream::new(self.tx.subscribe()).filter_map(|item| async move {
            match item {
                Ok(event) => Some(Ok(event)),
                Err(BroadcastStreamRecvError::Lagged(_)) => None,
            }
        });

        Ok(Response::new(Box::pin(stream)))
    }
}

pub struct GrpcServerHandle {
    tx: broadcast::Sender<pb::LiveEvent>,
    shutdown: CancellationToken,
    join: JoinHandle<()>,
}

impl GrpcServerHandle {
    pub async fn start(port: u16) -> Result<Self> {
        let address: SocketAddr = format!("0.0.0.0:{port}")
            .parse()
            .with_context(|| format!("invalid gRPC listen address for port {port}"))?;
        let (tx, _) = broadcast::channel(128);
        let shutdown = CancellationToken::new();
        let shutdown_signal = shutdown.clone();
        let service = LiveServiceImpl { tx: tx.clone() };

        let join = tokio::spawn(async move {
            let result = tonic::transport::Server::builder()
                .add_service(pb::live_service_server::LiveServiceServer::new(service))
                .serve_with_shutdown(address, async move {
                    shutdown_signal.cancelled().await;
                })
                .await;

            if let Err(err) = result {
                eprintln!("gRPC server error on {address}: {err}");
            }
        });

        Ok(Self { tx, shutdown, join })
    }

    pub fn dispatch(&self, event: &LiveEvent) {
        if let Some(proto_event) = map_to_proto(event) {
            let _ = self.tx.send(proto_event);
        }
    }

    pub async fn stop(self) {
        self.shutdown.cancel();
        let _ = self.join.await;
    }
}

fn map_to_proto(event: &LiveEvent) -> Option<pb::LiveEvent> {
    match event {
        LiveEvent::Danmu(data) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::Danmu(map_danmu(data))),
        }),
        LiveEvent::Popularity(data) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::Popularity(map_popularity(data))),
        }),
        LiveEvent::Gift(data) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::Gift(map_gift(data))),
        }),
        LiveEvent::SysMsg(message) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::SysMsg(message.clone())),
        }),
        LiveEvent::Error(message) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::Error(message.clone())),
        }),
        LiveEvent::SuperChat(data) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::SuperChat(map_super_chat(data))),
        }),
        LiveEvent::Interaction(data) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::Interaction(map_interaction(data))),
        }),
        LiveEvent::Toast(data) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::Toast(map_toast(data))),
        }),
        LiveEvent::GiftStarProcess(data) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::GiftStarProcess(
                map_gift_star_process(data),
            )),
        }),
        LiveEvent::OnlineRankCount(data) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::OnlineRankCount(
                map_online_rank_count(data),
            )),
        }),
        LiveEvent::ComboSend(data) => Some(pb::LiveEvent {
            payload: Some(pb::live_event::Payload::ComboSend(map_combo_send(data))),
        }),
    }
}

fn map_danmu(data: &DanmuMsg) -> pb::DanmuMsg {
    pb::DanmuMsg {
        content: data.content.clone(),
        user_id: data.user_id,
        nickname: data.nickname.clone(),
        medal_name: data.medal_name.clone(),
        medal_level: data.medal_level,
    }
}

fn map_popularity(data: &PopularityMsg) -> pb::PopularityMsg {
    pb::PopularityMsg {
        popularity: data.popularity,
    }
}

fn map_gift(data: &GiftData) -> pb::GiftData {
    pb::GiftData {
        uid: data.uid,
        uname: data.uname.clone(),
        face: data.face.clone(),
        gift_name: data.gift_name.clone(),
        gift_num: data.gift_num,
        price: data.price,
        combo_total_coin: data.combo_total_coin,
        total_coin: data.total_coin,
        coin_type: data.coin_type.clone(),
        action: data.action.clone(),
        gift_info: Some(pb::GiftInfo {
            img_basic: data.gift_info.img_basic.clone(),
            gif: data.gift_info.gif.clone(),
        }),
        medal_info: Some(pb::MedalInfo {
            medal_name: data.medal_info.medal_name.clone(),
            medal_level: data.medal_info.medal_level,
        }),
        combo_send: Some(pb::ComboSend {
            combo_id: data.combo_send.combo_id.clone(),
            combo_num: data.combo_send.combo_num,
        }),
    }
}

fn map_super_chat(data: &SuperChatMsgData) -> pb::SuperChatMsg {
    pb::SuperChatMsg {
        medal_info: Some(pb::MedalInfo {
            medal_name: data.medal_info.medal_name.clone(),
            medal_level: data.medal_info.medal_level,
        }),
        message: data.message.clone(),
        font_color: data.font_color.clone(),
        price: data.price,
        user_info: Some(pb::UserInfo {
            face: data.user_info.face.clone(),
            uname: data.user_info.uname.clone(),
        }),
        start_time: data.start_time,
        end_time: data.end_time,
    }
}

fn map_interaction(data: &InteractMsg) -> pb::InteractMsg {
    pb::InteractMsg {
        id: data.id,
        status: data.status,
        r#type: data.kind,
        data: data.data.to_string(),
    }
}

fn map_toast(data: &ToastMsgData) -> pb::ToastMsg {
    pb::ToastMsg {
        guard_level: data.guard_level,
        username: data.username.clone(),
        price: data.price,
        uid: data.uid,
        num: data.num,
        unit: data.unit.clone(),
        role_name: data.role_name.clone(),
    }
}

fn map_gift_star_process(data: &GiftStarProcessData) -> pb::GiftStarProcessMsg {
    pb::GiftStarProcessMsg {
        message: data.message.clone(),
    }
}

fn map_online_rank_count(data: &OnlineRankCountData) -> pb::OnlineRankCountMsg {
    pb::OnlineRankCountMsg {
        count: data.count,
        count_text: data.count_text.clone(),
        online_count: data.online_count,
        online_count_text: data.online_count_text.clone(),
    }
}

fn map_combo_send(data: &ComboSendData) -> pb::ComboSendData {
    pb::ComboSendData {
        action: data.action.clone(),
        batch_combo_id: data.batch_combo_id.clone(),
        batch_combo_num: data.batch_combo_num,
        combo_id: data.combo_id.clone(),
        combo_num: data.combo_num,
        combo_total_coin: data.combo_total_coin,
        dmscore: data.dmscore,
        gift_id: data.gift_id,
        gift_name: data.gift_name.clone(),
        gift_num: data.gift_num,
        is_join_receiver: data.is_join_receiver,
        is_naming: data.is_naming,
        is_show: data.is_show,
        medal_info: Some(pb::MedalInfo {
            medal_name: data.medal_info.medal_name.clone(),
            medal_level: data.medal_info.medal_level,
        }),
        name_color: data.name_color.clone(),
        r_uname: data.r_uname.clone(),
        receive_user_info: Some(pb::UserInfo {
            face: String::new(),
            uname: data.receive_user_info.uname.clone(),
        }),
        ruid: data.ruid,
        total_num: data.total_num,
        uid: data.uid,
        uname: data.uname.clone(),
    }
}
