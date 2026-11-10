//! Measures this computer: how long a picture, a passage and 30 seconds of sound take on the processor, and on the
//! graphics card when llama.cpp finds one. The graphics card is used only when it's faster and gives the same vectors.

use crate::{emit, open_model};
use anyhow::Result;
use glint_core::config::{self, Speed};
use glint_embed::{cosine, Device, Embedder};
use serde_json::json;
use std::time::Instant;

struct Measured {
    image: f64,
    screenshot: f64,
    passage: f64,
    sound: f64,
    vectors: Vec<Vec<f32>>,
}

/// A picture with shapes and gradients, the same every time.
fn picture(width: usize, height: usize) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let stripe = ((x / 40 + y / 60) % 2) as u8 * 60;
            let disc = (((x as f64 - width as f64 / 2.0).powi(2) + (y as f64 - height as f64 / 2.0).powi(2)).sqrt()
                < height as f64 / 3.0) as u8
                * 90;
            rgb.push((x * 255 / width) as u8 / 2 + stripe);
            rgb.push((y * 255 / height) as u8 / 2 + disc);
            rgb.push(((x + y) % 256) as u8 / 3 + stripe / 2);
        }
    }
    rgb
}

fn measure(device: Device, quick: bool) -> Result<Measured> {
    let mut model = open_model(false, device)?;
    let rounds = if quick { 1 } else { 3 };
    let photo = picture(1024, 768);
    let passage = glint_embed::prompts::document(
        Some("Trip notes"),
        &"We took the morning ferry across the bay and walked up to the old lighthouse before the fog came in. "
            .repeat(18),
    );
    let sound: Vec<f32> = (0..glint_embed::AUDIO_RATE * 30)
        .map(|i| {
            let t = i as f32 / glint_embed::AUDIO_RATE as f32;
            0.2 * (t * 220.0 * std::f32::consts::TAU).sin() * (0.5 + 0.5 * (t * 3.0).sin())
        })
        .collect();
    let mut vectors = Vec::new();
    // The first call of each kind sets things up; it isn't timed.
    model.embed_image(&photo, 1024, 768, 70)?;
    let timed = |run: &mut dyn FnMut() -> Result<Vec<f32>>| -> Result<(f64, Vec<f32>)> {
        let started = Instant::now();
        let mut last = Vec::new();
        for _ in 0..rounds {
            last = run()?;
        }
        Ok((started.elapsed().as_secs_f64() / rounds as f64, last))
    };
    let (image, vector) = timed(&mut || model.embed_image(&photo, 1024, 768, 140))?;
    vectors.push(vector);
    let (screenshot, vector) = timed(&mut || model.embed_image(&photo, 1024, 768, 280))?;
    vectors.push(vector);
    let (passage_time, vector) = timed(&mut || model.embed_text(&passage))?;
    vectors.push(vector);
    let (sound_time, vector) = timed(&mut || model.embed_audio(&sound))?;
    vectors.push(vector);
    Ok(Measured { image, screenshot, passage: passage_time, sound: sound_time, vectors })
}

pub fn run(quick: bool) -> Result<()> {
    emit(&json!({"event": "measuring", "device": "cpu"}));
    let cpu = measure(Device::Cpu, quick)?;
    let mut speed = Speed {
        device: "cpu".into(),
        image: cpu.image,
        screenshot: cpu.screenshot,
        passage: cpu.passage,
        sound: cpu.sound,
        gpu_name: None,
        agreement: None,
        cpu_image: Some(cpu.image),
        gpu_image: None,
    };
    // Backends are loaded once per process, and the processor run left the graphics card's out: the card is measured
    // in a process of its own.
    if std::env::var_os("GLINT_SPEED_CHILD").is_none() {
        if let Some(gpu) = measure_gpu_in_child(quick) {
            let agreement = cpu.vectors.iter().zip(&gpu.vectors).map(|(a, b)| cosine(a, b) as f64).fold(1.0, f64::min);
            speed.agreement = Some(agreement);
            speed.gpu_image = Some(gpu.image);
            speed.gpu_name = gpu_name();
            let faster = gpu.image + gpu.passage < (cpu.image + cpu.passage) * 0.8;
            if faster && agreement >= 0.995 {
                speed.device = "gpu".into();
                speed.image = gpu.image;
                speed.screenshot = gpu.screenshot;
                speed.passage = gpu.passage;
                speed.sound = gpu.sound;
            }
        }
    }
    std::fs::create_dir_all(config::home())?;
    std::fs::write(config::speed_file(), serde_json::to_vec_pretty(&speed)?)?;
    emit(&json!({"event": "finished", "speed": speed}));
    Ok(())
}

fn gpu_name() -> Option<String> {
    let backends = Embedder::backends();
    backends.split(", ").find(|name| !name.to_lowercase().starts_with("cpu")).map(str::to_string)
}

/// `glint-helper speed-gpu`: prints the graphics card's timings and vectors as one JSON line, or nothing.
pub fn gpu_child(quick: bool) -> Result<()> {
    if !Embedder::init_backends(None, false) {
        return Ok(());
    }
    let measured = measure(Device::Gpu, quick)?;
    println!(
        "{}",
        json!({
            "image": measured.image, "screenshot": measured.screenshot, "passage": measured.passage,
            "sound": measured.sound, "vectors": measured.vectors, "name": gpu_name(),
        })
    );
    Ok(())
}

fn measure_gpu_in_child(quick: bool) -> Option<Measured> {
    let exe = std::env::current_exe().ok()?;
    let mut command = std::process::Command::new(exe);
    command.arg("speed-gpu").env("GLINT_SPEED_CHILD", "1").stdin(std::process::Stdio::null());
    if quick {
        command.arg("--quick");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    emit(&json!({"event": "measuring", "device": "gpu"}));
    let output = command.stderr(std::process::Stdio::null()).output().ok()?;
    let line = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(line.lines().last()?).ok()?;
    let vectors: Vec<Vec<f32>> = serde_json::from_value(value["vectors"].clone()).ok()?;
    Some(Measured {
        image: value["image"].as_f64()?,
        screenshot: value["screenshot"].as_f64()?,
        passage: value["passage"].as_f64()?,
        sound: value["sound"].as_f64()?,
        vectors,
    })
}
