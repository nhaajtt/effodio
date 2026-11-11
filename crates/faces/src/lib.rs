//! Faces: where they are in a picture (YuNet), and a vector for each that is close to the vectors of the same
//! person's other pictures (SFace). Both models are small and run on the processor; nothing leaves the computer.

use anyhow::{anyhow, Context, Result};
use std::path::Path;
use tract_onnx::prelude::*;

/// The two model files, from OpenCV's model zoo.
pub mod files {
    pub struct File {
        pub name: &'static str,
        pub repository: &'static str,
        pub revision: &'static str,
        pub bytes: u64,
        pub sha256: &'static str,
    }

    impl File {
        pub fn url(&self) -> String {
            format!("https://huggingface.co/{}/resolve/{}/{}", self.repository, self.revision, self.name)
        }
    }

    pub const DETECTOR: File = File {
        name: "face_detection_yunet_2023mar.onnx",
        repository: "opencv/face_detection_yunet",
        revision: "3cc26e7f1014a5ee5d74a42acee58bafc9d0a310",
        bytes: 232_589,
        sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
    };
    pub const RECOGNIZER: File = File {
        name: "face_recognition_sface_2021dec.onnx",
        repository: "opencv/face_recognition_sface",
        revision: "3d7082438a6e4551e840c9b2bb60b71e8da4b524",
        bytes: 38_696_353,
        sha256: "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
    };
    pub const ALL: [&File; 2] = [&DETECTOR, &RECOGNIZER];
}

/// The length of a face's vector.
pub const DIMENSION: usize = 128;
/// Two faces at least this close (by cosine) are the same person, by the model's own measure.
pub const SAME: f32 = 0.363;

/// The detector looks at the picture fitted into a square of this side.
const SIDE: usize = 640;
/// The recognizer looks at the face turned upright in a square of this side.
const CROP: usize = 112;
/// Where the eyes, the nose and the corners of the mouth sit in that square.
const TEMPLATE: [(f32, f32); 5] =
    [(38.2946, 51.6963), (73.5318, 51.5014), (56.0252, 71.7366), (41.5493, 92.3655), (70.7299, 92.2041)];

type Model = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

pub struct Faces {
    detector: Model,
    recognizer: Model,
}

#[derive(Clone, Debug)]
pub struct Face {
    /// The box around the face, as fractions of the picture's width and height.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// How sure the detector is, 0 to 1.
    pub score: f32,
    /// Normalized to length one.
    pub vector: Vec<f32>,
}

struct Found {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    score: f32,
    marks: [(f32, f32); 5],
}

impl Faces {
    pub fn ready(folder: &Path) -> bool {
        files::ALL.iter().all(|file| folder.join(file.name).exists())
    }

    pub fn open(folder: &Path) -> Result<Faces> {
        let load = |file: &files::File, shape: [usize; 4]| -> Result<Model> {
            tract_onnx::onnx()
                .model_for_path(folder.join(file.name))
                .with_context(|| format!("can't read {}", file.name))?
                .with_input_fact(0, f32::fact(shape).into())?
                .into_optimized()?
                .into_runnable()
        };
        Ok(Faces {
            detector: load(&files::DETECTOR, [1, 3, SIDE, SIDE])?,
            recognizer: load(&files::RECOGNIZER, [1, 3, CROP, CROP])?,
        })
    }

    /// The faces in a picture (`rgb`: three bytes a pixel, row by row), the largest first.
    pub fn find(&self, rgb: &[u8], width: usize, height: usize) -> Result<Vec<Face>> {
        if width == 0 || height == 0 || rgb.len() < width * height * 3 {
            return Ok(Vec::new());
        }
        let found = self.detect(rgb, width, height)?;
        let mut faces = Vec::with_capacity(found.len());
        for face in found {
            let vector = self.describe(rgb, width, height, &face.marks)?;
            faces.push(Face {
                x: (face.left / width as f32).clamp(0.0, 1.0),
                y: (face.top / height as f32).clamp(0.0, 1.0),
                width: (face.width / width as f32).clamp(0.0, 1.0),
                height: (face.height / height as f32).clamp(0.0, 1.0),
                score: face.score,
                vector,
            });
        }
        faces.sort_by(|a, b| (b.width * b.height).total_cmp(&(a.width * a.height)));
        Ok(faces)
    }

    fn detect(&self, rgb: &[u8], width: usize, height: usize) -> Result<Vec<Found>> {
        // The picture fitted into the square's upper left corner, the rest left black.
        let scale = (SIDE as f32 / width as f32).min(SIDE as f32 / height as f32).min(1.0);
        let (fit_width, fit_height) =
            (((width as f32 * scale) as usize).clamp(1, SIDE), ((height as f32 * scale) as usize).clamp(1, SIDE));
        let mut input = Tensor::zero::<f32>(&[1, 3, SIDE, SIDE])?;
        {
            let data = input.as_slice_mut::<f32>()?;
            for y in 0..fit_height {
                let from_y = ((y as f32 + 0.5) / scale) as usize;
                for x in 0..fit_width {
                    let from_x = ((x as f32 + 0.5) / scale) as usize;
                    let pixel = (from_y.min(height - 1) * width + from_x.min(width - 1)) * 3;
                    // The detector was trained on blue, green, red.
                    for channel in 0..3 {
                        data[channel * SIDE * SIDE + y * SIDE + x] = rgb[pixel + 2 - channel] as f32;
                    }
                }
            }
        }
        let outputs = self.detector.run(tvec!(input.into()))?;
        // Twelve outputs: for each of three grid sizes, two scores a cell, its box and its five marks.
        let mut found: Vec<Found> = Vec::new();
        for stride in [8usize, 16, 32] {
            let cells = (SIDE / stride) * (SIDE / stride);
            let mut scores: Vec<&[f32]> = Vec::new();
            let (mut boxes, mut marks): (Option<&[f32]>, Option<&[f32]>) = (None, None);
            for output in outputs.iter() {
                let values = output.as_slice::<f32>()?;
                // By shape, [1, cells, values a cell]: a count alone can't tell one grid's boxes from another's scores.
                let shape = output.shape();
                if shape.len() != 3 || shape[1] != cells {
                    continue;
                }
                match shape[2] {
                    1 => scores.push(values),
                    4 => boxes = Some(values),
                    10 => marks = Some(values),
                    _ => {}
                }
            }
            let (Some(boxes), Some(marks)) = (boxes, marks) else {
                let shapes: Vec<String> = outputs.iter().map(|output| format!("{:?}", output.shape())).collect();
                return Err(anyhow!("the face detector's outputs aren't the expected ones: {}", shapes.join(" ")));
            };
            if scores.len() != 2 {
                return Err(anyhow!("the face detector's outputs aren't the expected ones"));
            }
            let columns = SIDE / stride;
            for cell in 0..cells {
                let score = (scores[0][cell].clamp(0.0, 1.0) * scores[1][cell].clamp(0.0, 1.0)).sqrt();
                if score < 0.7 {
                    continue;
                }
                let (row, column) = ((cell / columns) as f32, (cell % columns) as f32);
                let step = stride as f32;
                let centre_x = (column + boxes[cell * 4]) * step;
                let centre_y = (row + boxes[cell * 4 + 1]) * step;
                let box_width = boxes[cell * 4 + 2].exp() * step;
                let box_height = boxes[cell * 4 + 3].exp() * step;
                let mut five = [(0f32, 0f32); 5];
                for (index, mark) in five.iter_mut().enumerate() {
                    *mark = (
                        (marks[cell * 10 + index * 2] + column) * step / scale,
                        (marks[cell * 10 + index * 2 + 1] + row) * step / scale,
                    );
                }
                found.push(Found {
                    left: (centre_x - box_width / 2.0) / scale,
                    top: (centre_y - box_height / 2.0) / scale,
                    width: box_width / scale,
                    height: box_height / scale,
                    score,
                    marks: five,
                });
            }
        }
        // Of boxes that overlap, the surest stays.
        found.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<Found> = Vec::new();
        for face in found {
            let overlaps = kept.iter().any(|other| {
                let across = (face.left + face.width).min(other.left + other.width) - face.left.max(other.left);
                let down = (face.top + face.height).min(other.top + other.height) - face.top.max(other.top);
                if across <= 0.0 || down <= 0.0 {
                    return false;
                }
                let shared = across * down;
                shared / (face.width * face.height + other.width * other.height - shared) > 0.3
            });
            // Faces smaller than this are too few pixels to tell apart.
            if !overlaps && face.width >= 20.0 && face.height >= 20.0 {
                kept.push(face);
            }
        }
        kept.truncate(30);
        Ok(kept)
    }

    /// The vector of the face with these marks: the face turned and scaled so that its marks land on the
    /// template's, then read by the recognizer.
    fn describe(&self, rgb: &[u8], width: usize, height: usize, marks: &[(f32, f32); 5]) -> Result<Vec<f32>> {
        let mean = |points: &[(f32, f32); 5]| {
            let sum = points.iter().fold((0.0, 0.0), |sum, point| (sum.0 + point.0, sum.1 + point.1));
            (sum.0 / 5.0, sum.1 / 5.0)
        };
        let (from, to) = (mean(marks), mean(&TEMPLATE));
        let (mut along, mut across, mut spread) = (0f32, 0f32, 0f32);
        for (mark, target) in marks.iter().zip(&TEMPLATE) {
            let (sx, sy) = (mark.0 - from.0, mark.1 - from.1);
            let (dx, dy) = (target.0 - to.0, target.1 - to.1);
            along += sx * dx + sy * dy;
            across += sx * dy - sy * dx;
            spread += sx * sx + sy * sy;
        }
        // The picture's point (x, y) lands at (c·x − s·y, s·x + c·y) around the means.
        let (c, s) = (along / spread.max(1e-6), across / spread.max(1e-6));
        let size = (c * c + s * s).max(1e-9);
        let mut input = Tensor::zero::<f32>(&[1, 3, CROP, CROP])?;
        {
            let data = input.as_slice_mut::<f32>()?;
            let at = |x: isize, y: isize, channel: usize| -> f32 {
                let x = x.clamp(0, width as isize - 1) as usize;
                let y = y.clamp(0, height as isize - 1) as usize;
                rgb[(y * width + x) * 3 + channel] as f32
            };
            for row in 0..CROP {
                for column in 0..CROP {
                    let (dx, dy) = (column as f32 - to.0, row as f32 - to.1);
                    let x = (c * dx + s * dy) / size + from.0;
                    let y = (-s * dx + c * dy) / size + from.1;
                    let (left, top) = (x.floor(), y.floor());
                    let (fx, fy) = (x - left, y - top);
                    let (left, top) = (left as isize, top as isize);
                    for channel in 0..3 {
                        let value = at(left, top, channel) * (1.0 - fx) * (1.0 - fy)
                            + at(left + 1, top, channel) * fx * (1.0 - fy)
                            + at(left, top + 1, channel) * (1.0 - fx) * fy
                            + at(left + 1, top + 1, channel) * fx * fy;
                        data[channel * CROP * CROP + row * CROP + column] = value;
                    }
                }
            }
        }
        let outputs = self.recognizer.run(tvec!(input.into()))?;
        let mut vector: Vec<f32> = outputs[0].as_slice::<f32>()?.to_vec();
        let length = vector.iter().map(|value| value * value).sum::<f32>().sqrt().max(1e-12);
        for value in &mut vector {
            *value /= length;
        }
        Ok(vector)
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
