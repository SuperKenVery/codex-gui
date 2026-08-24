use std::time::Duration;

use gpui::{IntoElement, ParentElement, Styled, div};
use gpui_component::{Icon, IconName, Sizable as _, h_flex, theme::Theme};

pub(super) fn render(duration: Duration, theme: &Theme, expanded: bool) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .py_1()
        .cursor_pointer()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(
            h_flex()
                .items_center()
                .gap_1()
                .child(disclosure_icon(expanded, theme))
                .child(format!("Worked for {}", format_duration(duration))),
        )
}

fn disclosure_icon(expanded: bool, theme: &Theme) -> impl IntoElement {
    Icon::new(if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    })
    .xsmall()
    .text_color(theme.muted_foreground)
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let minutes = seconds / 60;
    let seconds = seconds % 60;

    if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}
