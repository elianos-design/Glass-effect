//! A built-in sample shot (a warm, dim bar with bokeh lights, a figure and a glass),
//! so the looks can be tried before loading a frame of your own footage.

use glass_lens::glass::Img;

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn rnd(i: u32) -> f32 {
    let mut x = i.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x & 0xffffff) as f32 / 16_777_216.0
}

pub fn sample_scene(w: usize, h: usize) -> Img {
    let mut img = Img::new(w, h);
    let wf = w as f32;
    let hf = h as f32;
    // bokeh lights (x, y, radius, colour) in 0..1 of height
    let mut lights: Vec<(f32, f32, f32, [f32; 3])> = vec![
        (0.31, 0.27, 0.045, [1.0, 0.86, 0.55]),
        (0.36, 0.26, 0.045, [1.0, 0.88, 0.6]),
        (0.66, 0.18, 0.048, [1.0, 0.9, 0.62]),
        (0.73, 0.17, 0.046, [1.0, 0.88, 0.6]),
        (0.73, 0.29, 0.03, [0.95, 0.8, 0.55]),
        (0.09, 0.28, 0.035, [1.0, 0.82, 0.5]),
        (0.13, 0.28, 0.035, [1.0, 0.84, 0.52]),
        (0.71, 0.47, 0.035, [0.25, 0.45, 1.0]),
        (0.27, 0.47, 0.03, [0.9, 0.35, 0.15]),
        (0.86, 0.12, 0.04, [0.85, 0.7, 0.45]),
        (0.93, 0.32, 0.05, [0.35, 0.55, 0.85]),
    ];
    for i in 0..26u32 {
        let x = rnd(i * 3 + 1);
        let y = 0.08 + rnd(i * 3 + 2) * 0.55;
        let r = 0.01 + rnd(i * 3 + 3) * 0.025;
        let warm = [0.9, 0.55 + rnd(i + 99) * 0.2, 0.25];
        lights.push((x, y, r, warm));
    }
    let aspect = wf / hf;
    for y in 0..h {
        for x in 0..w {
            let u = (x as f32 + 0.5) / wf;
            let v = (y as f32 + 0.5) / hf;
            let xa = u * aspect;
            // warm wall gradient with soft shelf bands
            let mut col = lerp3([0.11, 0.06, 0.03], [0.20, 0.11, 0.05], smooth(0.0, 0.55, v));
            let band = ((v * 9.0).sin() * 0.5 + 0.5) * 0.04;
            col = [col[0] + band, col[1] + band * 0.6, col[2] + band * 0.3];
            let side_glow = (1.0 - ((u - 0.62) * 2.2).abs()).max(0.0) * 0.08;
            col = [col[0] + side_glow, col[1] + side_glow * 0.55, col[2] + side_glow * 0.2];
            // bokeh
            for &(lx, ly, lr, lc) in &lights {
                let d = ((xa - lx * aspect).powi(2) + (v - ly).powi(2)).sqrt();
                let disc = 1.0 - smooth(lr * 0.85, lr, d);
                let halo = (1.0 - smooth(0.0, lr * 2.4, d)) * 0.12;
                let a = disc * 0.85 + halo;
                col = [col[0] + lc[0] * a, col[1] + lc[1] * a, col[2] + lc[2] * a];
            }
            // table
            let table = smooth(0.84, 0.86, v);
            col = lerp3(col, [0.09, 0.05, 0.025], table);
            let edge_line = (1.0 - smooth(0.0, 0.006, (v - 0.855).abs())) * 0.25;
            col = [col[0] + edge_line, col[1] + edge_line * 0.6, col[2] + edge_line * 0.3];
            // figure: shoulders / torso
            let fx = xa - 0.47 * aspect;
            let torso_top = 0.47 + (fx * 1.6).powi(2) * 0.9;
            let torso_w = 0.16 + (v - 0.45).max(0.0) * 0.45;
            let in_torso = smooth(torso_top - 0.01, torso_top + 0.01, v)
                * (1.0 - smooth(torso_w - 0.01, torso_w + 0.01, fx.abs()))
                * (1.0 - table);
            let rim = (1.0 - smooth(0.0, 0.05, (torso_w - fx.abs()).abs())) * 0.06;
            let jacket = [0.035 + rim * 0.9, 0.03 + rim * 0.6, 0.03 + rim * 0.4];
            col = lerp3(col, jacket, in_torso);
            // neck + head
            let neck = (1.0 - smooth(0.035, 0.045, fx.abs())) * smooth(0.36, 0.38, v) * (1.0 - smooth(0.47, 0.5, v));
            col = lerp3(col, [0.32, 0.18, 0.11], neck);
            let hx = fx / 0.075;
            let hy = (v - 0.27) / 0.12;
            let hd = (hx * hx + hy * hy).sqrt();
            let head = 1.0 - smooth(0.96, 1.0, hd);
            let light = (0.55 - hx * 0.35 - hy * 0.25).clamp(0.15, 1.0);
            let skin = [0.62 * light, 0.38 * light, 0.24 * light];
            col = lerp3(col, skin, head);
            // white shirt glimpse
            let shirt = (1.0 - smooth(0.02, 0.03, fx.abs() - (v - 0.48) * 0.15)) * smooth(0.48, 0.5, v) * (1.0 - smooth(0.6, 0.64, v));
            col = lerp3(col, [0.55, 0.5, 0.45], shirt * 0.8);
            // glass of beer
            let gx = (xa - 0.665 * aspect).abs();
            let gtop = 0.63;
            let gw = 0.035 + (v - gtop) * -0.02;
            let in_glass = (1.0 - smooth(gw - 0.003, gw + 0.003, gx)) * smooth(gtop - 0.004, gtop + 0.004, v) * (1.0 - smooth(0.88, 0.89, v));
            let foam = 1.0 - smooth(gtop + 0.02, gtop + 0.03, v);
            let beer = lerp3([0.85, 0.5, 0.1], [0.95, 0.85, 0.6], foam);
            let shine = (1.0 - smooth(0.0, 0.006, (gx - gw * 0.55).abs())) * 0.3;
            col = lerp3(col, [beer[0] + shine, beer[1] + shine, beer[2] + shine * 0.7], in_glass);
            // vignette + grain
            let vx = u - 0.5;
            let vy = v - 0.5;
            let vig = 1.0 - (vx * vx * 1.2 + vy * vy * 1.6) * 0.9;
            let g = (rnd((y * w + x) as u32) - 0.5) * 0.02;
            img.px[y * w + x] = [
                (col[0] * vig + g).clamp(0.0, 1.0),
                (col[1] * vig + g).clamp(0.0, 1.0),
                (col[2] * vig + g).clamp(0.0, 1.0),
                1.0,
            ];
        }
    }
    img
}
