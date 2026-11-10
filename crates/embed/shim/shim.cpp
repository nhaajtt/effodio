// A small C surface over llama.cpp + mtmd for EmbeddingGemma 2, so the Rust side never mirrors llama.cpp's structs.
//
// Inputs are framed as the reference frames them: `<bos> text <eos>`, and `<bos> <|image> soft tokens <image|> <eos>`
// (the same for audio). The model is non-causal with no KV cache, so each input, token ids and encoder rows together,
// goes through one llama_process call. Output is mean-pooled and L2-normalized.

#include "llama.h"
#include "ggml-backend.h"
#include "mtmd.h"
#include "mtmd-helper.h"

#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {

const int MAX_SEQUENCES = 16;
bool g_gpu_found = false;
bool g_ready = false;
ggml_log_level g_level = GGML_LOG_LEVEL_INFO;

void log_callback(ggml_log_level level, const char * text, void *) {
    if (level != GGML_LOG_LEVEL_CONT) g_level = level;
    if (g_level >= GGML_LOG_LEVEL_WARN && text) fputs(text, stderr);
}

bool is_gpu(ggml_backend_dev_t device) {
    auto type = ggml_backend_dev_type(device);
    return type == GGML_BACKEND_DEVICE_TYPE_GPU || type == GGML_BACKEND_DEVICE_TYPE_IGPU;
}

void fail(char * err, int errlen, const char * format, ...) {
    if (!err || errlen <= 0) return;
    va_list args;
    va_start(args, format);
    vsnprintf(err, errlen, format, args);
    va_end(args);
}

} // namespace

struct ge_ctx {
    llama_model * model = nullptr;
    llama_context * context = nullptr;
    mtmd_context * media = nullptr;
    llama_batch_ext * batch = nullptr;
    const llama_vocab * vocab = nullptr;
    int dim = 0;
    int row = 0;
    int max_tokens = 0;
    int sequences = 0;
    int tokens = 0;
    llama_pos position = 0;

    void reset() {
        llama_batch_ext_clear(batch);
        sequences = 0;
        tokens = 0;
    }
    void begin() {
        sequences += 1;
        position = 0;
    }
    bool place(int32_t index) {
        if (index < 0) return false;
        llama_pos pos[4] = {position, 0, 0, 0};
        if (!llama_batch_ext_set_pos(batch, index, pos)) return false;
        if (!llama_batch_ext_set_output_embd(batch, index, true)) return false;
        position += 1;
        tokens += 1;
        return true;
    }
    bool add_token(llama_token token) { return place(llama_batch_ext_add_token(batch, sequences - 1, token)); }
    bool add_rows(const float * rows, size_t count) {
        for (size_t i = 0; i < count; i++) {
            llama_embd embd = {rows + i * row, 1, (size_t) row};
            if (!place(llama_batch_ext_add_embd(batch, sequences - 1, embd))) return false;
        }
        return true;
    }
    // Runs the batch; writes `sequences` vectors of `dim` floats to `out`.
    bool run(float * out, char * err, int errlen) {
        int count = sequences;
        int32_t status = llama_process(context, LLAMA_PROCESS_TYPE_ENCODE, batch);
        bool ok = status == 0;
        if (!ok) fail(err, errlen, "llama_process failed (%d)", status);
        for (int s = 0; ok && s < count; s++) {
            const float * pooled = llama_get_embeddings_seq(context, s);
            if (!pooled) {
                fail(err, errlen, "no pooled embedding for input %d", s);
                ok = false;
                break;
            }
            double sum = 0;
            for (int i = 0; i < dim; i++) sum += (double) pooled[i] * (double) pooled[i];
            float scale = sum > 0 ? (float) (1.0 / std::sqrt(sum)) : 0.0f;
            for (int i = 0; i < dim; i++) out[s * dim + i] = pooled[i] * scale;
        }
        reset();
        return ok;
    }
    std::vector<llama_token> tokenize(const char * text, int len) {
        std::vector<llama_token> result(len + 8);
        int32_t count = llama_tokenize(vocab, text, len, result.data(), (int32_t) result.size(), true, true);
        if (count < 0) {
            result.resize(-count);
            count = llama_tokenize(vocab, text, len, result.data(), (int32_t) result.size(), true, true);
        }
        result.resize(count > 0 ? count : 0);
        if ((int) result.size() > max_tokens) {
            result.resize(max_tokens - 1);
            result.push_back(llama_vocab_eos(vocab));
        }
        return result;
    }
    bool embed_bitmap(const mtmd_bitmap * bitmap, float * out, char * err, int errlen) {
        mtmd_input_chunks * chunks = mtmd_input_chunks_init();
        const char * marker = mtmd_default_marker();
        mtmd_input_text text = {marker, strlen(marker), true, true};
        const mtmd_bitmap * bitmaps[1] = {bitmap};
        int32_t status = mtmd_tokenize(media, chunks, &text, bitmaps, 1);
        if (status != 0) {
            mtmd_input_chunks_free(chunks);
            fail(err, errlen, "can't tokenize the input (%d)", status);
            return false;
        }
        reset();
        begin();
        bool ok = true;
        for (size_t i = 0; ok && i < mtmd_input_chunks_size(chunks); i++) {
            const mtmd_input_chunk * chunk = mtmd_input_chunks_get(chunks, i);
            if (mtmd_input_chunk_get_type(chunk) == MTMD_INPUT_CHUNK_TYPE_TEXT) {
                size_t count = 0;
                const llama_token * tokens = mtmd_input_chunk_get_tokens_text(chunk, &count);
                for (size_t t = 0; ok && tokens && t < count; t++) ok = add_token(tokens[t]);
            } else {
                float * rows = mtmd_encode_chunk(media, chunk) == 0 ? mtmd_get_output_embd(media) : nullptr;
                if (!rows) {
                    fail(err, errlen, "the encoder failed");
                    mtmd_input_chunks_free(chunks);
                    reset();
                    return false;
                }
                ok = add_rows(rows, mtmd_input_chunk_get_n_tokens(chunk));
            }
        }
        mtmd_input_chunks_free(chunks);
        if (!ok) {
            fail(err, errlen, "the input doesn't fit the batch");
            reset();
            return false;
        }
        return run(out, err, errlen);
    }
};

extern "C" {

// Process-wide setup, once. `dir`: where ggml's backend DLLs are. Returns 1 when a GPU backend found a device.
int ge_init(const char * dir, int cpu_only) {
    if (g_ready) return g_gpu_found ? 1 : 0;
    g_ready = true;
    llama_log_set(log_callback, nullptr);
    mtmd_helper_log_set(log_callback, nullptr);
    if (dir && *dir) ggml_backend_load_all_from_path(dir);
    if (ggml_backend_dev_count() == 0) ggml_backend_load_all();
    for (size_t i = 0; i < ggml_backend_dev_count(); i++) {
        if (is_gpu(ggml_backend_dev_get(i))) g_gpu_found = true;
    }
    if (cpu_only) {
        // Unloaded rather than just unused: a GPU backend would still set itself up.
        for (size_t i = ggml_backend_reg_count(); i-- > 0;) {
            ggml_backend_reg_t registry = ggml_backend_reg_get(i);
            bool gpu = false;
            for (size_t d = 0; d < ggml_backend_reg_dev_count(registry); d++) {
                if (is_gpu(ggml_backend_reg_dev_get(registry, d))) gpu = true;
            }
            if (gpu) ggml_backend_unload(registry);
        }
    }
    llama_backend_init();
    return g_gpu_found ? 1 : 0;
}

// The loaded backends' names, comma separated.
void ge_backends(char * out, int len) {
    std::string names;
    for (size_t i = 0; i < ggml_backend_reg_count(); i++) {
        if (i) names += ", ";
        names += ggml_backend_reg_name(ggml_backend_reg_get(i));
    }
    snprintf(out, len, "%s", names.c_str());
}

ge_ctx * ge_open(const char * text_path, const char * media_path, int max_tokens, int threads, int use_gpu,
                 char * err, int errlen) {
    ge_ctx * ctx = new ge_ctx();
    llama_model_params model_params = llama_model_default_params();
    model_params.n_gpu_layers = use_gpu ? 999 : 0;
    ctx->model = llama_model_load_from_file(text_path, model_params);
    if (!ctx->model) {
        fail(err, errlen, "can't load %s", text_path);
        delete ctx;
        return nullptr;
    }
    llama_context_params params = llama_context_default_params();
    params.n_ctx = max_tokens;
    params.n_batch = max_tokens;
    params.n_ubatch = max_tokens;
    params.n_seq_max = MAX_SEQUENCES;
    params.kv_unified = true;
    params.embeddings = true;
    params.pooling_type = LLAMA_POOLING_TYPE_MEAN;
    params.n_threads = threads;
    params.n_threads_batch = threads;
    ctx->context = llama_init_from_model(ctx->model, params);
    if (!ctx->context) {
        fail(err, errlen, "can't create a llama.cpp context");
        llama_model_free(ctx->model);
        delete ctx;
        return nullptr;
    }
    if (media_path && *media_path) {
        mtmd_context_params media_params = mtmd_context_params_default();
        media_params.use_gpu = use_gpu != 0;
        media_params.print_timings = false;
        media_params.warmup = true;
        media_params.image_min_tokens = 1;   // never rescale: images arrive at their final size
        media_params.image_max_tokens = 1120;
        media_params.n_threads = threads;
        ctx->media = mtmd_init_from_file(media_path, ctx->model, media_params);
        if (!ctx->media) {
            fail(err, errlen, "can't load %s", media_path);
            llama_free(ctx->context);
            llama_model_free(ctx->model);
            delete ctx;
            return nullptr;
        }
    }
    ctx->batch = llama_batch_ext_init(ctx->context);
    ctx->vocab = llama_model_get_vocab(ctx->model);
    ctx->dim = llama_model_n_embd_out(ctx->model);
    ctx->row = llama_model_n_embd_inp(ctx->model);
    ctx->max_tokens = max_tokens;
    return ctx;
}

void ge_close(ge_ctx * ctx) {
    if (!ctx) return;
    if (ctx->batch) llama_batch_ext_free(ctx->batch);
    if (ctx->media) mtmd_free(ctx->media);
    if (ctx->context) llama_free(ctx->context);
    if (ctx->model) llama_model_free(ctx->model);
    delete ctx;
}

int ge_dim(const ge_ctx * ctx) { return ctx->dim; }

int ge_audio_rate(const ge_ctx * ctx) { return ctx->media ? mtmd_get_audio_sample_rate(ctx->media) : 16000; }

// `n` texts in, `n * dim` floats out. Returns 0, or -1 with `err` set.
int ge_embed_texts(ge_ctx * ctx, const char * const * texts, const int * lens, int n, float * out,
                   char * err, int errlen) {
    ctx->reset();
    int done = 0;
    for (int i = 0; i < n; i++) {
        std::vector<llama_token> tokens = ctx->tokenize(texts[i], lens[i]);
        if (ctx->sequences == MAX_SEQUENCES || ctx->tokens + (int) tokens.size() > ctx->max_tokens) {
            int count = ctx->sequences;
            if (!ctx->run(out + (size_t) done * ctx->dim, err, errlen)) return -1;
            done += count;
        }
        ctx->begin();
        for (llama_token token : tokens) {
            if (!ctx->add_token(token)) {
                fail(err, errlen, "batch full");
                ctx->reset();
                return -1;
            }
        }
    }
    if (ctx->sequences > 0 && !ctx->run(out + (size_t) done * ctx->dim, err, errlen)) return -1;
    return 0;
}

// Packed RGB at its final size (see sizing.rs).
int ge_embed_image(ge_ctx * ctx, const unsigned char * rgb, int width, int height, float * out,
                   char * err, int errlen) {
    if (!ctx->media) {
        fail(err, errlen, "this embedder was started text-only");
        return -1;
    }
    mtmd_bitmap * bitmap = mtmd_bitmap_init(width, height, rgb);
    if (!bitmap) {
        fail(err, errlen, "can't make a %dx%d bitmap", width, height);
        return -1;
    }
    bool ok = ctx->embed_bitmap(bitmap, out, err, errlen);
    mtmd_bitmap_free(bitmap);
    return ok ? 0 : -1;
}

// Mono float samples at ge_audio_rate; anything past 30 s is dropped, as in the reference.
int ge_embed_audio(ge_ctx * ctx, const float * samples, size_t count, float * out, char * err, int errlen) {
    if (!ctx->media) {
        fail(err, errlen, "this embedder was started text-only");
        return -1;
    }
    size_t limit = (size_t) 30 * mtmd_get_audio_sample_rate(ctx->media);
    mtmd_bitmap * bitmap = mtmd_bitmap_init_from_audio(count > limit ? limit : count, samples);
    if (!bitmap) {
        fail(err, errlen, "can't make an audio input");
        return -1;
    }
    bool ok = ctx->embed_bitmap(bitmap, out, err, errlen);
    mtmd_bitmap_free(bitmap);
    return ok ? 0 : -1;
}

// Decodes an image (stb_image) or audio file (miniaudio, resampled) held in memory, with llama.cpp's own readers.
// Returns a handle to free with ge_decoded_free, or null.
void * ge_decode(ge_ctx * ctx, const unsigned char * bytes, size_t len, int * is_audio, unsigned * width,
                 unsigned * height, const unsigned char ** data, size_t * data_len) {
    if (!ctx->media) return nullptr;
    mtmd_helper_bitmap_wrapper wrapper =
        mtmd_helper_bitmap_init_from_buf(ctx->media, bytes, len, false, mtmd_helper_init_opt_default());
    if (wrapper.video_ctx) {
        mtmd_helper_video_free(wrapper.video_ctx);
        if (wrapper.bitmap) mtmd_bitmap_free(wrapper.bitmap);
        return nullptr;
    }
    if (!wrapper.bitmap) return nullptr;
    *is_audio = mtmd_bitmap_is_audio(wrapper.bitmap) ? 1 : 0;
    *width = mtmd_bitmap_get_nx(wrapper.bitmap);
    *height = mtmd_bitmap_get_ny(wrapper.bitmap);
    *data = mtmd_bitmap_get_data(wrapper.bitmap);
    *data_len = mtmd_bitmap_get_n_bytes(wrapper.bitmap);
    return wrapper.bitmap;
}

void ge_decoded_free(void * handle) {
    if (handle) mtmd_bitmap_free((mtmd_bitmap *) handle);
}

} // extern "C"
