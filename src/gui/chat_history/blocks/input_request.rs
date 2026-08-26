use std::sync::Arc;

use gpui::{IntoElement, ParentElement, Styled, div, prelude::*, px};
use gpui_component::{
    Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    theme::Theme,
};

use crate::gui::PendingUserInputRequest;

pub(super) type AnswerHandler = Arc<dyn Fn(String, String, &mut gpui::App) + Send + Sync + 'static>;

pub(super) fn render(
    request: PendingUserInputRequest,
    theme: &Theme,
    on_answer: AnswerHandler,
    on_reject: impl Fn(&mut gpui::App) + Send + Sync + 'static,
) -> impl IntoElement {
    let key = request.request_id.to_string();
    div().w_full().min_w_0().py_2().child(
        div()
            .w_full()
            .min_w_0()
            .rounded_lg()
            .border_1()
            .border_color(theme.warning_foreground)
            .bg(theme.muted)
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().font_semibold().child("Codex needs your input"))
            .children(request.questions.into_iter().map(|question| {
                let question_id = question.id.clone();
                let selected = request
                    .answers
                    .get(&question.id)
                    .cloned()
                    .unwrap_or_default();
                let freeform = question.options.as_ref().is_none_or(Vec::is_empty);
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(div().text_xs().font_semibold().child(question.header))
                    .child(
                        div()
                            .text_sm()
                            .line_height(px(21.))
                            .whitespace_normal()
                            .child(question.question),
                    )
                    .when_some(question.options, |this, options| {
                        this.child(
                            h_flex()
                                .flex_wrap()
                                .gap_2()
                                .children(options.into_iter().map(|option| {
                                    let handler = on_answer.clone();
                                    let answer = option.label.clone();
                                    let option_question_id = question_id.clone();
                                    Button::new(format!(
                                        "input-{key}-{option_question_id}-{answer}"
                                    ))
                                    .small()
                                    .when(selected.contains(&answer), |button| button.primary())
                                    .label(option.label)
                                    .tooltip(option.description)
                                    .on_click(
                                        move |_, _, cx| {
                                            handler(option_question_id.clone(), answer.clone(), cx)
                                        },
                                    )
                                })),
                        )
                    })
                    .when(question.is_other, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Type your answer in the composer below."),
                        )
                    })
                    .when(freeform && !question.is_other, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Type your answer in the composer below."),
                        )
                    })
            }))
            .child(
                Button::new(format!("input-reject-{key}"))
                    .small()
                    .danger()
                    .label("Reject request")
                    .on_click(move |_, _, cx| on_reject(cx)),
            ),
    )
}
