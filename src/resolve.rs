//! Writes the DaVinci Resolve versions of the effect:
//! - GlassLens.fuse: a Fusion tool with a GPU kernel (free Resolve and Studio,
//!   Fusion page under Fuses > Lens).
//! - Edit-page effect presets, one per look, which wrap the fuse so each look can be
//!   dragged onto a clip on the Edit or Cut page (Effects > Fusion Effects).
//! - GlassLens.dctl: the same maths as a DCTL for the Color page (Resolve Studio's
//!   DCTL effect, or the LUT list).

use crate::glass::{Params, LOOKS};
use std::path::{Path, PathBuf};

/// The glass maths in DaVinci's kernel language. Mirrors glass::glass_map exactly.
const CORE: &str = r#"
__DEVICE__ float mg_smooth(float a, float b, float x) {
  float t = (x - a) / (b - a);
  t = _fminf(_fmaxf(t, 0.0f), 1.0f);
  return t * t * (3.0f - 2.0f * t);
}
__DEVICE__ float mg_hash(float n, float seed) {
  float s = _sinf(n * 3.731f + seed * 1.917f) * 143.758f;
  return s - _floor(s);
}
__DEVICE__ float mg_mod(float x, float y) { return x - y * _floor(x / y); }

__DEVICE__ float4 mg_map(int effect, float px, float py, float amount, float size, float softness, float facets, float seed, float band)
{
  float dx = 0.0f; float dy = 0.0f; float smear = 0.0f; float edge = 0.0f;
  float r = _sqrtf(px * px + py * py);
  if (effect == 0) {
    float R = 0.05f + size * 0.6f;
    float t = mg_smooth(R, R + 0.18f + softness * 0.3f + band, r);
    t = t * t;
    float ring = _sinf(r * 34.0f + seed);
    float k = -amount * t * (0.20f + 0.07f * ring);
    dx = px * k; dy = py * k;
    smear = amount * t * (0.015f + softness * 0.09f);
    edge = t * mg_smooth(0.55f, 1.0f, ring) * 0.7f;
  } else if (effect == 1) {
    float R = 0.05f + size * 0.6f;
    float t = mg_smooth(R, R + 0.25f + softness * 0.45f + band, r);
    float k = amount * t * t * 0.28f;
    dx = -px * k; dy = -py * k;
    smear = amount * t * t * (0.02f + softness * 0.12f);
    edge = 0.0f;
  } else if (effect == 2) {
    float n = _floor(_fmaxf(facets, 3.0f));
    float seg = 6.2831853f / n;
    float a = _atan2f(py, px) + 3.1415927f;
    float k = _floor(a / seg);
    float la = a - (k + 0.5f) * seg;
    float pd = r * _cosf(la);
    float R = 0.08f + size * 0.45f;
    float R2 = R * 2.1f;
    float w = 0.004f + softness * 0.02f;
    float bd = r * _sinf(seg * 0.5f - _fabs(la));
    if (pd < R) {
      edge = 1.0f - mg_smooth(0.0f, w, R - pd);
    } else {
      float ringi = (pd < R2) ? 1.0f : 2.0f;
      float id = k + ringi * 17.0f;
      float na = (k + 0.5f) * seg - 3.1415927f;
      float nx = _cosf(na); float ny = _sinf(na);
      float h1 = mg_hash(id, seed); float h2 = mg_hash(id + 3.1f, seed);
      float push = -amount * (0.10f + 0.16f * h1) * ringi;
      float side = amount * (h2 - 0.5f) * 0.16f;
      dx = nx * push - ny * side;
      dy = ny * push + nx * side;
      float de = _fminf(bd, _fminf(pd - R, _fabs(pd - R2)));
      edge = 1.0f - mg_smooth(0.0f, w, de);
      smear = amount * softness * 0.03f;
    }
  } else if (effect == 3) {
    float sc = 0.10f + size * 0.45f;
    float hx = px / sc; float hy = py / sc;
    float sx = 1.0f; float sy = 1.7320508f;
    float ax = mg_mod(hx, sx) - 0.5f * sx; float ay = mg_mod(hy, sy) - 0.5f * sy;
    float bx = mg_mod(hx - 0.5f * sx, sx) - 0.5f * sx; float by = mg_mod(hy - 0.5f * sy, sy) - 0.5f * sy;
    float gx = bx; float gy = by;
    if (ax * ax + ay * ay < bx * bx + by * by) { gx = ax; gy = ay; }
    float ccx = (hx - gx) * sc; float ccy = (hy - gy) * sc;
    float id = _floor(hx - gx + 0.5f) * 7.0f + _floor((hy - gy) / sy * 2.0f + 0.5f) * 13.0f;
    float cl = _fminf(_sqrtf(ccx * ccx + ccy * ccy) / sc, 1.0f);
    float h1 = mg_hash(id, seed); float h2 = mg_hash(id + 5.3f, seed);
    dx = -ccx * amount + (h1 - 0.5f) * 0.06f * amount * cl;
    dy = -ccy * amount + (h2 - 0.5f) * 0.06f * amount * cl;
    float hd = _fmaxf(_fabs(gx), 0.5f * _fabs(gx) + 0.8660254f * _fabs(gy));
    float w = 0.004f + softness * 0.02f;
    edge = 1.0f - mg_smooth(0.0f, w, (0.5f - hd) * sc);
    smear = amount * softness * 0.03f;
  } else if (effect == 4) {
    float L = (size - 0.5f) * 0.8f;
    float sgn = (facets >= 4.0f && py < 0.0f) ? -1.0f : 1.0f;
    float yy = py * sgn;
    float dist = yy - L;
    float w = 0.003f + softness * 0.03f;
    edge = 1.0f - mg_smooth(0.0f, w, _fabs(dist));
    if (dist > 0.0f) {
      dy = -2.0f * dist * amount * sgn;
      dx = amount * 0.06f;
      smear = amount * softness * 0.06f;
    }
  } else {
    float w = 0.02f + size * 0.12f;
    float xl = px / w;
    float f = xl - _floor(xl);
    dx = (f - 0.5f) * w * amount * 2.5f;
    float de = _fminf(f, 1.0f - f) * w;
    edge = (1.0f - mg_smooth(0.0f, 0.002f + softness * 0.01f, de)) * 0.6f;
    smear = amount * softness * 0.04f;
  }
  return MKF4(dx, dy, smear, edge);
}

__DEVICE__ float mg_edist(int effect, float px, float py, float size, float facets)
{
  float r = _sqrtf(px * px + py * py);
  if (effect == 0 || effect == 1) return _fabs(r - (0.05f + size * 0.6f));
  if (effect == 2) {
    float n = _floor(_fmaxf(facets, 3.0f));
    float seg = 6.2831853f / n;
    float a = _atan2f(py, px) + 3.1415927f;
    float k = _floor(a / seg);
    float la = a - (k + 0.5f) * seg;
    float pd = r * _cosf(la);
    float R = 0.08f + size * 0.45f;
    float R2 = R * 2.1f;
    if (pd < R) return R - pd;
    float bd = r * _sinf(seg * 0.5f - _fabs(la));
    return _fminf(bd, _fminf(pd - R, _fabs(pd - R2)));
  }
  if (effect == 3) {
    float sc = 0.10f + size * 0.45f;
    float hx = px / sc; float hy = py / sc;
    float sx = 1.0f; float sy = 1.7320508f;
    float ax = mg_mod(hx, sx) - 0.5f * sx; float ay = mg_mod(hy, sy) - 0.5f * sy;
    float bx = mg_mod(hx - 0.5f * sx, sx) - 0.5f * sx; float by = mg_mod(hy - 0.5f * sy, sy) - 0.5f * sy;
    float gx = bx; float gy = by;
    if (ax * ax + ay * ay < bx * bx + by * by) { gx = ax; gy = ay; }
    float hd = _fmaxf(_fabs(gx), 0.5f * _fabs(gx) + 0.8660254f * _fabs(gy));
    return (0.5f - hd) * sc;
  }
  if (effect == 4) {
    float L = (size - 0.5f) * 0.8f;
    float yy = (facets >= 4.0f) ? _fabs(py) : py;
    return _fabs(yy - L);
  }
  float w = 0.02f + size * 0.12f;
  float xl = px / w;
  float f = xl - _floor(xl);
  return _fminf(f, 1.0f - f) * w;
}
"#;

/// Per-pixel body. Expects u, v (top-down, 0..1), W, H, the parameters by name,
/// and a SAMPLE_CH(u, v, channel) macro. Leaves the result in out0..out2.
const BODY: &str = r#"
  float aspect = W / H;
  float rot = rotation * 0.017453293f;
  float cr = _cosf(rot); float sr = _sinf(rot);
  float px0 = (u - cx) * aspect; float py0 = v - cy;
  float lx = px0 * cr + py0 * sr;
  float ly = -px0 * sr + py0 * cr;
  float es = edge_soft * 0.03f;
  float band = (effect <= 1) ? (bevel * 0.25f + es * 3.0f) : 0.0f;
  float4 m = mg_map(effect, lx, ly, amount, size, softness, (float)facets, variation, band);
  float dx = m.x; float dy = m.y; float smear = m.z; float edge = m.w;
  if (es > 0.0001f) {
    for (int k = 0; k < 6; k++) {
      float a = (float)k * 1.0471976f + 0.5235988f;
      float4 mm = mg_map(effect, lx + es * _cosf(a), ly + es * _sinf(a), amount, size, softness, (float)facets, variation, band);
      dx += mm.x; dy += mm.y; smear += mm.z; edge += mm.w;
    }
    dx = dx / 7.0f; dy = dy / 7.0f; smear = smear / 7.0f; edge = edge / 7.0f;
  }
  float bw = (effect >= 2 && bevel > 0.0f) ? (bevel * 0.05f + es * 0.6f) : 0.0f;
  float slope = 0.0f; float litv = 0.0f;
  if (bw > 0.0001f) {
    float fc = (float)facets;
    float e0 = mg_edist(effect, lx, ly, size, fc);
    if (e0 < bw) {
      float ep = 0.0015f;
      float gx = mg_edist(effect, lx + ep, ly, size, fc) - mg_edist(effect, lx - ep, ly, size, fc);
      float gy = mg_edist(effect, lx, ly + ep, size, fc) - mg_edist(effect, lx, ly - ep, size, fc);
      float gl = _sqrtf(gx * gx + gy * gy);
      if (gl > 0.000001f) {
        float nx = gx / gl; float ny = gy / gl;
        float s = e0 / bw;
        slope = (1.0f - s) * (1.0f - s);
        dx += nx * slope * bw * 1.6f;
        dy += ny * slope * bw * 1.6f;
        smear += slope * bw * 0.4f;
        float wnx = nx * cr - ny * sr;
        float wny = nx * sr + ny * cr;
        litv = (wnx * 0.6f + wny * 0.8f) * slope;
      }
    }
  }
  float rr = _sqrtf(lx * lx + ly * ly) + 0.00001f;
  float dirx = 1.0f; float diry = 0.0f;
  if (effect == 0) { dirx = -ly / rr; diry = lx / rr; }
  else if (effect == 1) { dirx = lx / rr; diry = ly / rr; }
  else if (effect != 5) {
    float dl = _sqrtf(dx * dx + dy * dy);
    if (dl > 0.00001f) { dirx = dx / dl; diry = dy / dl; }
  }
  float acc0 = 0.0f; float acc1 = 0.0f; float acc2 = 0.0f;
  float ws0 = 0.0f; float ws1 = 0.0f; float ws2 = 0.0f;
  for (int i = 0; i < 7; i++) {
    float t = (float)i / 6.0f;
    float o = ((float)((i * 3) % 7) / 6.0f - 0.5f) * smear;
    float f = 1.0f + dispersion * 0.16f * (1.0f - 2.0f * t);
    float sx = lx + dx * f + dirx * o;
    float sy = ly + dy * f + diry * o;
    float wx = sx * cr - sy * sr;
    float wy = sx * sr + sy * cr;
    float su = cx + wx / aspect;
    float sv = cy + wy;
    float s0 = 0.0f; float s1 = 0.0f; float s2 = 0.0f;
    SAMPLE3(su, sv, s0, s1, s2);
    float w0 = 1.0f - t; float w1 = 1.0f - _fabs(2.0f * t - 1.0f); float w2 = t;
    acc0 += w0 * s0; acc1 += w1 * s1; acc2 += w2 * s2;
    ws0 += w0; ws1 += w1; ws2 += w2;
  }
  acc0 = acc0 / ws0; acc1 = acc1 / ws1; acc2 = acc2 / ws2;
  float luma = 0.2126f * acc0 + 0.7152f * acc1 + 0.0722f * acc2;
  float hue = rr * 2.5f + _atan2f(ly, lx) * 0.159155f + variation * 0.1f;
  float add = edge * (1.0f - 0.85f * _fminf(bevel, 1.0f)) * edge_light * (0.06f + 0.9f * luma);
  acc0 += add * (0.75f + 0.25f * _cosf(6.2831853f * hue));
  acc1 += add * (0.75f + 0.25f * _cosf(6.2831853f * (hue + 0.333f)));
  acc2 += add * (0.75f + 0.25f * _cosf(6.2831853f * (hue + 0.667f)));
  float lit = _fmaxf(litv, 0.0f);
  float spec = lit * lit * lit * edge_light * 1.2f * (0.25f + luma);
  float lift = 1.0f + (0.5f + edge_light) * 0.5f * lit;
  acc0 = acc0 * lift + spec; acc1 = acc1 * lift + spec; acc2 = acc2 * lift + spec;
  float o0 = 0.0f; float o1 = 0.0f; float o2 = 0.0f;
  SAMPLE3(u, v, o0, o1, o2);
  float out0 = o0 + (acc0 - o0) * mixv;
  float out1 = o1 + (acc1 - o1) * mixv;
  float out2 = o2 + (acc2 - o2) * mixv;
"#;

pub fn fuse_source() -> String {
    let kernel = format!(
        r#"
#define MKF4(a, b, c, d) to_float4(a, b, c, d)
#define SAMPLE3(U, V, A, B, C) {{ float4 _s = _tex2DVecN(src, (U), 1.0f - (V), 15); A = _s.x; B = _s.y; C = _s.z; }}
{CORE}
__KERNEL__ void GlassLensKernel(__CONSTANTREF__ GlassLensParams *params, __TEXTURE2D__ src, __TEXTURE2D_WRITE__ dst)
{{
  DEFINE_KERNEL_ITERATORS_XY(x, y);
  if (x < params->width && y < params->height) {{
    float W = (float)params->width; float H = (float)params->height;
    float u = ((float)x + 0.5f) / W;
    float v = 1.0f - ((float)y + 0.5f) / H;
    int effect = params->effect;
    float amount = params->amount; float size = params->size; float softness = params->softness;
    float dispersion = params->dispersion; int facets = params->facets; float rotation = params->rotation;
    float cx = params->cx; float cy = params->cy; float edge_light = params->edge_light; float bevel = params->bevel; float edge_soft = params->edge_soft;
    float variation = params->variation; float mixv = params->mixv;
{BODY}
    float4 alpha = _tex2DVecN(src, u, 1.0f - v, 15);
    _tex2DVec4Write(dst, x, y, to_float4(out0, out1, out2, alpha.w));
  }}
}}
"#
    );

    let combo: String = LOOKS
        .iter()
        .map(|n| format!("    {{ CCS_AddString = \"{n}\", }},\n"))
        .collect();

    format!(
        r#"--[[
  Glass Lens  -  glass in front of the lens for DaVinci Resolve / Fusion.
  Looks: {looks}.
  Written by Glass Lens Lab. Works in free DaVinci Resolve and Studio (GPU kernel).
  Install: copy this file to the Fusion "Fuses" folder and restart Resolve.
]]

FuRegisterClass("GlassLens", CT_Tool, {{
  REGS_Name = "Glass Lens",
  REGS_Category = "Fuses\\Lens",
  REGS_OpIconString = "GLs",
  REGS_OpDescription = "Glass in front of the lens: cone, sphere, facets, kaleidoscope, prism and fluted glass.",
  REG_NoMotionBlurCtrls = true,
  REG_OpNoMask = true,
  REG_SupportsDoD = false,
  REG_Fuse_NoEdit = false,
  REG_Fuse_NoReload = false,
}})

GlassLensParams = [[
  int effect;
  float amount;
  float size;
  float softness;
  float dispersion;
  int facets;
  float rotation;
  float cx;
  float cy;
  float edge_light;
  float edge_soft;
  float bevel;
  float variation;
  float mixv;
  int width;
  int height;
]]

GlassLensKernel = [[
{kernel}
]]

function Create()
  InEffect = self:AddInput("Look", "Effect", {{
    LINKID_DataType = "Number",
    INPID_InputControl = "ComboControl",
    INP_Default = 0.0,
    INP_Integer = true,
    ICD_Width = 1,
{combo}    CC_LabelPosition = "Horizontal",
  }})
  InAmount = self:AddInput("Amount", "Amount", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 0.6, INP_MinScale = 0.0, INP_MaxScale = 1.0, INP_MinAllowed = 0.0, INP_MaxAllowed = 2.0,
  }})
  InSize = self:AddInput("Size", "Size", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 0.4, INP_MinScale = 0.0, INP_MaxScale = 1.0,
  }})
  InSoftness = self:AddInput("Softness", "Softness", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 0.4, INP_MinScale = 0.0, INP_MaxScale = 1.0, INP_MinAllowed = 0.0,
  }})
  InDispersion = self:AddInput("Rainbow Split", "Dispersion", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 0.35, INP_MinScale = 0.0, INP_MaxScale = 1.0, INP_MinAllowed = 0.0,
  }})
  InFacets = self:AddInput("Facets", "Facets", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 6, INP_Integer = true, INP_MinScale = 3, INP_MaxScale = 12, INP_MinAllowed = 3, INP_MaxAllowed = 24,
  }})
  InRotation = self:AddInput("Rotation", "Rotation", {{
    LINKID_DataType = "Number", INPID_InputControl = "ScrewControl",
    INP_Default = 0.0, INP_MinScale = -180.0, INP_MaxScale = 180.0,
  }})
  InCenter = self:AddInput("Center", "Center", {{
    LINKID_DataType = "Point", INPID_InputControl = "OffsetControl", INPID_PreviewControl = "CrosshairControl",
    INP_DefaultX = 0.5, INP_DefaultY = 0.5,
  }})
  InEdgeLight = self:AddInput("Edge Light", "EdgeLight", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 0.3, INP_MinScale = 0.0, INP_MaxScale = 1.0, INP_MinAllowed = 0.0,
  }})
  InEdgeSoft = self:AddInput("Edge Softness", "EdgeSoftness", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 0.25, INP_MinScale = 0.0, INP_MaxScale = 1.0, INP_MinAllowed = 0.0, INP_MaxAllowed = 2.0,
  }})
  InBevel = self:AddInput("Bevel", "Bevel", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 0.4, INP_MinScale = 0.0, INP_MaxScale = 1.0, INP_MinAllowed = 0.0, INP_MaxAllowed = 2.0,
  }})
  InVariation = self:AddInput("Variation", "Variation", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 1.0, INP_MinScale = 0.0, INP_MaxScale = 10.0,
  }})
  InMix = self:AddInput("Mix", "Mix", {{
    LINKID_DataType = "Number", INPID_InputControl = "SliderControl",
    INP_Default = 1.0, INP_MinScale = 0.0, INP_MaxScale = 1.0, INP_MinAllowed = 0.0, INP_MaxAllowed = 1.0,
  }})
  InImage = self:AddInput("Input", "Input", {{
    LINKID_DataType = "Image", LINK_Main = 1,
  }})
  OutImage = self:AddOutput("Output", "Output", {{
    LINKID_DataType = "Image", LINK_Main = 1,
  }})
end

function Process(req)
  local src = InImage:GetValue(req)
  local dst = Image({{ IMG_Like = src, IMG_DeferAlloc = true }})
  local node = DVIPComputeNode(req, "GlassLensKernel", GlassLensKernel, "GlassLensParams", GlassLensParams)
  if not node then
    print("Glass Lens: this computer's GPU kernel could not be prepared; passing the picture through.")
    OutImage:Set(req, src)
    return
  end
  local p = node:GetParamBlock(GlassLensParams)
  p.effect = math.floor(InEffect:GetValue(req).Value + 0.5)
  p.amount = InAmount:GetValue(req).Value
  p.size = InSize:GetValue(req).Value
  p.softness = InSoftness:GetValue(req).Value
  p.dispersion = InDispersion:GetValue(req).Value
  p.facets = math.floor(InFacets:GetValue(req).Value + 0.5)
  p.rotation = InRotation:GetValue(req).Value
  local c = InCenter:GetValue(req)
  p.cx = c.X
  p.cy = 1.0 - c.Y
  p.edge_light = InEdgeLight:GetValue(req).Value
  p.bevel = InBevel:GetValue(req).Value
  p.edge_soft = InEdgeSoft:GetValue(req).Value
  p.variation = InVariation:GetValue(req).Value
  p.mixv = InMix:GetValue(req).Value
  p.width = src.Width
  p.height = src.Height
  node:SetParamBlock(p)
  node:AddSampler("RowSampler", TEX_FILTER_MODE_LINEAR, TEX_ADDRESS_MODE_MIRROR, TEX_NORMALIZED_COORDS_TRUE)
  node:AddInput("src", src)
  node:AddOutput("dst", dst)
  local ok = node:RunSession(req)
  if not ok then
    print("Glass Lens: GPU kernel failed:")
    dump(node:GetErrorLog())
    OutImage:Set(req, src)
    return
  end
  OutImage:Set(req, dst)
end
"#,
        looks = LOOKS.join(", "),
    )
}

pub fn dctl_source(p: &Params) -> String {
    let ids = ["CONE", "SPHERE", "ICOSA", "KALEIDO", "PRISM", "FLUTED"];
    format!(
        r#"// Glass Lens  -  glass in front of the lens (DCTL for DaVinci Resolve's Color page).
// Use with the DCTL effect (ResolveFX Color > DCTL) in Resolve Studio.
// Written by Glass Lens Lab. Same maths as the Glass Lens fuse.

DEFINE_UI_PARAMS(effect, Look, DCTLUI_COMBO_BOX, {eff}, {{ {ids} }}, {{ {names} }})
DEFINE_UI_PARAMS(amount, Amount, DCTLUI_SLIDER_FLOAT, {amount:.3}, 0.0, 1.0, 0.01)
DEFINE_UI_PARAMS(size, Size, DCTLUI_SLIDER_FLOAT, {size:.3}, 0.0, 1.0, 0.01)
DEFINE_UI_PARAMS(softness, Softness, DCTLUI_SLIDER_FLOAT, {soft:.3}, 0.0, 1.0, 0.01)
DEFINE_UI_PARAMS(dispersion, Rainbow Split, DCTLUI_SLIDER_FLOAT, {disp:.3}, 0.0, 1.0, 0.01)
DEFINE_UI_PARAMS(facets, Facets, DCTLUI_SLIDER_INT, {facets}, 3, 12, 1)
DEFINE_UI_PARAMS(rotation, Rotation, DCTLUI_SLIDER_FLOAT, {rot:.1}, -180.0, 180.0, 0.5)
DEFINE_UI_PARAMS(cx, Center X, DCTLUI_SLIDER_FLOAT, {cx:.3}, 0.0, 1.0, 0.005)
DEFINE_UI_PARAMS(cy, Center Y, DCTLUI_SLIDER_FLOAT, {cy:.3}, 0.0, 1.0, 0.005)
DEFINE_UI_PARAMS(edge_light, Edge Light, DCTLUI_SLIDER_FLOAT, {edge:.3}, 0.0, 1.0, 0.01)
DEFINE_UI_PARAMS(edge_soft, Edge Softness, DCTLUI_SLIDER_FLOAT, {esoft:.3}, 0.0, 1.0, 0.01)
DEFINE_UI_PARAMS(bevel, Bevel, DCTLUI_SLIDER_FLOAT, {bevel:.3}, 0.0, 1.0, 0.01)
DEFINE_UI_PARAMS(variation, Variation, DCTLUI_SLIDER_FLOAT, {var:.2}, 0.0, 10.0, 0.1)
DEFINE_UI_PARAMS(mixv, Mix, DCTLUI_SLIDER_FLOAT, {mix:.3}, 0.0, 1.0, 0.01)

#define MKF4(a, b, c, d) make_float4(a, b, c, d)

__DEVICE__ float mg_mirror(float t) {{
  t = _fabs(t);
  t = t - 2.0f * _floor(t * 0.5f);
  return (t > 1.0f) ? (2.0f - t) : t;
}}
__DEVICE__ int mg_clampi(int a, int hi) {{ return a < 0 ? 0 : (a > hi ? hi : a); }}
__DEVICE__ float mg_tex(__TEXTURE__ t, float u, float v, int w, int h) {{
  float x = mg_mirror(u) * (float)w - 0.5f;
  float y = mg_mirror(v) * (float)h - 0.5f;
  float fx0 = _floor(x); float fy0 = _floor(y);
  float fx = x - fx0; float fy = y - fy0;
  int x0 = mg_clampi((int)fx0, w - 1); int x1 = mg_clampi((int)fx0 + 1, w - 1);
  int y0 = mg_clampi((int)fy0, h - 1); int y1 = mg_clampi((int)fy0 + 1, h - 1);
  float a = _tex2D(t, x0, y0); float b = _tex2D(t, x1, y0);
  float c = _tex2D(t, x0, y1); float d = _tex2D(t, x1, y1);
  return (a * (1.0f - fx) + b * fx) * (1.0f - fy) + (c * (1.0f - fx) + d * fx) * fy;
}}
#define SAMPLE3(U, V, A, B, C) {{ A = mg_tex(p_TexR, (U), (V), p_Width, p_Height); B = mg_tex(p_TexG, (U), (V), p_Width, p_Height); C = mg_tex(p_TexB, (U), (V), p_Width, p_Height); }}
{CORE}
__DEVICE__ float3 transform(int p_Width, int p_Height, int p_X, int p_Y, __TEXTURE__ p_TexR, __TEXTURE__ p_TexG, __TEXTURE__ p_TexB)
{{
  float W = (float)p_Width; float H = (float)p_Height;
  float u = ((float)p_X + 0.5f) / W;
  float v = ((float)p_Y + 0.5f) / H;
{BODY}
  return make_float3(out0, out1, out2);
}}
"#,
        eff = p.effect,
        ids = ids.join(", "),
        names = LOOKS.join(", "),
        amount = p.amount,
        size = p.size,
        soft = p.softness,
        disp = p.dispersion,
        facets = p.facets,
        rot = p.rotation,
        cx = p.cx,
        cy = p.cy,
        edge = p.edge_light,
        bevel = p.bevel,
        esoft = p.edge_soft,
        var = p.variation,
        mix = p.mix,
    )
}

/// An Edit-page effect preset: a macro wrapping the fuse with this look's settings.
pub fn template_source(p: &Params) -> String {
    let name = format!("GlassLens_{}", LOOKS[p.effect].replace(' ', ""));
    format!(
        r#"{{
	Tools = ordered() {{
		{name} = MacroOperator {{
			CtrlWZoom = false,
			NameSet = true,
			Inputs = ordered() {{
				MainInput1 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Input", }},
				Input1 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Effect", Default = {eff}, }},
				Input2 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Amount", Default = {amount}, }},
				Input3 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Size", Default = {size}, }},
				Input4 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Softness", Default = {soft}, }},
				Input5 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Dispersion", Default = {disp}, }},
				Input6 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Facets", Default = {facets}, }},
				Input7 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Rotation", Default = {rot}, }},
				Input8 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Center", DefaultX = {cx}, DefaultY = {cy}, }},
				Input9 = InstanceInput {{ SourceOp = "GlassLens1", Source = "EdgeLight", Default = {edge}, }},
				Input10 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Variation", Default = {var}, }},
				Input11 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Mix", Default = {mix}, }},
				Input12 = InstanceInput {{ SourceOp = "GlassLens1", Source = "Bevel", Default = {bevel}, }},
				Input13 = InstanceInput {{ SourceOp = "GlassLens1", Source = "EdgeSoftness", Default = {esoft}, }},
			}},
			Outputs = {{
				MainOutput1 = InstanceOutput {{ SourceOp = "GlassLens1", Source = "Output", }},
			}},
			ViewInfo = GroupInfo {{ Pos = {{ 0, 0 }}, }},
			Tools = ordered() {{
				GlassLens1 = Fuse.GlassLens {{
					Inputs = {{
						Effect = Input {{ Value = {eff}, }},
						Amount = Input {{ Value = {amount}, }},
						Size = Input {{ Value = {size}, }},
						Softness = Input {{ Value = {soft}, }},
						Dispersion = Input {{ Value = {disp}, }},
						Facets = Input {{ Value = {facets}, }},
						Rotation = Input {{ Value = {rot}, }},
						Center = Input {{ Value = {{ {cx}, {cy} }}, }},
						EdgeLight = Input {{ Value = {edge}, }},
						Bevel = Input {{ Value = {bevel}, }},
						EdgeSoftness = Input {{ Value = {esoft}, }},
						Variation = Input {{ Value = {var}, }},
						Mix = Input {{ Value = {mix}, }},
					}},
					ViewInfo = OperatorInfo {{ Pos = {{ 0, 0 }}, }},
				}},
			}},
		}},
	}},
	ActiveTool = "{name}",
}}
"#,
        eff = p.effect,
        amount = num(p.amount),
        size = num(p.size),
        soft = num(p.softness),
        disp = num(p.dispersion),
        facets = p.facets,
        rot = num(p.rotation),
        cx = num(p.cx),
        cy = num(1.0 - p.cy),
        edge = num(p.edge_light),
        bevel = num(p.bevel),
        esoft = num(p.edge_soft),
        var = num(p.variation),
        mix = num(p.mix),
    )
}

fn num(v: f32) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s
    }
}

pub fn template_file_name(effect: usize) -> String {
    format!("Glass Lens - {}.setting", LOOKS[effect])
}

const README: &str = "Glass Lens for DaVinci Resolve
==============================

GlassLens.ofx.bundle   (the effect - free DaVinci Resolve and Studio)
  An OpenFX plug-in. Copy the whole GlassLens.ofx.bundle folder to:
    Windows: C:\\Program Files\\Common Files\\OFX\\Plugins
    macOS:   /Library/OFX/Plugins
    Linux:   /usr/OFX/Plugins
  Restart DaVinci Resolve. Then:
    Edit / Cut page: Effects > OpenFX > Filters > Glass Lens - drag it onto a clip.
    Color page:      OpenFX panel > Glass Lens - drag it onto a node.
  Change Look, Amount, Size, Bevel and the rest in the Inspector / Settings panel.
  Picking a new Look loads that look's starting settings.

GlassLens.dctl   (optional, Resolve Studio, Color page, runs on the GPU)
  Copy to C:\\ProgramData\\Blackmagic Design\\DaVinci Resolve\\Support\\LUT (Windows)
  or /Library/Application Support/Blackmagic Design/DaVinci Resolve/LUT (macOS),
  then add ResolveFX Color > DCTL to a node and choose GlassLens.

Fusion (optional) folder
  The same effect as a Fusion tool, only if you also want it on the Fusion page.
";

fn ofx_plugins_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        let pf = std::env::var_os("CommonProgramFiles").unwrap_or_else(|| "C:\\Program Files\\Common Files".into());
        PathBuf::from(pf).join("OFX").join("Plugins")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Library/OFX/Plugins")
    } else {
        PathBuf::from("/usr/OFX/Plugins")
    }
}

fn ofx_arch() -> &'static str {
    if cfg!(target_os = "windows") {
        "Win64"
    } else if cfg!(target_os = "macos") {
        "MacOS"
    } else {
        "Linux-x86-64"
    }
}

/// The plug-in library cargo builds next to this program.
fn plugin_library() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") {
        "glass_lens.dll"
    } else if cfg!(target_os = "macos") {
        "libglass_lens.dylib"
    } else {
        "libglass_lens.so"
    };
    let p = std::env::current_exe().ok()?.parent()?.join(name);
    p.exists().then_some(p)
}

fn bundle_file(root: &Path) -> PathBuf {
    root.join("GlassLens.ofx.bundle").join("Contents").join(ofx_arch()).join("GlassLens.ofx")
}

/// Makes GlassLens.ofx.bundle inside `dir`; returns the plug-in file inside it.
fn stage_bundle(dir: &Path) -> std::io::Result<PathBuf> {
    let lib = plugin_library().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "the plug-in library wasn't built next to this program")
    })?;
    let file = bundle_file(dir);
    std::fs::create_dir_all(file.parent().unwrap())?;
    std::fs::copy(&lib, &file)?;
    Ok(file)
}

/// Writes every DaVinci file into one folder (the "save to folder" button).
pub fn write_bundle(dir: &Path, looks: &[Params]) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    match stage_bundle(dir) {
        Ok(f) => written.push(f),
        Err(e) => eprintln!("OpenFX plug-in not written: {e}"),
    }
    let mut put = |sub: &Path, name: &str, body: String| -> std::io::Result<()> {
        std::fs::create_dir_all(sub)?;
        let p = sub.join(name);
        std::fs::write(&p, body)?;
        written.push(p);
        Ok(())
    };
    put(dir, "GlassLens.dctl", dctl_source(&looks[0]))?;
    put(dir, "README.txt", README.to_string())?;
    let fusion = dir.join("Fusion (optional)");
    put(&fusion, "GlassLens.fuse", fuse_source())?;
    for p in looks {
        put(&fusion, &template_file_name(p.effect), template_source(p))?;
    }
    Ok(written)
}

/// Whether the effect is in DaVinci Resolve's OpenFX folder, and whether it's this version.
pub fn install_state() -> (bool, &'static str) {
    let target = bundle_file(&ofx_plugins_dir());
    match plugin_library() {
        None => (false, "The plug-in wasn't built next to this program."),
        Some(_) if !target.exists() => (false, "Not installed yet: close DaVinci Resolve, then click Install and answer Yes."),
        Some(lib) if same_file(&lib, &target) => (true, "Installed and up to date. Restart DaVinci Resolve if it was open."),
        Some(_) => (false, "An older Glass Lens is installed: close DaVinci Resolve and click Install to update it."),
    }
}

pub struct InstallReport {
    pub lines: Vec<String>,
}

fn user_fusion_dirs() -> Option<(PathBuf, PathBuf)> {
    let fusion = if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA")?).join("Blackmagic Design").join("DaVinci Resolve").join("Support").join("Fusion")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Blackmagic Design/DaVinci Resolve/Fusion")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".local/share/DaVinciResolve/Fusion")
    };
    Some((fusion.join("Fuses"), fusion.join("Templates").join("Edit").join("Effects")))
}

fn lut_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        let prog = std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into());
        PathBuf::from(prog).join("Blackmagic Design").join("DaVinci Resolve").join("Support").join("LUT")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/Blackmagic Design/DaVinci Resolve/LUT")
    } else {
        PathBuf::from("/opt/resolve/LUT")
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

/// PowerShell's -EncodedCommand wants UTF-16LE in base64.
fn ps_encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    base64(&bytes)
}

fn ps_quote(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "''"))
}

/// Copies files into protected folders by asking Windows for permission (one prompt).
#[cfg(target_os = "windows")]
fn copy_elevated(pairs: &[(PathBuf, PathBuf)]) -> bool {
    use std::os::windows::process::CommandExt;
    let mut inner = String::from("$ErrorActionPreference = 'Continue'\n");
    for (from, to) in pairs {
        if let Some(parent) = to.parent() {
            inner += &format!("New-Item -ItemType Directory -Force -Path {} | Out-Null\n", ps_quote(parent));
        }
        inner += &format!("Copy-Item -Force -LiteralPath {} -Destination {}\n", ps_quote(from), ps_quote(to));
    }
    let outer = format!(
        "try {{ Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-EncodedCommand','{}') }} catch {{ exit 1 }}",
        ps_encoded(&inner)
    );
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &ps_encoded(&outer)])
        .creation_flags(0x0800_0000)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn copy_elevated(_pairs: &[(PathBuf, PathBuf)]) -> bool {
    false
}

/// Installs the OpenFX plug-in (Edit/Cut page effect and Color page node) and the DCTL,
/// and removes the Fusion-page versions an earlier install put in place.
pub fn install(looks: &[Params]) -> InstallReport {
    let mut lines = Vec::new();

    // the person doesn't want the Fusion versions: take away what an earlier install added
    if let Some((fuses, templates)) = user_fusion_dirs() {
        let mut removed = 0;
        let mut old: Vec<PathBuf> = vec![fuses.join("GlassLens.fuse")];
        old.extend((0..LOOKS.len()).map(|k| templates.join(template_file_name(k))));
        for f in old {
            if f.exists() && std::fs::remove_file(&f).is_ok() {
                removed += 1;
            }
        }
        if removed > 0 {
            lines.push(format!("Removed {removed} old Fusion-effect files from an earlier install."));
        }
    }

    let stage = std::env::temp_dir().join("GlassLensInstall");
    let staged = match stage_bundle(&stage) {
        Ok(f) => f,
        Err(e) => {
            lines.push(format!("Couldn't prepare the plug-in: {e}"));
            return InstallReport { lines };
        }
    };
    let staged_dctl = stage.join("GlassLens.dctl");
    let _ = std::fs::write(&staged_dctl, dctl_source(&looks[0]));

    let target = bundle_file(&ofx_plugins_dir());
    let target_dctl = lut_dir().join("GlassLens.dctl");
    let pairs = vec![(staged.clone(), target.clone()), (staged_dctl.clone(), target_dctl.clone())];

    // straight copy first (works when allowed), otherwise one permission prompt
    let mut direct = true;
    for (from, to) in &pairs {
        let ok = to.parent().map(|d| std::fs::create_dir_all(d).is_ok()).unwrap_or(false) && std::fs::copy(from, to).is_ok();
        direct &= ok;
    }
    if !direct {
        lines.push("Asked Windows for permission to copy into the shared OpenFX folder.".into());
        copy_elevated(&pairs);
    }

    if same_file(&staged, &target) {
        lines.push("Installed the Glass Lens effect.".into());
        lines.push("Edit page: Effects > OpenFX > Filters > Glass Lens - drag onto a clip.  Color page: OpenFX > Glass Lens - drag onto a node.".into());
    } else {
        lines.push(
            "The effect couldn't be copied. Close DaVinci Resolve (it locks the old plug-in), click Install again and answer Yes to the Windows prompt."
                .into(),
        );
    }
    if same_file(&staged_dctl, &target_dctl) {
        lines.push("Also installed the DCTL version (Resolve Studio: Color page > DCTL > GlassLens).".into());
    }
    lines.push("Restart DaVinci Resolve to see it.".into());
    InstallReport { lines }
}
