use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AvailableSpace, Bounds, ContentMask, Element,
    ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window,
    div, relative, size, prelude::*
};
use gpui_component::animation::ease_out_cubic;

const APPEAR_DURATION: Duration = Duration::from_millis(240);

#[derive(Clone, Copy, Default)]
struct AppearingState {
    natural_height: Option<Pixels>,
    started_at: Option<Instant>,
}

/// Reveals newly inserted transcript content from zero to its natural height.
///
/// The child remains laid out at full size while a shrinking layout box and
/// content mask expose only the animated portion. This keeps text and controls
/// from reflowing during the transition.
pub struct Appearing {
    id: ElementId,
    child: AnyElement,
    animate: bool,
}

impl Appearing {
    pub fn new(id: String, child: impl IntoElement, animate: bool) -> Self {
        let child = child.into_any_element();
        let child = if animate {
            div()
                .w_full()
                .child(child)
                .with_animation(
                    format!("{id}-fade"),
                    Animation::new(APPEAR_DURATION).with_easing(ease_out_cubic),
                    |this, progress| this.opacity(progress),
                )
                .into_any_element()
        } else {
            child
        };

        Self {
            id: id.into(),
            child,
            animate,
        }
    }
}

impl IntoElement for Appearing {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Appearing {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let state = window.with_element_state(
            global_id.expect("Appearing must have an id"),
            |state: Option<AppearingState>, _| {
                let state = state.unwrap_or_default();
                (state, state)
            },
        );
        let progress = appear_progress(state.started_at, self.animate, Instant::now());

        let mut style = Style::default();
        style.size.width = relative(1.).into();
        match state.natural_height {
            None if progress > 0. => {}
            None => style.size.height = gpui::px(0.).into(),
            Some(height) => style.size.height = (height * progress).into(),
        }

        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let available = size(
            AvailableSpace::Definite(bounds.size.width),
            AvailableSpace::MinContent,
        );
        let measured = self.child.layout_as_root(available, window, cx);
        let now = Instant::now();
        let (changed, started_at) = window.with_element_state(
            global_id.expect("Appearing must have an id"),
            |state: Option<AppearingState>, _| {
                let mut state = state.unwrap_or_default();
                let changed = state.natural_height != Some(measured.height);
                state.natural_height = Some(measured.height);
                if self.animate && state.started_at.is_none() {
                    state.started_at = Some(now);
                }
                ((changed, state.started_at), state)
            },
        );

        if changed || appear_progress(started_at, self.animate, now) < 1. {
            window.request_animation_frame();
        }

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.child.prepaint_at(bounds.origin, window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.child.paint(window, cx);
        });
    }
}

fn appear_progress(started_at: Option<Instant>, animate: bool, now: Instant) -> f32 {
    if !animate {
        return 1.;
    }
    let Some(started_at) = started_at else {
        return 0.;
    };
    let linear = (now.saturating_duration_since(started_at).as_secs_f32()
        / APPEAR_DURATION.as_secs_f32())
    .clamp(0., 1.);
    ease_out_cubic(linear)
}
