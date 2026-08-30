use gpui::{
    AnyElement, App, Bounds, Display, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, Position, Style, Window,
};

/// Positions the child's top-left corner at an exact window-space point.
///
/// Unlike `anchored()`, it does not applying overflow fitting or reserving space in the parent's normal flow.
pub(super) struct WindowPositioned {
    position: Point<Pixels>,
    child: AnyElement,
}

impl WindowPositioned {
    pub(super) fn new(position: Point<Pixels>, child: impl IntoElement) -> Self {
        Self {
            position,
            child: child.into_any_element(),
        }
    }
}

impl IntoElement for WindowPositioned {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for WindowPositioned {
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let child_layout_id = self.child.request_layout(window, cx);
        let style = Style {
            display: Display::Flex,
            position: Position::Absolute,
            ..Style::default()
        };
        let layout_id = window.request_layout(style, Some(child_layout_id), cx);
        (layout_id, child_layout_id)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child_layout_id: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let child_bounds = window.layout_bounds(*child_layout_id);
        let offset = self.position - child_bounds.origin;
        window.with_element_offset(offset, |window| self.child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;
    use crate::gui::chat_history::motion::BoundsReporter;
    use gpui::{
        Context, Render, TestAppContext, VisualTestContext, deferred, div, point, prelude::*, px,
    };

    struct WindowPositionProbe {
        desired: Point<Pixels>,
        observed: Rc<Cell<Option<Bounds<Pixels>>>>,
    }

    impl Render for WindowPositionProbe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let observed = self.observed.clone();
            let bubble = BoundsReporter::new(div().w(px(54.)).h(px(165.)), move |bounds, _, _| {
                observed.set(Some(bounds));
            });

            div()
                .size_full()
                .relative()
                .pl(px(12.))
                .child(div().h(px(157.)))
                .child(deferred(WindowPositioned::new(self.desired, bubble)).with_priority(10))
        }
    }

    #[gpui::test]
    fn deferred_child_lands_at_requested_window_origin(cx: &mut TestAppContext) {
        let desired = point(px(500.), px(200.));
        let observed = Rc::new(Cell::new(None));
        let observed_in_view = observed.clone();
        let (_, cx) = cx.add_window_view(move |_, _| WindowPositionProbe {
            desired,
            observed: observed_in_view,
        });
        let cx: &mut VisualTestContext = cx;

        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        assert_eq!(observed.get().map(|bounds| bounds.origin), Some(desired));
    }
}
