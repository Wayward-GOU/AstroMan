//! AstroMan CLI 入口（无窗口）
//!
//! 该二进制只调用库里的无头实现，不初始化 Tauri / WebView；
//! Windows 下保持控制台子系统，CI 才能拿到 stdout / 退出码。

fn main() {
    std::process::exit(astroman_lib::cli::main());
}
