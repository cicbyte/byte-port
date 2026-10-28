//! byte-port GUI（gpui-kit 版）。
//!
//! 启动流程：application() → init → 切暗色 → 内嵌加载 Catppuccin Mocha 主题
//! → 打开主窗口。中文渲染零配置（Windows DirectWrite 系统字体回退）。

mod app;
mod scan;
mod tables;

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::*;

use app::PortApp;

/// Catppuccin 主题集（内嵌，单文件分发无需附带主题目录）。
const CATPPUCCIN: &str = include_str!("../themes/catppuccin.json");

fn main() {
    let app = application().with_assets(gpui_kit::assets::Assets);

    app.run(move |cx| {
        // 必须在使用任何组件特性之前调用
        gpui_kit::init(cx);

        // 暗色模式 + Catppuccin Mocha（dark）
        Theme::change(ThemeMode::Dark, None, cx);
        ThemeRegistry::global_mut(cx).load_themes_from_str(CATPPUCCIN).ok();
        if let Some(mocha) = ThemeRegistry::global(cx)
            .themes()
            .get("Catppuccin Mocha")
            .cloned()
        {
            Theme::update(cx, |theme| theme.apply_config(&mocha));
        }

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1040.), px(660.)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some("Byte Port".into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(720.), px(480.))),
            ..Default::default()
        };

        gpui_kit::open_window(window_options, cx, |window, cx| {
            cx.new(|cx| PortApp::new(window, cx))
        })
        .expect("Failed to open window");
    });
}
