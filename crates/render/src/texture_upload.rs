//! Bounded row and layer copies for texture generations that are not visible yet.

use std::ops::Range;

pub(crate) const TEXTURE_UPLOAD_BATCH_BYTES: usize = 512 * 1024;

#[derive(Default)]
pub(crate) struct TextureUploadCursor {
    row: u32,
    layer: u32,
}

pub(crate) struct TextureUploadSlice {
    pub bytes: Range<usize>,
    pub row: u32,
    pub layer: u32,
    pub rows: u32,
    pub layers: u32,
}

impl TextureUploadCursor {
    /// Returns whole rows within the remaining byte budget, batching whole layers when possible.
    pub(crate) fn take(
        &mut self,
        size: [u32; 3],
        bytes_per_texel: usize,
        remaining: &mut usize,
    ) -> Option<TextureUploadSlice> {
        let [width, height, layers] = size;
        if self.layer >= layers || width == 0 || height == 0 {
            return None;
        }
        let row_bytes = width as usize * bytes_per_texel;
        let available_rows = *remaining / row_bytes;
        if available_rows == 0 {
            return None;
        }
        let rows = (height - self.row).min(available_rows as u32);
        let copied_layers = if self.row == 0 && rows == height {
            (layers - self.layer).min((available_rows / height as usize) as u32)
        } else {
            1
        };
        let start = (self.layer as usize * height as usize + self.row as usize) * row_bytes;
        let bytes = rows as usize * copied_layers as usize * row_bytes;
        let slice = TextureUploadSlice {
            bytes: start..start + bytes,
            row: self.row,
            layer: self.layer,
            rows,
            layers: copied_layers,
        };
        *remaining -= bytes;
        self.row += rows;
        if self.row == height {
            self.row = 0;
            self.layer += copied_layers;
        }
        Some(slice)
    }

    /// A texture can be published only after every row of every layer was issued.
    pub(crate) fn complete(&self, layers: u32) -> bool {
        self.layer == layers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_layers_are_contiguous_bounded_and_complete() {
        for size in [[1024, 1024, 3], [16, 16, 300], [3, 7, 2]] {
            let mut cursor = TextureUploadCursor::default();
            let mut offset = 0;
            while !cursor.complete(size[2]) {
                let mut remaining = TEXTURE_UPLOAD_BATCH_BYTES;
                let before = offset;
                while let Some(slice) = cursor.take(size, 4, &mut remaining) {
                    assert_eq!(slice.bytes.start, offset);
                    assert_eq!(
                        slice.bytes.len(),
                        slice.rows as usize * slice.layers as usize * size[0] as usize * 4
                    );
                    assert!(slice.row + slice.rows <= size[1]);
                    assert!(slice.layer + slice.layers <= size[2]);
                    offset = slice.bytes.end;
                }
                assert!(offset > before);
                assert!(offset - before <= TEXTURE_UPLOAD_BATCH_BYTES);
            }
            assert_eq!(offset, size.iter().product::<u32>() as usize * 4);
            let mut remaining = TEXTURE_UPLOAD_BATCH_BYTES;
            assert!(cursor.take(size, 4, &mut remaining).is_none());
        }
    }
}
