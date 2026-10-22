/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use jni::errors::Result;
use jni::objects::{Global, JClass, JFloatArray, JMethodID, JObject, JStaticMethodID};
use jni::signature::{FieldSignature, Primitive, ReturnType};
use jni::strings::JNIStr;
use jni::sys::{jint, jvalue};
use jni::vm::JavaVM;
use jni::{Env, JValue, jni_sig, jni_str};

use crate::path::{Command, Path, Transform};
use crate::{CanvasState, Color, FontWeight, LineCap, LineJoin, TextAlign, TextBaseline};

/// Run `f` with the JNI environment of the current thread in a new local reference frame.
/// Java exceptions and JNI errors panic.
pub(super) fn with_env<T>(f: impl FnOnce(&mut Env) -> Result<T>) -> T {
    JavaVM::singleton()
        .and_then(|vm| vm.attach_current_thread(f))
        .expect("JNI call failed")
}

// A global object reference.
pub(super) type GlobalObject = Global<JObject<'static>>;

const VOID: ReturnType = ReturnType::Primitive(Primitive::Void);
const BOOLEAN: ReturnType = ReturnType::Primitive(Primitive::Boolean);
const INT: ReturnType = ReturnType::Primitive(Primitive::Int);
const FLOAT: ReturnType = ReturnType::Primitive(Primitive::Float);
pub(super) const OBJECT: ReturnType = ReturnType::Object;

/// Call a cached void method.
///
/// # Safety
/// `method` must be a void method of `object` that takes `args`.
pub(super) unsafe fn call_void(
    env: &mut Env,
    object: &JObject,
    method: JMethodID,
    args: &[jvalue],
) -> Result<()> {
    unsafe { env.call_method_unchecked(object, method, VOID, args) }?.v()
}

// MARK: JNI classes
// Framework classes and methods, looked up once and valid on every thread.
pub(super) struct Jni {
    pub canvas_class: Global<JClass<'static>>,
    pub canvas_new: JMethodID,
    save: JMethodID,
    restore: JMethodID,
    restore_to_count: JMethodID,
    set_matrix: JMethodID,
    clip_rect: JMethodID,
    clip_path: JMethodID,
    pub draw_color: JMethodID,
    draw_rect: JMethodID,
    draw_path: JMethodID,
    draw_text: JMethodID,
    paint_class: Global<JClass<'static>>,
    paint_new: JMethodID,
    set_color: JMethodID,
    set_style: JMethodID,
    set_stroke_width: JMethodID,
    set_stroke_cap: JMethodID,
    set_stroke_join: JMethodID,
    set_stroke_miter: JMethodID,
    set_text_size: JMethodID,
    set_typeface: JMethodID,
    set_text_align: JMethodID,
    measure_text: JMethodID,
    ascent: JMethodID,
    descent: JMethodID,
    path_class: Global<JClass<'static>>,
    path_new: JMethodID,
    rewind: JMethodID,
    move_to: JMethodID,
    line_to: JMethodID,
    quad_to: JMethodID,
    cubic_to: JMethodID,
    close: JMethodID,
    matrix_class: Global<JClass<'static>>,
    matrix_new: JMethodID,
    set_values: JMethodID,
    typeface_class: Global<JClass<'static>>,
    typeface_create: JStaticMethodID,
    #[cfg(feature = "window")]
    pub lock_hardware_canvas: JMethodID,
    #[cfg(feature = "window")]
    pub unlock_canvas_and_post: JMethodID,
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub mode_src: GlobalObject,
    mode_clear: GlobalObject,
    style_stroke: GlobalObject,
    caps: [GlobalObject; 3],
    joins: [GlobalObject; 3],
    aligns: [GlobalObject; 3],
}

fn class(env: &mut Env, name: &JNIStr) -> Result<Global<JClass<'static>>> {
    let class = env.find_class(name)?;
    env.new_global_ref(&class)
}

// Read the enum constants `names` of `class`.
fn constants<const N: usize>(
    env: &mut Env,
    class: &JNIStr,
    sig: impl AsRef<FieldSignature<'static>>,
    names: [&JNIStr; N],
) -> Result<[GlobalObject; N]> {
    let class = env.find_class(class)?;
    let mut values = Vec::with_capacity(N);
    for name in names {
        let value = env.get_static_field(&class, name, &sig)?.l()?;
        values.push(env.new_global_ref(&value)?);
    }
    Ok(values.try_into().unwrap_or_else(|_| unreachable!()))
}

impl Jni {
    pub(super) fn get(env: &mut Env) -> Result<&'static Self> {
        static JNI: OnceLock<Jni> = OnceLock::new();
        if let Some(jni) = JNI.get() {
            return Ok(jni);
        }
        let jni = Self::new(env)?;
        Ok(JNI.get_or_init(|| jni))
    }

    fn new(env: &mut Env) -> Result<Self> {
        let canvas = class(env, jni_str!("android/graphics/Canvas"))?;
        let paint = class(env, jni_str!("android/graphics/Paint"))?;
        let path = class(env, jni_str!("android/graphics/Path"))?;
        let matrix = class(env, jni_str!("android/graphics/Matrix"))?;
        let typeface = class(env, jni_str!("android/graphics/Typeface"))?;
        #[cfg(feature = "window")]
        let surface = class(env, jni_str!("android/view/Surface"))?;
        let [mode_src, mode_clear] = constants(
            env,
            jni_str!("android/graphics/PorterDuff$Mode"),
            jni_sig!("Landroid/graphics/PorterDuff$Mode;"),
            [jni_str!("SRC"), jni_str!("CLEAR")],
        )?;
        let [style_stroke] = constants(
            env,
            jni_str!("android/graphics/Paint$Style"),
            jni_sig!("Landroid/graphics/Paint$Style;"),
            [jni_str!("STROKE")],
        )?;
        Ok(Self {
            canvas_new: env.get_method_id(
                &canvas,
                jni_str!("<init>"),
                jni_sig!("(Landroid/graphics/Bitmap;)V"),
            )?,
            save: env.get_method_id(&canvas, jni_str!("save"), jni_sig!("()I"))?,
            restore: env.get_method_id(&canvas, jni_str!("restore"), jni_sig!("()V"))?,
            restore_to_count: env.get_method_id(
                &canvas,
                jni_str!("restoreToCount"),
                jni_sig!("(I)V"),
            )?,
            set_matrix: env.get_method_id(
                &canvas,
                jni_str!("setMatrix"),
                jni_sig!("(Landroid/graphics/Matrix;)V"),
            )?,
            clip_rect: env.get_method_id(&canvas, jni_str!("clipRect"), jni_sig!("(FFFF)Z"))?,
            clip_path: env.get_method_id(
                &canvas,
                jni_str!("clipPath"),
                jni_sig!("(Landroid/graphics/Path;)Z"),
            )?,
            draw_color: env.get_method_id(
                &canvas,
                jni_str!("drawColor"),
                jni_sig!("(ILandroid/graphics/PorterDuff$Mode;)V"),
            )?,
            draw_rect: env.get_method_id(
                &canvas,
                jni_str!("drawRect"),
                jni_sig!("(FFFFLandroid/graphics/Paint;)V"),
            )?,
            draw_path: env.get_method_id(
                &canvas,
                jni_str!("drawPath"),
                jni_sig!("(Landroid/graphics/Path;Landroid/graphics/Paint;)V"),
            )?,
            draw_text: env.get_method_id(
                &canvas,
                jni_str!("drawText"),
                jni_sig!("(Ljava/lang/String;FFLandroid/graphics/Paint;)V"),
            )?,
            paint_new: env.get_method_id(&paint, jni_str!("<init>"), jni_sig!("(I)V"))?,
            set_color: env.get_method_id(&paint, jni_str!("setColor"), jni_sig!("(I)V"))?,
            set_style: env.get_method_id(
                &paint,
                jni_str!("setStyle"),
                jni_sig!("(Landroid/graphics/Paint$Style;)V"),
            )?,
            set_stroke_width: env.get_method_id(
                &paint,
                jni_str!("setStrokeWidth"),
                jni_sig!("(F)V"),
            )?,
            set_stroke_cap: env.get_method_id(
                &paint,
                jni_str!("setStrokeCap"),
                jni_sig!("(Landroid/graphics/Paint$Cap;)V"),
            )?,
            set_stroke_join: env.get_method_id(
                &paint,
                jni_str!("setStrokeJoin"),
                jni_sig!("(Landroid/graphics/Paint$Join;)V"),
            )?,
            set_stroke_miter: env.get_method_id(
                &paint,
                jni_str!("setStrokeMiter"),
                jni_sig!("(F)V"),
            )?,
            set_text_size: env.get_method_id(&paint, jni_str!("setTextSize"), jni_sig!("(F)V"))?,
            set_typeface: env.get_method_id(
                &paint,
                jni_str!("setTypeface"),
                jni_sig!("(Landroid/graphics/Typeface;)Landroid/graphics/Typeface;"),
            )?,
            set_text_align: env.get_method_id(
                &paint,
                jni_str!("setTextAlign"),
                jni_sig!("(Landroid/graphics/Paint$Align;)V"),
            )?,
            measure_text: env.get_method_id(
                &paint,
                jni_str!("measureText"),
                jni_sig!("(Ljava/lang/String;)F"),
            )?,
            ascent: env.get_method_id(&paint, jni_str!("ascent"), jni_sig!("()F"))?,
            descent: env.get_method_id(&paint, jni_str!("descent"), jni_sig!("()F"))?,
            path_new: env.get_method_id(&path, jni_str!("<init>"), jni_sig!("()V"))?,
            rewind: env.get_method_id(&path, jni_str!("rewind"), jni_sig!("()V"))?,
            move_to: env.get_method_id(&path, jni_str!("moveTo"), jni_sig!("(FF)V"))?,
            line_to: env.get_method_id(&path, jni_str!("lineTo"), jni_sig!("(FF)V"))?,
            quad_to: env.get_method_id(&path, jni_str!("quadTo"), jni_sig!("(FFFF)V"))?,
            cubic_to: env.get_method_id(&path, jni_str!("cubicTo"), jni_sig!("(FFFFFF)V"))?,
            close: env.get_method_id(&path, jni_str!("close"), jni_sig!("()V"))?,
            matrix_new: env.get_method_id(&matrix, jni_str!("<init>"), jni_sig!("()V"))?,
            set_values: env.get_method_id(&matrix, jni_str!("setValues"), jni_sig!("([F)V"))?,
            typeface_create: env.get_static_method_id(
                &typeface,
                jni_str!("create"),
                jni_sig!("(Ljava/lang/String;I)Landroid/graphics/Typeface;"),
            )?,
            #[cfg(feature = "window")]
            lock_hardware_canvas: env.get_method_id(
                &surface,
                jni_str!("lockHardwareCanvas"),
                jni_sig!("()Landroid/graphics/Canvas;"),
            )?,
            #[cfg(feature = "window")]
            unlock_canvas_and_post: env.get_method_id(
                &surface,
                jni_str!("unlockCanvasAndPost"),
                jni_sig!("(Landroid/graphics/Canvas;)V"),
            )?,
            mode_src,
            mode_clear,
            style_stroke,
            caps: constants(
                env,
                jni_str!("android/graphics/Paint$Cap"),
                jni_sig!("Landroid/graphics/Paint$Cap;"),
                [jni_str!("BUTT"), jni_str!("ROUND"), jni_str!("SQUARE")],
            )?,
            joins: constants(
                env,
                jni_str!("android/graphics/Paint$Join"),
                jni_sig!("Landroid/graphics/Paint$Join;"),
                [jni_str!("MITER"), jni_str!("ROUND"), jni_str!("BEVEL")],
            )?,
            aligns: constants(
                env,
                jni_str!("android/graphics/Paint$Align"),
                jni_sig!("Landroid/graphics/Paint$Align;"),
                [jni_str!("LEFT"), jni_str!("CENTER"), jni_str!("RIGHT")],
            )?,
            canvas_class: canvas,
            paint_class: paint,
            path_class: path,
            matrix_class: matrix,
            typeface_class: typeface,
        })
    }
}

// MARK: Resources
/// The paints, path, and matrix one canvas draws with, reused for every frame. The last
/// state set on each paint is remembered, so unchanged state costs no JNI calls.
pub(crate) struct Resources {
    fill: GlobalObject,
    stroke: GlobalObject,
    text: GlobalObject,
    fill_color: Cell<Option<u32>>,
    stroke_color: Cell<Option<u32>>,
    text_color: Cell<Option<u32>>,
    // Line width bits, cap, and join.
    stroke_style: Cell<Option<(u32, usize, usize)>>,
    // Typeface reference and font size bits.
    text_font: Cell<Option<(usize, u32)>>,
    text_align: Cell<Option<usize>>,
    path: GlobalObject,
    matrix: GlobalObject,
    values: Global<JFloatArray<'static>>,
    typefaces: RefCell<HashMap<(String, bool), Rc<GlobalObject>>>,
}

impl Resources {
    pub(super) fn new(env: &mut Env) -> Result<Rc<Self>> {
        const ANTI_ALIAS_FLAG: jint = 1;
        // The HTML canvas default, Android defaults to 4.
        const MITER_LIMIT: f32 = 10.0;
        let jni = Jni::get(env)?;
        let mut new = |class: &Global<JClass<'static>>, constructor, args: &[jvalue]| {
            let object = unsafe { env.new_object_unchecked(class, constructor, args) }?;
            env.new_global_ref(&object)
        };
        let paint_args = [jvalue { i: ANTI_ALIAS_FLAG }];
        let fill = new(&jni.paint_class, jni.paint_new, &paint_args)?;
        let stroke = new(&jni.paint_class, jni.paint_new, &paint_args)?;
        let text = new(&jni.paint_class, jni.paint_new, &paint_args)?;
        let path = new(&jni.path_class, jni.path_new, &[])?;
        let matrix = new(&jni.matrix_class, jni.matrix_new, &[])?;
        unsafe {
            call_void(
                env,
                &stroke,
                jni.set_style,
                &[jvalue {
                    l: jni.style_stroke.as_raw(),
                }],
            )?;
            call_void(
                env,
                &stroke,
                jni.set_stroke_miter,
                &[jvalue { f: MITER_LIMIT }],
            )?;
        }
        let values = env.new_float_array(9)?;
        Ok(Rc::new(Self {
            fill,
            stroke,
            text,
            fill_color: Cell::new(None),
            stroke_color: Cell::new(None),
            text_color: Cell::new(None),
            stroke_style: Cell::new(None),
            text_font: Cell::new(None),
            text_align: Cell::new(None),
            path,
            matrix,
            values: env.new_global_ref(&values)?,
            typefaces: RefCell::new(HashMap::new()),
        }))
    }

    fn typeface(
        &self,
        env: &mut Env,
        jni: &Jni,
        family: &str,
        bold: bool,
    ) -> Result<Rc<GlobalObject>> {
        const BOLD: jint = 1;
        let key = (family.to_string(), bold);
        if let Some(typeface) = self.typefaces.borrow().get(&key) {
            return Ok(typeface.clone());
        }
        let name = env.new_string(family)?;
        let typeface = unsafe {
            env.call_static_method_unchecked(
                &jni.typeface_class,
                jni.typeface_create,
                OBJECT,
                &[
                    JValue::from(&name).as_jni(),
                    jvalue {
                        i: if bold { BOLD } else { 0 },
                    },
                ],
            )
        }?
        .l()?;
        let typeface = Rc::new(env.new_global_ref(&typeface)?);
        self.typefaces.borrow_mut().insert(key, typeface.clone());
        Ok(typeface)
    }
}

// MARK: Context
/// What `clear_rect` fills with: an opaque window background, or transparency.
#[derive(Clone, Copy)]
pub(super) enum Clear {
    // Only window frames clear to an opaque background.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    Color(u32),
    Transparent,
}

/// Draws directly into an `android.graphics.Canvas`, in logical pixels mapped by `base`.
pub(crate) struct PlatformCanvasContext {
    jni: &'static Jni,
    canvas: GlobalObject,
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
        with_env(|env| unsafe {
            call_void(
                env,
                &self.canvas,
                self.jni.restore_to_count,
                &[jvalue { i: self.count }],
            )
        });
    }
}

// Remember a paint state, returns whether it differs from the previous one.
fn changed<T: Copy + PartialEq>(cache: &Cell<Option<T>>, value: T) -> bool {
    cache.replace(Some(value)) != Some(value)
}

impl PlatformCanvasContext {
    /// Draw into `canvas`, which must stay locked until the context is dropped.
    pub(super) fn new(
        env: &mut Env,
        canvas: &JObject,
        resources: Rc<Resources>,
        base: Transform,
        clear: Clear,
    ) -> Result<Self> {
        let jni = Jni::get(env)?;
        let count = unsafe { env.call_method_unchecked(canvas, jni.save, INT, &[]) }?.i()?;
        let mut context = Self {
            jni,
            canvas: env.new_global_ref(canvas)?,
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
        Ok(context)
    }

    fn set_matrix(&mut self, transform: Transform) {
        let Transform([a, b, c, d, e, f]) = self.base.concat(transform);
        let (jni, resources) = (self.jni, &self.resources);
        with_env(|env| unsafe {
            env.set_float_array_region(&*resources.values, 0, &[a, c, e, b, d, f, 0.0, 0.0, 1.0])?;
            let (values, matrix) = (resources.values.as_raw(), &resources.matrix);
            call_void(env, matrix, jni.set_values, &[jvalue { l: values }])?;
            call_void(
                env,
                &self.canvas,
                jni.set_matrix,
                &[jvalue { l: matrix.as_raw() }],
            )
        });
        self.synced = transform;
    }

    fn sync_transform(&mut self) {
        if self.synced != self.transform {
            self.set_matrix(self.transform);
        }
    }

    fn set_color(&self, paint: &JObject, cache: &Cell<Option<u32>>, color: Color, alpha: f32) {
        let (red, green, blue, alpha) = color.components(alpha);
        let [red, green, blue, alpha] =
            [red, green, blue, alpha].map(|c| (c * 255.0).round() as u32);
        let argb = (alpha << 24) | (red << 16) | (green << 8) | blue;
        if !changed(cache, argb) {
            return;
        }
        with_env(|env| unsafe {
            call_void(
                env,
                paint,
                self.jni.set_color,
                &[jvalue { i: argb as jint }],
            )
        });
    }

    fn stroke_paint(&self, state: &CanvasState) -> &JObject<'static> {
        let resources = &self.resources;
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
            let (jni, paint) = (self.jni, &resources.stroke);
            with_env(|env| unsafe {
                call_void(
                    env,
                    paint,
                    jni.set_stroke_width,
                    &[jvalue {
                        f: state.line_width,
                    }],
                )?;
                call_void(
                    env,
                    paint,
                    jni.set_stroke_cap,
                    &[jvalue {
                        l: jni.caps[cap].as_raw(),
                    }],
                )?;
                call_void(
                    env,
                    paint,
                    jni.set_stroke_join,
                    &[jvalue {
                        l: jni.joins[join].as_raw(),
                    }],
                )
            });
        }
        &resources.stroke
    }

    fn text_paint(&self, state: &CanvasState) -> &JObject<'static> {
        let (jni, resources) = (self.jni, &self.resources);
        let text = &resources.text;
        with_env(|env| {
            let typeface = resources.typeface(
                env,
                jni,
                &state.font_family,
                state.font_weight == FontWeight::Bold,
            )?;
            if changed(
                &resources.text_font,
                (typeface.as_raw() as usize, state.font_size.to_bits()),
            ) {
                unsafe {
                    env.call_method_unchecked(
                        text,
                        jni.set_typeface,
                        OBJECT,
                        &[jvalue {
                            l: typeface.as_raw(),
                        }],
                    )?;
                    call_void(
                        env,
                        text,
                        jni.set_text_size,
                        &[jvalue { f: state.font_size }],
                    )?;
                }
            }
            Ok(())
        });
        text
    }

    // Paths capture their construction transform, map them back into the current user space
    // so the canvas draws them, and scales strokes, with the current transform. Returns
    // whether the path was built.
    fn build_path(&mut self) -> bool {
        let Some(inverse) = self.transform.inverse() else {
            return false;
        };
        self.sync_transform();
        let (jni, path) = (self.jni, &self.resources.path);
        with_env(|env| unsafe {
            let mut point = |method, points: &[[f32; 2]]| {
                let mut args = [jvalue { f: 0.0 }; 6];
                for (pair, &point) in args.as_chunks_mut::<2>().0.iter_mut().zip(points) {
                    *pair = inverse.point(point).map(|f| jvalue { f });
                }
                call_void(env, path, method, &args[..points.len() * 2])
            };
            point(jni.rewind, &[])?;
            for command in &self.path.commands {
                match *command {
                    Command::Move(to) => point(jni.move_to, &[to])?,
                    Command::Line(to) => point(jni.line_to, &[to])?,
                    Command::Quadratic(control, to) => point(jni.quad_to, &[control, to])?,
                    Command::Cubic(a, b, to) => point(jni.cubic_to, &[a, b, to])?,
                    Command::Close => point(jni.close, &[])?,
                }
            }
            Ok(())
        });
        true
    }

    fn draw_rect(&mut self, paint: &JObject, x: f32, y: f32, w: f32, h: f32) {
        self.sync_transform();
        with_env(|env| unsafe {
            call_void(
                env,
                &self.canvas,
                self.jni.draw_rect,
                &[
                    jvalue { f: x },
                    jvalue { f: y },
                    jvalue { f: x + w },
                    jvalue { f: y + h },
                    jvalue { l: paint.as_raw() },
                ],
            )
        });
    }

    // Draw the built path with `paint`.
    fn draw_path(&self, paint: &JObject) {
        let path = self.resources.path.as_raw();
        with_env(|env| unsafe {
            call_void(
                env,
                &self.canvas,
                self.jni.draw_path,
                &[jvalue { l: path }, jvalue { l: paint.as_raw() }],
            )
        });
    }

    pub(crate) fn save(&mut self) {
        self.stack.push((self.transform, self.synced));
        with_env(|env| unsafe {
            env.call_method_unchecked(&self.canvas, self.jni.save, INT, &[])?;
            Ok(())
        });
    }
    pub(crate) fn restore(&mut self) {
        if let Some((transform, synced)) = self.stack.pop() {
            self.transform = transform;
            self.synced = synced;
            with_env(|env| unsafe { call_void(env, &self.canvas, self.jni.restore, &[]) });
        }
    }
    pub(crate) fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.sync_transform();
        let (color, mode) = match self.clear {
            Clear::Color(color) => (color | 0xff00_0000, &self.jni.mode_src),
            Clear::Transparent => (0, &self.jni.mode_clear),
        };
        let (jni, canvas) = (self.jni, &self.canvas);
        with_env(|env| unsafe {
            env.call_method_unchecked(canvas, jni.save, INT, &[])?;
            env.call_method_unchecked(
                canvas,
                jni.clip_rect,
                BOOLEAN,
                &[
                    jvalue { f: x },
                    jvalue { f: y },
                    jvalue { f: x + w },
                    jvalue { f: y + h },
                ],
            )?;
            call_void(
                env,
                canvas,
                jni.draw_color,
                &[jvalue { i: color as jint }, jvalue { l: mode.as_raw() }],
            )?;
            call_void(env, canvas, jni.restore, &[])
        });
    }
    pub(crate) fn fill_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        let resources = self.resources.clone();
        self.set_color(&resources.fill, &resources.fill_color, s.fill, s.alpha);
        self.draw_rect(&resources.fill, x, y, w, h);
    }
    pub(crate) fn stroke_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        let resources = self.resources.clone();
        self.stroke_paint(s);
        self.draw_rect(&resources.stroke, x, y, w, h);
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
        if self.build_path() {
            let resources = &self.resources;
            self.set_color(&resources.fill, &resources.fill_color, s.fill, s.alpha);
            self.draw_path(&resources.fill);
        }
    }
    pub(crate) fn stroke(&mut self, s: &CanvasState) {
        if self.build_path() {
            self.draw_path(self.stroke_paint(s));
        }
    }
    pub(crate) fn clip(&mut self) {
        let built = self.build_path();
        let (jni, canvas) = (self.jni, &self.canvas);
        let path = self.resources.path.as_raw();
        with_env(|env| unsafe {
            if built {
                env.call_method_unchecked(canvas, jni.clip_path, BOOLEAN, &[jvalue { l: path }])?;
            } else {
                // A path under a non-invertible transform is empty, so nothing stays visible.
                let empty = [jvalue { f: 0.0 }; 4];
                env.call_method_unchecked(canvas, jni.clip_rect, BOOLEAN, &empty)?;
            }
            Ok(())
        });
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
        self.set_color(paint, &self.resources.text_color, s.fill, s.alpha);
        let align = match s.text_align {
            TextAlign::Start | TextAlign::Left => 0,
            TextAlign::Center => 1,
            TextAlign::End | TextAlign::Right => 2,
        };
        let jni = self.jni;
        with_env(|env| unsafe {
            if changed(&self.resources.text_align, align) {
                call_void(
                    env,
                    paint,
                    jni.set_text_align,
                    &[jvalue {
                        l: jni.aligns[align].as_raw(),
                    }],
                )?;
            }
            // Offset from the requested baseline to the alphabetic baseline Android draws at.
            let ascent = env
                .call_method_unchecked(paint, jni.ascent, FLOAT, &[])?
                .f()?;
            let descent = env
                .call_method_unchecked(paint, jni.descent, FLOAT, &[])?
                .f()?;
            let offset = match s.text_baseline {
                TextBaseline::Top => -ascent,
                TextBaseline::Hanging => -ascent * 0.8,
                TextBaseline::Middle => -(ascent + descent) / 2.0,
                TextBaseline::Alphabetic => 0.0,
                TextBaseline::Ideographic | TextBaseline::Bottom => -descent,
            };
            let text = env.new_string(text)?;
            call_void(
                env,
                &self.canvas,
                jni.draw_text,
                &[
                    jvalue { l: text.as_raw() },
                    jvalue { f: x },
                    jvalue { f: y + offset },
                    jvalue { l: paint.as_raw() },
                ],
            )
        });
    }
    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> f32 {
        let paint = self.text_paint(s);
        with_env(|env| unsafe {
            let text = env.new_string(text)?;
            env.call_method_unchecked(
                paint,
                self.jni.measure_text,
                FLOAT,
                &[jvalue { l: text.as_raw() }],
            )?
            .f()
        })
    }
}
