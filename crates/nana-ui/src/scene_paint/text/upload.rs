//! Glyph atlas uploads: one region per newly placed or relocated glyph.
//!
//! Never the whole atlas. A frame that adds one glyph to a full page writes
//! that glyph's rectangle and nothing else, which is the difference between a
//! few hundred bytes and a megabyte of texture traffic per frame.
//!
//! In-flight safety across submissions is `wgpu::Queue::write_texture`'s: it
//! copies the bytes into queue-owned staging at call time and orders the
//! transfer ahead of the next submission, so nothing here can overwrite a
//! region an earlier submission is still reading. That is the reason this
//! queue stages through the `Queue` rather than keeping a hand-rolled ring — a
//! ring would have to track fences that the painter, which owns neither the
//! submit nor the surface, cannot see.
//!
//! Within one submission that ordering is the hazard. The atlas is shared by
//! every target the painter draws, and a frame that records several of them
//! lands all of its queue writes before any of their passes: a later target
//! that evicts or repacks an earlier one's glyphs would have the earlier pass
//! sample the later target's. A paint that follows another in the same frame
//! therefore records its regions in its own encoder instead, after the passes
//! already recorded and ahead of its own.

use std::sync::Arc;

use super::atlas::{AtlasPageKind, GlyphAtlasManager};
use super::raster::GlyphImage;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GlyphUploadCounters {
    pub regions: u64,
    pub bytes: u64,
}

struct PendingUpload {
    kind: AtlasPageKind,
    page: u32,
    /// Destination of the *padded* cell, gutter included.
    origin: [u32; 2],
    cell: [u32; 2],
    image: Arc<GlyphImage>,
    padding: u32,
}

#[derive(Default)]
pub(super) struct GlyphUploadQueue {
    pending: Vec<PendingUpload>,
    /// Scratch for the padded cell, reused across regions and frames so a
    /// frame that faults in a page of CJK does not allocate per glyph.
    scratch: Vec<u8>,
    counters: GlyphUploadCounters,
}

impl GlyphUploadQueue {
    pub(super) fn counters(&self) -> GlyphUploadCounters {
        self.counters
    }

    /// Queue one glyph's cell. Holding the `Arc` is what lets the raster cache
    /// evict the entry that produced it before this ever reaches the GPU.
    pub(super) fn push(
        &mut self,
        kind: AtlasPageKind,
        page: u32,
        origin: [u32; 2],
        cell: [u32; 2],
        image: Arc<GlyphImage>,
        padding: u32,
    ) {
        self.pending.push(PendingUpload {
            kind,
            page,
            origin,
            cell,
            image,
            padding,
        });
    }

    /// Write every queued region. Called before the pass that samples them.
    ///
    /// With `ordered`, the regions are copied in that encoder, where they land
    /// after every pass recorded in it so far; see the module docs. Returns
    /// whether it recorded any: those copies are lost with the encoder if its
    /// frame is dropped, unlike queue writes, which still land.
    pub(super) fn flush(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &GlyphAtlasManager,
        work: Option<&crate::gpu_work::GpuWorkSink>,
        ordered: Option<&mut wgpu::CommandEncoder>,
    ) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        if let Some(encoder) = ordered {
            self.flush_in_encoder(device, encoder, atlas, work);
            return true;
        }
        for upload in self.pending.drain(..) {
            let Some(texture) = atlas.page_texture(upload.page) else {
                continue;
            };
            debug_assert_eq!(
                upload.kind.bytes_per_texel(),
                upload.image.format.bytes_per_pixel(),
                "a region's page kind and its bitmap's format must agree, or \
                 the row stride below reads the wrong number of bytes"
            );
            let texel = upload.kind.bytes_per_texel();
            let row = upload.cell[0] as usize * texel;
            let total = row * upload.cell[1] as usize;
            self.scratch.clear();
            self.scratch.resize(total, 0);
            let pad = upload.padding as usize;
            let glyph_row = upload.image.width as usize * texel;
            for y in 0..upload.image.height as usize {
                let src = y * glyph_row;
                let dest = (y + pad) * row + pad * texel;
                self.scratch[dest..dest + glyph_row]
                    .copy_from_slice(&upload.image.data[src..src + glyph_row]);
            }
            if let Some(work) = work {
                work.write_texture(
                    queue,
                    texture,
                    [upload.origin[0], upload.origin[1], 0],
                    &self.scratch,
                    row as u32,
                    upload.cell[1],
                    [upload.cell[0], upload.cell[1], 1],
                );
            } else {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: upload.origin[0],
                            y: upload.origin[1],
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &self.scratch,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row as u32),
                        rows_per_image: Some(upload.cell[1]),
                    },
                    wgpu::Extent3d {
                        width: upload.cell[0],
                        height: upload.cell[1],
                        depth_or_array_layers: 1,
                    },
                );
            }
            self.counters.regions += 1;
            self.counters.bytes += total as u64;
        }
        false
    }

    /// Every queued region in staging buffers of their own, one copy each.
    /// Rare by construction — only a paint that follows another in the same
    /// frame and places glyphs — so the buffers are not pooled; each lives
    /// until the copies that read it have run.
    fn flush_in_encoder(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        atlas: &GlyphAtlasManager,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64;
        // A repack of a large atlas can relocate thousands of glyphs; split it
        // so no buffer exceeds what the device allows. One region is a single
        // padded glyph cell, far below any device's limit.
        let limit = device.limits().max_buffer_size;
        let pending = std::mem::take(&mut self.pending);
        let mut start = 0;
        while start < pending.len() {
            let mut size = 0u64;
            let mut end = start;
            while end < pending.len() {
                let upload = &pending[end];
                let stride = (u64::from(upload.cell[0]) * upload.kind.bytes_per_texel() as u64)
                    .next_multiple_of(align);
                let next = size + stride * u64::from(upload.cell[1]);
                if next > limit && end > start {
                    break;
                }
                size = next;
                end += 1;
            }
            self.copy_chunk(device, encoder, atlas, work, &pending[start..end], size);
            start = end;
        }
    }

    fn copy_chunk(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        atlas: &GlyphAtlasManager,
        work: Option<&crate::gpu_work::GpuWorkSink>,
        uploads: &[PendingUpload],
        size: u64,
    ) {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        // Mapped at creation, so zeroed: the gutters need no writes.
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.text.atlas.ordered"),
            size,
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        let Ok(mut view) = staging.slice(..).get_mapped_range_mut() else {
            // A lost device. Its error sink has the report, and nothing this
            // frame records will reach the GPU.
            return;
        };
        // Each region at a multiple of the row alignment, which is also a
        // multiple of any texel size.
        let mut regions = Vec::with_capacity(uploads.len());
        let mut at = 0usize;
        for upload in uploads {
            let texel = upload.kind.bytes_per_texel();
            debug_assert_eq!(
                texel,
                upload.image.format.bytes_per_pixel(),
                "a region's page kind and its bitmap's format must agree, or \
                 the row stride below reads the wrong number of bytes"
            );
            let row = upload.cell[0] as usize * texel;
            let stride = row.next_multiple_of(align);
            let pad = upload.padding as usize;
            let glyph_row = upload.image.width as usize * texel;
            for y in 0..upload.image.height as usize {
                let src = y * glyph_row;
                let dest = at + (y + pad) * stride + pad * texel;
                view.slice(dest..dest + glyph_row)
                    .copy_from_slice(&upload.image.data[src..src + glyph_row]);
            }
            regions.push((at, row, stride));
            at += stride * upload.cell[1] as usize;
        }
        drop(view);
        staging.unmap();
        for (upload, (at, row, stride)) in uploads.iter().zip(regions) {
            let Some(texture) = atlas.page_texture(upload.page) else {
                continue;
            };
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: at as u64,
                        bytes_per_row: Some(stride as u32),
                        rows_per_image: Some(upload.cell[1]),
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: upload.origin[0],
                        y: upload.origin[1],
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: upload.cell[0],
                    height: upload.cell[1],
                    depth_or_array_layers: 1,
                },
            );
            let total = row * upload.cell[1] as usize;
            if let Some(work) = work {
                work.record_upload(total);
            }
            self.counters.regions += 1;
            self.counters.bytes += total as u64;
        }
    }
}
