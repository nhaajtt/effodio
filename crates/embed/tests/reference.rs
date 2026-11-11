//! The vectors this build makes for fixed inputs, against stored reference vectors made by the reference pipeline
//! (tests/reference/vectors.json, vector version 1). Needs the model files; skipped without them.

use magpie_embed::{cosine, models_folder, prompts, Decoded, Device, Embedder, Options};
use std::{collections::BTreeMap, fs, path::PathBuf};

/// Runtimes that agree measure at least this; less is a change people would see in their results.
const AGREEMENT: f32 = 0.999;

fn folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/reference")
}

fn made(device: Device) -> Option<BTreeMap<String, Vec<f32>>> {
    let options = Options::in_folder(&models_folder(), false, device);
    if !options.text.exists() || !options.media.as_ref().unwrap().exists() {
        eprintln!("skipped: no model files in {}", models_folder().display());
        return None;
    }
    let mut embedder = Embedder::open(&options).expect("the model loads");
    eprintln!("backends: {} (gpu: {})", Embedder::backends(), embedder.gpu);
    let long: Vec<String> = (1..=60)
        .map(|n| {
            format!(
                "Note {n}: the review moved to room {} because the projector in the main hall failed again, and the \
                 team agreed to send the slides the night before.",
                n % 7 + 2
            )
        })
        .collect();
    let texts = [
        ("query", prompts::query("payment declined error")),
        ("query-bn", prompts::query("ইলিশ মাছ রান্না")),
        ("query-ar", prompts::query("متى تغلق المكتبة")),
        (
            "document",
            prompts::document(
                Some("Tokyo itinerary"),
                "Flight to Tokyo Haneda on November 3. Then the train to Kamakura to see the Great Buddha, and back \
                 for dinner in Shibuya.",
            ),
        ),
        ("document-long", prompts::document(None, &long.join(" "))),
    ];
    let mut out = BTreeMap::new();
    let inputs: Vec<&str> = texts.iter().map(|(_, text)| text.as_str()).collect();
    for ((name, _), vector) in texts.iter().zip(embedder.embed_texts(&inputs).unwrap()) {
        out.insert(name.to_string(), vector);
    }
    for (name, file, budget) in [("screen-280", "screen.png", 280), ("photo-140", "photo.jpg", 140), ("frame-70", "photo.jpg", 70)] {
        let bytes = fs::read(folder().join(file)).unwrap();
        let Decoded::Image { width, height, rgb } = embedder.decode(&bytes).unwrap() else { panic!("{file} is an image") };
        out.insert(name.to_string(), embedder.embed_image(&rgb, width, height, budget).unwrap());
    }
    let bytes = fs::read(folder().join("speech.wav")).unwrap();
    let Decoded::Audio { samples } = embedder.decode(&bytes).unwrap() else { panic!("speech.wav is audio") };
    out.insert("speech".to_string(), embedder.embed_audio(&samples).unwrap());
    Some(out)
}

#[test]
fn vectors_match_the_reference() {
    let device = if std::env::var("MAGPIE_TEST_DEVICE").as_deref() == Ok("gpu") { Device::Gpu } else { Device::Cpu };
    let Some(made) = made(device) else { return };
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(folder().join("vectors.json")).unwrap()).unwrap();
    assert_eq!(stored["vector_version"], magpie_embed::VECTOR_VERSION);
    let reference = stored["vectors"].as_object().unwrap();
    assert_eq!(made.len(), reference.len(), "the same inputs as the reference");
    let mut worst = (String::new(), 1f32);
    for (name, vector) in &made {
        let expected: Vec<f32> =
            reference[name].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
        let agreement = cosine(vector, &expected);
        eprintln!("{name}: cosine {agreement:.5}");
        if agreement < worst.1 {
            worst = (name.clone(), agreement);
        }
    }
    assert!(worst.1 >= AGREEMENT, "{} moved to cosine {}", worst.0, worst.1);
}
