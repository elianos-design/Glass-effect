//! The glass effects, computed on the CPU for the preview.
//! This is a line-by-line twin of the GPU kernel written to DaVinci Resolve
//! (see resolve.rs), so what you see here is what Resolve renders.

pub const LOOKS: [&str; 6] = [
    "Circular Cone",
    "Circular Spherical",
    "Icosahedron",
    "Kaleidoscope",
    "Prism",
    "Fluted Glass",
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Params {
    pub effect: usize,
    pub amount: f32,
    pub size: f32,
    pub softness: f32,
    pub dispersion: f32,
    pub facets: i32,
    pub rotation: f32,
    pub cx: f32,
    pub cy: f32,
    pub edge_light: f32,
    pub edge_soft: f32,
    pub bevel: f32,
    pub variation: f32,
    pub mix: f32,
}

impl Params {
    pub fn preset(effect: usize) -> Params {
        let base = Params {
            effect,
            amount: 0.6,
            size: 0.4,
            softness: 0.4,
            dispersion: 0.35,
            facets: 6,
            rotation: 0.0,
            cx: 0.5,
            cy: 0.5,
            edge_light: 0.3,
            edge_soft: 0.25,
            bevel: 0.4,
            variation: 1.0,
            mix: 1.0,
        };
        match effect {
            0 => Params { amount: 0.75, size: 0.55, softness: 0.55, dispersion: 0.45, edge_light: 0.35, bevel: 0.3, ..base },
            1 => Params { amount: 0.6, size: 0.6, softness: 0.7, dispersion: 0.2, edge_light: 0.1, bevel: 0.15, ..base },
            2 => Params { amount: 0.55, size: 0.62, softness: 0.25, dispersion: 0.5, facets: 7, rotation: 12.0, edge_light: 0.4, bevel: 0.5, variation: 2.0, ..base },
            3 => Params { amount: 0.5, size: 0.62, softness: 0.2, dispersion: 0.4, rotation: 18.0, edge_light: 0.4, bevel: 0.45, cx: 0.47, cy: 0.42, ..base },
            4 => Params { amount: 0.8, size: 0.72, softness: 0.25, dispersion: 0.6, facets: 3, rotation: -12.0, edge_light: 0.5, bevel: 0.45, ..base },
            _ => Params { amount: 0.55, size: 0.3, softness: 0.3, dispersion: 0.3, edge_light: 0.3, bevel: 0.6, ..base },
        }
    }
}

#[derive(Clone)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 4]>,
}

impl Img {
    pub fn new(w: usize, h: usize) -> Img {
        Img { w, h, px: vec![[0.0, 0.0, 0.0, 1.0]; w * h] }
    }

    /// Box-filtered downscale so it fits in max_w x max_h.
    pub fn fit(&self, max_w: usize, max_h: usize) -> Img {
        let s = (max_w as f32 / self.w as f32).min(max_h as f32 / self.h as f32).min(1.0);
        let nw = ((self.w as f32 * s).round() as usize).max(1);
        let nh = ((self.h as f32 * s).round() as usize).max(1);
        if nw == self.w && nh == self.h {
            return self.clone();
        }
        let mut out = Img::new(nw, nh);
        for y in 0..nh {
            let y0 = y * self.h / nh;
            let y1 = ((y + 1) * self.h / nh).max(y0 + 1);
            for x in 0..nw {
                let x0 = x * self.w / nw;
                let x1 = ((x + 1) * self.w / nw).max(x0 + 1);
                let mut acc = [0.0f32; 4];
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        let p = self.px[yy * self.w + xx];
                        for c in 0..4 {
                            acc[c] += p[c];
                        }
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)) as f32;
                out.px[y * nw + x] = [acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n];
            }
        }
        out
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.w * self.h * 4);
        for p in &self.px {
            for c in 0..3 {
                v.push((p[c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            }
            v.push(255);
        }
        v
    }

    pub fn from_rgba8(w: usize, h: usize, data: &[u8]) -> Img {
        let mut img = Img::new(w, h);
        for i in 0..w * h {
            img.px[i] = [
                data[i * 4] as f32 / 255.0,
                data[i * 4 + 1] as f32 / 255.0,
                data[i * 4 + 2] as f32 / 255.0,
                1.0,
            ];
        }
        img
    }
}

// ---- helpers (same as the kernel) ----
fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn hash(n: f32, seed: f32) -> f32 {
    let s = (n * 3.731 + seed * 1.917).sin() * 143.758;
    s - s.floor()
}
fn fmod(x: f32, y: f32) -> f32 {
    x - y * (x / y).floor()
}
fn mirror(t: f32) -> f32 {
    let t = t.abs();
    let t = t - 2.0 * (t * 0.5).floor();
    if t > 1.0 {
        2.0 - t
    } else {
        t
    }
}

/// Returns (dx, dy, smear, edge) in aspect-corrected, rotated space.
/// `band` widens the round looks' gradual start of the glass (from Bevel and Edge softness).
#[allow(clippy::too_many_arguments)]
pub fn glass_map(effect: usize, px: f32, py: f32, amount: f32, size: f32, softness: f32, facets: f32, seed: f32, band: f32) -> [f32; 4] {
    let (mut dx, mut dy, mut smear) = (0.0f32, 0.0f32, 0.0f32);
    let edge: f32;
    let r = (px * px + py * py).sqrt();
    match effect {
        0 => {
            // Circular cone: rings of glass around a clear centre. The glass thickens
            // gradually (t squared), so the clear centre flows into it with no rim.
            let rr = 0.05 + size * 0.6;
            let t = smooth(rr, rr + 0.18 + softness * 0.3 + band, r);
            let t = t * t;
            let ring = (r * 34.0 + seed).sin();
            let k = -amount * t * (0.20 + 0.07 * ring);
            dx = px * k;
            dy = py * k;
            smear = amount * t * (0.015 + softness * 0.09);
            edge = t * smooth(0.55, 1.0, ring) * 0.7;
        }
        1 => {
            // Circular spherical: soft magnified, smeared rim
            let rr = 0.05 + size * 0.6;
            let t = smooth(rr, rr + 0.25 + softness * 0.45 + band, r);
            let k = amount * t * t * 0.28;
            dx = -px * k;
            dy = -py * k;
            smear = amount * t * t * (0.02 + softness * 0.12);
            edge = 0.0;
        }
        2 => {
            // Icosahedron: clear centre facet with two rings of cut facets
            let n = facets.max(3.0).floor();
            let seg = 6.283_185_3 / n;
            let a = py.atan2(px) + 3.141_592_7;
            let k = (a / seg).floor();
            let la = a - (k + 0.5) * seg;
            let pd = r * la.cos();
            let rr = 0.08 + size * 0.45;
            let rr2 = rr * 2.1;
            let w = 0.004 + softness * 0.02;
            let bd = r * (seg * 0.5 - la.abs()).sin();
            if pd < rr {
                edge = 1.0 - smooth(0.0, w, rr - pd);
            } else {
                let ringi = if pd < rr2 { 1.0 } else { 2.0 };
                let id = k + ringi * 17.0;
                let na = (k + 0.5) * seg - 3.141_592_7;
                let (nx, ny) = (na.cos(), na.sin());
                let h1 = hash(id, seed);
                let h2 = hash(id + 3.1, seed);
                let push = -amount * (0.10 + 0.16 * h1) * ringi;
                let side = amount * (h2 - 0.5) * 0.16;
                dx = nx * push - ny * side;
                dy = ny * push + nx * side;
                let de = bd.min((pd - rr).min((pd - rr2).abs()));
                edge = 1.0 - smooth(0.0, w, de);
                smear = amount * softness * 0.03;
            }
        }
        3 => {
            // Kaleidoscope: hexagonal cells, each showing a shifted copy
            let sc = 0.10 + size * 0.45;
            let hx = px / sc;
            let hy = py / sc;
            let (sx, sy) = (1.0f32, 1.732_050_8f32);
            let ax = fmod(hx, sx) - 0.5 * sx;
            let ay = fmod(hy, sy) - 0.5 * sy;
            let bx = fmod(hx - 0.5 * sx, sx) - 0.5 * sx;
            let by = fmod(hy - 0.5 * sy, sy) - 0.5 * sy;
            let (gx, gy) = if ax * ax + ay * ay < bx * bx + by * by { (ax, ay) } else { (bx, by) };
            let ccx = (hx - gx) * sc;
            let ccy = (hy - gy) * sc;
            let id = (hx - gx + 0.5).floor() * 7.0 + ((hy - gy) / sy * 2.0 + 0.5).floor() * 13.0;
            let cl = ((ccx * ccx + ccy * ccy).sqrt() / sc).min(1.0);
            let h1 = hash(id, seed);
            let h2 = hash(id + 5.3, seed);
            dx = -ccx * amount + (h1 - 0.5) * 0.06 * amount * cl;
            dy = -ccy * amount + (h2 - 0.5) * 0.06 * amount * cl;
            let hd = gx.abs().max(0.5 * gx.abs() + 0.866_025_4 * gy.abs());
            let w = 0.004 + softness * 0.02;
            edge = 1.0 - smooth(0.0, w, (0.5 - hd) * sc);
            smear = amount * softness * 0.03;
        }
        4 => {
            // Prism: a glass edge across the frame folding the image back
            let l = (size - 0.5) * 0.8;
            let sgn = if facets >= 4.0 && py < 0.0 { -1.0 } else { 1.0 };
            let y = py * sgn;
            let dist = y - l;
            let w = 0.003 + softness * 0.03;
            edge = 1.0 - smooth(0.0, w, dist.abs());
            if dist > 0.0 {
                dy = -2.0 * dist * amount * sgn;
                dx = amount * 0.06;
                smear = amount * softness * 0.06;
            }
        }
        _ => {
            // Fluted glass: vertical ribs, each a small lens
            let w = 0.02 + size * 0.12;
            let xl = px / w;
            let f = xl - xl.floor();
            dx = (f - 0.5) * w * amount * 2.5;
            let de = f.min(1.0 - f) * w;
            edge = (1.0 - smooth(0.0, 0.002 + softness * 0.01, de)) * 0.6;
            smear = amount * softness * 0.04;
        }
    }
    [dx, dy, smear, edge]
}

/// Distance from a point to the nearest edge of the glass (same space as glass_map).
pub fn glass_edist(effect: usize, px: f32, py: f32, size: f32, facets: f32) -> f32 {
    let r = (px * px + py * py).sqrt();
    match effect {
        0 | 1 => (r - (0.05 + size * 0.6)).abs(),
        2 => {
            let n = facets.max(3.0).floor();
            let seg = 6.283_185_3 / n;
            let a = py.atan2(px) + 3.141_592_7;
            let k = (a / seg).floor();
            let la = a - (k + 0.5) * seg;
            let pd = r * la.cos();
            let rr = 0.08 + size * 0.45;
            let rr2 = rr * 2.1;
            if pd < rr {
                rr - pd
            } else {
                let bd = r * (seg * 0.5 - la.abs()).sin();
                bd.min((pd - rr).min((pd - rr2).abs()))
            }
        }
        3 => {
            let sc = 0.10 + size * 0.45;
            let hx = px / sc;
            let hy = py / sc;
            let (sx, sy) = (1.0f32, 1.732_050_8f32);
            let ax = fmod(hx, sx) - 0.5 * sx;
            let ay = fmod(hy, sy) - 0.5 * sy;
            let bx = fmod(hx - 0.5 * sx, sx) - 0.5 * sx;
            let by = fmod(hy - 0.5 * sy, sy) - 0.5 * sy;
            let (gx, gy) = if ax * ax + ay * ay < bx * bx + by * by { (ax, ay) } else { (bx, by) };
            let hd = gx.abs().max(0.5 * gx.abs() + 0.866_025_4 * gy.abs());
            (0.5 - hd) * sc
        }
        4 => {
            let l = (size - 0.5) * 0.8;
            let y = if facets >= 4.0 { py.abs() } else { py };
            (y - l).abs()
        }
        _ => {
            let w = 0.02 + size * 0.12;
            let xl = px / w;
            let f = xl - xl.floor();
            f.min(1.0 - f) * w
        }
    }
}

fn tex(src: &Img, u: f32, v: f32, c: usize) -> f32 {
    let w = src.w as i32;
    let h = src.h as i32;
    let x = mirror(u) * w as f32 - 0.5;
    let y = mirror(v) * h as f32 - 0.5;
    let fx0 = x.floor();
    let fy0 = y.floor();
    let fx = x - fx0;
    let fy = y - fy0;
    let cl = |a: i32, m: i32| a.clamp(0, m - 1) as usize;
    let x0 = cl(fx0 as i32, w);
    let x1 = cl(fx0 as i32 + 1, w);
    let y0 = cl(fy0 as i32, h);
    let y1 = cl(fy0 as i32 + 1, h);
    let sw = src.w;
    let a = src.px[y0 * sw + x0][c];
    let b = src.px[y0 * sw + x1][c];
    let cc = src.px[y1 * sw + x0][c];
    let d = src.px[y1 * sw + x1][c];
    (a * (1.0 - fx) + b * fx) * (1.0 - fy) + (cc * (1.0 - fx) + d * fx) * fy
}

fn shade(src: &Img, p: &Params, u: f32, v: f32, aspect: f32) -> [f32; 4] {
    let rot = p.rotation * 0.017_453_293;
    let (cr, sr) = (rot.cos(), rot.sin());
    let px0 = (u - p.cx) * aspect;
    let py0 = v - p.cy;
    let lx = px0 * cr + py0 * sr;
    let ly = -px0 * sr + py0 * cr;
    let es = p.edge_soft * 0.03;
    // Round looks have no cut edge: Bevel and Edge softness make the glass begin
    // more gradually instead (wider, smoother start around the clear centre).
    let band = if p.effect <= 1 { p.bevel * 0.25 + es * 3.0 } else { 0.0 };
    let mut m = glass_map(p.effect, lx, ly, p.amount, p.size, p.softness, p.facets as f32, p.variation, band);
    // Edge softness: average the glass over a small ring so the cut between
    // two pieces of glass melts into a smooth blend instead of a hard line.
    if es > 0.0001 {
        for k in 0..6 {
            let a = k as f32 * 1.047_197_6 + 0.523_598_8;
            let mm = glass_map(p.effect, lx + es * a.cos(), ly + es * a.sin(), p.amount, p.size, p.softness, p.facets as f32, p.variation, band);
            for j in 0..4 {
                m[j] += mm[j];
            }
        }
        for v in m.iter_mut() {
            *v /= 7.0;
        }
    }
    let (mut dx, mut dy, mut smear, edge) = (m[0], m[1], m[2], m[3]);
    // Bevel (cut-glass looks): a rounded strip along every glass edge that bends the
    // picture and catches light. It never darkens anything: real glass edges don't cast shadows.
    let bw = if p.effect >= 2 && p.bevel > 0.0 { p.bevel * 0.05 + es * 0.6 } else { 0.0 };
    let mut litv = 0.0f32;
    if bw > 0.0001 {
        let fc = p.facets as f32;
        let e0 = glass_edist(p.effect, lx, ly, p.size, fc);
        if e0 < bw {
            let ep = 0.0015;
            let gx = glass_edist(p.effect, lx + ep, ly, p.size, fc) - glass_edist(p.effect, lx - ep, ly, p.size, fc);
            let gy = glass_edist(p.effect, lx, ly + ep, p.size, fc) - glass_edist(p.effect, lx, ly - ep, p.size, fc);
            let gl = (gx * gx + gy * gy).sqrt();
            if gl > 0.000001 {
                let nx = gx / gl;
                let ny = gy / gl;
                let s = e0 / bw;
                let slope = (1.0 - s) * (1.0 - s);
                dx += nx * slope * bw * 1.6;
                dy += ny * slope * bw * 1.6;
                smear += slope * bw * 0.4;
                let wnx = nx * cr - ny * sr;
                let wny = nx * sr + ny * cr;
                litv = (wnx * 0.6 + wny * 0.8) * slope;
            }
        }
    }
    let rr = (lx * lx + ly * ly).sqrt() + 0.00001;
    let (mut dirx, mut diry) = (1.0f32, 0.0f32);
    if p.effect == 0 {
        dirx = -ly / rr;
        diry = lx / rr;
    } else if p.effect == 1 {
        dirx = lx / rr;
        diry = ly / rr;
    } else if p.effect != 5 {
        let dl = (dx * dx + dy * dy).sqrt();
        if dl > 0.00001 {
            dirx = dx / dl;
            diry = dy / dl;
        }
    }
    // 7 taps: each is one "wavelength" (red..blue) bent a little differently,
    // and one point along the smear, so fringes come out as a smooth rainbow.
    let mut acc = [0.0f32; 3];
    let mut ws = [0.0f32; 3];
    for i in 0..7 {
        let t = i as f32 / 6.0;
        let o = (((i * 3) % 7) as f32 / 6.0 - 0.5) * smear;
        let f = 1.0 + p.dispersion * 0.16 * (1.0 - 2.0 * t);
        let sx = lx + dx * f + dirx * o;
        let sy = ly + dy * f + diry * o;
        let wx = sx * cr - sy * sr;
        let wy = sx * sr + sy * cr;
        let su = p.cx + wx / aspect;
        let sv = p.cy + wy;
        let s = tex3(src, su, sv);
        let w = [1.0 - t, 1.0 - (2.0 * t - 1.0).abs(), t];
        for c in 0..3 {
            acc[c] += w[c] * s[c];
            ws[c] += w[c];
        }
    }
    for c in 0..3 {
        acc[c] /= ws[c];
    }
    let luma = 0.2126 * acc[0] + 0.7152 * acc[1] + 0.0722 * acc[2];
    let hue = rr * 2.5 + ly.atan2(lx) * 0.159_155 + p.variation * 0.1;
    let add = edge * (1.0 - 0.85 * p.bevel.min(1.0)) * p.edge_light * (0.06 + 0.9 * luma);
    acc[0] += add * (0.75 + 0.25 * (6.283_185_3 * hue).cos());
    acc[1] += add * (0.75 + 0.25 * (6.283_185_3 * (hue + 0.333)).cos());
    acc[2] += add * (0.75 + 0.25 * (6.283_185_3 * (hue + 0.667)).cos());
    let lit = litv.max(0.0);
    let spec = lit * lit * lit * p.edge_light * 1.2 * (0.25 + luma);
    let lift = 1.0 + (0.5 + p.edge_light) * 0.5 * lit;
    for a in acc.iter_mut() {
        *a = *a * lift + spec;
    }
    let mut out = [0.0, 0.0, 0.0, 1.0];
    let o = tex3(src, u, v);
    for c in 0..3 {
        out[c] = o[c] + (acc[c] - o[c]) * p.mix;
    }
    out
}

fn tex3(src: &Img, u: f32, v: f32) -> [f32; 3] {
    [tex(src, u, v, 0), tex(src, u, v, 1), tex(src, u, v, 2)]
}

/// Renders the effect over the whole picture, using every CPU core.
pub fn render(src: &Img, p: &Params) -> Img {
    let (w, h) = (src.w, src.h);
    let mut out = Img::new(w, h);
    let aspect = w as f32 / h as f32;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).max(1);
    let rows = h.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for (ci, chunk) in out.px.chunks_mut(rows * w).enumerate() {
            s.spawn(move || {
                for (i, px) in chunk.iter_mut().enumerate() {
                    let x = i % w;
                    let y = ci * rows + i / w;
                    let u = (x as f32 + 0.5) / w as f32;
                    let v = (y as f32 + 0.5) / h as f32;
                    *px = shade(src, p, u, v, aspect);
                }
            });
        }
    });
    out
}
