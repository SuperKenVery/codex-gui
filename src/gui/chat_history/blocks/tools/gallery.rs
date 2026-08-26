use std::{path::PathBuf, time::Duration};

use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, Task, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, StyledExt as _, h_flex, scroll::ScrollableElement as _,
};

use super::simple::{ToolFrame, ToolStatus};

/// A standalone visual catalog for every tool-call presentation used by chat history.
pub struct ToolGallery {
    _screenshot_task: Option<Task<()>>,
}

impl ToolGallery {
    pub fn new(
        screenshot_path: Option<PathBuf>,
        quit_after_screenshot: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let screenshot_task = screenshot_path.map(|path| {
            window.spawn(cx, async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(700))
                    .await;

                let result = cx.update(|window, _| capture_window(window, &path));

                match result {
                    Ok(Ok(())) => eprintln!("TOOL_GALLERY_SCREENSHOT={}", path.display()),
                    Ok(Err(error)) => eprintln!("tool gallery screenshot failed: {error:#}"),
                    Err(error) => eprintln!("tool gallery window closed before capture: {error:#}"),
                }

                if quit_after_screenshot {
                    let _ = cx.update(|_, cx| cx.quit());
                }
            })
        });

        Self {
            _screenshot_task: screenshot_task,
        }
    }
}

#[cfg(target_os = "macos")]
fn capture_window(window: &mut Window, path: &std::path::Path) -> anyhow::Result<()> {
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use std::process::Command;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let window_handle = window
        .window_handle()
        .map_err(|error| anyhow::anyhow!("unable to access AppKit window handle: {error:?}"))?;
    let handle = window_handle.as_raw();
    let RawWindowHandle::AppKit(handle) = handle else {
        anyhow::bail!("GPUI did not provide an AppKit window handle");
    };
    // SAFETY: GPUI's AppKit raw handle owns this NSView for the lifetime of `window`,
    // and screenshot capture runs on GPUI's main thread.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let native_window = view
        .window()
        .ok_or_else(|| anyhow::anyhow!("GPUI's NSView is not attached to a window"))?;
    // SAFETY: AppKit window access is confined to the main thread above.
    let window_number = unsafe { native_window.windowNumber() };
    let status = Command::new("/usr/sbin/screencapture")
        .arg("-x")
        .arg("-o")
        .arg(format!("-l{window_number}"))
        .arg(path)
        .status()?;
    anyhow::ensure!(status.success(), "screencapture exited with {status}");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn capture_window(_: &mut Window, _: &std::path::Path) -> anyhow::Result<()> {
    anyhow::bail!("automatic gallery screenshots are currently supported on macOS")
}

impl Render for ToolGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                div()
                    .size_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(1320.))
                            .mx_auto()
                            .p_6()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_2xl().font_semibold().child("Tool call gallery"))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .child(
                                                "Every chat-history tool type, rendered with production ToolFrame styling.",
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_start()
                                    .gap_4()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap_4()
                                            .child(gallery_section(
                                                "Compact calls",
                                                "Tools whose result is communicated by the title alone.",
                                                vec![
                                                    frame(
                                                        IconName::Search,
                                                        "Searched the web for GPUI custom element styling",
                                                        None,
                                                        ToolStatus::Succeeded,
                                                    ),
                                                    frame(
                                                        IconName::Eye,
                                                        "Viewed /tmp/tool-gallery/reference.png",
                                                        None,
                                                        ToolStatus::Succeeded,
                                                    ),
                                                    frame(
                                                        IconName::Bot,
                                                        "Spawned visual-review agent",
                                                        None,
                                                        ToolStatus::Succeeded,
                                                    ),
                                                    frame(
                                                        IconName::Pause,
                                                        "Waited 2.5 s",
                                                        None,
                                                        ToolStatus::Succeeded,
                                                    ),
                                                ],
                                                theme,
                                            ))
                                            .child(gallery_section(
                                                "Lifecycle states",
                                                "Running and failed calls remain easy to scan.",
                                                vec![
                                                    frame(
                                                        IconName::SquareTerminal,
                                                        "Running cargo check",
                                                        Some(
                                                            "Checking codex-gui v0.1.0\nBuilding UI assets…",
                                                        ),
                                                        ToolStatus::Running,
                                                    ),
                                                    frame(
                                                        IconName::Globe,
                                                        "Called design.fetch_reference",
                                                        Some(
                                                            "error:\nConnection closed before a response was received.",
                                                        ),
                                                        ToolStatus::Failed,
                                                    ),
                                                ],
                                                theme,
                                            )),
                                    )
                                    .child(
                                        gallery_section(
                                            "Detailed calls",
                                            "Structured input, output, progress, and file statistics.",
                                            vec![
                                                frame(
                                                    IconName::SquareTerminal,
                                                    "Ran cargo check --bin tool-render-gallery",
                                                    Some(
                                                        "cwd: /Users/ken/Codes/codex-gui\nFinished `dev` profile in 1.84s\nexit code: 0",
                                                    ),
                                                    ToolStatus::Succeeded,
                                                ),
                                                frame_with_diff(
                                                    IconName::File,
                                                    "Edited src/gui/chat_history/blocks/tools/simple.rs",
                                                    Some(
                                                        "@@ -82,3 +82,5 @@\n-    .border_3()\n+    .border_1()\n+    .rounded_md()",
                                                    ),
                                                    ToolStatus::Succeeded,
                                                    (24, 11),
                                                ),
                                                frame(
                                                    IconName::Globe,
                                                    "Called browser.inspect_visible_state",
                                                    Some(
                                                        "{\n  \"target\": \"tool-render-gallery\"\n}\n\nresult:\n{ \"visible\": true, \"rows\": 9 }",
                                                    ),
                                                    ToolStatus::Succeeded,
                                                ),
                                                frame(
                                                    IconName::Asterisk,
                                                    "Called design.review_tool_surface",
                                                    Some(
                                                        "{ \"density\": \"comfortable\", \"theme\": \"system\" }\n\noutput:\nClear hierarchy and consistent status treatment.",
                                                    ),
                                                    ToolStatus::Succeeded,
                                                ),
                                                frame(
                                                    IconName::Palette,
                                                    "Generated image",
                                                    Some(
                                                        "A polished native desktop tool-call gallery\nsaved to: /tmp/tool-gallery/preview.png",
                                                    ),
                                                    ToolStatus::Succeeded,
                                                ),
                                            ],
                                            theme,
                                        )
                                        .flex_1(),
                                    ),
                            )
                    ),
            )
    }
}

fn gallery_section(
    title: &'static str,
    subtitle: &'static str,
    frames: Vec<ToolFrame>,
    theme: &gpui_component::theme::Theme,
) -> gpui::Div {
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border.opacity(0.75))
        .bg(theme.muted.opacity(0.35))
        .overflow_hidden()
        .child(
            div()
                .px_3()
                .py_2p5()
                .border_b_1()
                .border_color(theme.border.opacity(0.65))
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_semibold().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(subtitle),
                ),
        )
        .child(
            div()
                .px_2()
                .children(frames.into_iter().enumerate().map(|(index, frame)| {
                    div()
                        .when(index > 0, |row| {
                            row.border_t_1().border_color(theme.border.opacity(0.55))
                        })
                        .child(frame)
                })),
        )
}

fn frame(
    icon: IconName,
    title: &'static str,
    detail: Option<&'static str>,
    status: ToolStatus,
) -> ToolFrame {
    ToolFrame::new(icon, title.into(), detail.map(Into::into), status)
}

fn frame_with_diff(
    icon: IconName,
    title: &'static str,
    detail: Option<&'static str>,
    status: ToolStatus,
    diff: (usize, usize),
) -> ToolFrame {
    frame(icon, title, detail, status).diff(diff.0, diff.1)
}
