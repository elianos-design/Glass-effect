//! A minimal OpenFX image-effect plug-in (filter context, 32-bit float RGBA, CPU).
//! DaVinci Resolve lists it under Effects > OpenFX > Filters > Glass Lens on the Edit
//! and Cut pages, and in the OpenFX panel of the Color page (drop it on a node).
//! It renders with exactly the same code as the preview app (glass::render).

#![allow(non_snake_case, clippy::missing_safety_doc)]

use crate::glass::{self, Img, Params, LOOKS};
use std::ffi::{c_char, c_double, c_int, c_uint, c_void, CStr, CString};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};

type OfxStatus = c_int;
type H = *mut c_void;

const STAT_OK: OfxStatus = 0;
const STAT_FAILED: OfxStatus = 1;
const STAT_MISSING_HOST_FEATURE: OfxStatus = 4;
const STAT_REPLY_DEFAULT: OfxStatus = 14;

#[repr(C)]
pub struct OfxHost {
    host: H,
    fetch_suite: unsafe extern "C" fn(H, *const c_char, c_int) -> *const c_void,
}

#[repr(C)]
pub struct OfxPlugin {
    plugin_api: *const c_char,
    api_version: c_int,
    plugin_identifier: *const c_char,
    plugin_version_major: c_uint,
    plugin_version_minor: c_uint,
    set_host: unsafe extern "C" fn(*mut OfxHost),
    main_entry: unsafe extern "C" fn(*const c_char, *const c_void, H, H) -> OfxStatus,
}
unsafe impl Sync for OfxPlugin {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct OfxRectD {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

#[repr(C)]
struct PropertySuite {
    set_pointer: unsafe extern "C" fn(H, *const c_char, c_int, *mut c_void) -> OfxStatus,
    set_string: unsafe extern "C" fn(H, *const c_char, c_int, *const c_char) -> OfxStatus,
    set_double: unsafe extern "C" fn(H, *const c_char, c_int, c_double) -> OfxStatus,
    set_int: unsafe extern "C" fn(H, *const c_char, c_int, c_int) -> OfxStatus,
    _set_n: [*const c_void; 4],
    get_pointer: unsafe extern "C" fn(H, *const c_char, c_int, *mut *mut c_void) -> OfxStatus,
    get_string: unsafe extern "C" fn(H, *const c_char, c_int, *mut *mut c_char) -> OfxStatus,
    get_double: unsafe extern "C" fn(H, *const c_char, c_int, *mut c_double) -> OfxStatus,
    get_int: unsafe extern "C" fn(H, *const c_char, c_int, *mut c_int) -> OfxStatus,
    _rest: [*const c_void; 6],
}

#[repr(C)]
struct ImageEffectSuite {
    get_property_set: unsafe extern "C" fn(H, *mut H) -> OfxStatus,
    get_param_set: unsafe extern "C" fn(H, *mut H) -> OfxStatus,
    clip_define: unsafe extern "C" fn(H, *const c_char, *mut H) -> OfxStatus,
    clip_get_handle: unsafe extern "C" fn(H, *const c_char, *mut H, *mut H) -> OfxStatus,
    _clip_get_property_set: *const c_void,
    clip_get_image: unsafe extern "C" fn(H, c_double, *const OfxRectD, *mut H) -> OfxStatus,
    clip_release_image: unsafe extern "C" fn(H) -> OfxStatus,
    _rest: [*const c_void; 6],
}

#[repr(C)]
struct ParameterSuite {
    param_define: unsafe extern "C" fn(H, *const c_char, *const c_char, *mut H) -> OfxStatus,
    param_get_handle: unsafe extern "C" fn(H, *const c_char, *mut H, *mut H) -> OfxStatus,
    _param_set_get_property_set: *const c_void,
    _param_get_property_set: *const c_void,
    _param_get_value: *const c_void,
    param_get_value_at_time: unsafe extern "C" fn(H, c_double, ...) -> OfxStatus,
    _param_get_derivative: *const c_void,
    _param_get_integral: *const c_void,
    param_set_value: unsafe extern "C" fn(H, ...) -> OfxStatus,
    _rest: [*const c_void; 9],
}

static HOST: AtomicPtr<OfxHost> = AtomicPtr::new(null_mut());
static PROPS: AtomicPtr<PropertySuite> = AtomicPtr::new(null_mut());
static EFFECT: AtomicPtr<ImageEffectSuite> = AtomicPtr::new(null_mut());
static PARAMS: AtomicPtr<ParameterSuite> = AtomicPtr::new(null_mut());

unsafe fn props() -> &'static PropertySuite {
    &*PROPS.load(Ordering::Acquire)
}
unsafe fn effect_suite() -> &'static ImageEffectSuite {
    &*EFFECT.load(Ordering::Acquire)
}
unsafe fn param_suite() -> &'static ParameterSuite {
    &*PARAMS.load(Ordering::Acquire)
}

static PLUGIN: OfxPlugin = OfxPlugin {
    plugin_api: c"OfxImageEffectPluginAPI".as_ptr(),
    api_version: 1,
    plugin_identifier: c"com.glasslenslab.GlassLens".as_ptr(),
    plugin_version_major: 1,
    plugin_version_minor: 0,
    set_host,
    main_entry,
};

#[no_mangle]
pub extern "C" fn OfxGetNumberOfPlugins() -> c_int {
    1
}

#[no_mangle]
pub extern "C" fn OfxGetPlugin(nth: c_int) -> *const OfxPlugin {
    if nth == 0 {
        &PLUGIN
    } else {
        null()
    }
}

unsafe extern "C" fn set_host(host: *mut OfxHost) {
    HOST.store(host, Ordering::Release);
}

unsafe extern "C" fn main_entry(action: *const c_char, handle: *const c_void, in_args: H, out_args: H) -> OfxStatus {
    if action.is_null() {
        return STAT_FAILED;
    }
    std::panic::catch_unwind(|| unsafe { dispatch(action, handle as H, in_args, out_args) }).unwrap_or(STAT_FAILED)
}

unsafe fn dispatch(action: *const c_char, handle: H, in_args: H, _out_args: H) -> OfxStatus {
    match CStr::from_ptr(action).to_bytes() {
        b"OfxActionLoad" => load(),
        b"OfxActionDescribe" => describe(handle),
        b"OfxImageEffectActionDescribeInContext" => describe_in_context(handle),
        b"OfxActionCreateInstance" | b"OfxActionDestroyInstance" | b"OfxActionUnload" => STAT_OK,
        b"OfxImageEffectActionRender" => render(handle, in_args),
        b"OfxActionInstanceChanged" => instance_changed(handle, in_args),
        _ => STAT_REPLY_DEFAULT,
    }
}

unsafe fn load() -> OfxStatus {
    let host = HOST.load(Ordering::Acquire);
    if host.is_null() {
        return STAT_MISSING_HOST_FEATURE;
    }
    let fetch = (*host).fetch_suite;
    let h = (*host).host;
    let p = fetch(h, c"OfxPropertySuite".as_ptr(), 1) as *mut PropertySuite;
    let e = fetch(h, c"OfxImageEffectSuite".as_ptr(), 1) as *mut ImageEffectSuite;
    let pa = fetch(h, c"OfxParameterSuite".as_ptr(), 1) as *mut ParameterSuite;
    if p.is_null() || e.is_null() || pa.is_null() {
        return STAT_MISSING_HOST_FEATURE;
    }
    PROPS.store(p, Ordering::Release);
    EFFECT.store(e, Ordering::Release);
    PARAMS.store(pa, Ordering::Release);
    STAT_OK
}

unsafe fn set_str(ps: H, name: &CStr, i: c_int, v: &CStr) {
    (props().set_string)(ps, name.as_ptr(), i, v.as_ptr());
}
unsafe fn set_int(ps: H, name: &CStr, i: c_int, v: c_int) {
    (props().set_int)(ps, name.as_ptr(), i, v);
}
unsafe fn set_dbl(ps: H, name: &CStr, i: c_int, v: f64) {
    (props().set_double)(ps, name.as_ptr(), i, v);
}

unsafe fn describe(effect: H) -> OfxStatus {
    let mut ps: H = null_mut();
    if (effect_suite().get_property_set)(effect, &mut ps) != STAT_OK {
        return STAT_FAILED;
    }
    set_str(ps, c"OfxPropLabel", 0, c"Glass Lens");
    set_str(ps, c"OfxImageEffectPluginPropGrouping", 0, c"Glass Lens");
    set_str(
        ps,
        c"OfxPropPluginDescription",
        0,
        c"Glass in front of the lens: Circular Cone, Circular Spherical, Icosahedron, Kaleidoscope, Prism and Fluted Glass.",
    );
    set_str(ps, c"OfxImageEffectPropSupportedContexts", 0, c"OfxImageEffectContextFilter");
    set_str(ps, c"OfxImageEffectPropSupportedContexts", 1, c"OfxImageEffectContextGeneral");
    set_str(ps, c"OfxImageEffectPropSupportedPixelDepths", 0, c"OfxBitDepthFloat");
    set_int(ps, c"OfxImageEffectPluginPropSingleInstance", 0, 0);
    set_str(ps, c"OfxImageEffectPluginRenderThreadSafety", 0, c"OfxImageEffectRenderFullySafe");
    set_int(ps, c"OfxImageEffectPluginPropHostFrameThreading", 0, 0);
    set_int(ps, c"OfxImageEffectPropSupportsTiles", 0, 0);
    set_int(ps, c"OfxImageEffectPropSupportsMultiResolution", 0, 0);
    set_int(ps, c"OfxImageEffectPropTemporalClipAccess", 0, 0);
    STAT_OK
}

struct NumParam {
    name: &'static CStr,
    label: &'static CStr,
    hint: &'static CStr,
    lo: f64,
    hi: f64,
}

const NUM_PARAMS: [NumParam; 12] = [
    NumParam { name: c"amount", label: c"Amount", hint: c"How strongly the glass bends the picture.", lo: 0.0, hi: 1.0 },
    NumParam { name: c"size", label: c"Size", hint: c"Clear centre / facet / cell size / edge position / rib width.", lo: 0.0, hi: 1.0 },
    NumParam { name: c"softness", label: c"Softness", hint: c"Blur inside the glass.", lo: 0.0, hi: 1.0 },
    NumParam { name: c"dispersion", label: c"Rainbow Split", hint: c"Colour fringes where the glass bends light.", lo: 0.0, hi: 1.0 },
    NumParam { name: c"edgeLight", label: c"Edge Light", hint: c"Light caught by the glass edges.", lo: 0.0, hi: 1.0 },
    NumParam { name: c"edgeSoft", label: c"Edge Softness", hint: c"Melts the cut between pieces of glass into a smooth blend.", lo: 0.0, hi: 1.0 },
    NumParam { name: c"bevel", label: c"Bevel", hint: c"Rounded, light-catching glass edges.", lo: 0.0, hi: 1.0 },
    NumParam { name: c"rotation", label: c"Rotation", hint: c"Turns the glass (degrees).", lo: -180.0, hi: 180.0 },
    NumParam { name: c"centerX", label: c"Centre X", hint: c"Where the glass centre sits, left (0) to right (1).", lo: 0.0, hi: 1.0 },
    NumParam { name: c"centerY", label: c"Centre Y", hint: c"Where the glass centre sits, top (0) to bottom (1).", lo: 0.0, hi: 1.0 },
    NumParam { name: c"variation", label: c"Variation", hint: c"A different cut of the same glass.", lo: 0.0, hi: 10.0 },
    NumParam { name: c"mix", label: c"Mix", hint: c"Blend with the untouched picture.", lo: 0.0, hi: 1.0 },
];

fn num_value(p: &Params, i: usize) -> f64 {
    (match i {
        0 => p.amount,
        1 => p.size,
        2 => p.softness,
        3 => p.dispersion,
        4 => p.edge_light,
        5 => p.edge_soft,
        6 => p.bevel,
        7 => p.rotation,
        8 => p.cx,
        9 => p.cy,
        10 => p.variation,
        _ => p.mix,
    }) as f64
}

fn set_num_value(p: &mut Params, i: usize, v: f64) {
    let v = v as f32;
    match i {
        0 => p.amount = v,
        1 => p.size = v,
        2 => p.softness = v,
        3 => p.dispersion = v,
        4 => p.edge_light = v,
        5 => p.edge_soft = v,
        6 => p.bevel = v,
        7 => p.rotation = v,
        8 => p.cx = v,
        9 => p.cy = v,
        10 => p.variation = v,
        _ => p.mix = v,
    }
}

unsafe fn describe_in_context(effect: H) -> OfxStatus {
    let es = effect_suite();
    for clip in [c"Source", c"Output"] {
        let mut cp: H = null_mut();
        if (es.clip_define)(effect, clip.as_ptr(), &mut cp) != STAT_OK {
            return STAT_FAILED;
        }
        set_str(cp, c"OfxImageEffectPropSupportedComponents", 0, c"OfxImageComponentRGBA");
        set_int(cp, c"OfxImageEffectPropSupportsTiles", 0, 0);
    }
    let mut set: H = null_mut();
    if (es.get_param_set)(effect, &mut set) != STAT_OK {
        return STAT_FAILED;
    }
    let pa = param_suite();
    let def = Params::preset(0);

    let mut pp: H = null_mut();
    (pa.param_define)(set, c"OfxParamTypeChoice".as_ptr(), c"look".as_ptr(), &mut pp);
    set_str(pp, c"OfxPropLabel", 0, c"Look");
    set_str(pp, c"OfxParamPropHint", 0, c"Which glass is in front of the lens. Picking one loads its starting settings.");
    for (i, name) in LOOKS.iter().enumerate() {
        let s = CString::new(*name).unwrap();
        set_str(pp, c"OfxParamPropChoiceOption", i as c_int, &s);
    }
    set_int(pp, c"OfxParamPropDefault", 0, 0);

    for (i, np) in NUM_PARAMS.iter().enumerate() {
        if i == 3 {
            // Facets sits with the glass controls
            let mut fp: H = null_mut();
            (pa.param_define)(set, c"OfxParamTypeInteger".as_ptr(), c"facets".as_ptr(), &mut fp);
            set_str(fp, c"OfxPropLabel", 0, c"Facets");
            set_str(fp, c"OfxParamPropHint", 0, c"Icosahedron: facets around the centre. Prism: 4 or more mirrors both sides.");
            set_int(fp, c"OfxParamPropDefault", 0, def.facets);
            set_int(fp, c"OfxParamPropMin", 0, 3);
            set_int(fp, c"OfxParamPropMax", 0, 24);
            set_int(fp, c"OfxParamPropDisplayMin", 0, 3);
            set_int(fp, c"OfxParamPropDisplayMax", 0, 12);
        }
        let mut dp: H = null_mut();
        (pa.param_define)(set, c"OfxParamTypeDouble".as_ptr(), np.name.as_ptr(), &mut dp);
        set_str(dp, c"OfxPropLabel", 0, np.label);
        set_str(dp, c"OfxParamPropHint", 0, np.hint);
        set_str(dp, c"OfxParamPropDoubleType", 0, c"OfxParamDoubleTypePlain");
        set_dbl(dp, c"OfxParamPropDefault", 0, num_value(&def, i));
        let (lo, hi) = if np.hi > 1.0 { (np.lo, np.hi) } else { (np.lo, np.hi * 2.0) };
        set_dbl(dp, c"OfxParamPropMin", 0, lo);
        set_dbl(dp, c"OfxParamPropMax", 0, hi);
        set_dbl(dp, c"OfxParamPropDisplayMin", 0, np.lo);
        set_dbl(dp, c"OfxParamPropDisplayMax", 0, np.hi);
        set_dbl(dp, c"OfxParamPropIncrement", 0, if np.hi > 1.0 { 0.5 } else { 0.01 });
        set_int(dp, c"OfxParamPropDigits", 0, 2);
    }
    STAT_OK
}

unsafe fn param_handle(set: H, name: &CStr) -> H {
    let mut h: H = null_mut();
    (param_suite().param_get_handle)(set, name.as_ptr(), &mut h, null_mut());
    h
}

unsafe fn read_params(effect: H, time: f64) -> Option<Params> {
    let mut set: H = null_mut();
    if (effect_suite().get_param_set)(effect, &mut set) != STAT_OK {
        return None;
    }
    let pa = param_suite();
    let mut look: c_int = 0;
    let h = param_handle(set, c"look");
    if !h.is_null() {
        (pa.param_get_value_at_time)(h, time, &mut look as *mut c_int);
    }
    let effect_idx = (look.max(0) as usize).min(LOOKS.len() - 1);
    let mut p = Params::preset(effect_idx);
    let mut facets: c_int = p.facets;
    let h = param_handle(set, c"facets");
    if !h.is_null() {
        (pa.param_get_value_at_time)(h, time, &mut facets as *mut c_int);
    }
    p.facets = facets;
    for (i, np) in NUM_PARAMS.iter().enumerate() {
        let h = param_handle(set, np.name);
        if !h.is_null() {
            let mut v: f64 = num_value(&p, i);
            (pa.param_get_value_at_time)(h, time, &mut v as *mut f64);
            set_num_value(&mut p, i, v);
        }
    }
    Some(p)
}

/// Picking a new look loads that look's starting settings into the other controls.
unsafe fn instance_changed(effect: H, in_args: H) -> OfxStatus {
    let mut name: *mut c_char = null_mut();
    (props().get_string)(in_args, c"OfxPropName".as_ptr(), 0, &mut name);
    if name.is_null() || CStr::from_ptr(name).to_bytes() != b"look" {
        return STAT_REPLY_DEFAULT;
    }
    let mut reason: *mut c_char = null_mut();
    (props().get_string)(in_args, c"OfxPropChangeReason".as_ptr(), 0, &mut reason);
    if reason.is_null() || CStr::from_ptr(reason).to_bytes() != b"OfxChangeUserEdited" {
        return STAT_REPLY_DEFAULT;
    }
    let mut time: f64 = 0.0;
    (props().get_double)(in_args, c"OfxPropTime".as_ptr(), 0, &mut time);
    let mut set: H = null_mut();
    if (effect_suite().get_param_set)(effect, &mut set) != STAT_OK {
        return STAT_FAILED;
    }
    let pa = param_suite();
    let mut look: c_int = 0;
    let h = param_handle(set, c"look");
    if h.is_null() {
        return STAT_FAILED;
    }
    (pa.param_get_value_at_time)(h, time, &mut look as *mut c_int);
    let preset = Params::preset((look.max(0) as usize).min(LOOKS.len() - 1));
    let h = param_handle(set, c"facets");
    if !h.is_null() {
        (pa.param_set_value)(h, preset.facets as c_int);
    }
    for (i, np) in NUM_PARAMS.iter().enumerate() {
        let h = param_handle(set, np.name);
        if !h.is_null() {
            (pa.param_set_value)(h, num_value(&preset, i));
        }
    }
    STAT_OK
}

struct ImageView {
    handle: H,
    data: *mut u8,
    bounds: [c_int; 4],
    row_bytes: isize,
}

unsafe fn fetch_image(effect: H, clip: &CStr, time: f64) -> Option<ImageView> {
    let es = effect_suite();
    let mut ch: H = null_mut();
    if (es.clip_get_handle)(effect, clip.as_ptr(), &mut ch, null_mut()) != STAT_OK || ch.is_null() {
        return None;
    }
    let mut img: H = null_mut();
    if (es.clip_get_image)(ch, time, null(), &mut img) != STAT_OK || img.is_null() {
        return None;
    }
    let pr = props();
    let mut data: *mut c_void = null_mut();
    (pr.get_pointer)(img, c"OfxImagePropData".as_ptr(), 0, &mut data);
    let mut b = [0 as c_int; 4];
    for (i, v) in b.iter_mut().enumerate() {
        (pr.get_int)(img, c"OfxImagePropBounds".as_ptr(), i as c_int, v);
    }
    let mut rb: c_int = 0;
    (pr.get_int)(img, c"OfxImagePropRowBytes".as_ptr(), 0, &mut rb);
    let mut depth: *mut c_char = null_mut();
    (pr.get_string)(img, c"OfxImageEffectPropPixelDepth".as_ptr(), 0, &mut depth);
    let mut comps: *mut c_char = null_mut();
    (pr.get_string)(img, c"OfxImageEffectPropComponents".as_ptr(), 0, &mut comps);
    let float_ok = !depth.is_null() && CStr::from_ptr(depth).to_bytes() == b"OfxBitDepthFloat";
    let rgba_ok = !comps.is_null() && CStr::from_ptr(comps).to_bytes() == b"OfxImageComponentRGBA";
    if data.is_null() || !float_ok || !rgba_ok || b[2] <= b[0] || b[3] <= b[1] {
        (es.clip_release_image)(img);
        return None;
    }
    Some(ImageView { handle: img, data: data as *mut u8, bounds: b, row_bytes: rb as isize })
}

unsafe fn pixel(v: &ImageView, x: c_int, y: c_int) -> *mut f32 {
    let row = v.data.offset((y - v.bounds[1]) as isize * v.row_bytes);
    (row as *mut f32).offset((x - v.bounds[0]) as isize * 4)
}

unsafe fn render(effect: H, in_args: H) -> OfxStatus {
    let pr = props();
    let mut time: f64 = 0.0;
    (pr.get_double)(in_args, c"OfxPropTime".as_ptr(), 0, &mut time);
    let mut win = [0 as c_int; 4];
    for (i, v) in win.iter_mut().enumerate() {
        (pr.get_int)(in_args, c"OfxImageEffectPropRenderWindow".as_ptr(), i as c_int, v);
    }
    let Some(params) = read_params(effect, time) else {
        return STAT_FAILED;
    };
    let Some(src) = fetch_image(effect, c"Source", time) else {
        return STAT_FAILED;
    };
    let Some(dst) = fetch_image(effect, c"Output", time) else {
        (effect_suite().clip_release_image)(src.handle);
        return STAT_FAILED;
    };

    // Copy the source into a top-down picture, render it, write it back bottom-up.
    let w = (src.bounds[2] - src.bounds[0]) as usize;
    let h = (src.bounds[3] - src.bounds[1]) as usize;
    let mut img = Img::new(w, h);
    for row in 0..h {
        let y = src.bounds[1] + row as c_int;
        let top = h - 1 - row;
        for col in 0..w {
            let p = pixel(&src, src.bounds[0] + col as c_int, y);
            img.px[top * w + col] = [*p, *p.add(1), *p.add(2), *p.add(3)];
        }
    }
    let out = glass::render(&img, &params);

    let x0 = win[0].max(dst.bounds[0]);
    let x1 = win[2].min(dst.bounds[2]);
    let y0 = win[1].max(dst.bounds[1]);
    let y1 = win[3].min(dst.bounds[3]);
    for y in y0..y1 {
        for x in x0..x1 {
            let d = pixel(&dst, x, y);
            let sx = x - src.bounds[0];
            let sy = y - src.bounds[1];
            if sx < 0 || sy < 0 || sx as usize >= w || sy as usize >= h {
                for c in 0..4 {
                    *d.add(c) = 0.0;
                }
                continue;
            }
            let idx = (h - 1 - sy as usize) * w + sx as usize;
            let o = out.px[idx];
            *d = o[0];
            *d.add(1) = o[1];
            *d.add(2) = o[2];
            *d.add(3) = img.px[idx][3];
        }
    }
    (effect_suite().clip_release_image)(src.handle);
    (effect_suite().clip_release_image)(dst.handle);
    STAT_OK
}
