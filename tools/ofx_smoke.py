"""Smoke test: load the Glass Lens OpenFX plug-in through a tiny stand-in host
(the way DaVinci Resolve does), describe it, and render one frame.
Usage: python -I ofx_smoke.py <path to glass_lens.dll> [look]"""
import ctypes as C
import math
import sys

dll_path = sys.argv[1]
look = int(sys.argv[2]) if len(sys.argv) > 2 else 2
P = C.c_void_p
ST = C.c_int

store = {}          # handle id -> {prop name: {index: value}}
keep = []           # keep buffers alive
next_id = [100]


def new_set():
    next_id[0] += 1
    store[next_id[0]] = {}
    return next_id[0]


def put(h, name, i, v):
    store[h].setdefault(name, {})[i] = v


def get(h, name, i):
    return store.get(h, {}).get(name, {}).get(i)


def dec(b):
    return b.decode() if isinstance(b, bytes) else b

# ---- property suite
SET_P = C.CFUNCTYPE(ST, P, C.c_char_p, C.c_int, P)
SET_S = C.CFUNCTYPE(ST, P, C.c_char_p, C.c_int, C.c_char_p)
SET_D = C.CFUNCTYPE(ST, P, C.c_char_p, C.c_int, C.c_double)
SET_I = C.CFUNCTYPE(ST, P, C.c_char_p, C.c_int, C.c_int)
GET_P = C.CFUNCTYPE(ST, P, C.c_char_p, C.c_int, C.POINTER(P))
GET_S = C.CFUNCTYPE(ST, P, C.c_char_p, C.c_int, C.POINTER(C.c_char_p))
GET_D = C.CFUNCTYPE(ST, P, C.c_char_p, C.c_int, C.POINTER(C.c_double))
GET_I = C.CFUNCTYPE(ST, P, C.c_char_p, C.c_int, C.POINTER(C.c_int))


def ok(f):
    def w(*a):
        try:
            return f(*a)
        except Exception as e:  # never let Python errors cross into the plug-in
            print("host error:", e)
            return 1
    return w

cb = dict(
    sp=SET_P(ok(lambda h, n, i, v: put(h, dec(n), i, v) or 0)),
    ss=SET_S(ok(lambda h, n, i, v: put(h, dec(n), i, dec(v)) or 0)),
    sd=SET_D(ok(lambda h, n, i, v: put(h, dec(n), i, v) or 0)),
    si=SET_I(ok(lambda h, n, i, v: put(h, dec(n), i, v) or 0)),
)


def gp(h, n, i, out):
    v = get(h, dec(n), i)
    if v is None:
        return 10
    out[0] = v
    return 0


def gs(h, n, i, out):
    v = get(h, dec(n), i)
    if v is None:
        return 10
    b = C.create_string_buffer(v.encode())
    keep.append(b)
    out[0] = C.cast(b, C.c_char_p)
    return 0

cb.update(gp=GET_P(ok(gp)), gs=GET_S(ok(gs)), gd=GET_D(ok(gp)), gi=GET_I(ok(gp)))


class PropSuite(C.Structure):
    _fields_ = [("sp", SET_P), ("ss", SET_S), ("sd", SET_D), ("si", SET_I)] + [(f"n{i}", P) for i in range(4)] + \
        [("gp", GET_P), ("gs", GET_S), ("gd", GET_D), ("gi", GET_I)] + [(f"r{i}", P) for i in range(6)]

props = PropSuite(cb["sp"], cb["ss"], cb["sd"], cb["si"], None, None, None, None, cb["gp"], cb["gs"], cb["gd"], cb["gi"])

# ---- image effect suite
effect = new_set()
paramset = new_set()
clips = {}
params = {}   # name -> (handle, type)
W, H = 320, 180
src_buf = (C.c_float * (W * H * 4))()
dst_buf = (C.c_float * (W * H * 4))()
for y in range(H):
    for x in range(W):
        i = (y * W + x) * 4
        src_buf[i] = x / W
        src_buf[i + 1] = y / H
        src_buf[i + 2] = 0.5 + 0.5 * math.sin(x * 0.2) * math.sin(y * 0.2)
        src_buf[i + 3] = 1.0

F_HH = C.CFUNCTYPE(ST, P, C.POINTER(P))
F_DEF = C.CFUNCTYPE(ST, P, C.c_char_p, C.POINTER(P))
F_GETH = C.CFUNCTYPE(ST, P, C.c_char_p, C.POINTER(P), C.POINTER(P))
F_IMG = C.CFUNCTYPE(ST, P, C.c_double, P, C.POINTER(P))
F_REL = C.CFUNCTYPE(ST, P)


def get_ps(h, out):
    out[0] = h
    return 0


def get_pset(h, out):
    out[0] = paramset
    return 0


def clip_define(h, name, out):
    c = new_set()
    clips[dec(name)] = c
    out[0] = c
    return 0


def clip_get(h, name, out, ps):
    out[0] = clips[dec(name)]
    if ps:
        ps[0] = clips[dec(name)]
    return 0


def clip_image(clip, t, region, out):
    img = new_set()
    buf = src_buf if clip == clips["Source"] else dst_buf
    put(img, "OfxImagePropData", 0, C.addressof(buf))
    for i, v in enumerate([0, 0, W, H]):
        put(img, "OfxImagePropBounds", i, v)
    put(img, "OfxImagePropRowBytes", 0, W * 16)
    put(img, "OfxImageEffectPropPixelDepth", 0, "OfxBitDepthFloat")
    put(img, "OfxImageEffectPropComponents", 0, "OfxImageComponentRGBA")
    out[0] = img
    return 0

ecb = [F_HH(ok(get_ps)), F_HH(ok(get_pset)), F_DEF(ok(clip_define)), F_GETH(ok(clip_get)), None,
       F_IMG(ok(clip_image)), F_REL(ok(lambda h: 0))]


class EffectSuite(C.Structure):
    _fields_ = [("a", F_HH), ("b", F_HH), ("c", F_DEF), ("d", F_GETH), ("e", P), ("f", F_IMG), ("g", F_REL)] + \
        [(f"r{i}", P) for i in range(6)]

esuite = EffectSuite(*ecb)

# ---- parameter suite
F_PDEF = C.CFUNCTYPE(ST, P, C.c_char_p, C.c_char_p, C.POINTER(P))
F_PGET = C.CFUNCTYPE(ST, P, C.c_char_p, C.POINTER(P), C.POINTER(P))
F_VAL = C.CFUNCTYPE(ST, P, C.c_double, P)   # variadic on the plug-in side; one pointer argument


def pdef(ps, typ, name, out):
    h = new_set()
    params[dec(name)] = (h, dec(typ))
    out[0] = h
    return 0


def pget(ps, name, out, pps):
    n = dec(name)
    if n not in params:
        return 9
    out[0] = params[n][0]
    return 0


def pval(h, t, ptr):
    for n, (ph, typ) in params.items():
        if ph == h:
            v = get(ph, "OfxParamPropDefault", 0)
            if n == "look":
                v = look
            if typ in ("OfxParamTypeChoice", "OfxParamTypeInteger"):
                C.cast(ptr, C.POINTER(C.c_int))[0] = int(v)
            else:
                C.cast(ptr, C.POINTER(C.c_double))[0] = float(v)
            return 0
    return 9

pcb = [F_PDEF(ok(pdef)), F_PGET(ok(pget)), F_VAL(ok(pval))]


class ParamSuite(C.Structure):
    _fields_ = [("a", F_PDEF), ("b", F_PGET), ("c", P), ("d", P), ("e", P), ("f", F_VAL)] + [(f"r{i}", P) for i in range(12)]

psuite = ParamSuite(pcb[0], pcb[1], None, None, None, pcb[2])

# ---- host
F_FETCH = C.CFUNCTYPE(P, P, C.c_char_p, C.c_int)
suites = {b"OfxPropertySuite": props, b"OfxImageEffectSuite": esuite, b"OfxParameterSuite": psuite}
fetch = F_FETCH(lambda h, name, v: C.addressof(suites[name]) if name in suites else None)


class Host(C.Structure):
    _fields_ = [("host", P), ("fetch", F_FETCH)]

host = Host(new_set(), fetch)

F_SETHOST = C.CFUNCTYPE(None, C.POINTER(Host))
F_MAIN = C.CFUNCTYPE(ST, C.c_char_p, P, P, P)


class Plugin(C.Structure):
    _fields_ = [("api", C.c_char_p), ("ver", C.c_int), ("id", C.c_char_p), ("maj", C.c_uint), ("min", C.c_uint),
                ("set_host", F_SETHOST), ("main", F_MAIN)]

lib = C.CDLL(dll_path)
lib.OfxGetNumberOfPlugins.restype = C.c_int
lib.OfxGetPlugin.restype = C.POINTER(Plugin)
print("plugins:", lib.OfxGetNumberOfPlugins())
pl = lib.OfxGetPlugin(0).contents
print("api:", pl.api.decode(), "id:", pl.id.decode(), f"v{pl.maj}.{pl.min}")
pl.set_host(C.byref(host))


def act(name, handle=None, inargs=None):
    r = pl.main(name.encode(), handle, inargs, None)
    print(f"{name}: {r}")
    return r

assert act("OfxActionLoad") == 0
assert act("OfxActionDescribe", effect) == 0
print("label:", get(effect, "OfxPropLabel", 0), "| grouping:", get(effect, "OfxImageEffectPluginPropGrouping", 0))
ctx = new_set()
put(ctx, "OfxImageEffectPropContext", 0, "OfxImageEffectContextFilter")
assert act("OfxImageEffectActionDescribeInContext", effect, ctx) == 0
print("clips:", list(clips), "| params:", list(params))
print("look options:", [store[params['look'][0]]["OfxParamPropChoiceOption"][i] for i in range(6)])
assert act("OfxActionCreateInstance", effect) == 0
ra = new_set()
put(ra, "OfxPropTime", 0, 0.0)
for i, v in enumerate([0, 0, W, H]):
    put(ra, "OfxImageEffectPropRenderWindow", i, v)
assert act("OfxImageEffectActionRender", effect, ra) == 0
diff = sum(abs(dst_buf[i] - src_buf[i]) for i in range(0, W * H * 4, 4)) / (W * H)
finite = all(math.isfinite(dst_buf[i]) for i in range(W * H * 4))
alpha = min(dst_buf[i] for i in range(3, W * H * 4, 4))
print(f"render: mean change {diff:.4f}, all finite {finite}, min alpha {alpha}")
if len(sys.argv) > 3:   # write a PPM to look at
    with open(sys.argv[3], "wb") as f:
        f.write(f"P6 {W} {H} 255\n".encode())
        for y in range(H - 1, -1, -1):
            for x in range(W):
                i = (y * W + x) * 4
                f.write(bytes(max(0, min(255, int(dst_buf[i + c] * 255))) for c in range(3)))
assert diff > 0.001 and finite
print("OK")
