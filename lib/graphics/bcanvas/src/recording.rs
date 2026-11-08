/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// The backend of a drawing context, which paints natively or records the native calls to
// replay them later into any other context.

use std::sync::Arc;

use crate::canvas::{CanvasState, FillRule};
use crate::platforms::{PlatformCanvasContext, PlatformImage, PlatformOffscreenCanvas};
use crate::transform::Transform;

pub(crate) enum Backend {
    Platform(PlatformCanvasContext),
    Recording(Recorder),
}

// Recorded ops, shared by the recordings that draw them.
pub(crate) type Ops = Arc<Vec<Op>>;

pub(crate) struct Recorder {
    pub(crate) ops: Vec<Op>,
    // The last recorded state, shared by the following ops while it is unchanged.
    state: Option<Arc<CanvasState>>,
}

impl Recorder {
    pub(crate) const fn new(ops: Vec<Op>) -> Self {
        Self { ops, state: None }
    }

    // The drawing state without the transform, which native contexts keep themselves.
    fn state(&mut self, s: &CanvasState) -> Arc<CanvasState> {
        let s = CanvasState {
            transform: Transform::IDENTITY,
            ..s.clone()
        };
        match &self.state {
            Some(state) if **state == s => state.clone(),
            _ => self.state.insert(Arc::new(s)).clone(),
        }
    }
}

// Generates the ops of the native calls without drawing state, with a backend method for
// each that draws or records it.
macro_rules! ops {
    ($($op:ident => $method:ident($($arg:ident: $ty:ty),*);)*) => {
        // The rare large ops are boxed, which keeps every op small.
        pub(crate) enum Op {
            $($op { $($arg: $ty),* },)*
            SetTransform(Transform),
            Replay(Box<(Ops, Transform)>),
            FillRect(Arc<CanvasState>, [f32; 4]),
            StrokeRect(Arc<CanvasState>, [f32; 4]),
            BeginLayer(Arc<CanvasState>),
            BeginMaskLayer(Arc<CanvasState>),
            Fill(Arc<CanvasState>, FillRule),
            Stroke(Arc<CanvasState>),
            FillText(Box<(Arc<CanvasState>, Box<str>, f32, f32)>),
            DrawImage(Box<(Arc<CanvasState>, u32, u32, Arc<[u8]>, [f32; 4])>),
        }

        impl Backend {
            $(pub(crate) fn $method(&mut self, $($arg: $ty),*) {
                match self {
                    Self::Platform(platform) => platform.$method($($arg),*),
                    Self::Recording(recorder) => recorder.ops.push(Op::$op { $($arg),* }),
                }
            })*

            // Replay recorded ops, with their transforms relative to `base`. A recording keeps
            // them shared.
            pub(crate) fn replay(&mut self, ops: &Ops, base: Transform) {
                if let Self::Recording(recorder) = self {
                    recorder.ops.push(Op::Replay(Box::new((ops.clone(), base))));
                    return;
                }
                for op in ops.iter() {
                    match op {
                        $(Op::$op { $($arg),* } => self.$method($(*$arg),*),)*
                        Op::SetTransform(transform) => self.set_transform(base.concat(*transform)),
                        Op::Replay(replay) => self.replay(&replay.0, base.concat(replay.1)),
                        Op::FillRect(s, [x, y, w, h]) => self.fill_rect(s, *x, *y, *w, *h),
                        Op::StrokeRect(s, [x, y, w, h]) => self.stroke_rect(s, *x, *y, *w, *h),
                        Op::BeginLayer(s) => self.begin_layer(s),
                        Op::BeginMaskLayer(s) => self.begin_mask_layer(s),
                        Op::Fill(s, rule) => self.fill(s, *rule),
                        Op::Stroke(s) => self.stroke(s),
                        Op::FillText(text) => {
                            let (s, text, x, y) = &**text;
                            self.fill_text(s, text, *x, *y);
                        }
                        Op::DrawImage(image) => {
                            let (s, width, height, pixels, [x, y, w, h]) = &**image;
                            let image = PlatformImage::new(*width, *height, pixels.clone());
                            self.draw_image(s, &image, pixels, *x, *y, *w, *h);
                        }
                    }
                }
            }
        }
    };
}

ops! {
    Save => save();
    Restore => restore();
    ClearRect => clear_rect(x: f32, y: f32, w: f32, h: f32);
    EndMask => end_mask();
    EndLayer => end_layer();
    BeginPath => begin_path();
    ClosePath => close_path();
    MoveTo => move_to(x: f32, y: f32);
    LineTo => line_to(x: f32, y: f32);
    Rect => rect(x: f32, y: f32, w: f32, h: f32);
    Ellipse => ellipse(x: f32, y: f32, rx: f32, ry: f32);
    ArcSegment => arc(x: f32, y: f32, r: f32, start: f32, end: f32, ccw: bool);
    QuadraticCurveTo => quadratic_curve_to(cpx: f32, cpy: f32, x: f32, y: f32);
    BezierCurveTo => bezier_curve_to(a: f32, b: f32, c: f32, d: f32, x: f32, y: f32);
    Clip => clip(rule: FillRule);
    Translate => translate(x: f32, y: f32);
    Rotate => rotate(angle: f32);
    Scale => scale(x: f32, y: f32);
    ConcatTransform => transform(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32);
}

impl Backend {
    pub(crate) fn set_transform(&mut self, transform: Transform) {
        match self {
            Self::Platform(platform) => {
                let [a, b, c, d, e, f] = transform.0;
                platform.set_transform(a, b, c, d, e, f);
            }
            Self::Recording(recorder) => recorder.ops.push(Op::SetTransform(transform)),
        }
    }

    pub(crate) fn reset_transform(&mut self) {
        match self {
            Self::Platform(platform) => platform.reset_transform(),
            Self::Recording(recorder) => recorder.ops.push(Op::SetTransform(Transform::IDENTITY)),
        }
    }

    pub(crate) fn fill_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        match self {
            Self::Platform(platform) => platform.fill_rect(s, x, y, w, h),
            Self::Recording(recorder) => {
                let state = recorder.state(s);
                recorder.ops.push(Op::FillRect(state, [x, y, w, h]));
            }
        }
    }

    pub(crate) fn stroke_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        match self {
            Self::Platform(platform) => platform.stroke_rect(s, x, y, w, h),
            Self::Recording(recorder) => {
                let state = recorder.state(s);
                recorder.ops.push(Op::StrokeRect(state, [x, y, w, h]));
            }
        }
    }

    pub(crate) fn begin_layer(&mut self, s: &CanvasState) {
        match self {
            Self::Platform(platform) => platform.begin_layer(s),
            Self::Recording(recorder) => {
                let state = recorder.state(s);
                recorder.ops.push(Op::BeginLayer(state));
            }
        }
    }

    pub(crate) fn begin_mask_layer(&mut self, s: &CanvasState) {
        match self {
            Self::Platform(platform) => platform.begin_mask_layer(s),
            Self::Recording(recorder) => {
                let state = recorder.state(s);
                recorder.ops.push(Op::BeginMaskLayer(state));
            }
        }
    }

    pub(crate) fn fill(&mut self, s: &CanvasState, rule: FillRule) {
        match self {
            Self::Platform(platform) => platform.fill(s, rule),
            Self::Recording(recorder) => {
                let state = recorder.state(s);
                recorder.ops.push(Op::Fill(state, rule));
            }
        }
    }

    pub(crate) fn stroke(&mut self, s: &CanvasState) {
        match self {
            Self::Platform(platform) => platform.stroke(s),
            Self::Recording(recorder) => {
                let state = recorder.state(s);
                recorder.ops.push(Op::Stroke(state));
            }
        }
    }

    pub(crate) fn fill_text(&mut self, s: &CanvasState, text: &str, x: f32, y: f32) {
        match self {
            Self::Platform(platform) => platform.fill_text(s, text, x, y),
            Self::Recording(recorder) => {
                let state = recorder.state(s);
                recorder
                    .ops
                    .push(Op::FillText(Box::new((state, text.into(), x, y))));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_image(
        &mut self,
        s: &CanvasState,
        image: &PlatformImage,
        pixels: &Arc<[u8]>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        match self {
            Self::Platform(platform) => platform.draw_image(s, image, x, y, w, h),
            Self::Recording(recorder) => {
                let state = recorder.state(s);
                recorder.ops.push(Op::DrawImage(Box::new((
                    state,
                    image.width,
                    image.height,
                    pixels.clone(),
                    [x, y, w, h],
                ))));
            }
        }
    }

    // A recording measures with a native context of its own.
    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> f32 {
        match self {
            Self::Platform(platform) => platform.measure_text(s, text),
            Self::Recording(_) => {
                let mut canvas = PlatformOffscreenCanvas::new(1, 1, 4);
                let mut platform = canvas.begin_draw(1.0);
                let width = platform.measure_text(s, text);
                drop(platform);
                canvas.end_draw();
                width
            }
        }
    }
}
