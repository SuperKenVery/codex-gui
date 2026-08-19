#![cfg_attr(target_family = "wasm", no_main)]

mod app;
mod bridge;
mod gui;
mod workspace;

use app::CodexGui;
use bridge::start_app_server_bridge;
use codex_arg0::Arg0DispatchPaths;
use gpui::{
    App, AppContext, Bounds, Styled, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, point, px, size, transparent_black,
};
use gpui_component::{Root, Theme};
use gpui_component_assets::Assets;
use gpui_platform::application;

#[cfg(not(target_family = "wasm"))]
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("codex_gui=info,codex_app_server=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn run_app(runtime: tokio::runtime::Handle, arg0_paths: Arg0DispatchPaths) {
    application().with_assets(Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        Theme::sync_system_appearance(None, cx);
        let (bridge, bridge_rx) = start_app_server_bridge(runtime, arg0_paths);

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
            move |window, cx| {
                window.set_window_title("codex-gui");
                let view = cx.new(|cx| CodexGui::new(bridge, bridge_rx, window, cx));
                cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    let arg0_guard = codex_arg0::arg0_dispatch();
    let current_exe = std::env::current_exe().ok();
    let arg0_paths = Arg0DispatchPaths {
        codex_self_exe: current_exe.clone(),
        codex_linux_sandbox_exe: if cfg!(target_os = "linux") {
            arg0_guard
                .as_ref()
                .and_then(|guard| guard.paths().codex_linux_sandbox_exe.clone())
                .or(current_exe)
        } else {
            None
        },
        main_execve_wrapper_exe: arg0_guard
            .as_ref()
            .and_then(|guard| guard.paths().main_execve_wrapper_exe.clone()),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .expect("failed to create embedded Codex runtime");

    init_tracing();
    run_app(runtime.handle().clone(), arg0_paths);
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    drop(arg0_guard);
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    panic!("the embedded Codex runtime is not available on wasm");
}
