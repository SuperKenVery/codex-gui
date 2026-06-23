#![cfg_attr(target_family = "wasm", no_main)]

mod app;
mod bridge;
mod gui;
mod workspace;

use app::CodexGui;
use gpui::{
    App, AppContext, Bounds, Styled, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, point, px, size, transparent_black,
};
use gpui_component::{Root, Theme};
use gpui_component_assets::Assets;
use gpui_platform::application;

fn init_zed_markdown_theme(cx: &mut App) {
    if !cx.has_global::<zed_settings::SettingsStore>() {
        zed_settings::init(cx);
    }
    zed_theme_settings::init(zed_theme::LoadThemes::JustBase, cx);
}

fn run_app() {
    application().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);
        init_zed_markdown_theme(cx);
        Theme::sync_system_appearance(None, cx);

        let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(9.), px(9.))),
                }),
                window_background: WindowBackgroundAppearance::Blurred,
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("codex-gui");
                let view = cx.new(|cx| CodexGui::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_app();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_app();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn zed_markdown_theme_globals_are_initialized(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            init_zed_markdown_theme(cx);

            let _theme = zed_theme::GlobalTheme::theme(cx);
            let provider = zed_theme::theme_settings(cx);
            let _ui_font = provider.ui_font(cx);
            let _buffer_font = provider.buffer_font(cx);
        });
    }
}
