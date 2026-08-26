use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::*, transparent_white,
};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, spinner::Spinner};

#[derive(Clone, Copy)]
pub(super) enum ToolStatus {
    Running,
    Succeeded,
    Failed,
}

impl ToolStatus {
    pub(super) fn done(self) -> bool {
        !matches!(self, Self::Running)
    }
}

pub(super) trait SimpleTool: 'static {
    fn icon(&self) -> IconName;
    fn title(&self) -> SharedString;
    fn detail(&self) -> Option<SharedString>;
    fn status(&self) -> ToolStatus;
}

#[derive(IntoElement)]
pub(super) struct SimpleToolElement<T: SimpleTool>(T);

impl<T: SimpleTool> SimpleToolElement<T> {
    pub(super) fn new(tool: T) -> Self {
        Self(tool)
    }
}

impl<T: SimpleTool> RenderOnce for SimpleToolElement<T> {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        ToolFrame::new(
            self.0.icon(),
            self.0.title(),
            self.0.detail(),
            self.0.status(),
        )
    }
}

#[derive(IntoElement)]
pub(super) struct ToolFrame {
    icon: IconName,
    title: SharedString,
    detail: Option<SharedString>,
    status: ToolStatus,
    diff: Option<(usize, usize)>,
}

impl ToolFrame {
    pub(super) fn new(
        icon: IconName,
        title: SharedString,
        detail: Option<SharedString>,
        status: ToolStatus,
    ) -> Self {
        Self {
            icon,
            title,
            detail,
            status,
            diff: None,
        }
    }

    pub(super) fn diff(mut self, additions: usize, deletions: usize) -> Self {
        self.diff = Some((additions, deletions));
        self
    }
}

impl RenderOnce for ToolFrame {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .max_w_full()
            .min_w_0()
            .flex_shrink(1.)
            .flex()
            .flex_col()
            .gap_2()
            .rounded(theme.radius)
            .border_3()
            .border_color(theme.border)
            .bg(transparent_white())
            .px_2()
            .py_1()
            .text_sm()
            .child(
                h_flex()
                    .min_w_0()
                    .gap_1p5()
                    .child(
                        Icon::new(self.icon)
                            .xsmall()
                            .flex_none()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .whitespace_nowrap()
                            .text_color(theme.foreground)
                            .child(self.title),
                    )
                    .child(render_trailing(self.diff, self.status, cx)),
            )
            .when_some(self.detail, |this, detail| {
                this.child(
                    div()
                        .min_w_0()
                        .overflow_x_scrollbar()
                        .whitespace_normal()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(detail),
                )
            })
    }
}

fn render_trailing(diff: Option<(usize, usize)>, status: ToolStatus, cx: &App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .flex_none()
        .items_center()
        .gap_1()
        .when_some(diff, |trailing, (additions, deletions)| {
            trailing
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.success_foreground)
                        .child(format!("+{additions}")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.danger_foreground)
                        .child(format!("-{deletions}")),
                )
        })
        .child(render_status(status, cx))
        .into_any_element()
}

fn render_status(status: ToolStatus, cx: &App) -> AnyElement {
    let theme = cx.theme();
    match status {
        ToolStatus::Running => Spinner::new()
            .xsmall()
            .color(theme.warning_foreground)
            .into_any_element(),
        ToolStatus::Succeeded => Icon::new(IconName::Check)
            .xsmall()
            .text_color(theme.success_foreground)
            .into_any_element(),
        ToolStatus::Failed => Icon::new(IconName::CircleX)
            .xsmall()
            .text_color(theme.danger_foreground)
            .into_any_element(),
    }
}

pub(super) fn append_progress(
    detail: Option<String>,
    progress: Option<&[SharedString]>,
) -> Option<SharedString> {
    let mut parts = Vec::new();
    if let Some(detail) = detail.filter(|detail| !detail.is_empty()) {
        parts.push(detail);
    }
    if let Some(progress) = progress {
        let progress = progress
            .iter()
            .map(AsRef::<str>::as_ref)
            .collect::<Vec<_>>()
            .join("\n");
        if !progress.is_empty() {
            parts.push(progress);
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n\n").into())
}

pub(super) fn format_json(value: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "<unavailable>".into())
}
