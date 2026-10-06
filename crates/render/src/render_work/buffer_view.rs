use crate::render_systems::Span;
use std::ops::{Deref, DerefMut};

/// Keeps API timing open through the upload copy and the queue enqueue on drop.
pub(crate) struct BufferWriteView {
    view: Option<wgpu::QueueWriteBufferView>,
    _span: Span,
}

impl BufferWriteView {
    /// Takes ownership of the staging view and the span started before its allocation.
    pub(super) fn new(view: wgpu::QueueWriteBufferView, span: Span) -> Self {
        Self {
            view: Some(view),
            _span: span,
        }
    }
}

impl Deref for BufferWriteView {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.view.as_ref().expect("live upload view")
    }
}

impl DerefMut for BufferWriteView {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.view.as_mut().expect("live upload view")
    }
}

impl Drop for BufferWriteView {
    fn drop(&mut self) {
        drop(self.view.take());
    }
}
