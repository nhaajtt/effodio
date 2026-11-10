//! Readers turn files into what the model takes: pictures, text and sound.

pub mod images;
pub mod media;
pub mod office;
pub mod pdf;
#[cfg(windows)]
pub mod winrt;

/// Packed RGB, row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

impl Picture {
    /// The picture no larger than `side` on its longer side (a quick box average: for OCR, thumbnails and
    /// signatures, not for the model, which resizes with its own filter).
    pub fn fit(&self, side: usize) -> Picture {
        let longest = self.width.max(self.height);
        if longest <= side || longest == 0 {
            return self.clone();
        }
        let width = (self.width * side / longest).max(1);
        let height = (self.height * side / longest).max(1);
        let mut rgb = vec![0u8; width * height * 3];
        for y in 0..height {
            let y0 = y * self.height / height;
            let y1 = ((y + 1) * self.height / height).max(y0 + 1).min(self.height);
            for x in 0..width {
                let x0 = x * self.width / width;
                let x1 = ((x + 1) * self.width / width).max(x0 + 1).min(self.width);
                let mut sum = [0u32; 3];
                let mut count = 0u32;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let p = (sy * self.width + sx) * 3;
                        sum[0] += self.rgb[p] as u32;
                        sum[1] += self.rgb[p + 1] as u32;
                        sum[2] += self.rgb[p + 2] as u32;
                        count += 1;
                    }
                }
                let o = (y * width + x) * 3;
                for c in 0..3 {
                    rgb[o + c] = (sum[c] / count.max(1)) as u8;
                }
            }
        }
        Picture { width, height, rgb }
    }
}
