use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyhow::{Context, Result};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::bilibili::BiliClient;
use crate::config::{AppConfig, ServerType};
use crate::grpc::GrpcServerHandle;
use crate::live::{LiveEvent, SessionConfig, run_session};

#[derive(Debug)]
pub enum BackendCommand {
    SaveConfig(AppConfig),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    Live(LiveEvent),
    ConfigUpdated(AppConfig),
}

pub struct BackendHandle {
    _runtime: Arc<Runtime>,
    command_tx: UnboundedSender<BackendCommand>,
    pub ui_rx: Receiver<UiEvent>,
}

impl BackendHandle {
    pub fn start(initial_config: AppConfig) -> Result<Self> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("创建 Tokio 运行时失败")?,
        );

        let (command_tx, command_rx) = unbounded_channel();
        let (live_tx, live_rx) = unbounded_channel();
        let (ui_tx, ui_rx) = channel();
        let api = BiliClient::new()?;

        runtime.spawn(async move {
            let mut controller =
                BackendController::new(api, initial_config, ui_tx, live_tx, live_rx);
            controller.run(command_rx).await;
        });

        Ok(Self {
            _runtime: runtime,
            command_tx,
            ui_rx,
        })
    }

    pub fn save_config(&self, config: AppConfig) -> Result<()> {
        self.command_tx
            .send(BackendCommand::SaveConfig(config))
            .context("发送保存配置命令失败")
    }

    pub fn shutdown(&self) {
        let _ = self.command_tx.send(BackendCommand::Shutdown);
    }
}

struct BackendController {
    api: BiliClient,
    config: AppConfig,
    ui_tx: Sender<UiEvent>,
    live_tx: UnboundedSender<LiveEvent>,
    live_rx: UnboundedReceiver<LiveEvent>,
    session_cancel: Option<CancellationToken>,
    session_task: Option<JoinHandle<()>>,
    grpc_servers: Vec<GrpcServerHandle>,
}

impl BackendController {
    fn new(
        api: BiliClient,
        config: AppConfig,
        ui_tx: Sender<UiEvent>,
        live_tx: UnboundedSender<LiveEvent>,
        live_rx: UnboundedReceiver<LiveEvent>,
    ) -> Self {
        Self {
            api,
            config,
            ui_tx,
            live_tx,
            live_rx,
            session_cancel: None,
            session_task: None,
            grpc_servers: Vec::new(),
        }
    }

    async fn run(&mut self, mut command_rx: UnboundedReceiver<BackendCommand>) {
        self.restart_all().await;

        loop {
            tokio::select! {
                Some(command) = command_rx.recv() => {
                    match command {
                        BackendCommand::SaveConfig(config) => {
                            self.config = config;
                            match self.config.save() {
                                Ok(()) => {
                                    let _ = self.ui_tx.send(UiEvent::ConfigUpdated(self.config.clone()));
                                    let _ = self.ui_tx.send(UiEvent::Live(LiveEvent::SysMsg("配置已保存".to_owned())));
                                    self.restart_all().await;
                                }
                                Err(err) => {
                                    let _ = self.ui_tx.send(UiEvent::Live(LiveEvent::Error(format!(
                                        "保存配置失败：{err}"
                                    ))));
                                }
                            }
                        }
                        BackendCommand::Shutdown => {
                            self.stop_all().await;
                            break;
                        }
                    }
                }
                Some(event) = self.live_rx.recv() => {
                    for grpc in &self.grpc_servers {
                        grpc.dispatch(&event);
                    }
                    let _ = self.ui_tx.send(UiEvent::Live(event));
                }
                else => {
                    self.stop_all().await;
                    break;
                }
            }
        }
    }

    async fn restart_all(&mut self) {
        self.stop_all().await;
        self.refresh_cookie_if_needed().await;
        self.start_grpc_servers().await;
        self.start_session();
        let _ = self.ui_tx.send(UiEvent::ConfigUpdated(self.config.clone()));
    }

    async fn refresh_cookie_if_needed(&mut self) {
        match self
            .api
            .check_and_refresh_cookie(&self.config.refresh_token, &self.config.cookie)
            .await
        {
            Ok(Some((cookie, refresh_token))) => {
                self.config.cookie = cookie;
                self.config.refresh_token = refresh_token;
                if let Err(err) = self.config.save() {
                    let _ = self.ui_tx.send(UiEvent::Live(LiveEvent::Error(format!(
                        "刷新 Cookie 后保存配置失败：{err}"
                    ))));
                }
            }
            Ok(None) => {}
            Err(err) => {
                let _ = self.ui_tx.send(UiEvent::Live(LiveEvent::Error(format!(
                    "Cookie 刷新失败：{err}"
                ))));
            }
        }
    }

    async fn start_grpc_servers(&mut self) {
        for server in &self.config.servers {
            if !server.enabled || server.kind != ServerType::Grpc {
                continue;
            }

            match GrpcServerHandle::start(server.port).await {
                Ok(handle) => {
                    self.grpc_servers.push(handle);
                    let _ = self.ui_tx.send(UiEvent::Live(LiveEvent::SysMsg(format!(
                        "gRPC 服务已监听端口 {}",
                        server.port
                    ))));
                }
                Err(err) => {
                    let _ = self.ui_tx.send(UiEvent::Live(LiveEvent::Error(format!(
                        "启动 gRPC 服务失败（端口 {}）：{err}",
                        server.port
                    ))));
                }
            }
        }
    }

    fn start_session(&mut self) {
        let cancel = CancellationToken::new();
        let session_config = SessionConfig {
            room_id: self.config.room_id,
            cookie: self.config.cookie.clone(),
        };
        let api = self.api.clone();
        let live_tx = self.live_tx.clone();
        let task_cancel = cancel.clone();

        self.session_task = Some(tokio::spawn(async move {
            if let Err(err) =
                run_session(api, session_config, live_tx.clone(), task_cancel.clone()).await
            {
                let _ = live_tx.send(LiveEvent::Error(format!("直播会话错误：{err}")));
            }
        }));
        self.session_cancel = Some(cancel);
    }

    async fn stop_all(&mut self) {
        if let Some(cancel) = self.session_cancel.take() {
            cancel.cancel();
        }
        if let Some(task) = self.session_task.take() {
            let _ = task.await;
        }

        let grpc_servers = std::mem::take(&mut self.grpc_servers);
        for server in grpc_servers {
            server.stop().await;
        }
    }
}
