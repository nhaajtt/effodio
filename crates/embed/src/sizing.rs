//! How the reference sizes an image for the vision encoder, so llama.cpp sees the same pixels: scale to fill a token
//! budget keeping the aspect ratio, each side a multiple of 48 px, resampled as Pillow's bicubic does.

const PRECISION: i32 = 22;

/// The size that fills `budget` soft tokens (each 3 × 3 patches of 16 px), as (height, width). None for an image with
/// a zero side, or one too thin to give a single patch row.
pub fn target_size(height: usize, width: usize, budget: usize) -> Option<(usize, usize)> {
    if height == 0 || width == 0 {
        return None;
    }
    let (patch, pooling) = (16usize, 3usize);
    let max_patches = budget * pooling * pooling;
    let factor = ((max_patches * patch * patch) as f64 / (height * width) as f64).sqrt();
    let side = pooling * patch;
    let mut target_height = (factor * height as f64 / side as f64).floor() as usize * side;
    let mut target_width = (factor * width as f64 / side as f64).floor() as usize * side;
    if target_height == 0 && target_width == 0 {
        return None;
    }
    let max_side = (max_patches / (pooling * pooling)) * side;
    if target_height == 0 {
        target_height = side;
        target_width = (width / height * side).min(max_side);
    } else if target_width == 0 {
        target_width = side;
        target_height = (height / width * side).min(max_side);
    }
    Some((target_height, target_width))
}

/// Pillow's bicubic resampling of packed RGB (a = −0.5, the kernel widened when shrinking, 22-bit fixed point).
pub fn resize_bicubic(source: &[u8], width: usize, height: usize, to_width: usize, to_height: usize) -> Vec<u8> {
    match (to_width != width, to_height != height) {
        (true, true) => {
            let pass = horizontal(source, width, height, to_width);
            vertical(&pass, height, to_width, to_height)
        }
        (true, false) => horizontal(source, width, height, to_width),
        (false, true) => vertical(source, height, width, to_height),
        (false, false) => source[..width * height * 3].to_vec(),
    }
}

fn filter(value: f64) -> f64 {
    let (x, a) = (value.abs(), -0.5);
    if x < 1.0 {
        ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * a
    } else {
        0.0
    }
}

/// For each output pixel: the first contributing input pixel, how many contribute, and their weights.
fn coefficients(in_size: usize, out_size: usize) -> (Vec<usize>, Vec<i32>, usize) {
    let scale = in_size as f64 / out_size as f64;
    let filter_scale = scale.max(1.0);
    let support = 2.0 * filter_scale;
    let size = support.ceil() as usize * 2 + 1;
    let mut bounds = vec![0usize; out_size * 2];
    let mut weights = vec![0i32; out_size * size];
    let mut row = vec![0f64; size];
    for out in 0..out_size {
        let center = (out as f64 + 0.5) * scale;
        let first = ((center - support + 0.5) as i64).max(0) as usize;
        let count = ((center + support + 0.5) as usize).min(in_size) - first;
        let mut total = 0.0;
        for (x, slot) in row.iter_mut().enumerate() {
            *slot = if x < count { filter(((x + first) as f64 - center + 0.5) / filter_scale) } else { 0.0 };
            total += *slot;
        }
        for x in 0..size {
            let weight = if x < count && total != 0.0 { row[x] / total } else { row[x] };
            weights[out * size + x] =
                (weight * (1i64 << PRECISION) as f64 + if weight < 0.0 { -0.5 } else { 0.5 }) as i32;
        }
        bounds[out * 2] = first;
        bounds[out * 2 + 1] = count;
    }
    (bounds, weights, size)
}

fn clip(value: i32) -> u8 {
    (value >> PRECISION).clamp(0, 255) as u8
}

fn horizontal(src: &[u8], in_width: usize, rows: usize, out_width: usize) -> Vec<u8> {
    let (bounds, weights, size) = coefficients(in_width, out_width);
    let mut out = vec![0u8; out_width * rows * 3];
    for y in 0..rows {
        for x in 0..out_width {
            let mut p = (y * in_width + bounds[x * 2]) * 3;
            let (mut r, mut g, mut b) = (1i32 << (PRECISION - 1), 1i32 << (PRECISION - 1), 1i32 << (PRECISION - 1));
            for k in 0..bounds[x * 2 + 1] {
                let w = weights[x * size + k];
                r = r.wrapping_add((src[p] as i32).wrapping_mul(w));
                g = g.wrapping_add((src[p + 1] as i32).wrapping_mul(w));
                b = b.wrapping_add((src[p + 2] as i32).wrapping_mul(w));
                p += 3;
            }
            let o = (y * out_width + x) * 3;
            out[o] = clip(r);
            out[o + 1] = clip(g);
            out[o + 2] = clip(b);
        }
    }
    out
}

fn vertical(src: &[u8], in_height: usize, row_width: usize, out_height: usize) -> Vec<u8> {
    let (bounds, weights, size) = coefficients(in_height, out_height);
    let stride = row_width * 3;
    let mut out = vec![0u8; stride * out_height];
    let mut sums = vec![0i32; stride];
    for y in 0..out_height {
        sums.fill(1 << (PRECISION - 1));
        for k in 0..bounds[y * 2 + 1] {
            let row = &src[(bounds[y * 2] + k) * stride..][..stride];
            let w = weights[y * size + k];
            for (sum, &value) in sums.iter_mut().zip(row) {
                *sum = sum.wrapping_add((value as i32).wrapping_mul(w));
            }
        }
        for (slot, &sum) in out[y * stride..][..stride].iter_mut().zip(&sums) {
            *slot = clip(sum);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_fill_the_budget_in_multiples_of_48() {
        // 280 tokens = 2520 patches of 16 px = 645,120 px.
        let (h, w) = target_size(1080, 1920, 280).unwrap();
        assert_eq!((h % 48, w % 48), (0, 0));
        assert!(h * w <= 280 * 9 * 256);
        assert_eq!((h, w), (576, 1056));
        assert_eq!(target_size(0, 10, 70), None);
        // Very thin: one side would round to zero, so it gets one patch row.
        let (h, w) = target_size(10, 4000, 70).unwrap();
        assert_eq!(h, 48);
        assert!(w <= 70 * 48);
    }

    #[test]
    fn resizing_keeps_a_flat_colour() {
        let source = vec![200u8; 96 * 64 * 3];
        let out = resize_bicubic(&source, 96, 64, 48, 48);
        assert_eq!(out.len(), 48 * 48 * 3);
        assert!(out.iter().all(|&v| v == 200));
    }
}
