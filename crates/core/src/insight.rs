//! What else the index can tell besides answering a search: a picture's main colours, pictures that belong
//! together, and the thread that runs from one file through the others about the same thing.

use crate::kinds::FileKind;
use crate::kinds::Modality;
use crate::readers::Picture;
use crate::search::{dot, FileMeta, Searcher};
use crate::store::{Store, DIMENSION};
use crate::text::fold;
use anyhow::Result;
use half::slice::HalfFloatSliceExt;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

pub const TONES: &[&str] =
    &["red", "orange", "yellow", "green", "teal", "blue", "purple", "pink", "brown", "black", "white", "gray"];

/// A colour's name as someone types it (English or Vietnamese, with or without accents).
pub fn tone_named(word: &str) -> Option<&'static str> {
    Some(match fold(word).as_str() {
        "red" | "do" => "red",
        "orange" | "cam" => "orange",
        "yellow" | "gold" | "golden" | "vang" => "yellow",
        "green" | "xanhla" | "luc" | "la" => "green",
        "teal" | "cyan" | "turquoise" | "ngoc" => "teal",
        "blue" | "xanh" | "xanhduong" | "lam" => "blue",
        "purple" | "violet" | "tim" => "purple",
        "pink" | "hong" => "pink",
        "brown" | "nau" => "brown",
        "black" | "dark" | "den" => "black",
        "white" | "trang" => "white",
        "gray" | "grey" | "xam" => "gray",
        _ => return None,
    })
}

/// The colours that fill at least a fifth of a picture, the largest first (three at most).
pub fn tones(picture: &Picture) -> Vec<&'static str> {
    let small = picture.fit(64);
    let mut counts = [0usize; 12];
    for pixel in small.rgb.chunks_exact(3) {
        let (r, g, b) = (pixel[0] as f32 / 255.0, pixel[1] as f32 / 255.0, pixel[2] as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let spread = max - min;
        let saturation = if max <= 0.0 { 0.0 } else { spread / max };
        let tone = if max < 0.16 {
            9
        } else if saturation < 0.14 {
            if max > 0.82 {
                10
            } else {
                11
            }
        } else {
            let mut hue = if max == r {
                60.0 * ((g - b) / spread)
            } else if max == g {
                60.0 * ((b - r) / spread + 2.0)
            } else {
                60.0 * ((r - g) / spread + 4.0)
            };
            if hue < 0.0 {
                hue += 360.0;
            }
            match hue as u32 {
                0..=14 | 340..=360 => 0,
                15..=44 if max < 0.6 => 8,
                15..=44 => 1,
                45..=69 if max < 0.5 => 8,
                45..=69 => 2,
                70..=159 => 3,
                160..=194 => 4,
                195..=254 => 5,
                255..=289 => 6,
                _ => 7,
            }
        };
        counts[tone] += 1;
    }
    let total = counts.iter().sum::<usize>().max(1);
    let mut ranked: Vec<usize> = (0..12).filter(|&tone| counts[tone] * 5 >= total).collect();
    ranked.sort_by(|a, b| counts[*b].cmp(&counts[*a]));
    ranked.into_iter().take(3).map(|tone| TONES[tone]).collect()
}

/// The colours written in a file's `info`.
pub(crate) fn tones_in(info: &str) -> Vec<String> {
    if !info.contains("\"tones\"") {
        return Vec::new();
    }
    serde_json::from_str::<serde_json::Value>(info)
        .ok()
        .and_then(|value| value["tones"].as_array().cloned())
        .map(|list| list.iter().filter_map(|tone| tone.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// Pictures read before colours were kept: works them out now. Returns how many it did.
pub fn backfill_tones(store: &mut Store, stop: &std::sync::atomic::AtomicBool) -> Result<usize> {
    let waiting: Vec<(i64, String, String)> = {
        let mut select = store.db.prepare(
            "SELECT id, path, info FROM files WHERE state IN ('done', 'partial') AND kind IN ('image', 'screenshot')
             AND info NOT LIKE '%\"tones\"%'",
        )?;
        let rows = select.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut done = 0;
    for batch in waiting.chunks(64) {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        let found: Vec<(i64, String)> = batch
            .par_iter()
            .map(|(id, path, info)| {
                let tones = crate::readers::images::load(std::path::Path::new(path)).map(|p| tones(&p)).unwrap_or_default();
                let mut value: serde_json::Value = serde_json::from_str(info).unwrap_or_else(|_| serde_json::json!({}));
                value["tones"] = tones.into();
                (*id, value.to_string())
            })
            .collect();
        let change = store.db.transaction()?;
        for (id, info) in &found {
            change.execute("UPDATE files SET info = ? WHERE id = ?", (info, id))?;
        }
        change.commit()?;
        done += found.len();
    }
    Ok(done)
}

/// Who is in which files, by folded name. (Empty until faces have been read and someone has been named.)
pub(crate) fn people(store: &Store) -> Result<HashMap<String, HashSet<i64>>> {
    let mut out: HashMap<String, HashSet<i64>> = HashMap::new();
    let Ok(mut select) =
        store.db.prepare("SELECT people.name, faces.file FROM faces JOIN people ON people.id = faces.person")
    else {
        return Ok(out);
    };
    let mut rows = select.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let file: i64 = row.get(1)?;
        // Every word of a name finds the person: "lan" for "Lan Anh".
        let whole = fold(&name);
        for part in whole.split_whitespace() {
            out.entry(part.to_string()).or_default().insert(file);
        }
        out.entry(whole.replace(' ', "")).or_default().insert(file);
        out.entry(whole).or_default().insert(file);
    }
    Ok(out)
}

fn unit(mut vector: Vec<f32>) -> Vec<f32> {
    let length = dot(&vector, &vector).sqrt().max(1e-12);
    for value in &mut vector {
        *value /= length;
    }
    vector
}

/// Pictures that belong together.
pub struct Album {
    pub files: Vec<i64>,
    /// The mean of its pictures' vectors as the model made them, for naming it.
    pub centre: Vec<f32>,
}

impl Searcher {
    fn float_row(&self, row: usize) -> Vec<f32> {
        let mut out = vec![0f32; DIMENSION];
        self.halves[row * DIMENSION..(row + 1) * DIMENSION].convert_to_f32_slice(&mut out);
        out
    }

    /// One row per file (its first whole vector), for the files `keep` lets in; and those vectors with their
    /// modality's mean taken off and scaled to length one, so that a cosine between two of them says how much nearer
    /// they are than files usually are.
    fn centred(&mut self, limit: usize, keep: impl Fn(&FileMeta) -> bool) -> Result<(Vec<usize>, Vec<Vec<f32>>)> {
        if !self.loaded {
            self.reload()?;
        }
        let mut seen = HashSet::new();
        let rows: Vec<usize> = (0..self.row_file.len())
            .filter(|&row| {
                self.row_sets_scale[row]
                    && self.files.get(&self.row_file[row]).is_some_and(&keep)
                    && seen.insert(self.row_file[row])
            })
            .take(limit)
            .collect();
        let mut data: Vec<Vec<f32>> = rows.iter().map(|&row| self.float_row(row)).collect();
        for modality in Modality::ALL {
            let members: Vec<usize> = (0..rows.len()).filter(|&i| self.row_modality[rows[i]] == modality).collect();
            if members.is_empty() {
                continue;
            }
            let mut mean = vec![0f32; DIMENSION];
            for &i in &members {
                for (m, v) in mean.iter_mut().zip(&data[i]) {
                    *m += v / members.len() as f32;
                }
            }
            for &i in &members {
                for (v, m) in data[i].iter_mut().zip(&mean) {
                    *v -= m;
                }
            }
        }
        let data = data.into_iter().map(unit).collect();
        Ok((rows, data))
    }

    /// Photos and videos gathered into albums: each picture joins the album whose middle it is nearest to, if it is
    /// at least `near` close (by centred cosine), or starts one. Albums with fewer than `least` pictures are left out.
    /// Also returns the mean of all the pictures' vectors, to tell what sets an album apart.
    pub fn albums(&mut self, near: f32, least: usize, limit: usize) -> Result<(Vec<Album>, Vec<f32>)> {
        let (rows, data) =
            self.centred(12_000, |meta| matches!(meta.kind, FileKind::Image | FileKind::Video))?;
        if rows.len() < least {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut sums: Vec<Vec<f32>> = Vec::new();
        let mut middles: Vec<Vec<f32>> = Vec::new();
        let mut home = vec![usize::MAX; rows.len()];
        for pass in 0..2 {
            for (index, vector) in data.iter().enumerate() {
                let best = middles
                    .par_iter()
                    .enumerate()
                    .map(|(album, middle)| (album, dot(vector, middle)))
                    .reduce(|| (usize::MAX, f32::MIN), |a, b| if b.1 > a.1 { b } else { a });
                let target = if best.1 >= near {
                    best.0
                } else if pass == 0 {
                    sums.push(vec![0f32; DIMENSION]);
                    middles.push(vector.clone());
                    sums.len() - 1
                } else {
                    usize::MAX
                };
                if pass == 0 && target != usize::MAX {
                    for (sum, value) in sums[target].iter_mut().zip(vector) {
                        *sum += value;
                    }
                    middles[target] = unit(sums[target].clone());
                }
                home[index] = target;
            }
        }
        let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
        for (index, &album) in home.iter().enumerate() {
            if album != usize::MAX {
                members.entry(album).or_default().push(index);
            }
        }
        let mut whole = vec![0f32; DIMENSION];
        let raw: Vec<Vec<f32>> = rows.iter().map(|&row| self.float_row(row)).collect();
        for vector in &raw {
            for (sum, value) in whole.iter_mut().zip(vector) {
                *sum += value / raw.len() as f32;
            }
        }
        let mut albums: Vec<Album> = members
            .into_values()
            .filter(|list| list.len() >= least)
            .map(|mut list| {
                list.sort_by(|a, b| {
                    let when = |i: &usize| self.files.get(&self.row_file[rows[*i]]).map_or(0.0, |meta| meta.modified);
                    when(a).total_cmp(&when(b))
                });
                let mut centre = vec![0f32; DIMENSION];
                for &index in &list {
                    for (sum, value) in centre.iter_mut().zip(&raw[index]) {
                        *sum += value;
                    }
                }
                Album { files: list.iter().map(|&index| self.row_file[rows[index]]).collect(), centre: unit(centre) }
            })
            .collect();
        albums.sort_by(|a, b| b.files.len().cmp(&a.files.len()).then(a.files[0].cmp(&b.files[0])));
        albums.truncate(limit);
        Ok((albums, whole))
    }

    /// From one file, the files about the same thing, one after another: each step goes to the file nearest both
    /// the last one and the first, and would rather go to a kind of file the thread hasn't passed through yet (from
    /// a contract to the photo of its signing to the recording of the meeting). Each comes with how near it was.
    pub fn thread(&mut self, file: i64, steps: usize) -> Result<Vec<(i64, f32)>> {
        let (rows, data) = self.centred(8_000, |_| true)?;
        let Some(start) = rows.iter().position(|&row| self.row_file[row] == file) else {
            return Ok(Vec::new());
        };
        let kind_of = |index: usize| self.files.get(&self.row_file[rows[index]]).map(|meta| meta.kind);
        let mut used: HashSet<usize> = HashSet::from([start]);
        let mut kinds: HashSet<Option<FileKind>> = HashSet::from([kind_of(start)]);
        let mut chain = vec![(file, 1.0)];
        let mut current = start;
        let mut passed = vec![start];
        for _ in 0..steps {
            let best = (0..rows.len())
                .into_par_iter()
                .filter(|index| !used.contains(index))
                // Not a copy of something already on the thread: it should move on.
                .filter(|&index| passed.iter().all(|&before: &usize| dot(&data[before], &data[index]) < 0.8))
                .map(|index| {
                    let nearness = 0.5 * dot(&data[current], &data[index]) + 0.5 * dot(&data[start], &data[index]);
                    let fresh = if kinds.contains(&kind_of(index)) { 0.0 } else { 0.05 };
                    (index, nearness, nearness + fresh)
                })
                .reduce(|| (usize::MAX, f32::MIN, f32::MIN), |a, b| if b.2 > a.2 { b } else { a });
            if best.0 == usize::MAX || best.1 < 0.08 {
                break;
            }
            used.insert(best.0);
            kinds.insert(kind_of(best.0));
            passed.push(best.0);
            chain.push((self.row_file[rows[best.0]], best.1));
            current = best.0;
        }
        Ok(chain)
    }

    /// Photos taken one after another of the same thing: within `seconds` of the one before and at least `near`
    /// like it.
    pub fn bursts(&mut self, seconds: f64, near: f32, limit: usize) -> Result<Vec<Vec<i64>>> {
        if !self.loaded {
            self.reload()?;
        }
        let mut seen = HashSet::new();
        let mut rows: Vec<(usize, f64)> = (0..self.row_file.len())
            .filter(|&row| self.row_modality[row] == Modality::Image && seen.insert(self.row_file[row]))
            .filter_map(|row| {
                let meta = self.files.get(&self.row_file[row])?;
                (meta.kind == FileKind::Image).then_some((row, meta.modified))
            })
            .collect();
        rows.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut out: Vec<Vec<i64>> = Vec::new();
        let mut run: Vec<i64> = Vec::new();
        let mut last: Option<(Vec<f32>, f64)> = None;
        for (row, when) in rows {
            let vector = self.float_row(row);
            let joins = last.as_ref().is_some_and(|(before, then)| when - then <= seconds && dot(before, &vector) >= near);
            if !joins {
                if run.len() > 1 {
                    out.push(std::mem::take(&mut run));
                }
                run.clear();
            }
            run.push(self.row_file[row]);
            last = Some((vector, when));
        }
        if run.len() > 1 {
            out.push(run);
        }
        out.sort_by(|a, b| b.len().cmp(&a.len()));
        out.truncate(limit);
        Ok(out)
    }

    /// The files nearest a vector, each with the cosine of its nearest segment, the nearest first.
    pub fn nearest(&mut self, vector: &[f32], limit: usize) -> Result<Vec<(i64, f32)>> {
        if !self.loaded {
            self.reload()?;
        }
        let mut best: HashMap<i64, f32> = HashMap::new();
        for (row, score) in self.scores(vector).into_iter().enumerate() {
            let entry = best.entry(self.row_file[row]).or_insert(f32::MIN);
            if score > *entry {
                *entry = score;
            }
        }
        let mut ranked: Vec<(i64, f32)> = best.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        ranked.truncate(limit);
        Ok(ranked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_main_colours() {
        let mut rgb = Vec::new();
        for index in 0..100 {
            rgb.extend_from_slice(if index < 70 { &[20, 60, 220] } else { &[250, 250, 250] });
        }
        let picture = Picture { width: 10, height: 10, rgb };
        assert_eq!(tones(&picture), vec!["blue", "white"]);
        assert_eq!(tone_named("Vàng"), Some("yellow"));
        assert_eq!(tone_named("tím"), Some("purple"));
        assert_eq!(tone_named("loud"), None);
    }
}
