use gpui::{
    AnyElement, App, ClickEvent, ClipboardItem, IntoElement, ParentElement, SharedString, Styled,
    Window, div, prelude::*, px,
};
use gpui_component::{
    ElementExt as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    spinner::Spinner,
    theme::Theme,
};

use super::{
    super::motion::{AnimatedUserMessage, SendAnimationLaunch, UserMessageTarget},
    UserMessageDelivery,
};

pub(super) fn render_assistant_header(author: &'static str, theme: &Theme) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .pt_2()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(author)
}

pub(super) fn render_user(
    key: &str,
    body: SharedString,
    delivery: UserMessageDelivery,
    actions_available: bool,
    animation: Option<SendAnimationLaunch>,
    theme: &Theme,
    on_animation_complete: impl FnOnce(&mut App) + 'static,
    on_edit: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_fork: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let copy_body = body.clone();
    let animation_target = animation.as_ref().map(|_| UserMessageTarget::default());
    let animation_overlay = animation
        .as_ref()
        .map(|_| render_user_bubble(body.clone(), theme).into_any_element());
    let animating = animation_target.is_some();
    let delivery = if animating {
        UserMessageDelivery::Sending
    } else {
        delivery
    };
    let bubble = render_user_bubble(body, theme)
        .when_some(animation_target.clone(), |bubble, target| {
            bubble.on_prepaint(move |bounds, _, _| target.report(bounds))
        });

    let row = div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .flex()
        .justify_end()
        .child(
            div()
                .w_full()
                .max_w(px(620.))
                .min_w_0()
                .flex()
                .flex_col()
                .items_end()
                .when(animating, |content| content.opacity(0.))
                .child(bubble)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pt_1()
                        .pr_1()
                        .when(matches!(delivery, UserMessageDelivery::Sending), |footer| {
                            footer
                                .child(Spinner::new().xsmall().color(theme.muted_foreground))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child("Sending…"),
                                )
                        })
                        .when(matches!(delivery, UserMessageDelivery::Failed), |footer| {
                            footer.child(div().text_xs().text_color(theme.danger).child("Not sent"))
                        })
                        .when(actions_available, |footer| {
                            footer
                                .child(
                                    Button::new(format!("copy-user-message-{key}"))
                                        .xsmall()
                                        .ghost()
                                        .label("Copy")
                                        .tooltip("Copy message")
                                        .on_click(move |_, _, cx| {
                                            cx.stop_propagation();
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy_body.to_string(),
                                            ));
                                        }),
                                )
                                .child(
                                    Button::new(format!("edit-user-message-{key}"))
                                        .xsmall()
                                        .ghost()
                                        .label("Edit")
                                        .tooltip("Edit message in a fork")
                                        .on_click(on_edit),
                                )
                                .child(
                                    Button::new(format!("fork-user-message-{key}"))
                                        .xsmall()
                                        .ghost()
                                        .label("Fork")
                                        .tooltip("Fork chat from this message")
                                        .on_click(on_fork),
                                )
                        }),
                ),
        );

    if let Some(animation) = animation {
        AnimatedUserMessage::new(
            format!("animated-user-message-{key}"),
            animation,
            animation_target.expect("animation target must exist"),
            row.into_any_element(),
            animation_overlay.expect("animation overlay must exist"),
            on_animation_complete,
        )
        .into_any_element()
    } else {
        row.into_any_element()
    }
}

fn render_user_bubble(body: SharedString, theme: &Theme) -> gpui::Div {
    div()
        .min_w_0()
        .max_w(px(620.))
        .overflow_x_hidden()
        .rounded_3xl()
        .bg(theme.secondary)
        .px_3()
        .py_2()
        .text_base()
        .line_height(px(25.))
        .text_color(theme.secondary_foreground)
        .whitespace_normal()
        .child(body)
}
