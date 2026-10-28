/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use jni::errors::Result;
use jni::objects::{Global, JClass, JFloatArray, JMethodID, JObject, JStaticMethodID};
use jni::signature::{FieldSignature, Primitive, ReturnType};
use jni::strings::JNIStr;
use jni::sys::{jint, jvalue};
use jni::vm::JavaVM;
use jni::{Env, JValue, jni_sig, jni_str};

use crate::path::{Command, Path, Transform};
use crate::{
    CanvasState, CanvasStyle, Color, CompositeOperation, FillRule, FontWeight, Gradient,
    GradientShape, GradientSpread, LineCap, LineJoin, TextAlign, TextBaseline,
};

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
    save_layer: JMethodID,
    draw_bitmap: JMethodID,
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
    set_shader: JMethodID,
    set_path_effect: JMethodID,
    set_filter_bitmap: JMethodID,
    // Paint.setBlendMode on Android 10 and later, Paint.setXfermode before.
    set_composite: JMethodID,
    set_composite_returns: ReturnType,
    path_class: Global<JClass<'static>>,
    path_new: JMethodID,
    rewind: JMethodID,
    move_to: JMethodID,
    line_to: JMethodID,
    quad_to: JMethodID,
    cubic_to: JMethodID,
    close: JMethodID,
    set_fill_type: JMethodID,
    matrix_class: Global<JClass<'static>>,
    matrix_new: JMethodID,
    set_values: JMethodID,
    typeface_class: Global<JClass<'static>>,
    typeface_create: JStaticMethodID,
    linear_gradient_class: Global<JClass<'static>>,
    linear_gradient_new: JMethodID,
    radial_gradient_class: Global<JClass<'static>>,
    radial_gradient_new: JMethodID,
    // The two point conical gradient constructor of Android 12 and later.
    focal_gradient_new: Option<JMethodID>,
    set_local_matrix: JMethodID,
    dash_class: Global<JClass<'static>>,
    dash_new: JMethodID,
    rect_class: Global<JClass<'static>>,
    rect_new: JMethodID,
    bitmap_class: Global<JClass<'static>>,
    create_bitmap: JStaticMethodID,
    argb_8888: GlobalObject,
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
    fill_types: [GlobalObject; 2],
    tile_modes: [GlobalObject; 3],
    // Blend modes or Porter-Duff transfer modes in the order of `CompositeOperation`.
    composites: Vec<GlobalObject>,
}

// The blend modes of CompositeOperation, with their Porter-Duff mode before Android 10.
const COMPOSITES: [(&JNIStr, Option<&JNIStr>); 26] = [
    (jni_str!("SRC_OVER"), Some(jni_str!("SRC_OVER"))),
    (jni_str!("SRC_IN"), Some(jni_str!("SRC_IN"))),
    (jni_str!("SRC_OUT"), Some(jni_str!("SRC_OUT"))),
    (jni_str!("SRC_ATOP"), Some(jni_str!("SRC_ATOP"))),
    (jni_str!("DST_OVER"), Some(jni_str!("DST_OVER"))),
    (jni_str!("DST_IN"), Some(jni_str!("DST_IN"))),
    (jni_str!("DST_OUT"), Some(jni_str!("DST_OUT"))),
    (jni_str!("DST_ATOP"), Some(jni_str!("DST_ATOP"))),
    (jni_str!("PLUS"), Some(jni_str!("ADD"))),
    (jni_str!("SRC"), Some(jni_str!("SRC"))),
    (jni_str!("XOR"), Some(jni_str!("XOR"))),
    (jni_str!("MULTIPLY"), Some(jni_str!("MULTIPLY"))),
    (jni_str!("SCREEN"), Some(jni_str!("SCREEN"))),
    (jni_str!("OVERLAY"), Some(jni_str!("OVERLAY"))),
    (jni_str!("DARKEN"), Some(jni_str!("DARKEN"))),
    (jni_str!("LIGHTEN"), Some(jni_str!("LIGHTEN"))),
    (jni_str!("COLOR_DODGE"), None),
    (jni_str!("COLOR_BURN"), None),
    (jni_str!("HARD_LIGHT"), None),
    (jni_str!("SOFT_LIGHT"), None),
    (jni_str!("DIFFERENCE"), None),
    (jni_str!("EXCLUSION"), None),
    (jni_str!("HUE"), None),
    (jni_str!("SATURATION"), None),
    (jni_str!("COLOR"), None),
    (jni_str!("LUMINOSITY"), None),
];

const fn composite_index(operation: CompositeOperation) -> usize {
    use CompositeOperation::*;
    match operation {
        SourceOver => 0,
        SourceIn => 1,
        SourceOut => 2,
        SourceAtop => 3,
        DestinationOver => 4,
        DestinationIn => 5,
        DestinationOut => 6,
        DestinationAtop => 7,
        Lighter => 8,
        Copy => 9,
        Xor => 10,
        Multiply => 11,
        Screen => 12,
        Overlay => 13,
        Darken => 14,
        Lighten => 15,
        ColorDodge => 16,
        ColorBurn => 17,
        HardLight => 18,
        SoftLight => 19,
        Difference => 20,
        Exclusion => 21,
        Hue => 22,
        Saturation => 23,
        Color => 24,
        Luminosity => 25,
    }
}

// The composites of this Android version: blend modes from Android 10, Porter-Duff transfer
// modes before, which lack the separable and non-separable blend modes.
fn composites(env: &mut Env, sdk: jint) -> Result<(JMethodID, ReturnType, Vec<GlobalObject>)> {
    let paint = env.find_class(jni_str!("android/graphics/Paint"))?;
    if sdk >= 29 {
        let class = env.find_class(jni_str!("android/graphics/BlendMode"))?;
        let mut modes = Vec::with_capacity(COMPOSITES.len());
        for (name, _) in COMPOSITES {
            let mode = env
                .get_static_field(&class, name, jni_sig!("Landroid/graphics/BlendMode;"))?
                .l()?;
            modes.push(env.new_global_ref(&mode)?);
        }
        let method = env.get_method_id(
            &paint,
            jni_str!("setBlendMode"),
            jni_sig!("(Landroid/graphics/BlendMode;)V"),
        )?;
        return Ok((method, VOID, modes));
    }
    let class = env.find_class(jni_str!("android/graphics/PorterDuff$Mode"))?;
    let xfermode = env.find_class(jni_str!("android/graphics/PorterDuffXfermode"))?;
    let mut modes = Vec::with_capacity(COMPOSITES.len());
    for (_, name) in COMPOSITES {
        let mode = env
            .get_static_field(
                &class,
                name.unwrap_or(jni_str!("SRC_OVER")),
                jni_sig!("Landroid/graphics/PorterDuff$Mode;"),
            )?
            .l()?;
        let mode = env.new_object(
            &xfermode,
            jni_sig!("(Landroid/graphics/PorterDuff$Mode;)V"),
            &[JValue::from(&mode)],
        )?;
        modes.push(env.new_global_ref(&mode)?);
    }
    let method = env.get_method_id(
        &paint,
        jni_str!("setXfermode"),
        jni_sig!("(Landroid/graphics/Xfermode;)Landroid/graphics/Xfermode;"),
    )?;
    Ok((method, OBJECT, modes))
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
        let version = env.find_class(jni_str!("android/os/Build$VERSION"))?;
        let sdk = env
            .get_static_field(&version, jni_str!("SDK_INT"), jni_sig!("I"))?
            .i()?;
        let (set_composite, set_composite_returns, composites) = composites(env, sdk)?;
        let shader = env.find_class(jni_str!("android/graphics/Shader"))?;
        let linear_gradient = class(env, jni_str!("android/graphics/LinearGradient"))?;
        let radial_gradient = class(env, jni_str!("android/graphics/RadialGradient"))?;
        let dash = class(env, jni_str!("android/graphics/DashPathEffect"))?;
        let rect = class(env, jni_str!("android/graphics/RectF"))?;
        let bitmap = class(env, jni_str!("android/graphics/Bitmap"))?;
        let [argb_8888] = constants(
            env,
            jni_str!("android/graphics/Bitmap$Config"),
            jni_sig!("Landroid/graphics/Bitmap$Config;"),
            [jni_str!("ARGB_8888")],
        )?;
        Ok(Self {
            save_layer: env.get_method_id(
                &canvas,
                jni_str!("saveLayer"),
                jni_sig!("(Landroid/graphics/RectF;Landroid/graphics/Paint;)I"),
            )?,
            draw_bitmap: env.get_method_id(
                &canvas,
                jni_str!("drawBitmap"),
                jni_sig!(
                    "(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V"
                ),
            )?,
            set_shader: env.get_method_id(
                &paint,
                jni_str!("setShader"),
                jni_sig!("(Landroid/graphics/Shader;)Landroid/graphics/Shader;"),
            )?,
            set_path_effect: env.get_method_id(
                &paint,
                jni_str!("setPathEffect"),
                jni_sig!("(Landroid/graphics/PathEffect;)Landroid/graphics/PathEffect;"),
            )?,
            set_filter_bitmap: env.get_method_id(
                &paint,
                jni_str!("setFilterBitmap"),
                jni_sig!("(Z)V"),
            )?,
            set_composite,
            set_composite_returns,
            set_fill_type: env.get_method_id(
                &path,
                jni_str!("setFillType"),
                jni_sig!("(Landroid/graphics/Path$FillType;)V"),
            )?,
            linear_gradient_new: env.get_method_id(
                &linear_gradient,
                jni_str!("<init>"),
                jni_sig!("(FFFF[I[FLandroid/graphics/Shader$TileMode;)V"),
            )?,
            radial_gradient_new: env.get_method_id(
                &radial_gradient,
                jni_str!("<init>"),
                jni_sig!("(FFF[I[FLandroid/graphics/Shader$TileMode;)V"),
            )?,
            focal_gradient_new: if sdk >= 31 {
                Some(env.get_method_id(
                    &radial_gradient,
                    jni_str!("<init>"),
                    jni_sig!("(FFFFFF[J[FLandroid/graphics/Shader$TileMode;)V"),
                )?)
            } else {
                None
            },
            set_local_matrix: env.get_method_id(
                &shader,
                jni_str!("setLocalMatrix"),
                jni_sig!("(Landroid/graphics/Matrix;)V"),
            )?,
            dash_new: env.get_method_id(&dash, jni_str!("<init>"), jni_sig!("([FF)V"))?,
            rect_new: env.get_method_id(&rect, jni_str!("<init>"), jni_sig!("(FFFF)V"))?,
            create_bitmap: env.get_static_method_id(
                &bitmap,
                jni_str!("createBitmap"),
                jni_sig!("([IIILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;"),
            )?,
            argb_8888,
            fill_types: constants(
                env,
                jni_str!("android/graphics/Path$FillType"),
                jni_sig!("Landroid/graphics/Path$FillType;"),
                [jni_str!("WINDING"), jni_str!("EVEN_ODD")],
            )?,
            tile_modes: constants(
                env,
                jni_str!("android/graphics/Shader$TileMode"),
                jni_sig!("Landroid/graphics/Shader$TileMode;"),
                [jni_str!("CLAMP"), jni_str!("REPEAT"), jni_str!("MIRROR")],
            )?,
            composites,
            linear_gradient_class: linear_gradient,
            radial_gradient_class: radial_gradient,
            dash_class: dash,
            rect_class: rect,
            bitmap_class: bitmap,
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
    fill: Paint,
    stroke: Paint,
    text: Paint,
    layer: Paint,
    image: Paint,
    // Line width and miter limit bits, cap, and join.
    stroke_style: Cell<Option<(u32, u32, usize, usize)>>,
    stroke_dash: RefCell<Option<(Rc<[f32]>, u32)>>,
    image_filter: Cell<Option<bool>>,
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
        const FILTER_BITMAP_FLAG: jint = 2;
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
        let layer = new(&jni.paint_class, jni.paint_new, &paint_args)?;
        let image = new(
            &jni.paint_class,
            jni.paint_new,
            &[jvalue {
                i: ANTI_ALIAS_FLAG | FILTER_BITMAP_FLAG,
            }],
        )?;
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
            fill: Paint::new(fill),
            stroke: Paint::new(stroke),
            text: Paint::new(text),
            layer: Paint::new(layer),
            image: Paint::new(image),
            stroke_style: Cell::new(None),
            stroke_dash: RefCell::new(None),
            image_filter: Cell::new(Some(true)),
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

/// A paint and the state last set on it.
struct Paint {
    object: GlobalObject,
    color: Cell<Option<u32>>,
    shaded: Cell<bool>,
    composite: Cell<Option<usize>>,
}

impl Paint {
    const fn new(object: GlobalObject) -> Self {
        Self {
            object,
            color: Cell::new(None),
            shaded: Cell::new(false),
            composite: Cell::new(Some(0)),
        }
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
    // Whether each open layer is masked.
    layers: Vec<bool>,
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
            layers: Vec::new(),
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

    fn set_color(&self, paint: &Paint, color: Color, alpha: f32) {
        let (red, green, blue, alpha) = color.components(alpha);
        let [red, green, blue, alpha] =
            [red, green, blue, alpha].map(|c| (c * 255.0).round() as u32);
        let argb = (alpha << 24) | (red << 16) | (green << 8) | blue;
        if !changed(&paint.color, argb) {
            return;
        }
        with_env(|env| unsafe {
            call_void(
                env,
                &paint.object,
                self.jni.set_color,
                &[jvalue { i: argb as jint }],
            )
        });
    }

    fn set_composite(&self, paint: &Paint, operation: CompositeOperation) {
        let index = composite_index(operation);
        if !changed(&paint.composite, index) {
            return;
        }
        let jni = self.jni;
        with_env(|env| unsafe {
            // setXfermode returns the previous mode, setBlendMode nothing.
            env.call_method_unchecked(
                &paint.object,
                jni.set_composite,
                jni.set_composite_returns,
                &[jvalue {
                    l: jni.composites[index].as_raw(),
                }],
            )?;
            Ok(())
        });
    }

    // Paints with a color or with a gradient in the current user space.
    fn set_style(&self, paint: &Paint, style: &CanvasStyle, s: &CanvasState) {
        self.set_composite(paint, s.composite);
        let jni = self.jni;
        let set_shader = |env: &mut Env, shader: &JObject| unsafe {
            env.call_method_unchecked(
                &paint.object,
                jni.set_shader,
                OBJECT,
                &[jvalue { l: shader.as_raw() }],
            )?;
            Ok(())
        };
        match style {
            CanvasStyle::Color(color) => {
                if paint.shaded.replace(false) {
                    with_env(|env| set_shader(env, &JObject::null()));
                }
                self.set_color(paint, *color, s.alpha);
            }
            CanvasStyle::Gradient(gradient) => {
                paint.shaded.set(true);
                // A shader takes the alpha of the paint color.
                self.set_color(paint, Color::rgb(255, 255, 255), s.alpha);
                with_env(|env| {
                    let shader = self.gradient_shader(env, &gradient.0)?;
                    set_shader(env, &shader)
                });
            }
        }
    }

    fn gradient_shader<'local>(
        &self,
        env: &mut Env<'local>,
        gradient: &Gradient,
    ) -> Result<JObject<'local>> {
        let jni = self.jni;
        // Android gradients need two stops, a single one is a uniform color.
        let mut stops = gradient.stops.clone();
        match stops.len() {
            0 => stops = vec![(0.0, Color::rgba(0, 0, 0, 0)); 2],
            1 => stops.push((1.0, stops[0].1)),
            _ => {}
        }
        let argb = |color: Color| {
            (u32::from(color.alpha) << 24)
                | (u32::from(color.red) << 16)
                | (u32::from(color.green) << 8)
                | u32::from(color.blue)
        };
        let offsets: Vec<f32> = stops.iter().map(|(offset, _)| *offset).collect();
        let positions = env.new_float_array(offsets.len())?;
        env.set_float_array_region(&positions, 0, &offsets)?;
        let tile = jni.tile_modes[match gradient.spread {
            GradientSpread::Pad => 0,
            GradientSpread::Repeat => 1,
            GradientSpread::Reflect => 2,
        }]
        .as_raw();
        let int_colors: Vec<jint> = stops
            .iter()
            .map(|(_, color)| argb(*color) as jint)
            .collect();
        let colors = env.new_int_array(int_colors.len())?;
        env.set_int_array_region(&colors, 0, &int_colors)?;
        let shader = match gradient.shape {
            GradientShape::Linear { start, end } => unsafe {
                env.new_object_unchecked(
                    &jni.linear_gradient_class,
                    jni.linear_gradient_new,
                    &[
                        jvalue { f: start.0 },
                        jvalue { f: start.1 },
                        jvalue { f: end.0 },
                        jvalue { f: end.1 },
                        jvalue { l: colors.as_raw() },
                        jvalue {
                            l: positions.as_raw(),
                        },
                        jvalue { l: tile },
                    ],
                )
            }?,
            GradientShape::Radial {
                focal,
                center,
                radius,
            } => {
                // Android rejects an empty radius.
                let radius = radius.max(f32::MIN_POSITIVE);
                match jni.focal_gradient_new {
                    Some(focal_gradient_new) if focal != center => {
                        // The color longs of sRGB colors hold the color in their upper half.
                        let long_colors: Vec<i64> = stops
                            .iter()
                            .map(|(_, color)| i64::from(argb(*color)) << 32)
                            .collect();
                        let colors = env.new_long_array(long_colors.len())?;
                        env.set_long_array_region(&colors, 0, &long_colors)?;
                        unsafe {
                            env.new_object_unchecked(
                                &jni.radial_gradient_class,
                                focal_gradient_new,
                                &[
                                    jvalue { f: focal.0 },
                                    jvalue { f: focal.1 },
                                    jvalue { f: 0.0 },
                                    jvalue { f: center.0 },
                                    jvalue { f: center.1 },
                                    jvalue { f: radius },
                                    jvalue { l: colors.as_raw() },
                                    jvalue {
                                        l: positions.as_raw(),
                                    },
                                    jvalue { l: tile },
                                ],
                            )
                        }?
                    }
                    // Before Android 12 radial gradients start at their center.
                    _ => unsafe {
                        env.new_object_unchecked(
                            &jni.radial_gradient_class,
                            jni.radial_gradient_new,
                            &[
                                jvalue { f: center.0 },
                                jvalue { f: center.1 },
                                jvalue { f: radius },
                                jvalue { l: colors.as_raw() },
                                jvalue {
                                    l: positions.as_raw(),
                                },
                                jvalue { l: tile },
                            ],
                        )
                    }?,
                }
            }
        };
        let [a, b, c, d, e, f] = gradient.transform;
        let (values, matrix) = (&self.resources.values, &self.resources.matrix);
        env.set_float_array_region(&**values, 0, &[a, c, e, b, d, f, 0.0, 0.0, 1.0])?;
        unsafe {
            call_void(
                env,
                matrix,
                jni.set_values,
                &[jvalue { l: values.as_raw() }],
            )?;
            call_void(
                env,
                &shader,
                jni.set_local_matrix,
                &[jvalue { l: matrix.as_raw() }],
            )?;
        }
        Ok(shader)
    }

    fn stroke_paint(&self, state: &CanvasState) -> &JObject<'static> {
        let resources = &self.resources;
        self.set_style(&resources.stroke, &state.stroke, state);
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
        let (jni, paint) = (self.jni, &resources.stroke.object);
        let style = (
            state.line_width.to_bits(),
            state.miter_limit.to_bits(),
            cap,
            join,
        );
        if changed(&resources.stroke_style, style) {
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
                    jni.set_stroke_miter,
                    &[jvalue {
                        f: state.miter_limit,
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
        let dash = (state.line_dash.clone(), state.line_dash_offset.to_bits());
        if resources.stroke_dash.borrow().as_ref() != Some(&dash) {
            with_env(|env| {
                let effect = if dash.0.is_empty() {
                    JObject::null()
                } else {
                    let intervals = env.new_float_array(dash.0.len())?;
                    env.set_float_array_region(&intervals, 0, &dash.0)?;
                    unsafe {
                        env.new_object_unchecked(
                            &jni.dash_class,
                            jni.dash_new,
                            &[
                                jvalue {
                                    l: intervals.as_raw(),
                                },
                                jvalue {
                                    f: state.line_dash_offset,
                                },
                            ],
                        )
                    }?
                };
                unsafe {
                    env.call_method_unchecked(
                        paint,
                        jni.set_path_effect,
                        OBJECT,
                        &[jvalue { l: effect.as_raw() }],
                    )
                }?;
                Ok(())
            });
            *resources.stroke_dash.borrow_mut() = Some(dash);
        }
        paint
    }

    fn text_paint(&self, state: &CanvasState) -> &JObject<'static> {
        let (jni, resources) = (self.jni, &self.resources);
        let text = &resources.text.object;
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
    // A layer draws into an offscreen buffer, composited with the layer paint when restored.
    fn save_layer(&mut self, alpha: f32, operation: CompositeOperation) {
        let layer = &self.resources.layer;
        self.set_color(layer, Color::rgb(0, 0, 0), alpha);
        self.set_composite(layer, operation);
        self.stack.push((self.transform, self.synced));
        let (jni, canvas, paint) = (self.jni, &self.canvas, layer.object.as_raw());
        with_env(|env| unsafe {
            env.call_method_unchecked(
                canvas,
                jni.save_layer,
                INT,
                &[
                    jvalue {
                        l: std::ptr::null_mut(),
                    },
                    jvalue { l: paint },
                ],
            )?;
            Ok(())
        });
    }
    pub(crate) fn begin_layer(&mut self, s: &CanvasState) {
        self.save_layer(s.alpha, s.composite);
        self.layers.push(false);
    }
    pub(crate) fn begin_mask_layer(&mut self, s: &CanvasState) {
        self.save_layer(s.alpha, s.composite);
        self.layers.push(true);
    }
    /// The content of a masked layer is a nested layer that composites into the mask.
    pub(crate) fn end_mask(&mut self) {
        self.save_layer(1.0, CompositeOperation::SourceIn);
    }
    pub(crate) fn end_layer(&mut self) {
        if self.layers.pop().expect("open layer") {
            self.restore();
        }
        self.restore();
    }
    pub(crate) fn draw_image(
        &mut self,
        s: &CanvasState,
        image: &PlatformImage,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        self.sync_transform();
        let paint = &self.resources.image;
        self.set_color(paint, Color::rgb(0, 0, 0), s.alpha);
        self.set_composite(paint, s.composite);
        let jni = self.jni;
        if changed(&self.resources.image_filter, s.image_smoothing) {
            with_env(|env| unsafe {
                call_void(
                    env,
                    &paint.object,
                    jni.set_filter_bitmap,
                    &[jvalue {
                        z: s.image_smoothing,
                    }],
                )
            });
        }
        with_env(|env| {
            let bitmap = image.bitmap(env, jni)?;
            let rect = unsafe {
                env.new_object_unchecked(
                    &jni.rect_class,
                    jni.rect_new,
                    &[
                        jvalue { f: x },
                        jvalue { f: y },
                        jvalue { f: x + w },
                        jvalue { f: y + h },
                    ],
                )
            }?;
            unsafe {
                call_void(
                    env,
                    &self.canvas,
                    jni.draw_bitmap,
                    &[
                        jvalue { l: bitmap.as_raw() },
                        jvalue {
                            l: std::ptr::null_mut(),
                        },
                        jvalue { l: rect.as_raw() },
                        jvalue {
                            l: paint.object.as_raw(),
                        },
                    ],
                )
            }
        });
    }
    // Paths are reused, so they get the fill rule of every fill and clip.
    fn set_fill_rule(&self, rule: FillRule) {
        let jni = self.jni;
        let rule = jni.fill_types[match rule {
            FillRule::NonZero => 0,
            FillRule::EvenOdd => 1,
        }]
        .as_raw();
        with_env(|env| unsafe {
            call_void(
                env,
                &self.resources.path,
                jni.set_fill_type,
                &[jvalue { l: rule }],
            )
        });
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
        self.set_style(&resources.fill, &s.fill, s);
        self.draw_rect(&resources.fill.object, x, y, w, h);
    }
    pub(crate) fn stroke_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        let resources = self.resources.clone();
        self.stroke_paint(s);
        self.draw_rect(&resources.stroke.object, x, y, w, h);
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
    pub(crate) fn fill(&mut self, s: &CanvasState, rule: FillRule) {
        if self.build_path() {
            self.set_fill_rule(rule);
            let resources = &self.resources;
            self.set_style(&resources.fill, &s.fill, s);
            self.draw_path(&resources.fill.object);
        }
    }
    pub(crate) fn stroke(&mut self, s: &CanvasState) {
        if self.build_path() {
            self.set_fill_rule(FillRule::NonZero);
            self.draw_path(self.stroke_paint(s));
        }
    }
    pub(crate) fn clip(&mut self, rule: FillRule) {
        let built = self.build_path();
        self.set_fill_rule(rule);
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
    pub(crate) fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.transform = self.transform.concat(Transform([a, b, c, d, e, f]));
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
        self.set_style(&self.resources.text, &s.fill, s);
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

/// An Android bitmap of a canvas image, created when it is first drawn.
pub(crate) struct PlatformImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pixels: Arc<[u8]>,
    bitmap: RefCell<Option<Rc<GlobalObject>>>,
}

impl PlatformImage {
    pub(crate) const fn new(width: u32, height: u32, pixels: Arc<[u8]>) -> Self {
        Self {
            width,
            height,
            pixels,
            bitmap: RefCell::new(None),
        }
    }

    fn bitmap(&self, env: &mut Env, jni: &Jni) -> Result<Rc<GlobalObject>> {
        if let Some(bitmap) = &*self.bitmap.borrow() {
            return Ok(bitmap.clone());
        }
        // Bitmap colors are straight-alpha ARGB integers.
        let colors: Vec<jint> = self
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[blue, green, red, alpha]| {
                let straight = |value: u8| {
                    if alpha == 0 {
                        0
                    } else {
                        ((u32::from(value) * 255 + u32::from(alpha) / 2) / u32::from(alpha))
                            .min(255)
                    }
                };
                ((u32::from(alpha) << 24)
                    | (straight(red) << 16)
                    | (straight(green) << 8)
                    | straight(blue)) as jint
            })
            .collect();
        let array = env.new_int_array(colors.len())?;
        env.set_int_array_region(&array, 0, &colors)?;
        let bitmap = unsafe {
            env.call_static_method_unchecked(
                &jni.bitmap_class,
                jni.create_bitmap,
                OBJECT,
                &[
                    jvalue { l: array.as_raw() },
                    jvalue {
                        i: self.width as jint,
                    },
                    jvalue {
                        i: self.height as jint,
                    },
                    jvalue {
                        l: jni.argb_8888.as_raw(),
                    },
                ],
            )
        }?
        .l()?;
        let bitmap = Rc::new(env.new_global_ref(&bitmap)?);
        *self.bitmap.borrow_mut() = Some(bitmap.clone());
        Ok(bitmap)
    }
}
