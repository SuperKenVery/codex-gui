use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, ParentElement as _, Pixels, Point, Style, Window,
    anchored, point, px, relative, size,
};
use gpui_component::animation::ease_in_out_cubic;

const SEND_ANIMATION_DURATION: Duration = Duration::from_millis(360);
pub(super) const SEND_DESTINATION_TIMEOUT: Duration = Duration::from_millis(750);

const BUBBLE_PADDING_X: Pixels = px(12.);
const BUBBLE_PADDING_Y: Pixels = px(8.);

/// The handoff from the composer to the user-message element. All frame-by-frame
/// animation state lives in `AnimatedUserMessageState` once the row appears.
#[derive(Clone)]
pub(super) struct SendAnimationLaunch {
    pub(super) client_id: String,
    source: Point<Pixels>,
    started: Rc<Cell<bool>>,
}

impl SendAnimationLaunch {
    pub(super) fn new(client_id: String, source_bounds: Bounds<Pixels>) -> Self {
        // The textarea reports the text origin. Offset the bubble by its padding
        // so the flying copy's glyphs initially cover the composer glyphs.
        let source = point(
            source_bounds.origin.x - BUBBLE_PADDING_X,
            source_bounds.origin.y - BUBBLE_PADDING_Y,
        );
        Self {
            client_id,
            source,
            started: Rc::new(Cell::new(false)),
        }
    }

    pub(super) fn is_waiting(&self) -> bool {
        !self.started.get()
    }

    fn mark_started(&self) {
        self.started.set(true);
    }
}

/// Receives the bubble's window-space origin while the complete, invisible row
/// is prepainted. The animation element reads it later in the same prepaint.
#[derive(Clone, Default)]
pub(super) struct UserMessageTarget(Rc<Cell<Option<Point<Pixels>>>>);

impl UserMessageTarget {
    pub(super) fn report(&self, bounds: Bounds<Pixels>) {
        self.0.set(Some(bounds.origin));
    }

    fn get(&self) -> Option<Point<Pixels>> {
        self.0.get()
    }
}

#[derive(Clone, Copy, Default)]
struct AnimatedUserMessageState {
    natural_height: Option<Pixels>,
    target: Option<Point<Pixels>>,
    started_at: Option<Instant>,
    completion_scheduled: bool,
}

/// Owns the complete send transition once its matching history row exists:
/// intrinsic measurement, animated list height, target tracking, flying overlay,
/// frame scheduling, and completion.
pub(super) struct AnimatedUserMessage {
    id: ElementId,
    launch: SendAnimationLaunch,
    target: UserMessageTarget,
    row: AnyElement,
    overlay: Option<AnyElement>,
    on_complete: Option<Box<dyn FnOnce(&mut App)>>,
}

impl AnimatedUserMessage {
    pub(super) fn new(
        id: impl Into<ElementId>,
        launch: SendAnimationLaunch,
        target: UserMessageTarget,
        row: AnyElement,
        overlay: AnyElement,
        on_complete: impl FnOnce(&mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            launch,
            target,
            row,
            overlay: Some(overlay),
            on_complete: Some(Box::new(on_complete)),
        }
    }
}

impl IntoElement for AnimatedUserMessage {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for AnimatedUserMessage {
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
            global_id.expect("AnimatedUserMessage must have an id"),
            |state: Option<AnimatedUserMessageState>, _| {
                let state = state.unwrap_or_default();
                (state, state)
            },
        );
        let sample = state
            .target
            .zip(state.started_at)
            .map(|(target, started_at)| {
                sample_animation(self.launch.source, target, started_at, Instant::now())
            });
        let (progress, overlay_origin) = sample.map_or((0., self.launch.source), |sample| {
            (sample.progress, sample.origin)
        });
        let mut overlay = anchored()
            .position(overlay_origin)
            .child(
                self.overlay
                    .take()
                    .expect("AnimatedUserMessage overlay must exist during layout"),
            )
            .into_any_element();
        let overlay_layout_id = overlay.request_layout(window, cx);
        self.overlay = Some(overlay);

        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = state
            .natural_height
            .map(|height| height * progress)
            .unwrap_or(px(0.))
            .into();

        (
            window.request_layout(style, Some(overlay_layout_id), cx),
            (),
        )
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
        let measured = self.row.layout_as_root(available, window, cx);

        // Prepainting the invisible row records the bubble target and installs
        // its normal mouse handlers, while the mask exposes only the animated
        // fraction of its height to painting and hit testing.
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.row.prepaint_at(bounds.origin, window, cx);
        });

        let now = Instant::now();
        let current_target = self.target.get();
        let (state, measurement_changed) = window.with_element_state(
            global_id.expect("AnimatedUserMessage must have an id"),
            |state: Option<AnimatedUserMessageState>, _| {
                let mut state = state.unwrap_or_default();
                let measurement_changed = state.natural_height != Some(measured.height);
                state.natural_height = Some(measured.height);
                if let Some(target) = current_target {
                    state.target = Some(target);
                }
                if state.started_at.is_none() && state.target.is_some() {
                    state.started_at = Some(now);
                }
                ((state, measurement_changed), state)
            },
        );

        if state.started_at.is_some() {
            self.launch.mark_started();
        }
        if measurement_changed {
            window.request_animation_frame();
        }

        let (Some(target), Some(started_at), Some(overlay)) =
            (state.target, state.started_at, self.overlay.take())
        else {
            return;
        };
        let sample = sample_animation(self.launch.source, target, started_at, now);
        // Layout and text measurement already ran through the absolute child
        // layout returned by `request_layout`; only drawing is deferred here.
        window.defer_draw(overlay, window.element_offset(), 10, None);

        if sample.complete && !state.completion_scheduled {
            window.with_element_state(
                global_id.expect("AnimatedUserMessage must have an id"),
                |state: Option<AnimatedUserMessageState>, _| {
                    let mut state = state.unwrap_or_default();
                    state.completion_scheduled = true;
                    ((), state)
                },
            );
            if let Some(on_complete) = self.on_complete.take() {
                window.on_next_frame(move |_, cx| on_complete(cx));
            }
        } else if !sample.complete {
            window.request_animation_frame();
        }
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
            self.row.paint(window, cx);
        });
    }
}

struct SendAnimationSample {
    origin: Point<Pixels>,
    progress: f32,
    complete: bool,
}

fn sample_animation(
    source: Point<Pixels>,
    target: Point<Pixels>,
    started_at: Instant,
    now: Instant,
) -> SendAnimationSample {
    let linear = (now.saturating_duration_since(started_at).as_secs_f32()
        / SEND_ANIMATION_DURATION.as_secs_f32())
    .clamp(0., 1.);
    let progress = ease_in_out_cubic(linear);
    SendAnimationSample {
        origin: cubic_flight(source, target, progress),
        progress,
        complete: linear >= 1.,
    }
}

fn cubic_flight(start: Point<Pixels>, end: Point<Pixels>, progress: f32) -> Point<Pixels> {
    let start_x = f32::from(start.x);
    let start_y = f32::from(start.y);
    let end_x = f32::from(end.x);
    let end_y = f32::from(end.y);
    let dx = end_x - start_x;
    let dy = end_y - start_y;

    // Leave the composer mostly vertically, then bend into the right-aligned
    // destination. This also behaves sensibly for resized or narrow windows.
    let control_1 = (start_x + dx * 0.08, start_y + dy * 0.42);
    let control_2 = (end_x - dx * 0.32, end_y - dy * 0.08);
    point(
        px(cubic_axis(
            start_x,
            control_1.0,
            control_2.0,
            end_x,
            progress,
        )),
        px(cubic_axis(
            start_y,
            control_1.1,
            control_2.1,
            end_y,
            progress,
        )),
    )
}

fn cubic_axis(start: f32, control_1: f32, control_2: f32, end: f32, t: f32) -> f32 {
    let inverse = 1. - t;
    inverse.powi(3) * start
        + 3. * inverse.powi(2) * t * control_1
        + 3. * inverse * t.powi(2) * control_2
        + t.powi(3) * end
}
