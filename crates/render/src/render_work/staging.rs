use super::*;

/// Counts payload copied into retained upload storage without a queue write.
pub(crate) fn staged_buffer_upload(bytes: usize) {
    record!(buffer_upload_bytes, bytes);
    record!(staged_uploads, 1);
    record!(staged_upload_bytes, bytes);
}

/// Marks a nonblocking fallback; the queue wrapper counts its payload separately.
pub(crate) fn fallback_buffer_upload(bytes: usize) {
    record!(fallback_uploads, 1);
    record!(fallback_upload_bytes, bytes);
}

/// Records retained staging growth independently of destination-buffer uploads.
pub(crate) fn staging_buffer_allocation(bytes: usize) {
    record!(staging_buffers, 1);
    record!(staging_capacity_bytes, bytes);
}
