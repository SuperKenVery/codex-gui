use std::{env, path::PathBuf, process};

use codex_gui::gui::ToolGallery;
use gpui::{App, AppContext as _, Bounds, Styled as _, WindowBounds, WindowOptions, px, size};
use gpui_component::{ActiveTheme as _, Root, Theme};
use gpui_component_assets::Assets;
use gpui_platform::application;

#[derive(Default)]
struct Options {
    screenshot: Option<PathBuf>,
    stay_open: bool,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = env::args().skip(1).peekable();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--screenshot" => {
                    options.screenshot = Some(
                        args.next_if(|value| !value.starts_with('-'))
                            .map(PathBuf::from)
                            .unwrap_or_else(|| "target/tool-render-gallery.png".into()),
                    );
                }
                "--stay-open" => options.stay_open = true,
                "--help" | "-h" => return Err(usage().into()),
                _ => return Err(format!("unknown argument {arg:?}\n\n{}", usage())),
            }
        }

        Ok(options)
    }
}

fn usage() -> &'static str {
    "Usage: tool-render-gallery [--screenshot [PATH]] [--stay-open]"
}

fn run(options: Options) {
    application().with_assets(Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        Theme::sync_system_appearance(None, cx);

        let bounds = Bounds::centered(None, size(px(1360.), px(940.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("Tool call gallery");
                let view = cx
                    .new(|cx| ToolGallery::new(options.screenshot, !options.stay_open, window, cx));
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            },
        )
        .expect("open tool gallery window");
        cx.activate(true);
    });
}

fn main() {
    match Options::parse() {
        Ok(options) => run(options),
        Err(message) => {
            eprintln!("{message}");
            process::exit(2);
        }
    }
}
