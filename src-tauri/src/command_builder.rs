use crate::types::{GenerationRequest, LoraSelection, ModelRef};

/// Engine knobs that aren't part of the generation request itself. A struct
/// (not a bare bool) leaves room for the remaining expert controls
/// (per-component `--backend`, `--params-backend`) without another signature churn.
#[derive(Debug, Clone, Default)]
pub struct EngineOptions {
    pub low_vram: bool,
    /// When `Some(path)`, enable a live preview written to `path`
    /// (`--preview proj --preview-interval 2`); `None` disables it.
    pub preview_path: Option<String>,
    /// When `Some(type)`, re-quantise the diffusion model at load time to that
    /// engine weight type (`q8_0`, `q5_1`, `q4_K`). See `DIFFUSION_TENSOR_RULES`.
    pub weight_type: Option<String>,
    /// Absolute path of the flat LoRA pool (`models_dir/loras`). `Some` only
    /// when the caller has validated every selection; `None` suppresses both
    /// `--lora-model-dir` and the prompt tags, because tags without a directory
    /// resolve against the engine's default path and silently apply nothing.
    pub lora_dir: Option<String>,
    /// Whether `req.ref_images` may reach the engine. `true` only when the
    /// caller has confirmed the resolved model belongs to an edit family and
    /// every path exists. `false` drops the list entirely: a `--ref-image` on a
    /// model that cannot consume one is not ignored by the engine, it changes
    /// the run. Mirrors the `lora_dir` gate above.
    pub ref_images: bool,
}

/// Tensor-name prefixes that identify diffusion-model weights.
///
/// Load-time quantisation is applied through `--tensor-type-rules` rather than
/// the global `--type` because `--type` also re-quantises the text encoder — and
/// when that encoder is an already-compact GGUF, the "quantisation" is an
/// *upcast*. Measured on engine `b290693` with a FLUX.2 klein q2_K encoder:
///
/// ```text
/// baseline                       22,296 MB   (diffusion 17,316 | encoder 4,816)
/// --type q8_0                    17,119 MB   (diffusion  9,291 | encoder 7,670)  ← encoder grew
/// --tensor-type-rules …=q8_0     14,271 MB   (diffusion  9,291 | encoder 4,816)
/// ```
///
/// Three prefixes because checkpoints differ in how deeply they namespace: the
/// ComfyUI export path writes `model.diffusion_model.*`, while many FLUX
/// checkpoints store bare `double_blocks.*` / `single_blocks.*`. Prefixes that
/// match nothing are simply inert.
const DIFFUSION_TENSOR_RULES: [&str; 3] =
    [r"^model\.diffusion_model\.", r"^double_blocks\.", r"^single_blocks\."];

/// The prompt as the engine should receive it: the user's text followed by one
/// `<lora:NAME:WEIGHT>` tag per selection.
///
/// This is how stable-diffusion.cpp selects LoRAs — there is no CLI flag for
/// it. The engine strips each tag before tokenisation, so the text the model
/// actually sees is unchanged; weights are formatted `%.2f` to match the
/// engine's own formatting. The user's stored prompt is never modified, only
/// this copy.
fn prompt_with_loras(prompt: &str, loras: &[LoraSelection]) -> String {
    if loras.is_empty() {
        return prompt.to_string();
    }
    let tags = loras
        .iter()
        .map(|l| format!("<lora:{}:{:.2}>", l.name, l.weight))
        .collect::<Vec<_>>()
        .join(" ");
    if prompt.is_empty() {
        tags
    } else {
        format!("{prompt} {tags}")
    }
}

/// Whether this model carries a vision tower, i.e. whether the run will emit
/// `--llm_vision`. See the graph-cut suppression in `build_args`.
fn has_vision_tower(model: &ModelRef) -> bool {
    matches!(model, ModelRef::MultiFile(c) if c.llm_vision.is_some())
}

/// Build the argument vector for stable-diffusion.cpp's CLI.
/// Pure function (no I/O) so it is fully unit-testable.
/// Flag spellings are confirmed against `fixtures/sd-help.txt`.
pub fn build_args(
    req: &GenerationRequest,
    output_path: &str,
    backend: Option<&str>,
    opts: EngineOptions,
) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    a.push("-M".into());
    a.push("img_gen".into());
    // Selections are honoured only alongside a pool directory (see EngineOptions).
    let loras: &[LoraSelection] =
        if opts.lora_dir.is_some() { req.loras.as_slice() } else { &[] };
    // Same shape for reference images: intent lives on the request, permission
    // lives on the options.
    let ref_images: &[String] = if opts.ref_images { req.ref_images.as_slice() } else { &[] };
    let mut push = |flag: &str, val: String| {
        a.push(flag.to_string());
        a.push(val);
    };
    match &req.model {
        ModelRef::SingleFile { path } => push("-m", path.clone()),
        ModelRef::MultiFile(c) => {
            push("--diffusion-model", c.diffusion_model.clone());
            if let Some(v) = &c.vae {
                push("--vae", v.clone());
            }
            if let Some(v) = &c.clip_l {
                push("--clip_l", v.clone());
            }
            if let Some(v) = &c.clip_g {
                push("--clip_g", v.clone());
            }
            if let Some(v) = &c.t5xxl {
                push("--t5xxl", v.clone());
            }
            if let Some(v) = &c.llm {
                push("--llm", v.clone());
            }
            if let Some(v) = &c.llm_vision {
                push("--llm_vision", v.clone());
            }
            if let Some(v) = &c.vae_format {
                push("--vae-format", v.clone());
            }
            if let Some(v) = &c.prediction {
                push("--prediction", v.clone());
            }
        }
    }
    for r in ref_images {
        push("--ref-image", r.clone());
    }
    push("-p", prompt_with_loras(&req.prompt, loras));
    if !req.negative_prompt.is_empty() {
        push("-n", req.negative_prompt.clone());
    }
    push("--steps", req.steps.to_string());
    push("--cfg-scale", format!("{}", req.cfg_scale));
    push("--sampling-method", req.sampler.cli_name().to_string());
    push("-W", req.width.to_string());
    push("-H", req.height.to_string());
    push("-s", req.seed.to_string());
    push("-b", req.batch_count.to_string());
    push("-o", output_path.to_string());
    if let Some(b) = backend {
        a.push("--backend".into());
        a.push(b.to_string());
    }
    if !loras.is_empty() {
        a.push("--lora-model-dir".into());
        a.push(opts.lora_dir.clone().expect("lora_dir is Some when loras is non-empty"));
    }
    if let Some(t) = &opts.weight_type {
        a.push("--tensor-type-rules".into());
        let rules: Vec<String> =
            DIFFUSION_TENSOR_RULES.iter().map(|p| format!("{p}={t}")).collect();
        a.push(rules.join(","));
    }
    // Flash attention and tiled VAE decode are NOT low-VRAM relief, despite
    // having shipped inside that bundle: they cap the size of an *individual*
    // buffer, and the Vulkan per-allocation ceiling does not grow when the card
    // does. Gating them on `fit::resolve_low_vram` made a GPU upgrade a
    // regression — measured 2026-08-21, qwen-image-edit-2511-Q3_K_S + a
    // Qwen2.5-VL-7B encoder estimates 18510 MB, which is over a 3060's 12010 MB
    // (bundle on) but under an A5000's 24564 MB (bundle off). With the bundle
    // off, the same 1024x1024 edit:
    //
    //   --backend vulkan1                        never reached step 1 in 4+ min;
    //                                            GPU 0%, CPU 812%, and
    //                                            "ggml_vulkan: Failed to allocate
    //                                            pinned memory (Requested buffer
    //                                            size exceeds device buffer size
    //                                            limit)" -> silent CPU fallback
    //   + --diffusion-fa                         ~21 s/it on GPU, then the VAE
    //                                            decode asks for one 16.5 GB
    //                                            buffer and fails
    //   + --diffusion-fa --vae-tiling            ~21 s/it, decode buffer 469 MB
    //                                            in 11.9 s, image written
    //
    // So both are unconditional. They do shift output slightly (flash attention
    // reorders accumulation; tiling decodes in blended tiles of --vae-tile-size,
    // default 32x32 latent), which is the accepted cost of one code path that
    // cannot stall on any card.
    a.push("--vae-tiling".into());
    a.push("--diffusion-fa".into());
    if opts.low_vram {
        // Weights paged from RAM — the actual capacity/speed trade, and the only
        // part of the old bundle that belongs behind the Low-VRAM gate.
        a.push("--offload-to-cpu".into());
    }
    // Graph-cut is part of the low-VRAM bundle, but only for runs with no vision
    // tower: `master-813-bfbef5b` aborts (SIGABRT in `ggml_graph_cut::
    // build_segment`, via `LLMRunner::encode_image`) when the planner tries to
    // segment the vision tower's graph. Measured on an RTX 3060: it aborts with
    // and without `--tensor-type-rules`, and both runs without graph-cut got
    // past `get_learned_condition` — so graph-cut alone is the cause, and
    // dropping it is what makes editing run at all. Keyed off the component and
    // not off `ref_images` because the two are equivalent at this point:
    // `resolve_ref_images` rejects a vision-tower model with no reference before
    // anything spawns.
    if opts.low_vram && !has_vision_tower(&req.model) {
        // Graph-cut segmented execution against a VRAM budget, plus
        // residency+prefetch streaming of layers (inert without --max-vram).
        // A negative budget makes the engine auto-detect free VRAM and spare
        // that many GiB, which beats any number the app could guess: measured
        // "auto-detected 11.83 GiB free VRAM (12.24 GiB total), reserving
        // 1.00 GiB; using 10.83 GiB".
        //
        // NOT `--auto-fit`, despite it being the more automatic-sounding flag:
        // its own help says it overrides `--backend`, so it can silently move
        // work onto a weaker integrated GPU that the user did not select.
        //
        // The value must be a separate argv entry — the engine's parser rejects
        // `--max-vram=-1` as an unknown argument.
        a.push("--max-vram".into());
        a.push("-1".into());
        a.push("--stream-layers".into());
    }
    if let Some(p) = &opts.preview_path {
        // Cheap linear latent→RGB projection written every 2 steps; the app
        // watches the file to show a live draft so the user can cancel early.
        a.push("--preview".into());
        a.push("proj".into());
        a.push("--preview-path".into());
        a.push(p.clone());
        a.push("--preview-interval".into());
        a.push("2".into());
    }
    a.push("-v".into()); // verbose: ensures progress lines are emitted
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Sampler;

    fn sample() -> GenerationRequest {
        GenerationRequest {
            model: ModelRef::SingleFile { path: "/m/model.safetensors".into() },
            prompt: "a cat".into(),
            negative_prompt: "blurry".into(),
            steps: 25,
            cfg_scale: 7.5,
            sampler: Sampler::DpmPp2m,
            width: 768,
            height: 512,
            seed: 42,
            batch_count: 2,
            ..Default::default()
        }
    }

    /// Helper: value immediately following a flag.
    fn val_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.iter().position(|x| x == flag).map(|i| args[i + 1].as_str())
    }

    #[test]
    fn includes_core_flags_and_values() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions::default());
        assert_eq!(val_after(&args, "-m"), Some("/m/model.safetensors"));
        assert_eq!(val_after(&args, "-p"), Some("a cat"));
        assert_eq!(val_after(&args, "-n"), Some("blurry"));
        assert_eq!(val_after(&args, "--steps"), Some("25"));
        assert_eq!(val_after(&args, "--cfg-scale"), Some("7.5"));
        assert_eq!(val_after(&args, "--sampling-method"), Some("dpm++2m"));
        assert_eq!(val_after(&args, "-W"), Some("768"));
        assert_eq!(val_after(&args, "-H"), Some("512"));
        assert_eq!(val_after(&args, "-s"), Some("42"));
        assert_eq!(val_after(&args, "-b"), Some("2"));
        assert_eq!(val_after(&args, "-o"), Some("/out/x.png"));
    }

    #[test]
    fn uses_img_gen_mode() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions::default());
        assert_eq!(val_after(&args, "-M"), Some("img_gen"));
    }

    #[test]
    fn omits_negative_prompt_when_empty() {
        let mut req = sample();
        req.negative_prompt = "".into();
        let args = build_args(&req, "/out/x.png", None, EngineOptions::default());
        assert!(!args.iter().any(|x| x == "-n"));
    }

    #[test]
    fn appends_backend_when_some() {
        let args = build_args(&sample(), "/out/x.png", Some("vulkan1"), EngineOptions::default());
        assert_eq!(val_after(&args, "--backend"), Some("vulkan1"));
    }

    #[test]
    fn omits_backend_when_none() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions::default());
        assert!(!args.iter().any(|x| x == "--backend"));
    }

    #[test]
    fn single_file_emits_dash_m_and_no_diffusion_model() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions::default());
        assert_eq!(val_after(&args, "-m"), Some("/m/model.safetensors"));
        assert!(!args.iter().any(|x| x == "--diffusion-model"));
    }

    #[test]
    fn multi_file_maps_each_role_to_its_flag() {
        use crate::types::ModelComponents;
        let mut req = sample();
        req.model = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/flux1-dev.safetensors".into(),
            t5xxl: Some("/m/t5xxl.safetensors".into()),
            clip_l: Some("/m/clip_l.safetensors".into()),
            clip_g: Some("/m/clip_g.safetensors".into()),
            llm: Some("/m/llm.safetensors".into()),
            llm_vision: Some("/m/mmproj.safetensors".into()),
            vae: Some("/m/ae.safetensors".into()),
            vae_format: Some("flux".into()),
            prediction: Some("flux_flow".into()),
        });
        let args = build_args(&req, "/out/x.png", None, EngineOptions::default());
        assert_eq!(val_after(&args, "--diffusion-model"), Some("/m/flux1-dev.safetensors"));
        assert_eq!(val_after(&args, "--t5xxl"), Some("/m/t5xxl.safetensors"));
        assert_eq!(val_after(&args, "--clip_l"), Some("/m/clip_l.safetensors"));
        assert_eq!(val_after(&args, "--clip_g"), Some("/m/clip_g.safetensors"));
        assert_eq!(val_after(&args, "--llm"), Some("/m/llm.safetensors"));
        assert_eq!(val_after(&args, "--llm_vision"), Some("/m/mmproj.safetensors"));
        assert_eq!(val_after(&args, "--vae"), Some("/m/ae.safetensors"));
        assert_eq!(val_after(&args, "--vae-format"), Some("flux"));
        assert_eq!(val_after(&args, "--prediction"), Some("flux_flow"));
        assert!(!args.iter().any(|x| x == "-m"), "multi-file must not emit -m");
    }

    #[test]
    fn a_vision_tower_emits_llm_vision() {
        use crate::types::ModelComponents;
        let mut req = sample();
        req.model = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/qwen-edit.gguf".into(),
            llm: Some("/m/shared/qwen2.5-vl.gguf".into()),
            llm_vision: Some("/m/shared/mmproj.gguf".into()),
            ..Default::default()
        });
        let args = build_args(&req, "/out/x.png", None, EngineOptions::default());
        assert_eq!(val_after(&args, "--llm_vision"), Some("/m/shared/mmproj.gguf"));
    }

    #[test]
    fn multi_file_omits_absent_optional_roles() {
        use crate::types::ModelComponents;
        let mut req = sample();
        req.model = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/d.safetensors".into(),
            ..Default::default()
        });
        let args = build_args(&req, "/out/x.png", None, EngineOptions::default());
        assert_eq!(val_after(&args, "--diffusion-model"), Some("/m/d.safetensors"));
        for flag in ["--vae", "--clip_l", "--clip_g", "--t5xxl", "--llm", "--llm_vision", "--vae-format", "--prediction"] {
            assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
        }
    }

    #[test]
    fn output_path_extension_passes_through_verbatim() {
        // build_args is format-agnostic: whatever extension the caller chose on
        // the -o path is forwarded unchanged (the engine infers format from it).
        let args = build_args(&sample(), "/out/x.jpg", None, EngineOptions::default());
        assert_eq!(val_after(&args, "-o"), Some("/out/x.jpg"));
    }

    #[test]
    fn low_vram_appends_offload_flags() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions { low_vram: true, ..Default::default() });
        assert!(args.iter().any(|x| x == "--offload-to-cpu"));
        assert!(args.iter().any(|x| x == "--vae-tiling"));
        assert!(args.iter().any(|x| x == "--diffusion-fa"));
    }

    #[test]
    fn low_vram_appends_graph_cut_and_streaming() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions { low_vram: true, ..Default::default() });
        assert_eq!(val_after(&args, "--max-vram"), Some("-1"));
        assert!(args.iter().any(|x| x == "--stream-layers"));
    }

    #[test]
    fn a_vision_tower_keeps_the_offload_flags_but_loses_graph_cut() {
        use crate::types::ModelComponents;
        let mut req = sample();
        req.model = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/qwen-edit.gguf".into(),
            llm: Some("/m/shared/qwen2.5-vl.gguf".into()),
            llm_vision: Some("/m/shared/mmproj.gguf".into()),
            ..Default::default()
        });
        let args =
            build_args(&req, "/out/x.png", None, EngineOptions { low_vram: true, ..Default::default() });
        // Offload is what lets a 16 GB stack run on a 12 GB card at all.
        for flag in ["--offload-to-cpu", "--vae-tiling", "--diffusion-fa"] {
            assert!(args.iter().any(|x| x == flag), "{flag} must survive");
        }
        // Graph-cut is what aborts on the vision tower's encode_image.
        for flag in ["--max-vram", "--stream-layers"] {
            assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
        }
    }

    #[test]
    fn max_vram_value_is_a_separate_argv_entry() {
        // The engine's parser rejects `--max-vram=-1` outright; joining them
        // would break every low-VRAM run with "unknown argument".
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions { low_vram: true, ..Default::default() });
        assert!(args.iter().any(|x| x == "--max-vram"));
        assert!(!args.iter().any(|x| x.contains("--max-vram=")));
    }

    #[test]
    fn never_emits_auto_fit() {
        // --auto-fit overrides --backend, which would let the engine relocate
        // work to a GPU the user didn't pick. Pinned so it can't creep back in.
        for low_vram in [true, false] {
            let args = build_args(&sample(), "/out/x.png", Some("vulkan1"), EngineOptions { low_vram, ..Default::default() });
            assert!(!args.iter().any(|x| x == "--auto-fit"));
        }
    }

    #[test]
    fn low_vram_off_omits_offload_flags() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions { low_vram: false, ..Default::default() });
        for flag in ["--offload-to-cpu", "--max-vram", "--stream-layers"] {
            assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
        }
    }

    #[test]
    fn flash_attention_and_vae_tiling_do_not_depend_on_low_vram() {
        // Regression: these two were bundled with --offload-to-cpu behind the
        // Low-VRAM gate, so swapping a 12 GB card for a 24 GB one pushed
        // qwen-image-edit under fit::resolve_low_vram's threshold and silently
        // dropped them. They guard the Vulkan *per-allocation* ceiling, which
        // does not grow with VRAM: without --diffusion-fa the attention buffer
        // overflows it and ggml falls back to CPU mid-run (GPU idle, never
        // reaches step 1); without --vae-tiling the decode asks for a single
        // 16.5 GB buffer and fails outright.
        for low_vram in [true, false] {
            let args = build_args(&sample(), "/out/x.png", Some("vulkan1"), EngineOptions { low_vram, ..Default::default() });
            for flag in ["--diffusion-fa", "--vae-tiling"] {
                assert!(args.iter().any(|x| x == flag), "{flag} must be present with low_vram={low_vram}");
            }
        }
    }

    #[test]
    fn an_edit_model_on_a_card_it_fits_still_gets_the_vulkan_buffer_caps() {
        // The exact reported scenario: qwen-image-edit on a 24 GB card, so
        // fit::resolve_low_vram says the stack fits and Low-VRAM stays off. The
        // buffer caps must still be there, offload must not be, and graph-cut
        // must stay suppressed off the vision tower rather than off low_vram.
        use crate::types::ModelComponents;
        let mut req = sample();
        req.width = 1024;
        req.height = 1024;
        req.model = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/qwen-image-edit-2511-Q3_K_S.gguf".into(),
            llm: Some("/m/shared/qwen-image/Qwen2.5-VL-7B-Instruct.gguf".into()),
            llm_vision: Some("/m/shared/qwen-image/mmproj-f16.gguf".into()),
            ..Default::default()
        });
        let args =
            build_args(&req, "/out/x.png", Some("vulkan1"), EngineOptions { low_vram: false, ..Default::default() });
        for flag in ["--diffusion-fa", "--vae-tiling"] {
            assert!(args.iter().any(|x| x == flag), "{flag} must be present");
        }
        for flag in ["--offload-to-cpu", "--max-vram", "--stream-layers"] {
            assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
        }
    }

    #[test]
    fn flash_attention_and_vae_tiling_are_emitted_once() {
        // Un-bundling must not leave a duplicate behind the low_vram branch.
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions { low_vram: true, ..Default::default() });
        for flag in ["--diffusion-fa", "--vae-tiling"] {
            assert_eq!(args.iter().filter(|x| *x == flag).count(), 1, "{flag} must appear exactly once");
        }
    }

    #[test]
    fn weight_type_emits_diffusion_only_tensor_rules() {
        let opts = EngineOptions { weight_type: Some("q8_0".into()), ..Default::default() };
        let args = build_args(&sample(), "/out/x.png", None, opts);
        let rules = val_after(&args, "--tensor-type-rules").expect("rules present");
        assert_eq!(
            rules,
            r"^model\.diffusion_model\.=q8_0,^double_blocks\.=q8_0,^single_blocks\.=q8_0"
        );
    }

    #[test]
    fn weight_type_never_emits_the_global_type_flag() {
        // Global --type would re-quantise the text encoder too, which upcasts an
        // already-compact GGUF and costs more RAM than it saves.
        let opts = EngineOptions { weight_type: Some("q4_K".into()), ..Default::default() };
        let args = build_args(&sample(), "/out/x.png", None, opts);
        assert!(!args.iter().any(|x| x == "--type"));
    }

    #[test]
    fn no_weight_type_omits_tensor_rules() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions::default());
        assert!(!args.iter().any(|x| x == "--tensor-type-rules"));
    }

    #[test]
    fn preview_flags_present_when_preview_path_some() {
        let opts = EngineOptions { preview_path: Some("/tmp/p/preview.png".into()), ..Default::default() };
        let args = build_args(&sample(), "/out/x.png", None, opts);
        assert_eq!(val_after(&args, "--preview"), Some("proj"));
        assert_eq!(val_after(&args, "--preview-path"), Some("/tmp/p/preview.png"));
        assert_eq!(val_after(&args, "--preview-interval"), Some("2"));
    }

    #[test]
    fn preview_flags_absent_when_preview_path_none() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions::default());
        for flag in ["--preview", "--preview-path", "--preview-interval"] {
            assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
        }
    }

    fn with_loras(pairs: &[(&str, f32)]) -> GenerationRequest {
        let mut req = sample();
        req.loras =
            pairs.iter().map(|(n, w)| LoraSelection { name: (*n).into(), weight: *w }).collect();
        req
    }

    fn lora_opts() -> EngineOptions {
        EngineOptions { lora_dir: Some("/models/loras".into()), ..Default::default() }
    }

    #[test]
    fn lora_tags_are_appended_to_the_prompt_in_order() {
        // The engine has no CLI flag for LoRA selection: the tag rides in the
        // prompt and is stripped before tokenisation.
        let req = with_loras(&[("film-grain", 0.8), ("detail-tweaker", 1.0)]);
        let args = build_args(&req, "/out/x.png", None, lora_opts());
        assert_eq!(
            val_after(&args, "-p"),
            Some("a cat <lora:film-grain:0.80> <lora:detail-tweaker:1.00>")
        );
    }

    #[test]
    fn lora_weights_use_two_decimals() {
        // The engine formats weights as %.2f internally; matching it keeps the
        // argv we log identical to what the engine reports back.
        let req = with_loras(&[("a", 0.5), ("b", 1.0), ("c", 1.333), ("d", 0.0)]);
        let args = build_args(&req, "/out/x.png", None, lora_opts());
        assert_eq!(
            val_after(&args, "-p"),
            Some("a cat <lora:a:0.50> <lora:b:1.00> <lora:c:1.33> <lora:d:0.00>")
        );
    }

    #[test]
    fn lora_model_dir_is_emitted_once_when_a_lora_is_selected() {
        let args = build_args(&with_loras(&[("film-grain", 0.8)]), "/out/x.png", None, lora_opts());
        assert_eq!(val_after(&args, "--lora-model-dir"), Some("/models/loras"));
        assert_eq!(args.iter().filter(|x| *x == "--lora-model-dir").count(), 1);
    }

    #[test]
    fn tags_are_the_whole_prompt_when_the_prompt_is_empty() {
        let mut req = with_loras(&[("film-grain", 0.8)]);
        req.prompt = String::new();
        let args = build_args(&req, "/out/x.png", None, lora_opts());
        assert_eq!(val_after(&args, "-p"), Some("<lora:film-grain:0.80>"));
    }

    #[test]
    fn no_selection_produces_a_byte_identical_command() {
        // A user who never touches LoRAs must get exactly today's argv — no
        // stray flag, no trailing space on the prompt.
        let baseline = build_args(&sample(), "/out/x.png", Some("vulkan1"), EngineOptions::default());
        let with_dir = build_args(&sample(), "/out/x.png", Some("vulkan1"), lora_opts());
        assert_eq!(with_dir, baseline);
        assert!(!with_dir.iter().any(|x| x == "--lora-model-dir"));
    }

    #[test]
    fn selection_is_ignored_when_no_pool_directory_is_supplied() {
        // Without a directory the engine would resolve tags against its own
        // default path and silently apply nothing. Emitting neither is honest.
        let args = build_args(&with_loras(&[("film-grain", 0.8)]), "/out/x.png", None, EngineOptions::default());
        assert_eq!(val_after(&args, "-p"), Some("a cat"));
        assert!(!args.iter().any(|x| x == "--lora-model-dir"));
    }

    #[test]
    fn never_emits_the_lora_apply_mode_flag() {
        // `auto` is already correct (at_runtime for quantised weights,
        // immediately otherwise) and an override is a footgun. Pinned.
        let args = build_args(&with_loras(&[("a", 1.0)]), "/out/x.png", None, lora_opts());
        assert!(!args.iter().any(|x| x == "--lora-apply-mode"));
    }

    fn with_refs(paths: &[&str]) -> GenerationRequest {
        let mut req = sample();
        req.ref_images = paths.iter().map(|p| p.to_string()).collect();
        req
    }

    fn ref_opts() -> EngineOptions {
        EngineOptions { ref_images: true, ..Default::default() }
    }

    #[test]
    fn a_reference_image_becomes_a_ref_image_flag() {
        let args = build_args(&with_refs(&["/pics/cat.png"]), "/out/x.png", None, ref_opts());
        assert_eq!(val_after(&args, "--ref-image"), Some("/pics/cat.png"));
    }

    #[test]
    fn every_reference_image_gets_its_own_flag() {
        let args = build_args(
            &with_refs(&["/pics/a.png", "/pics/b.png"]),
            "/out/x.png",
            None,
            ref_opts(),
        );
        let refs: Vec<&str> = args
            .windows(2)
            .filter(|w| w[0] == "--ref-image")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(refs, vec!["/pics/a.png", "/pics/b.png"], "order is preserved");
    }

    #[test]
    fn reference_images_are_dropped_when_the_caller_did_not_enable_them() {
        let args = build_args(&with_refs(&["/pics/cat.png"]), "/out/x.png", None, EngineOptions::default());
        assert!(
            !args.iter().any(|a| a == "--ref-image"),
            "a ref image on a model that cannot edit must never reach the engine"
        );
        assert!(
            !args.iter().any(|a| a == "/pics/cat.png"),
            "and neither must its path"
        );
    }

    #[test]
    fn no_reference_image_produces_a_byte_identical_command() {
        let baseline = build_args(&sample(), "/out/x.png", Some("vulkan1"), EngineOptions::default());
        let enabled = build_args(&sample(), "/out/x.png", Some("vulkan1"), ref_opts());
        assert_eq!(
            baseline, enabled,
            "enabling the gate with an empty list must not perturb an ordinary run"
        );
    }
}
