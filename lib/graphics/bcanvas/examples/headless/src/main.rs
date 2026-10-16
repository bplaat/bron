/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Draw without a window and save the canvas as a PNG.

use bcanvas::{Color, OffscreenCanvas, TextAlign};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut canvas = OffscreenCanvas::new(800, 600);
    canvas.draw(|ctx| {
        ctx.set_fill_style(Color::from_rgb(0x101820));
        ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());

        ctx.set_fill_style(Color::from_rgb(0x47b8e0));
        ctx.fill_rect(100.0, 100.0, 600.0, 400.0);

        ctx.set_fill_style(Color::from_rgb(0xffbd69));
        ctx.begin_path();
        ctx.ellipse(400.0, 300.0, 150.0, 150.0);
        ctx.fill();

        ctx.set_fill_style(Color::from_rgb(0x101820));
        ctx.set_font("sans-serif", 48.0);
        ctx.set_text_align(TextAlign::Center);
        ctx.fill_text("Headless canvas", 400.0, 315.0);
    });

    let bitmap = canvas.to_bitmap()?;
    std::fs::write("frame.png", image::encode(&bitmap, image::Format::Png)?)?;
    Ok(())
}
