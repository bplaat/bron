/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::CStr;
use std::rc::Rc;
use std::sync::OnceLock;

use bwindow::ffi::*;

use crate::path::{Command, Path, Transform};
use crate::{CanvasState, Color, FontWeight, LineCap, LineJoin, TextAlign, TextBaseline};

// MARK: JNI classes
// Framework classes and methods, looked up once and valid on every thread.
pub(super) struct Jni {
    canvas_class: GlobalRef,
    pub canvas_new: jmethodID,
    save: jmethodID,
    restore: jmethodID,
    pub restore_to_count: jmethodID,
    set_matrix: jmethodID,
    clip_rect: jmethodID,
    clip_path: jmethodID,
    pub draw_color: jmethodID,
    draw_rect: jmethodID,
    draw_path: jmethodID,
    draw_text: jmethodID,
    paint_class: GlobalRef,
    paint_new: jmethodID,
    set_color: jmethodID,
    set_style: jmethodID,
    set_stroke_width: jmethodID,
    set_stroke_cap: jmethodID,
    set_stroke_join: jmethodID,
    set_stroke_miter: jmethodID,
    set_text_size: jmethodID,
    set_typeface: jmethodID,
    set_text_align: jmethodID,
    measure_text: jmethodID,
    ascent: jmethodID,
    descent: jmethodID,
    path_class: GlobalRef,
    path_new: jmethodID,
    rewind: jmethodID,
    move_to: jmethodID,
    line_to: jmethodID,
    quad_to: jmethodID,
    cubic_to: jmethodID,
    close: jmethodID,
    matrix_class: GlobalRef,
    matrix_new: jmethodID,
    set_values: jmethodID,
    typeface_class: GlobalRef,
    typeface_create: jmethodID,
    pub lock_hardware_canvas: jmethodID,
    pub unlock_canvas_and_post: jmethodID,
    pub mode_src: GlobalRef,
    pub mode_clear: GlobalRef,
    style_stroke: GlobalRef,
    caps: [GlobalRef; 3],
    joins: [GlobalRef; 3],
    aligns: [GlobalRef; 3],
}

// SAFETY: Global references and method IDs are valid on every attached thread.
unsafe impl Send for Jni {}
// SAFETY: See above, the values are immutable.
unsafe impl Sync for Jni {}

impl Jni {
    pub(super) fn get(env: JniEnv) -> &'static Self {
        static JNI: OnceLock<Jni> = OnceLock::new();
        JNI.get_or_init(|| unsafe { Self::new(env) })
    }

    unsafe fn new(env: JniEnv) -> Self {
        unsafe {
            let class = |name: &CStr| GlobalRef::from_local(env, env.find_class(name));
            let canvas = class(c"android/graphics/Canvas");
            let paint = class(c"android/graphics/Paint");
            let path = class(c"android/graphics/Path");
            let matrix = class(c"android/graphics/Matrix");
            let typeface = class(c"android/graphics/Typeface");
            let surface = class(c"android/view/Surface");
            let method = |class: &GlobalRef, name: &CStr, signature: &CStr| {
                env.method(class.as_raw(), name, signature)
            };
            let constants = |class: &CStr, signature: &CStr, names: &[&CStr]| {
                let class = env.find_class(class);
                let values = names
                    .iter()
                    .map(|name| {
                        GlobalRef::from_local(env, env.static_object_field(class, name, signature))
                    })
                    .collect::<Vec<_>>();
                env.delete_local_ref(class);
                values
            };
            let [mode_src, mode_clear]: [GlobalRef; 2] = constants(
                c"android/graphics/PorterDuff$Mode",
                c"Landroid/graphics/PorterDuff$Mode;",
                &[c"SRC", c"CLEAR"],
            )
            .try_into()
            .unwrap_or_else(|_| unreachable!());
            let [style_stroke]: [GlobalRef; 1] = constants(
                c"android/graphics/Paint$Style",
                c"Landroid/graphics/Paint$Style;",
                &[c"STROKE"],
            )
            .try_into()
            .unwrap_or_else(|_| unreachable!());
            let triple = |class: &CStr, signature: &CStr, names: [&CStr; 3]| -> [GlobalRef; 3] {
                constants(class, signature, &names)
                    .try_into()
                    .unwrap_or_else(|_| unreachable!())
            };
            Self {
                canvas_new: method(&canvas, c"<init>", c"(Landroid/graphics/Bitmap;)V"),
                save: method(&canvas, c"save", c"()I"),
                restore: method(&canvas, c"restore", c"()V"),
                restore_to_count: method(&canvas, c"restoreToCount", c"(I)V"),
                set_matrix: method(&canvas, c"setMatrix", c"(Landroid/graphics/Matrix;)V"),
                clip_rect: method(&canvas, c"clipRect", c"(FFFF)Z"),
                clip_path: method(&canvas, c"clipPath", c"(Landroid/graphics/Path;)Z"),
                draw_color: method(
                    &canvas,
                    c"drawColor",
                    c"(ILandroid/graphics/PorterDuff$Mode;)V",
                ),
                draw_rect: method(&canvas, c"drawRect", c"(FFFFLandroid/graphics/Paint;)V"),
                draw_path: method(
                    &canvas,
                    c"drawPath",
                    c"(Landroid/graphics/Path;Landroid/graphics/Paint;)V",
                ),
                draw_text: method(
                    &canvas,
                    c"drawText",
                    c"(Ljava/lang/String;FFLandroid/graphics/Paint;)V",
                ),
                paint_new: method(&paint, c"<init>", c"(I)V"),
                set_color: method(&paint, c"setColor", c"(I)V"),
                set_style: method(&paint, c"setStyle", c"(Landroid/graphics/Paint$Style;)V"),
                set_stroke_width: method(&paint, c"setStrokeWidth", c"(F)V"),
                set_stroke_cap: method(&paint, c"setStrokeCap", c"(Landroid/graphics/Paint$Cap;)V"),
                set_stroke_join: method(
                    &paint,
                    c"setStrokeJoin",
                    c"(Landroid/graphics/Paint$Join;)V",
                ),
                set_stroke_miter: method(&paint, c"setStrokeMiter", c"(F)V"),
                set_text_size: method(&paint, c"setTextSize", c"(F)V"),
                set_typeface: method(
                    &paint,
                    c"setTypeface",
                    c"(Landroid/graphics/Typeface;)Landroid/graphics/Typeface;",
                ),
                set_text_align: method(
                    &paint,
                    c"setTextAlign",
                    c"(Landroid/graphics/Paint$Align;)V",
                ),
                measure_text: method(&paint, c"measureText", c"(Ljava/lang/String;)F"),
                ascent: method(&paint, c"ascent", c"()F"),
                descent: method(&paint, c"descent", c"()F"),
                path_new: method(&path, c"<init>", c"()V"),
                rewind: method(&path, c"rewind", c"()V"),
                move_to: method(&path, c"moveTo", c"(FF)V"),
                line_to: method(&path, c"lineTo", c"(FF)V"),
                quad_to: method(&path, c"quadTo", c"(FFFF)V"),
                cubic_to: method(&path, c"cubicTo", c"(FFFFFF)V"),
                close: method(&path, c"close", c"()V"),
                matrix_new: method(&matrix, c"<init>", c"()V"),
                set_values: method(&matrix, c"setValues", c"([F)V"),
                lock_hardware_canvas: method(
                    &surface,
                    c"lockHardwareCanvas",
                    c"()Landroid/graphics/Canvas;",
                ),
                unlock_canvas_and_post: method(
                    &surface,
                    c"unlockCanvasAndPost",
                    c"(Landroid/graphics/Canvas;)V",
                ),
                typeface_create: env.static_method(
                    typeface.as_raw(),
                    c"create",
                    c"(Ljava/lang/String;I)Landroid/graphics/Typeface;",
                ),
                mode_src,
                mode_clear,
                style_stroke,
                caps: triple(
                    c"android/graphics/Paint$Cap",
                    c"Landroid/graphics/Paint$Cap;",
                    [c"BUTT", c"ROUND", c"SQUARE"],
                ),
                joins: triple(
                    c"android/graphics/Paint$Join",
                    c"Landroid/graphics/Paint$Join;",
                    [c"MITER", c"ROUND", c"BEVEL"],
                ),
                aligns: triple(
                    c"android/graphics/Paint$Align",
                    c"Landroid/graphics/Paint$Align;",
                    [c"LEFT", c"CENTER", c"RIGHT"],
                ),
                canvas_class: canvas,
                paint_class: paint,
                path_class: path,
                matrix_class: matrix,
                typeface_class: typeface,
            }
        }
    }

    pub(super) const fn canvas_class(&self) -> jclass {
        self.canvas_class.as_raw()
    }
}

// MARK: Resources
/// The paints, path, and matrix one canvas draws with, reused for every frame. The last
/// state set on each paint is remembered, so unchanged state costs no JNI calls.
pub(crate) struct Resources {
    fill: GlobalRef,
    stroke: GlobalRef,
    text: GlobalRef,
    fill_color: Cell<Option<u32>>,
    stroke_color: Cell<Option<u32>>,
    text_color: Cell<Option<u32>>,
    // Line width bits, cap, and join.
    stroke_style: Cell<Option<(u32, usize, usize)>>,
    // Typeface reference and font size bits.
    text_font: Cell<Option<(usize, u32)>>,
    text_align: Cell<Option<usize>>,
    path: GlobalRef,
    matrix: GlobalRef,
    values: GlobalRef,
    typefaces: RefCell<HashMap<(String, bool), GlobalRef>>,
}

impl Resources {
    pub(super) fn new(env: JniEnv) -> Rc<Self> {
        const ANTI_ALIAS_FLAG: jint = 1;
        // The HTML canvas default, Android defaults to 4.
        const MITER_LIMIT: f32 = 10.0;
        let jni = Jni::get(env);
        unsafe {
            let paint = || {
                let paint = env.new_object(
                    jni.paint_class.as_raw(),
                    jni.paint_new,
                    &[jvalue { i: ANTI_ALIAS_FLAG }],
                );
                GlobalRef::from_local(env, paint)
            };
            let (fill, stroke, text) = (paint(), paint(), paint());
            env.call_void(
                stroke.as_raw(),
                jni.set_style,
                &[jvalue {
                    l: jni.style_stroke.as_raw(),
                }],
            );
            env.call_void(
                stroke.as_raw(),
                jni.set_stroke_miter,
                &[jvalue { f: MITER_LIMIT }],
            );
            let new = |class: &GlobalRef, constructor| {
                GlobalRef::from_local(env, env.new_object(class.as_raw(), constructor, &[]))
            };
            Rc::new(Self {
                fill,
                stroke,
                text,
                fill_color: Cell::new(None),
                stroke_color: Cell::new(None),
                text_color: Cell::new(None),
                stroke_style: Cell::new(None),
                text_font: Cell::new(None),
                text_align: Cell::new(None),
                path: new(&jni.path_class, jni.path_new),
                matrix: new(&jni.matrix_class, jni.matrix_new),
                values: GlobalRef::from_local(env, env.new_float_array(9)),
                typefaces: RefCell::new(HashMap::new()),
            })
        }
    }

    fn typeface(&self, env: JniEnv, jni: &Jni, family: &str, bold: bool) -> jobject {
        const BOLD: jint = 1;
        let mut typefaces = self.typefaces.borrow_mut();
        typefaces
            .entry((family.to_string(), bold))
            .or_insert_with(|| unsafe {
                let name = env.new_string(family);
                let typeface = env.call_static_object(
                    jni.typeface_class.as_raw(),
                    jni.typeface_create,
                    &[
                        jvalue { l: name },
                        jvalue {
                            i: if bold { BOLD } else { 0 },
                        },
                    ],
                );
                env.delete_local_ref(name);
                GlobalRef::from_local(env, typeface)
            })
            .as_raw()
    }
}

// MARK: Context
/// What `clear_rect` fills with: an opaque window background, or transparency.
#[derive(Clone, Copy)]
pub(super) enum Clear {
    Color(u32),
    Transparent,
}

/// Draws directly into an `android.graphics.Canvas`, in logical pixels mapped by `base`.
pub(crate) struct PlatformCanvasContext {
    env: JniEnv,
    jni: &'static Jni,
    canvas: jobject,
    resources: Rc<Resources>,
    base: Transform,
    clear: Clear,
    count: jint,
    path: Path,
    transform: Transform,
    // Saved transforms with the canvas transform at that save.
    stack: Vec<(Transform, Transform)>,
    // The transform the canvas currently uses on top of the base.
    synced: Transform,
}

impl Drop for PlatformCanvasContext {
    fn drop(&mut self) {
        unsafe {
            self.env.call_void(
                self.canvas,
                self.jni.restore_to_count,
                &[jvalue { i: self.count }],
            );
        }
    }
}

// Remember a paint state, returns whether it differs from the previous one.
fn changed<T: Copy + PartialEq>(cache: &Cell<Option<T>>, value: T) -> bool {
    cache.replace(Some(value)) != Some(value)
}

impl PlatformCanvasContext {
    /// Draw into `canvas`, which must stay valid until the context is dropped.
    pub(super) fn new(
        canvas: jobject,
        resources: Rc<Resources>,
        base: Transform,
        clear: Clear,
    ) -> Self {
        let env = JniEnv::current();
        let jni = Jni::get(env);
        let count = unsafe { env.call_int(canvas, jni.save, &[]) };
        let mut context = Self {
            env,
            jni,
            canvas,
            resources,
            base,
            clear,
            count,
            path: Path::default(),
            transform: Transform::IDENTITY,
            stack: Vec::new(),
            synced: Transform::IDENTITY,
        };
        context.set_matrix(Transform::IDENTITY);
        context
    }

    fn set_matrix(&mut self, transform: Transform) {
        let Transform([a, b, c, d, e, f]) = self.base.concat(transform);
        unsafe {
            let values = self.resources.values.as_raw();
            self.env
                .set_float_array(values, &[a, c, e, b, d, f, 0.0, 0.0, 1.0]);
            let matrix = self.resources.matrix.as_raw();
            self.env
                .call_void(matrix, self.jni.set_values, &[jvalue { l: values }]);
            self.env
                .call_void(self.canvas, self.jni.set_matrix, &[jvalue { l: matrix }]);
        }
        self.synced = transform;
    }

    fn sync_transform(&mut self) {
        if self.synced != self.transform {
            self.set_matrix(self.transform);
        }
    }

    fn set_color(&self, paint: &GlobalRef, cache: &Cell<Option<u32>>, color: Color, alpha: f32) {
        let (red, green, blue, alpha) = color.components(alpha);
        let [red, green, blue, alpha] =
            [red, green, blue, alpha].map(|c| (c * 255.0).round() as u32);
        let argb = (alpha << 24) | (red << 16) | (green << 8) | blue;
        if !changed(cache, argb) {
            return;
        }
        unsafe {
            self.env.call_void(
                paint.as_raw(),
                self.jni.set_color,
                &[jvalue { i: argb as jint }],
            );
        }
    }

    fn stroke_paint(&self, state: &CanvasState) -> jobject {
        let resources = &self.resources;
        let paint = resources.stroke.as_raw();
        self.set_color(
            &resources.stroke,
            &resources.stroke_color,
            state.stroke,
            state.alpha,
        );
        let cap = match state.line_cap {
            LineCap::Butt => 0,
            LineCap::Round => 1,
            LineCap::Square => 2,
        };
        let join = match state.line_join {
            LineJoin::Miter => 0,
            LineJoin::Round => 1,
            LineJoin::Bevel => 2,
        };
        if changed(
            &resources.stroke_style,
            (state.line_width.to_bits(), cap, join),
        ) {
            let (env, jni) = (self.env, self.jni);
            unsafe {
                env.call_void(
                    paint,
                    jni.set_stroke_width,
                    &[jvalue {
                        f: state.line_width,
                    }],
                );
                env.call_void(
                    paint,
                    jni.set_stroke_cap,
                    &[jvalue {
                        l: jni.caps[cap].as_raw(),
                    }],
                );
                env.call_void(
                    paint,
                    jni.set_stroke_join,
                    &[jvalue {
                        l: jni.joins[join].as_raw(),
                    }],
                );
            }
        }
        paint
    }

    fn text_paint(&self, state: &CanvasState) -> jobject {
        let resources = &self.resources;
        let text = resources.text.as_raw();
        let typeface = resources.typeface(
            self.env,
            self.jni,
            &state.font_family,
            state.font_weight == FontWeight::Bold,
        );
        if changed(
            &resources.text_font,
            (typeface as usize, state.font_size.to_bits()),
        ) {
            unsafe {
                self.env
                    .call_object(text, self.jni.set_typeface, &[jvalue { l: typeface }]);
                self.env.call_void(
                    text,
                    self.jni.set_text_size,
                    &[jvalue { f: state.font_size }],
                );
            }
        }
        text
    }

    // Paths capture their construction transform, map them back into the current user space
    // so the canvas draws them, and scales strokes, with the current transform.
    fn build_path(&mut self) -> Option<jobject> {
        let inverse = self.transform.inverse()?;
        self.sync_transform();
        let (env, jni) = (self.env, self.jni);
        let path = self.resources.path.as_raw();
        let point = |method, points: &[[f32; 2]]| unsafe {
            let mut args = [jvalue { f: 0.0 }; 6];
            for (pair, &point) in args.as_chunks_mut::<2>().0.iter_mut().zip(points) {
                *pair = inverse.point(point).map(|f| jvalue { f });
            }
            env.call_void(path, method, &args[..points.len() * 2]);
        };
        unsafe { env.call_void(path, jni.rewind, &[]) };
        for command in &self.path.commands {
            match *command {
                Command::Move(to) => point(jni.move_to, &[to]),
                Command::Line(to) => point(jni.line_to, &[to]),
                Command::Quadratic(control, to) => point(jni.quad_to, &[control, to]),
                Command::Cubic(a, b, to) => point(jni.cubic_to, &[a, b, to]),
                Command::Close => unsafe { env.call_void(path, jni.close, &[]) },
            }
        }
        Some(path)
    }

    fn draw_rect(&mut self, paint: jobject, x: f32, y: f32, w: f32, h: f32) {
        self.sync_transform();
        unsafe {
            self.env.call_void(
                self.canvas,
                self.jni.draw_rect,
                &[
                    jvalue { f: x },
                    jvalue { f: y },
                    jvalue { f: x + w },
                    jvalue { f: y + h },
                    jvalue { l: paint },
                ],
            );
        }
    }

    pub(crate) fn save(&mut self) {
        self.stack.push((self.transform, self.synced));
        unsafe { self.env.call_int(self.canvas, self.jni.save, &[]) };
    }
    pub(crate) fn restore(&mut self) {
        if let Some((transform, synced)) = self.stack.pop() {
            self.transform = transform;
            self.synced = synced;
            unsafe { self.env.call_void(self.canvas, self.jni.restore, &[]) };
        }
    }
    pub(crate) fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.sync_transform();
        let (color, mode) = match self.clear {
            Clear::Color(color) => (color | 0xff00_0000, &self.jni.mode_src),
            Clear::Transparent => (0, &self.jni.mode_clear),
        };
        let (env, jni, canvas) = (self.env, self.jni, self.canvas);
        unsafe {
            env.call_int(canvas, jni.save, &[]);
            env.call_bool(
                canvas,
                jni.clip_rect,
                &[
                    jvalue { f: x },
                    jvalue { f: y },
                    jvalue { f: x + w },
                    jvalue { f: y + h },
                ],
            );
            env.call_void(
                canvas,
                jni.draw_color,
                &[jvalue { i: color as jint }, jvalue { l: mode.as_raw() }],
            );
            env.call_void(canvas, jni.restore, &[]);
        }
    }
    pub(crate) fn fill_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        let resources = &self.resources;
        self.set_color(&resources.fill, &resources.fill_color, s.fill, s.alpha);
        self.draw_rect(self.resources.fill.as_raw(), x, y, w, h);
    }
    pub(crate) fn stroke_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        let paint = self.stroke_paint(s);
        self.draw_rect(paint, x, y, w, h);
    }
    pub(crate) fn begin_path(&mut self) {
        self.path = Path::default();
    }
    pub(crate) fn close_path(&mut self) {
        self.path.close();
    }
    pub(crate) fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.transform, [x, y]);
    }
    pub(crate) fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.transform, [x, y]);
    }
    pub(crate) fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.path.rect(self.transform, x, y, w, h);
    }
    pub(crate) fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        self.path.round_rect(self.transform, x, y, w, h, r);
    }
    pub(crate) fn ellipse(&mut self, x: f32, y: f32, rx: f32, ry: f32) {
        self.path.ellipse(self.transform, [x, y], [rx, ry]);
    }
    pub(crate) fn arc(&mut self, x: f32, y: f32, r: f32, start: f32, end: f32, ccw: bool) {
        self.path
            .arc(self.transform, [x, y], [r, r], start, end, ccw);
    }
    pub(crate) fn quadratic_curve_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
        self.path.quadratic(self.transform, [a, b], [x, y]);
    }
    pub(crate) fn bezier_curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.path.cubic(self.transform, [a, b], [c, d], [x, y]);
    }
    pub(crate) fn fill(&mut self, s: &CanvasState) {
        if let Some(path) = self.build_path() {
            let paint = &self.resources.fill;
            self.set_color(paint, &self.resources.fill_color, s.fill, s.alpha);
            unsafe {
                self.env.call_void(
                    self.canvas,
                    self.jni.draw_path,
                    &[jvalue { l: path }, jvalue { l: paint.as_raw() }],
                );
            }
        }
    }
    pub(crate) fn stroke(&mut self, s: &CanvasState) {
        if let Some(path) = self.build_path() {
            let paint = self.stroke_paint(s);
            unsafe {
                self.env.call_void(
                    self.canvas,
                    self.jni.draw_path,
                    &[jvalue { l: path }, jvalue { l: paint }],
                );
            }
        }
    }
    pub(crate) fn clip(&mut self) {
        unsafe {
            match self.build_path() {
                Some(path) => {
                    self.env
                        .call_bool(self.canvas, self.jni.clip_path, &[jvalue { l: path }]);
                }
                // A path under a non-invertible transform is empty, so nothing stays visible.
                None => {
                    let empty = [jvalue { f: 0.0 }; 4];
                    self.env.call_bool(self.canvas, self.jni.clip_rect, &empty);
                }
            }
        }
    }
    pub(crate) fn translate(&mut self, x: f32, y: f32) {
        self.transform = self.transform.concat(Transform([1.0, 0.0, 0.0, 1.0, x, y]));
    }
    pub(crate) fn rotate(&mut self, angle: f32) {
        let (s, c) = angle.sin_cos();
        self.transform = self.transform.concat(Transform([c, s, -s, c, 0.0, 0.0]));
    }
    pub(crate) fn scale(&mut self, x: f32, y: f32) {
        self.transform = self.transform.concat(Transform([x, 0.0, 0.0, y, 0.0, 0.0]));
    }
    #[allow(
        clippy::missing_const_for_fn,
        reason = "keeps the shared context API non-const"
    )]
    pub(crate) fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.transform = Transform([a, b, c, d, e, f]);
    }
    #[allow(
        clippy::missing_const_for_fn,
        reason = "keeps the shared context API non-const"
    )]
    pub(crate) fn reset_transform(&mut self) {
        self.transform = Transform::IDENTITY;
    }
    pub(crate) fn fill_text(&mut self, s: &CanvasState, text: &str, x: f32, y: f32) {
        self.sync_transform();
        let paint = self.text_paint(s);
        self.set_color(
            &self.resources.text,
            &self.resources.text_color,
            s.fill,
            s.alpha,
        );
        let align = match s.text_align {
            TextAlign::Start | TextAlign::Left => 0,
            TextAlign::Center => 1,
            TextAlign::End | TextAlign::Right => 2,
        };
        let (env, jni) = (self.env, self.jni);
        unsafe {
            if changed(&self.resources.text_align, align) {
                env.call_void(
                    paint,
                    jni.set_text_align,
                    &[jvalue {
                        l: jni.aligns[align].as_raw(),
                    }],
                );
            }
            // Offset from the requested baseline to the alphabetic baseline Android draws at.
            let ascent = env.call_float(paint, jni.ascent, &[]);
            let descent = env.call_float(paint, jni.descent, &[]);
            let offset = match s.text_baseline {
                TextBaseline::Top => -ascent,
                TextBaseline::Hanging => -ascent * 0.8,
                TextBaseline::Middle => -(ascent + descent) / 2.0,
                TextBaseline::Alphabetic => 0.0,
                TextBaseline::Ideographic | TextBaseline::Bottom => -descent,
            };
            let text = env.new_string(text);
            env.call_void(
                self.canvas,
                jni.draw_text,
                &[
                    jvalue { l: text },
                    jvalue { f: x },
                    jvalue { f: y + offset },
                    jvalue { l: paint },
                ],
            );
            env.delete_local_ref(text);
        }
    }
    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> f32 {
        let paint = self.text_paint(s);
        unsafe {
            let text = self.env.new_string(text);
            let width = self
                .env
                .call_float(paint, self.jni.measure_text, &[jvalue { l: text }]);
            self.env.delete_local_ref(text);
            width
        }
    }
}
