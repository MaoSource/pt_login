# PT Login (道聚城扫码登录助手)

一个基于 Rust 开发的跨平台道聚城扫码登录辅助工具。采用 `eframe` (egui) 构建原生 GUI 界面，具备轻量、高性能和安全可靠的特点。

## ✨ 功能特点

- **扫码登录**：支持腾讯道聚城快捷扫码登录。
- **跨平台支持**：原生支持 Windows 和 macOS (Intel & Apple Silicon)。
- **原生 GUI**：基于 Rust 生态的 `egui` 框架，无 Electron 等冗余依赖，启动极快。
- **自动构建**：集成 GitHub Actions，自动打包并发布 Release。

## 🚀 快速开始

### 下载运行
前往 [Releases](https://github.com/您的用户名/pt_login/releases) 页面下载对应系统的版本：
- **Windows**: 下载 `pt_login-windows-x86_64.exe` 直接运行。
- **macOS**: 下载 `pt_login-macos-universal`，赋予执行权限后运行。

### 本地开发
确保您已安装 [Rust](https://www.rust-lang.org/) 环境。

```bash
# 克隆仓库
git clone https://github.com/您的用户名/pt_login.git
cd pt_login

# 运行 (开发模式)
cargo run

# 编译 (发布模式)
cargo build --release
```

## 🛠 技术栈

- **语言**: Rust 2021 Edition
- **UI 框架**: [eframe / egui](https://github.com/emilk/egui)
- **异步运行时**: [Tokio](https://tokio.rs/)
- **网络请求**: [Reqwest](https://github.com/seanmonstar/reqwest)
- **CI/CD**: GitHub Actions

## 📦 自动打包发布说明

本项目已配置完善的 CI 工作流。当您需要发布新版本时：

1. 修改 `Cargo.toml` 中的 `version`。
2. 在本地提交更改。
3. 推送版本标签：
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
GitHub Actions 会自动编译 Windows 和 macOS 的二进制文件，并创建一个新的 Release。

## ⚖️ 许可证

[MIT License](LICENSE)
