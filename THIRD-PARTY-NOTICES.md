# Third-party notices

Magpie includes or builds on the work below.

## DigUp

The image sizing (`crates/embed/src/sizing.rs`), the llama.cpp patch (`scripts/llama-no-logits.patch`), the
reference vectors and their inputs (`crates/embed/tests/reference`) and the approach to ranking come from
DigUp, https://github.com/ARahim3/DigUp

```
MIT License

Copyright (c) 2026 Abdur Rahim

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## llama.cpp

https://github.com/ggml-org/llama.cpp, linked as shared libraries.

```
MIT License

Copyright (c) 2023-2026 The ggml authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## EmbeddingGemma 2

The model is downloaded by the app from https://huggingface.co/ggml-org/embeddinggemma-2-GGUF and is not part of
this repository. It is by Google DeepMind; see its model card for its license and terms of use.
Gemma is a trademark of Google LLC.

## Rust and JavaScript libraries

Tauri, rusqlite and SQLite, image, pdf-extract, zip, quick-xml, rayon, half, blake3, notify, ureq, windows-rs and
the others listed in `Cargo.lock` and `app/package-lock.json` are used under their own licenses (MIT, Apache 2.0,
or similar). SQLite is in the public domain.
tract (the runtime the face models run on) is by Sonos, under MIT or Apache 2.0.

## Models and programs fetched on request

None of these is part of this repository or of the installer; the app downloads each one only when its feature
is turned on, from the address given.

- **Gemma 3 (4B, instruction-tuned)**, for answers from the files: https://huggingface.co/ggml-org/gemma-3-4b-it-GGUF.
  By Google DeepMind, under the Gemma terms of use.
- **Whisper (small)**, for the words spoken in recordings: https://huggingface.co/ggerganov/whisper.cpp. The model
  is by OpenAI (MIT); it runs on whisper.cpp by Georgi Gerganov and contributors (MIT),
  https://github.com/ggml-org/whisper.cpp
- **YuNet** (finding faces, MIT) and **SFace** (telling them apart, Apache 2.0), from the OpenCV model zoo:
  https://huggingface.co/opencv/face_detection_yunet and https://huggingface.co/opencv/face_recognition_sface
- **llama.cpp** programs, to run Gemma 3: https://github.com/ggml-org/llama.cpp (MIT).
- **FFmpeg**, to read videos and recordings, when the computer has none: the "essentials" build by Gyan Doshi,
  https://github.com/GyanD/codexffmpeg, under the GPL v3. https://ffmpeg.org

## Typefaces

Geist and Geist Mono (Vercel), Playpen Sans (TypeTogether) and Departure Mono (Helena Zhang) are used under the
SIL Open Font License 1.1; the license texts are beside the font files.
