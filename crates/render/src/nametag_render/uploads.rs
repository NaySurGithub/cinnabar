//! Command-owned RGBA pixels for dirty nameplate cells. The browser's dynamic
//! writeTexture staging path corrupted one-off cells during actor-buffer uploads.

use super::{NAMETAG_ATLAS_SIDE, NametagAtlasRect, RenderDevice, RenderQueue};
use bevy::render::render_resource::{
    BufferInitDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, TexelCopyBufferInfo,
    TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
};

struct StagedCells<'a> {
    cells: Vec<&'a NametagAtlasRect>,
    origin: [u32; 2],
    pitch: u32,
    bytes: Vec<u8>,
}

impl<'a> StagedCells<'a> {
    fn new(cells: impl Iterator<Item = &'a NametagAtlasRect>) -> Option<Self> {
        let cells: Vec<_> = cells.filter(|cell| valid(cell)).collect();
        let first = cells.first()?;
        let [x, y, width, height] = first.cell;
        let (mut origin, mut end) = ([x, y], [x + width, y + height]);
        for cell in &cells[1..] {
            let [x, y, width, height] = cell.cell;
            origin = [origin[0].min(x), origin[1].min(y)];
            end = [end[0].max(x + width), end[1].max(y + height)];
        }
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let pitch = ((end[0] - origin[0]) * 4).div_ceil(alignment) * alignment;
        // The bounding box fits the atlas, so padded storage never exceeds one full atlas.
        let mut staged = Self {
            cells,
            origin,
            pitch,
            bytes: vec![0; (pitch * (end[1] - origin[1])) as usize],
        };
        for cell in &staged.cells {
            let [x, y, width, height] = cell.cell;
            let row_bytes = (width * 4) as usize;
            for row in 0..height {
                let target = staged.offset(x, y + row) as usize;
                let source = row as usize * row_bytes;
                staged.bytes[target..target + row_bytes]
                    .copy_from_slice(&cell.rgba8[source..source + row_bytes]);
            }
        }
        Some(staged)
    }

    fn offset(&self, x: u32, y: u32) -> u64 {
        u64::from((y - self.origin[1]) * self.pitch + (x - self.origin[0]) * 4)
    }
}

fn valid(cell: &NametagAtlasRect) -> bool {
    let [x, y, width, height] = cell.cell;
    width > 0
        && height > 0
        && x.checked_add(width)
            .is_some_and(|end| end <= NAMETAG_ATLAS_SIDE)
        && y.checked_add(height)
            .is_some_and(|end| end <= NAMETAG_ATLAS_SIDE)
        && cell.rgba8.len() == (width * height * 4) as usize
}

pub(super) fn upload<'a>(
    device: &RenderDevice,
    queue: &RenderQueue,
    texture: &Texture,
    cells: impl Iterator<Item = &'a NametagAtlasRect>,
) {
    let Some(staged) = StagedCells::new(cells) else {
        return;
    };
    // Mapped-at-creation initialization owns the texels in a GPU buffer before
    // recording the copy. Submitted commands retain that buffer through completion.
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("nametag atlas staging pixels"),
        contents: &staged.bytes,
        usage: BufferUsages::COPY_SRC,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("nametag atlas cell copies"),
    });
    for cell in &staged.cells {
        let [x, y, width, height] = cell.cell;
        encoder.copy_buffer_to_texture(
            TexelCopyBufferInfo {
                buffer: &buffer,
                layout: TexelCopyBufferLayout {
                    offset: staged.offset(x, y),
                    bytes_per_row: Some(staged.pitch),
                    rows_per_image: Some(height),
                },
            },
            TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: TextureAspect::All,
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
    queue.submit([encoder.finish()]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn dirty_cells_keep_their_rgba_rows_and_origins_in_aligned_owned_storage() {
        let cells = [
            NametagAtlasRect {
                cell: [2, 1, 3, 2],
                rgba8: Arc::from([255, 255, 255, 255, 0, 0, 0, 0, 255, 0, 0, 255].repeat(2)),
            },
            NametagAtlasRect {
                cell: [8, 3, 2, 1],
                rgba8: Arc::from([0, 255, 0, 255, 0, 0, 255, 255]),
            },
        ];
        let staged = StagedCells::new(cells.iter()).unwrap();
        assert_eq!(staged.origin, [2, 1]);
        assert_eq!(staged.pitch % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
        assert_eq!(staged.bytes.len(), (staged.pitch * 3) as usize);
        for cell in &cells {
            let [x, y, width, height] = cell.cell;
            for row in 0..height {
                let offset = staged.offset(x, y + row) as usize;
                let row_bytes = (width * 4) as usize;
                assert_eq!(
                    &staged.bytes[offset..offset + row_bytes],
                    &cell.rgba8[row as usize * row_bytes..(row as usize + 1) * row_bytes]
                );
            }
        }
        assert!(
            staged.bytes[12..staged.pitch as usize]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert!(StagedCells::new(std::iter::empty()).is_none());
    }

    #[test]
    fn invalid_public_cells_cannot_allocate_or_copy_outside_the_atlas() {
        let cells = [
            NametagAtlasRect {
                cell: [NAMETAG_ATLAS_SIDE, 0, 1, 1],
                rgba8: Arc::from([255; 4]),
            },
            NametagAtlasRect {
                cell: [0, 0, 2, 1],
                rgba8: Arc::from([255; 4]),
            },
            NametagAtlasRect {
                cell: [0, 0, 0, 1],
                rgba8: Arc::from([]),
            },
            NametagAtlasRect {
                cell: [u32::MAX, 0, 1, 1],
                rgba8: Arc::from([255; 4]),
            },
        ];
        assert!(StagedCells::new(cells.iter()).is_none());
    }
}
