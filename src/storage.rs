use std::any::Any;

/// Reference backend: owns one typed allocation, without execution semantics.
#[derive(Default)]
pub(crate) struct FrameStorage {
    frame: Option<Box<dyn Any + Send>>,
}

impl FrameStorage {
    pub(crate) fn is_empty(&self) -> bool {
        self.frame.is_none()
    }

    pub(crate) fn get_or_insert<S: Default + Send + 'static>(&mut self) -> Option<&mut S> {
        self.frame
            .get_or_insert_with(|| Box::<S>::default())
            .downcast_mut()
    }

    pub(crate) fn clear(&mut self) {
        self.frame = None;
    }
}
