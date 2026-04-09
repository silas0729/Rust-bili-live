# Rust-bili-live

这是一个使用 Rust 重写的 B 站直播弹幕工具，界面基于 `eframe/egui`，用于实时接收并显示直播间消息。

它的主要作用有两部分：

- 作为桌面弹幕窗口使用，实时展示直播间的弹幕、礼物、连击礼物、醒目留言、热度和系统状态。
- 作为直播消息转发工具使用，可选开启 gRPC 服务，把收到的直播事件继续推送给其他程序。

## 主要功能

- 实时接收 B 站直播间弹幕消息
- 显示礼物、礼物连击、醒目留言、互动消息、热度
- 支持透明模式
- 支持保存房间号、Cookie、Refresh Token
- 支持自动尝试刷新 Cookie
- 支持可选的 gRPC 推流
- 全界面中文显示

## 运行环境

- Windows
- Rust 工具链
- Cargo

说明：

- 项目使用 `build.rs` 和 `protoc-bin-vendored` 自动生成 proto 代码，一般不需要你额外安装 `protoc`
- 编译产物会输出到 `target/`
- 如果使用了自定义构建目录，也会生成类似 `target_verify/` 这样的目录，这些都不需要提交到 GitHub

## 快速启动

在项目根目录执行：

```bash
cargo run
```

首次运行时会：

1. 编译项目
2. 生成 gRPC 的 proto Rust 代码
3. 启动桌面窗口
4. 读取本地配置
5. 连接 B 站直播间弹幕 WebSocket

如果你只想编译不运行：

```bash
cargo build
```

如果你想生成发布版本：

```bash
cargo build --release
```

## 使用方式

启动后可以直接在窗口顶部点击“设置”，填写以下内容：

- 房间号：要监听的直播间房间号
- Cookie（SESSDATA）：用于访问需要登录态的接口
- 刷新令牌（ac_time_value）：用于自动刷新 Cookie
- gRPC 服务开关和端口：如果你希望其他程序订阅直播消息，可以开启

保存后程序会自动重新连接。

## 启动流程说明

项目启动的大致流程如下：

1. `src/main.rs`
   创建 egui 窗口，设置默认尺寸、透明窗口、无边框和置顶属性。
2. `src/config.rs`
   加载本地配置文件，如果没有配置则使用默认配置。
3. `src/app.rs`
   初始化界面，创建前端状态，并启动后台控制器。
4. `src/backend.rs`
   创建 Tokio 运行时，负责管理直播会话、配置保存和 gRPC 服务。
5. `src/bilibili.rs`
   请求 B 站接口，获取真实房间号、登录信息、弹幕服务器地址、鉴权参数等。
6. `src/live.rs`
   建立直播 WebSocket 连接，发送鉴权包和心跳包，并持续接收直播事件。
7. `src/app.rs`
   把后台事件更新到 UI 中，显示弹幕、礼物、醒目留言、热度和状态栏信息。
8. `src/grpc.rs`
   如果配置中启用了 gRPC 服务，则把直播事件同步广播给外部订阅端。

## 项目结构

```text
Rust-bili-live/
├─ proto/
│  └─ live.proto          # gRPC 协议定义
├─ src/
│  ├─ main.rs             # 程序入口
│  ├─ app.rs              # egui 界面
│  ├─ backend.rs          # 后台控制器
│  ├─ bilibili.rs         # B 站接口访问
│  ├─ live.rs             # WebSocket 直播消息处理
│  ├─ grpc.rs             # gRPC 服务
│  └─ config.rs           # 配置读写
├─ build.rs               # proto 生成脚本
├─ Cargo.toml
└─ README.md
```

## 配置说明

配置结构定义在 `src/config.rs` 中，核心字段包括：

- `room_id`：直播间房间号
- `cookie`：Cookie 字符串
- `refresh_token`：Cookie 刷新令牌
- `transparent`：是否透明模式
- `servers`：外部服务配置，目前支持 gRPC

程序会优先写入系统配置目录；如果系统配置目录不可用，则会回退到当前目录。

## gRPC 的作用

如果开启 gRPC 服务，外部程序可以订阅本项目收到的直播事件。当前 proto 中包含这些消息类型：

- 弹幕
- 热度
- 礼物
- 系统消息
- 错误消息
- 醒目留言
- 互动消息
- 在线榜/热度统计
- 舰队提示
- 礼物星球消息
- 连击礼物

协议定义见：

- `proto/live.proto`

## 常用命令

```bash
cargo run
cargo check
cargo fmt
cargo build
cargo build --release
```

## Git 提交建议

不要提交下面这些编译目录：

- `target/`
- `target_verify/`

这些目录已经在 `.gitignore` 中忽略。别人从 GitHub 拉取项目后，只要重新执行 `cargo build` 或 `cargo run`，Cargo 就会自动重新生成所需的编译文件。

## 这个项目适合做什么

这个项目适合以下用途：

- 作为直播时悬浮显示的弹幕窗口
- 作为接收 B 站直播消息的本地桌面工具
- 作为其他程序的直播事件数据源
- 作为 Rust + egui + Tokio + tonic 的综合练手项目

如果后面你还想要，我也可以继续把这份文档补成：

- 带截图的使用说明
- gRPC 客户端接入示例
- 发布打包说明
- 配置文件样例
