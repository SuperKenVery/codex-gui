#![cfg_attr(target_family = "wasm", no_main)]

mod app;
mod bridge;
mod models;
mod workspace;

use app::CodexGui;
use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
use gpui_component_assets::Assets;
use gpui_platform::application;

fn run_app() {
    application().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("codex-gui");
                let view = cx.new(|cx| CodexGui::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
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
