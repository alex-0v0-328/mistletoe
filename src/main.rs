//! 程序入口：声明四个模块并启动 app。
//! 不放任何逻辑；命令行参数（--port、--data）在第 4 步随 API 一起加入。

mod api;
mod app;
mod render;
// 第 4 步接入 API 之前，命令接口（Command、Reply、apply）只有单元测试在用
#[allow(dead_code)]
mod state;

fn main() {
    if let Err(e) = app::run() {
        eprintln!("Mistletoe 启动失败：{e}");
        std::process::exit(1);
    }
}
