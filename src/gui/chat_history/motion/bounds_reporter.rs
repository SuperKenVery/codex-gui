use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window,
};

type BoundsCallback = Box<dyn Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static>;

/// Reports the child's own resolved bounds during prepaint without adding an
/// observer child that can acquire a different absolute-layout origin.
pub(super) struct BoundsReporter {
    child: AnyElement,
    callback: BoundsCallback,
}

impl BoundsReporter {
    pub(super) fn new(
        child: impl IntoElement,
        callback: impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            child: child.into_any_element(),
            callback: Box::new(callback),
        }
    }
}

impl IntoElement for BoundsReporter {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BoundsReporter {
    type RequestLayoutState = ();
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
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        (self.callback)(bounds, window, cx);
        self.child.prepaint(window, cx);
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
