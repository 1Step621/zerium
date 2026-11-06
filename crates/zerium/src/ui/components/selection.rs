//! Pointer geometry shared by rectangular selections; panels own selection rules.
use gpui::{Bounds, Pixels, Point};

#[derive(Clone, Debug)]
pub(crate) struct SelectionMarquee {
    origin: Point<Pixels>,
    current: Point<Pixels>,
    active: bool,
}

impl SelectionMarquee {
    pub(crate) fn new(origin: Point<Pixels>) -> Self {
        Self {
            origin,
            current: origin,
            active: false,
        }
    }

    pub(crate) fn origin(&self) -> Point<Pixels> {
        self.origin
    }

    pub(crate) fn update(&mut self, position: Point<Pixels>) {
        self.current = position;
        self.active |= (self.current - self.origin).magnitude() >= 4.;
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    pub(crate) fn bounds(&self) -> Bounds<Pixels> {
        Bounds::from_corners(
            self.origin.min(&self.current),
            self.origin.max(&self.current),
        )
    }
}
