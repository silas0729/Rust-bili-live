use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use eframe::egui;
use eframe::egui::{
    Align, Button, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame,
    Layout, Margin, RichText, ScrollArea, Sense, Stroke, TextStyle, UiBuilder, Vec2,
    ViewportCommand,
};
use egui_extras::install_image_loaders;

use crate::backend::{BackendHandle, UiEvent};
use crate::config::{AppConfig, ServerSettings, ServerType};
use crate::live::{
    ComboSendData, GiftData, InteractData102, InteractDataNotice, InteractMsg, LiveEvent,
    SuperChatMsgData, ToastMsgData,
};

const MAX_FEED: usize = 100;
const MAX_STATUS: usize = 12;
const STATUS_H: f32 = 56.0;

const C_BG: Color32 = Color32::from_rgb(4, 6, 10);
const C_PANEL: Color32 = Color32::from_rgb(10, 13, 20);
const C_PANEL_ALT: Color32 = Color32::from_rgb(16, 20, 29);
const C_HEADER: Color32 = Color32::from_rgb(12, 16, 24);
const C_STATUS: Color32 = Color32::from_rgb(6, 9, 14);
const C_BORDER: Color32 = Color32::from_rgb(38, 46, 61);
const C_TEXT: Color32 = Color32::from_rgb(245, 247, 250);
const C_MUTED: Color32 = Color32::from_rgb(156, 166, 180);
const C_BLUE: Color32 = Color32::from_rgb(93, 177, 255);
const C_GOLD: Color32 = Color32::from_rgb(255, 198, 87);
const C_PINK: Color32 = Color32::from_rgb(255, 108, 141);
const C_GREEN: Color32 = Color32::from_rgb(92, 211, 169);
const C_RED: Color32 = Color32::from_rgb(214, 78, 102);

pub struct YuunaApp {
    backend: BackendHandle,
    feed: Vec<Item>,
    sc_list: Vec<SuperChatMsgData>,
    status: VecDeque<StatusLine>,
    room_id_input: String,
    cookie_input: String,
    refresh_token_input: String,
    servers: Vec<ServerSettings>,
    transparent: bool,
    popularity: i32,
    show_settings: bool,
}

struct StatusLine {
    text: String,
    error: bool,
}

enum Item {
    Danmu(DanmuEntry),
    Gift(GiftEntry),
}

struct DanmuEntry {
    medal_name: String,
    medal_level: i32,
    nickname: String,
    content: String,
}

struct GiftEntry {
    combo_id: String,
    gift_num: i32,
    total_num: i32,
    face: String,
    medal_name: String,
    medal_level: i32,
    uname: String,
    action: String,
    gift_name: String,
    gift_image: String,
    combo_total_coin: i32,
    coin_type: String,
}

impl YuunaApp {
    pub fn new(cc: &eframe::CreationContext<'_>, cfg: AppConfig) -> Result<Self> {
        install_image_loaders(&cc.egui_ctx);
        configure_fonts(&cc.egui_ctx);
        configure_style(&cc.egui_ctx);
        let backend = BackendHandle::start(cfg.clone())?;
        Ok(Self {
            backend,
            feed: Vec::new(),
            sc_list: Vec::new(),
            status: VecDeque::new(),
            room_id_input: cfg.room_id.to_string(),
            cookie_input: cfg.cookie,
            refresh_token_input: cfg.refresh_token,
            servers: cfg.servers,
            transparent: cfg.transparent,
            popularity: 0,
            show_settings: false,
        })
    }

    fn poll_backend(&mut self) {
        while let Ok(event) = self.backend.ui_rx.try_recv() {
            match event {
                UiEvent::Live(event) => self.apply_live_event(event),
                UiEvent::ConfigUpdated(cfg) => {
                    self.room_id_input = cfg.room_id.to_string();
                    self.cookie_input = cfg.cookie;
                    self.refresh_token_input = cfg.refresh_token;
                    self.servers = cfg.servers;
                    self.transparent = cfg.transparent;
                }
            }
        }
    }

    fn apply_live_event(&mut self, event: LiveEvent) {
        match event {
            LiveEvent::Danmu(data) => self.feed.push(Item::Danmu(DanmuEntry {
                medal_name: data.medal_name,
                medal_level: data.medal_level,
                nickname: data.nickname,
                content: data.content,
            })),
            LiveEvent::Gift(data) => self.push_gift(data),
            LiveEvent::ComboSend(data) => self.push_combo(data),
            LiveEvent::SysMsg(msg) => self.push_status(msg, false),
            LiveEvent::Error(msg) => self.push_status(msg, true),
            LiveEvent::SuperChat(data) => self.sc_list.push(data),
            LiveEvent::Interaction(data) => self.push_interaction(data),
            LiveEvent::Popularity(data) => self.popularity = data.popularity,
            LiveEvent::GiftStarProcess(data) => self.feed.push(Item::Danmu(DanmuEntry {
                medal_name: String::new(),
                medal_level: 0,
                nickname: "礼物".to_owned(),
                content: data.message,
            })),
            LiveEvent::OnlineRankCount(data) => {
                self.popularity = data.online_count.max(data.count);
            }
            LiveEvent::Toast(data) => self.push_toast(data),
        }
        self.trim_feed();
    }

    fn push_gift(&mut self, data: GiftData) {
        let count = if data.combo_send.combo_num > 0 {
            data.combo_send.combo_num
        } else {
            data.gift_num
        };
        if count <= 0 {
            return;
        }
        self.feed.push(Item::Gift(GiftEntry {
            combo_id: data.combo_send.combo_id,
            gift_num: count,
            total_num: count,
            face: data.face,
            medal_name: data.medal_info.medal_name,
            medal_level: data.medal_info.medal_level,
            uname: data.uname,
            action: if data.action.is_empty() {
                "投喂".to_owned()
            } else {
                data.action
            },
            gift_name: data.gift_name,
            gift_image: data.gift_info.gif,
            combo_total_coin: data.combo_total_coin,
            coin_type: data.coin_type,
        }));
    }

    fn push_combo(&mut self, data: ComboSendData) {
        let combo_id = data.combo_id.clone();
        let total_num = data
            .total_num
            .max(data.combo_num)
            .max(data.batch_combo_num)
            .max(1);
        if !combo_id.is_empty() {
            if let Some(old) = self.feed.iter_mut().find_map(|item| match item {
                Item::Gift(gift) if gift.combo_id == combo_id => Some(gift),
                _ => None,
            }) {
                old.gift_num = total_num;
                old.total_num = total_num;
                if !data.uname.is_empty() {
                    old.uname = data.uname.clone();
                }
                if !data.action.is_empty() {
                    old.action = data.action.clone();
                }
                if !data.gift_name.is_empty() {
                    old.gift_name = data.gift_name.clone();
                }
                if !data.medal_info.medal_name.is_empty() {
                    old.medal_name = data.medal_info.medal_name.clone();
                }
                if data.medal_info.medal_level > 0 {
                    old.medal_level = data.medal_info.medal_level;
                }
                if data.combo_total_coin > 0 {
                    old.combo_total_coin = data.combo_total_coin;
                }
                return;
            }
        }
        self.feed.push(Item::Gift(GiftEntry {
            combo_id,
            gift_num: total_num,
            total_num,
            face: String::new(),
            medal_name: data.medal_info.medal_name,
            medal_level: data.medal_info.medal_level,
            uname: data.uname,
            action: if data.action.is_empty() {
                "投喂".to_owned()
            } else {
                data.action
            },
            gift_name: data.gift_name,
            gift_image: String::new(),
            combo_total_coin: data.combo_total_coin,
            coin_type: "gold".to_owned(),
        }));
    }

    fn push_interaction(&mut self, data: InteractMsg) {
        if data.kind == 102 {
            if let Ok(parsed) = serde_json::from_value::<InteractData102>(data.data) {
                for combo in parsed.combo {
                    self.feed.push(Item::Danmu(DanmuEntry {
                        medal_name: String::new(),
                        medal_level: 0,
                        nickname: combo.guide.trim().trim_end_matches([':', '：']).to_owned(),
                        content: format!("{} x{}", combo.content, combo.cnt),
                    }));
                }
            }
        } else if [103, 104, 105, 106].contains(&data.kind) {
            if let Ok(parsed) = serde_json::from_value::<InteractDataNotice>(data.data) {
                self.feed.push(Item::Danmu(DanmuEntry {
                    medal_name: String::new(),
                    medal_level: 0,
                    nickname: String::new(),
                    content: format!("{} {}", parsed.cnt, parsed.suffix_text),
                }));
            }
        }
    }

    fn push_toast(&mut self, data: ToastMsgData) {
        if data.username.trim().is_empty() {
            return;
        }
        let role = if data.role_name.is_empty() {
            "舰队支持"
        } else {
            data.role_name.as_str()
        };
        let mut text = format!("{} 开通了 {}", data.username, role);
        if data.num > 0 {
            text.push_str(&format!(" x{}", data.num));
        }
        self.push_status(text, false);
    }

    fn push_status(&mut self, text: String, error: bool) {
        self.status.push_back(StatusLine { text, error });
        while self.status.len() > MAX_STATUS {
            self.status.pop_front();
        }
    }

    fn trim_feed(&mut self) {
        if self.feed.len() > MAX_FEED {
            let drop = self.feed.len().saturating_sub(MAX_FEED);
            self.feed.drain(0..drop);
        }
    }

    fn cleanup(&mut self) {
        let now = Utc::now().timestamp();
        self.sc_list
            .retain(|item| item.end_time <= 0 || item.end_time > now);
    }

    fn save_config(&mut self) {
        let room_id = match self.room_id_input.trim().parse::<u64>() {
            Ok(id) => id,
            Err(_) => {
                self.push_status("房间号必须是纯数字".to_owned(), true);
                return;
            }
        };
        let cfg = AppConfig {
            room_id,
            cookie: self.cookie_input.clone(),
            refresh_token: self.refresh_token_input.clone(),
            debug: false,
            servers: self.servers.clone(),
            transparent: self.transparent,
        };
        if let Err(err) = self.backend.save_config(cfg) {
            self.push_status(format!("发送配置失败：{err}"), true);
            return;
        }
        self.feed.clear();
        self.sc_list.clear();
        self.show_settings = false;
    }
}

impl YuunaApp {
    fn ui_header(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(C_HEADER)
            .corner_radius(CornerRadius {
                nw: 14,
                ne: 14,
                sw: 0,
                se: 0,
            })
            .stroke(Stroke::new(1.0, C_BORDER))
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                let (rect, drag) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), 46.0),
                    Sense::click_and_drag(),
                );
                if drag.drag_started() || drag.dragged() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
                }
                let mut ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                let button_zone_width = 150.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(
                        (ui.available_width() - button_zone_width).max(0.0),
                        rect.height(),
                    ),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new("Yuuna 弹幕")
                                .size(13.5)
                                .strong()
                                .color(C_TEXT),
                        );
                        ui.add_space(12.0);
                        badge(
                            ui,
                            "房间",
                            if self.room_id_input.trim().is_empty() {
                                "--"
                            } else {
                                self.room_id_input.trim()
                            },
                            C_BLUE,
                        );
                        if self.popularity > 0 {
                            ui.add_space(8.0);
                            let pop = format_int(self.popularity);
                            badge(ui, "热度", &pop, C_GOLD);
                        }
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if header_btn(ui, "关闭", C_RED).clicked() {
                        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                    }
                    if header_btn(
                        ui,
                        if self.show_settings {
                            "返回"
                        } else {
                            "设置"
                        },
                        C_GREEN,
                    )
                    .clicked()
                    {
                        self.show_settings = !self.show_settings;
                    }
                });
            });
    }

    fn ui_live(&mut self, ui: &mut egui::Ui) {
        let h = (ui.available_height() - STATUS_H).max(0.0);
        let fill = if self.transparent {
            Color32::from_rgba_unmultiplied(5, 8, 12, 48)
        } else {
            C_PANEL
        };
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), h),
            Layout::top_down(Align::LEFT),
            |ui| {
                Frame::new()
                    .fill(fill)
                    .stroke(Stroke::new(1.0, C_BORDER))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        if !self.sc_list.is_empty() {
                            ScrollArea::horizontal()
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for item in &self.sc_list {
                                            draw_sc(ui, item);
                                            ui.add_space(10.0);
                                        }
                                    });
                                });
                            ui.add_space(12.0);
                        }
                        ScrollArea::vertical()
                            .stick_to_bottom(true)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if self.feed.is_empty() {
                                    Frame::new()
                                        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 8))
                                        .stroke(Stroke::new(1.0, C_BORDER))
                                        .corner_radius(CornerRadius::same(10))
                                        .inner_margin(Margin::same(14))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(
                                                    "连接成功后，这里会显示弹幕、礼物和醒目留言。",
                                                )
                                                .size(12.0)
                                                .color(C_MUTED),
                                            );
                                        });
                                }
                                for item in &self.feed {
                                    match item {
                                        Item::Danmu(data) => draw_danmu(ui, data),
                                        Item::Gift(data) => draw_gift(ui, data),
                                    }
                                    ui.add_space(8.0);
                                }
                            });
                    });
            },
        );
        self.ui_status(ui);
    }

    fn ui_status(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(C_STATUS)
            .corner_radius(CornerRadius {
                nw: 0,
                ne: 0,
                sw: 14,
                se: 14,
            })
            .stroke(Stroke::new(1.0, C_BORDER))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_height(STATUS_H);
                let lines: Vec<&StatusLine> = self.status.iter().rev().take(2).collect();
                if lines.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("正在等待弹幕、礼物和系统消息…")
                                .size(11.5)
                                .color(C_MUTED),
                        );
                    });
                    return;
                }
                for line in lines.into_iter().rev() {
                    let color = if line.error {
                        Color32::from_rgb(255, 172, 188)
                    } else {
                        C_TEXT
                    };
                    ui.label(RichText::new(&line.text).size(11.5).color(color));
                }
            });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(C_PANEL_ALT)
            .corner_radius(CornerRadius {
                nw: 0,
                ne: 0,
                sw: 14,
                se: 14,
            })
            .stroke(Stroke::new(1.0, C_BORDER))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        section(ui, "界面设置");
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("透明模式").size(13.5).strong().color(C_TEXT));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                toggle(ui, &mut self.transparent);
                            });
                        });
                        ui.add_space(18.0);
                        section(ui, "直播间配置");
                        field(ui, "房间号");
                        dark_edit(
                            ui,
                            egui::TextEdit::singleline(&mut self.room_id_input)
                                .desired_width(f32::INFINITY)
                                .hint_text("请输入直播间房间号"),
                        );
                        ui.add_space(14.0);
                        field(ui, "Cookie（SESSDATA）");
                        dark_edit(
                            ui,
                            egui::TextEdit::multiline(&mut self.cookie_input)
                                .desired_width(f32::INFINITY)
                                .desired_rows(5)
                                .hint_text("请输入浏览器中的 Cookie"),
                        );
                        ui.add_space(14.0);
                        field(ui, "刷新令牌（ac_time_value）");
                        dark_edit(
                            ui,
                            egui::TextEdit::singleline(&mut self.refresh_token_input)
                                .desired_width(f32::INFINITY)
                                .hint_text("用于自动刷新 Cookie，可留空"),
                        );
                        ui.add_space(20.0);
                        section(ui, "服务器设置");
                        for server in &mut self.servers {
                            Frame::new()
                                .fill(Color32::from_rgba_unmultiplied(18, 22, 31, 220))
                                .stroke(Stroke::new(1.0, C_BORDER))
                                .corner_radius(CornerRadius::same(10))
                                .inner_margin(Margin::same(12))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(&server.name)
                                                    .size(13.0)
                                                    .strong()
                                                    .color(C_TEXT),
                                            );
                                            ui.label(
                                                RichText::new(match server.kind {
                                                    ServerType::Grpc => "gRPC 服务",
                                                })
                                                .size(11.0)
                                                .color(C_MUTED),
                                            );
                                        });
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                toggle(ui, &mut server.enabled);
                                            },
                                        );
                                    });
                                    ui.add_space(10.0);
                                    field(ui, "端口");
                                    let mut port = server.port.to_string();
                                    let rsp = dark_edit(
                                        ui,
                                        egui::TextEdit::singleline(&mut port)
                                            .desired_width(140.0)
                                            .hint_text("请输入端口"),
                                    );
                                    if rsp.changed() {
                                        if let Ok(port) = port.parse::<u16>() {
                                            server.port = port;
                                        }
                                    }
                                });
                            ui.add_space(10.0);
                        }
                        ui.add_space(16.0);
                        if ui
                            .add_sized(
                                [ui.available_width(), 44.0],
                                Button::new(
                                    RichText::new("应用并重连").size(14.0).strong().color(C_BG),
                                )
                                .fill(C_BLUE)
                                .stroke(Stroke::new(1.0, Color32::from_rgb(124, 198, 255)))
                                .corner_radius(CornerRadius::same(8)),
                            )
                            .clicked()
                        {
                            self.save_config();
                        }
                    });
            });
    }
}

impl Drop for YuunaApp {
    fn drop(&mut self) {
        self.backend.shutdown();
    }
}

impl eframe::App for YuunaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_backend();
        self.cleanup();
        egui::CentralPanel::default()
            .frame(Frame::new().fill(Color32::from_rgba_unmultiplied(0, 0, 0, 0)))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(if self.transparent {
                        Color32::from_rgba_unmultiplied(0, 0, 0, 0)
                    } else {
                        C_BG
                    })
                    .stroke(if self.transparent {
                        Stroke::NONE
                    } else {
                        Stroke::new(1.0, C_BORDER)
                    })
                    .corner_radius(CornerRadius::same(14))
                    .show(ui, |ui| {
                        self.ui_header(ui);
                        if self.show_settings {
                            self.ui_settings(ui);
                        } else {
                            self.ui_live(ui);
                        }
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn badge(ui: &mut egui::Ui, label: &str, value: &str, accent: Color32) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            accent.r(),
            accent.g(),
            accent.b(),
            26,
        ))
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 180),
        ))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(10.5).color(C_MUTED));
                ui.label(RichText::new(value).size(11.5).strong().color(C_TEXT));
            });
        });
}

fn header_btn(ui: &mut egui::Ui, label: &str, accent: Color32) -> egui::Response {
    ui.add_sized(
        [60.0, 30.0],
        Button::new(RichText::new(label).size(11.0).strong().color(C_TEXT))
            .fill(Color32::from_rgba_unmultiplied(
                accent.r(),
                accent.g(),
                accent.b(),
                24,
            ))
            .stroke(Stroke::new(1.0, accent))
            .corner_radius(CornerRadius::same(6)),
    )
}

fn draw_danmu(ui: &mut egui::Ui, data: &DanmuEntry) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 8))
        .stroke(Stroke::new(1.0, C_BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if !data.medal_name.is_empty() {
                    medal(ui, &data.medal_name, data.medal_level);
                }
                if !data.nickname.is_empty() {
                    ui.label(
                        RichText::new(format!("{}：", data.nickname))
                            .strong()
                            .color(C_PINK),
                    );
                }
                ui.label(RichText::new(&data.content).color(C_TEXT));
            });
        });
}

fn draw_gift(ui: &mut egui::Ui, data: &GiftEntry) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(57, 21, 34, 225))
        .stroke(Stroke::new(1.0, C_PINK))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if !data.face.is_empty() {
                    remote_image(ui, &data.face, Vec2::new(20.0, 20.0));
                }
                if !data.medal_name.is_empty() {
                    medal(ui, &data.medal_name, data.medal_level);
                }
                ui.label(RichText::new(&data.uname).strong().color(C_TEXT));
                ui.label(RichText::new(&data.action).color(C_MUTED));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if !data.gift_image.is_empty() {
                    remote_image(ui, &data.gift_image, Vec2::new(34.0, 34.0));
                } else {
                    ui.label(RichText::new("礼物").color(C_GOLD));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(&data.gift_name).strong().color(C_GOLD));
                    ui.label(
                        RichText::new(format!("x {}", data.gift_num.max(data.total_num)))
                            .size(18.0)
                            .strong()
                            .color(C_PINK),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if data.coin_type == "gold" && data.combo_total_coin > 0 {
                        ui.label(
                            RichText::new(format!(
                                "￥{:.1}",
                                data.combo_total_coin as f32 / 1000.0
                            ))
                            .strong()
                            .color(C_GOLD),
                        );
                    }
                });
            });
        });
}

fn draw_sc(ui: &mut egui::Ui, data: &SuperChatMsgData) {
    let accent = sc_color(data.price);
    let now = Utc::now().timestamp();
    let total = (data.end_time - data.start_time).max(1);
    let left = (data.end_time - now).clamp(0, total);
    let progress = if data.end_time > data.start_time {
        left as f32 / total as f32
    } else {
        1.0
    };
    Frame::new()
        .fill(C_PANEL_ALT)
        .stroke(Stroke::new(1.0, accent))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(0))
        .show(ui, |ui| {
            ui.set_width(224.0);
            Frame::new()
                .fill(accent)
                .corner_radius(CornerRadius {
                    nw: 10,
                    ne: 10,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if !data.user_info.face.is_empty() {
                            remote_image(ui, &data.user_info.face, Vec2::new(30.0, 30.0));
                        }
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&data.user_info.uname)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(format!("￥{}", data.price))
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                        });
                    });
                });
            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(&data.message).size(12.5).color(C_TEXT));
                ui.add_space(10.0);
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 4.0), Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(2),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 20),
                );
                let fill = egui::Rect::from_min_size(
                    rect.min,
                    Vec2::new(rect.width() * progress, rect.height()),
                );
                ui.painter()
                    .rect_filled(fill, CornerRadius::same(2), Color32::WHITE);
                ui.add_space(10.0);
            });
        });
}

fn medal(ui: &mut egui::Ui, name: &str, level: i32) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(58, 118, 201, 40))
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(93, 177, 255, 140),
        ))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(5, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).size(10.5).strong().color(C_BLUE));
                ui.label(
                    RichText::new(level.to_string())
                        .size(10.5)
                        .strong()
                        .color(C_GOLD),
                );
            });
        });
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(14.5).strong().color(C_TEXT));
    ui.add_space(10.0);
}

fn field(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).strong().color(C_MUTED));
    ui.add_space(6.0);
}

fn dark_edit(ui: &mut egui::Ui, widget: egui::TextEdit<'_>) -> egui::Response {
    ui.scope(|ui| {
        ui.style_mut().visuals.widgets.inactive.bg_fill = C_PANEL;
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = C_PANEL;
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, C_BORDER);
        ui.style_mut().visuals.widgets.hovered.bg_fill = C_PANEL;
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = C_PANEL;
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, C_BLUE);
        ui.style_mut().visuals.widgets.active.bg_fill = C_PANEL;
        ui.style_mut().visuals.widgets.active.weak_bg_fill = C_PANEL;
        ui.style_mut().visuals.widgets.active.bg_stroke = Stroke::new(1.2, C_BLUE);
        ui.add(widget.margin(Margin::symmetric(10, 8)))
    })
    .inner
}

fn toggle(ui: &mut egui::Ui, value: &mut bool) -> egui::Response {
    let (rect, mut rsp) = ui.allocate_exact_size(Vec2::new(42.0, 24.0), Sense::click());
    if rsp.clicked() {
        *value = !*value;
        rsp.mark_changed();
    }
    let bg = if *value {
        C_BLUE
    } else {
        Color32::from_rgb(77, 87, 103)
    };
    let x = if *value {
        rect.right() - 11.0
    } else {
        rect.left() + 11.0
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(12),
        bg,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 24)),
        egui::StrokeKind::Outside,
    );
    ui.painter()
        .circle_filled(egui::pos2(x, rect.center().y), 9.0, Color32::WHITE);
    rsp
}

fn remote_image(ui: &mut egui::Ui, uri: &str, size: Vec2) {
    let _ = ui.add(egui::Image::from_uri(uri.to_owned()).fit_to_exact_size(size));
}

fn sc_color(price: i32) -> Color32 {
    if price >= 1000 {
        C_PINK
    } else if price >= 500 {
        Color32::from_rgb(148, 127, 255)
    } else if price >= 100 {
        C_GOLD
    } else {
        C_BLUE
    }
}

fn format_int(value: i32) -> String {
    let negative = value < 0;
    let digits = value.abs().to_string();
    let mut out = String::new();
    let lead = digits.len() % 3;
    if lead > 0 {
        out.push_str(&digits[..lead]);
        if digits.len() > lead {
            out.push(',');
        }
    }
    for (i, chunk) in digits[lead..].as_bytes().chunks(3).enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
    }
    if out.is_empty() {
        out.push('0');
    }
    if negative { format!("-{out}") } else { out }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Some(bytes) = load_chinese_font_bytes() {
        fonts
            .font_data
            .insert("zh_cn".to_owned(), FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "zh_cn".to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "zh_cn".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = Color32::from_rgba_unmultiplied(0, 0, 0, 0);
    style.visuals.panel_fill = Color32::from_rgba_unmultiplied(0, 0, 0, 0);
    style.visuals.override_text_color = Some(C_TEXT);
    style.visuals.widgets.inactive.bg_fill = C_PANEL_ALT;
    style.visuals.widgets.inactive.weak_bg_fill = C_PANEL_ALT;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, C_BORDER);
    style.visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(21, 28, 40);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(21, 28, 40);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, C_BLUE);
    style.visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(24, 33, 46);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(24, 33, 46);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, C_BLUE);
    style.visuals.widgets.active.corner_radius = CornerRadius::same(8);
    style.visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(93, 177, 255, 90);
    style.visuals.selection.stroke = Stroke::new(1.0, C_BLUE);
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(22.0)),
        (TextStyle::Body, FontId::proportional(15.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Monospace, FontId::monospace(14.0)),
        (TextStyle::Small, FontId::proportional(11.0)),
    ]
    .into();
    ctx.set_style(style);
}

fn load_chinese_font_bytes() -> Option<Vec<u8>> {
    for path in [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ] {
        let path = PathBuf::from(path);
        if path.exists() {
            if let Ok(bytes) = fs::read(&path) {
                return Some(bytes);
            }
        }
    }
    None
}
