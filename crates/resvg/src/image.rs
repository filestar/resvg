// Copyright 2018 the Resvg Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

pub fn render(
    image: &usvg::Image,
    transform: tiny_skia::Transform,
    pixmap: &mut tiny_skia::PixmapMut,
) {
    if !image.is_visible() {
        return;
    }

    render_inner(image.kind(), transform, image.rendering_mode(), pixmap);
}

pub fn render_inner(
    image_kind: &usvg::ImageKind,
    transform: tiny_skia::Transform,
    #[allow(unused_variables)] rendering_mode: usvg::ImageRendering,
    pixmap: &mut tiny_skia::PixmapMut,
) {
    match image_kind {
        usvg::ImageKind::SVG(tree) => {
            render_vector(tree, transform, pixmap);
        }
        #[cfg(feature = "raster-images")]
        _ => {
            raster_images::render_raster(image_kind, transform, rendering_mode, pixmap);
        }
        #[cfg(not(feature = "raster-images"))]
        _ => {
            log::warn!("Images decoding was disabled by a build feature.");
        }
    }
}

fn render_vector(
    tree: &usvg::Tree,
    transform: tiny_skia::Transform,
    pixmap: &mut tiny_skia::PixmapMut,
) -> Option<()> {
    let mut sub_pixmap = tiny_skia::Pixmap::new(pixmap.width(), pixmap.height()).unwrap();
    crate::render(tree, transform, &mut sub_pixmap.as_mut());
    pixmap.draw_pixmap(
        0,
        0,
        sub_pixmap.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::default(),
        None,
    );

    Some(())
}

#[cfg(feature = "raster-images")]
mod raster_images {
    use crate::OptionLog;
    use std::io::Cursor;
    use usvg::ImageRendering;

    fn decode_raster(image: &usvg::ImageKind) -> Option<tiny_skia::Pixmap> {
        match image {
            usvg::ImageKind::SVG(_) => None,
            usvg::ImageKind::JPEG(data) => {
                decode_jpeg(data).log_none(|| log::warn!("Failed to decode a JPEG image."))
            }
            usvg::ImageKind::PNG(data) => {
                decode_png(data).log_none(|| log::warn!("Failed to decode a PNG image."))
            }
            usvg::ImageKind::GIF(data) => {
                decode_gif(data).log_none(|| log::warn!("Failed to decode a GIF image."))
            }
            usvg::ImageKind::WEBP(data) => {
                decode_webp(data).log_none(|| log::warn!("Failed to decode a WebP image."))
            }
        }
    }

    fn decode_png(data: &[u8]) -> Option<tiny_skia::Pixmap> {
        tiny_skia::Pixmap::decode_png(data).ok()
    }

    fn decode_jpeg(data: &[u8]) -> Option<tiny_skia::Pixmap> {
        use zune_jpeg::zune_core::colorspace::ColorSpace;
        use zune_jpeg::zune_core::options::DecoderOptions;

        let cursor = Cursor::new(data);
        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(cursor, options);
        decoder.decode_headers().ok()?;
        let output_cs = decoder.output_colorspace()?;

        let img_data = {
            let data = decoder.decode().ok()?;
            match output_cs {
                ColorSpace::RGBA => data,
                _ => return None,
            }
        };

        let info = decoder.info()?;

        let size = tiny_skia::IntSize::from_wh(info.width as u32, info.height as u32)?;
        tiny_skia::Pixmap::from_vec(img_data, size)
    }

    fn decode_gif(data: &[u8]) -> Option<tiny_skia::Pixmap> {
        let mut decoder = gif::DecodeOptions::new();
        decoder.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = decoder.read_info(data).ok()?;
        let first_frame = decoder.read_next_frame().ok()??;

        let size = tiny_skia::IntSize::from_wh(
            u32::from(first_frame.width),
            u32::from(first_frame.height),
        )?;

        let (w, h) = size.dimensions();
        let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
        rgba_to_pixmap(&first_frame.buffer, &mut pixmap);
        Some(pixmap)
    }

    fn decode_webp(data: &[u8]) -> Option<tiny_skia::Pixmap> {
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(data)).ok()?;
        let mut first_frame = vec![0; decoder.output_buffer_size()?];
        decoder.read_image(&mut first_frame).ok()?;

        let (w, h) = decoder.dimensions();
        let mut pixmap = tiny_skia::Pixmap::new(w, h)?;

        if decoder.has_alpha() {
            rgba_to_pixmap(&first_frame, &mut pixmap);
        } else {
            rgb_to_pixmap(&first_frame, &mut pixmap);
        }

        Some(pixmap)
    }

    fn rgb_to_pixmap(data: &[u8], pixmap: &mut tiny_skia::Pixmap) {
        use rgb::FromSlice;

        let mut i = 0;
        let dst = pixmap.data_mut();
        for p in data.as_rgb() {
            dst[i + 0] = p.r;
            dst[i + 1] = p.g;
            dst[i + 2] = p.b;
            dst[i + 3] = 255;

            i += tiny_skia::BYTES_PER_PIXEL;
        }
    }

    fn rgba_to_pixmap(data: &[u8], pixmap: &mut tiny_skia::Pixmap) {
        use rgb::FromSlice;

        let mut i = 0;
        let dst = pixmap.data_mut();
        for p in data.as_rgba() {
            let a = p.a as f64 / 255.0;
            dst[i + 0] = (p.r as f64 * a + 0.5) as u8;
            dst[i + 1] = (p.g as f64 * a + 0.5) as u8;
            dst[i + 2] = (p.b as f64 * a + 0.5) as u8;
            dst[i + 3] = p.a;

            i += tiny_skia::BYTES_PER_PIXEL;
        }
    }

    /// Average `bx` by `by` blocks of premultiplied pixels (partial blocks at the right and bottom edges
    /// average what they hold).
    fn box_reduce(src: &tiny_skia::Pixmap, bx: u32, by: u32) -> Option<tiny_skia::Pixmap> {
        let (w, h) = (src.width(), src.height());
        let (nw, nh) = (w.div_ceil(bx), h.div_ceil(by));
        let mut dst = tiny_skia::Pixmap::new(nw, nh)?;
        let s = src.data();
        let d = dst.data_mut();
        let mut row = vec![0u32; nw as usize * 4];
        for oy in 0..nh {
            row.iter_mut().for_each(|v| *v = 0);
            let (y0, y1) = (oy * by, ((oy + 1) * by).min(h));
            for y in y0..y1 {
                let line = &s[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
                for x in 0..w {
                    let o = ((x / bx) * 4) as usize;
                    let i = (x * 4) as usize;
                    row[o] += line[i] as u32;
                    row[o + 1] += line[i + 1] as u32;
                    row[o + 2] += line[i + 2] as u32;
                    row[o + 3] += line[i + 3] as u32;
                }
            }
            for ox in 0..nw {
                let n = ((((ox + 1) * bx).min(w) - ox * bx) * (y1 - y0)).max(1);
                let o = (ox * 4) as usize;
                let t = ((oy * nw + ox) * 4) as usize;
                for c in 0..4 {
                    d[t + c] = ((row[o + c] + n / 2) / n) as u8;
                }
            }
        }
        Some(dst)
    }

    pub(crate) fn render_raster(
        image: &usvg::ImageKind,
        transform: tiny_skia::Transform,
        rendering_mode: usvg::ImageRendering,
        pixmap: &mut tiny_skia::PixmapMut,
    ) -> Option<()> {
        let mut raster = decode_raster(image)?;
        let mut transform = transform;

        // tiny-skia samples an image at most bicubically, from the four or sixteen pixels nearest each
        // device pixel: an image shrunk more than twice skips most of its pixels and comes out speckled
        // (a 300 dpi photo on a 72 dpi page). Browsers and PDF viewers average instead. Average whole
        // blocks first so the sampler never shrinks by more than two. Nearest-neighbour images keep
        // their pixels: they asked not to be smoothed.
        if !matches!(
            rendering_mode,
            ImageRendering::OptimizeSpeed | ImageRendering::CrispEdges | ImageRendering::Pixelated
        ) {
            let scale_x = (transform.sx * transform.sx + transform.ky * transform.ky).sqrt();
            let scale_y = (transform.kx * transform.kx + transform.sy * transform.sy).sqrt();
            let block = |scale: f32| -> u32 {
                if scale.is_finite() && scale > 0.0 && scale < 0.5 {
                    ((0.5 / scale).floor() as u32).clamp(1, 256)
                } else {
                    1
                }
            };
            let (bx, by) = (block(scale_x), block(scale_y));
            if bx > 1 || by > 1 {
                if let Some(reduced) = box_reduce(&raster, bx, by) {
                    let (fx, fy) = (
                        raster.width() as f32 / reduced.width() as f32,
                        raster.height() as f32 / reduced.height() as f32,
                    );
                    transform = transform.pre_scale(fx, fy);
                    raster = reduced;
                }
            }
        }

        let rect = tiny_skia::Size::from_wh(raster.width() as f32, raster.height() as f32)?
            .to_rect(0.0, 0.0)?;

        let quality = match rendering_mode {
            ImageRendering::OptimizeQuality => tiny_skia::FilterQuality::Bicubic,
            ImageRendering::OptimizeSpeed => tiny_skia::FilterQuality::Nearest,
            ImageRendering::Smooth => tiny_skia::FilterQuality::Bilinear,
            ImageRendering::HighQuality => tiny_skia::FilterQuality::Bicubic,
            ImageRendering::CrispEdges => tiny_skia::FilterQuality::Nearest,
            ImageRendering::Pixelated => tiny_skia::FilterQuality::Nearest,
        };

        let pattern = tiny_skia::Pattern::new(
            raster.as_ref(),
            tiny_skia::SpreadMode::Pad,
            quality,
            1.0,
            tiny_skia::Transform::default(),
        );
        let mut paint = tiny_skia::Paint::default();
        paint.shader = pattern;

        pixmap.fill_rect(rect, &paint, transform, None);

        Some(())
    }
}
