use crate::types::{GenerationRequest, LoraSelection, ModelRef};

/// Engine knobs that aren't part of the generation request itself. A struct
/// leaves room for further knobs without another signature churn.
#[derive(Debug, Clone, Default)]
pub struct EngineOptions {
    /// When `Some(path)`, enable a live preview written to `path`
    /// (`--preview proj --preview-interval 2`); `None` disables it.
    pub preview_path: Option<String>,
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
            // Only a reference image uses it, and an edit-capable model can
            // also run from text alone.
            if let (Some(v), false) = (&c.llm_vision, ref_images.is_empty()) {
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
    // Flash attention and tiled VAE decode cap the size of an *individual*
    // buffer, and the Vulkan per-allocation ceiling does not grow when the card
    // does — they are not memory-budget relief, so they are not left to
    // `--auto-fit`. Measured 2026-08-21, qwen-image-edit-2511-Q3_K_S + a
    // Qwen2.5-VL-7B encoder at 1024x1024:
    //
    //   --backend vulkan1                        never reached step 1 in 4+ min;
    //                                            GPU 0%, CPU 812%, and
    //                                            "ggml_vulkan: Failed to allocate
    //                                            pinned memory (Requested buffer
    //                                            size exceeds device buffer size
    //                                            limit)" -> silent CPU fallback
    //   + flash attention                        ~21 s/it on GPU, then the VAE
    //                                            decode asks for one 16.5 GB
    //                                            buffer and fails
    //   + flash attention + --vae-tiling         ~21 s/it, decode buffer 469 MB
    //                                            in 11.9 s, image written
    //
    // So both are unconditional, at the accepted cost of slightly shifted
    // output (flash attention reorders accumulation; tiling decodes in blended
    // tiles). Flash attention is forced on the diffusion model only, where the
    // measurement above was taken, and left at the engine's `auto` for the
    // other modules: plain `auto` is free to decide against it on the one
    // module where that must not move.
    a.push("--vae-tiling".into());
    a.push("--flash-attn".into());
    a.push("auto,diffusion=on".into());
    // Where the weights live is the engine's call: `--auto-fit` is on by
    // default and places them on the compute GPU, then RAM, then disk, keeping
    // `--backend`. Nothing here may override it — `--params-backend` would
    // switch it off, and `--offload-to-cpu` / `--max-vram` second-guess it.
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
        // No reference image: see `the_vision_tower_goes_only_with_a_reference_image`.
        assert!(!args.iter().any(|x| x == "--llm_vision"));
        assert_eq!(val_after(&args, "--vae"), Some("/m/ae.safetensors"));
        assert_eq!(val_after(&args, "--vae-format"), Some("flux"));
        assert_eq!(val_after(&args, "--prediction"), Some("flux_flow"));
        assert!(!args.iter().any(|x| x == "-m"), "multi-file must not emit -m");
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

    fn edit_model() -> GenerationRequest {
        use crate::types::ModelComponents;
        let mut req = sample();
        req.model = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/qwen-image-edit-2511-Q3_K_S.gguf".into(),
            llm: Some("/m/shared/qwen-image/Qwen2.5-VL-7B-Instruct.gguf".into()),
            llm_vision: Some("/m/shared/qwen-image/mmproj-f16.gguf".into()),
            ..Default::default()
        });
        req
    }

    /// The vision tower only reads reference images. Qwen-Image 2.1 also
    /// generates from text, and that ran on the RTX 3060 without it; loading
    /// it anyway costs ~1.1 GB for nothing.
    #[test]
    fn the_vision_tower_goes_only_with_a_reference_image() {
        let mut req = edit_model();
        let gate_open = EngineOptions { ref_images: true, ..Default::default() };
        let args = build_args(&req, "/out/x.png", None, gate_open.clone());
        assert!(!args.iter().any(|x| x == "--llm_vision"), "no reference, no vision tower");

        req.ref_images = vec!["/in/ref.png".into()];
        let args = build_args(&req, "/out/x.png", None, gate_open);
        assert_eq!(val_after(&args, "--llm_vision"), Some("/m/shared/qwen-image/mmproj-f16.gguf"));

        // A reference the gate dropped takes the vision tower with it.
        let args = build_args(&req, "/out/x.png", None, EngineOptions::default());
        assert!(!args.iter().any(|x| x == "--llm_vision"));
    }

    #[test]
    fn flash_attention_and_vae_tiling_are_always_emitted_once() {
        // They cap the size of a single buffer against the Vulkan per-allocation
        // ceiling, which does not depend on the model or the card. Spelled the
        // 995 way: `--diffusion-fa` is a deprecated alias there.
        for req in [sample(), edit_model()] {
            let args = build_args(&req, "/out/x.png", Some("vulkan1"), EngineOptions::default());
            assert_eq!(val_after(&args, "--flash-attn"), Some("auto,diffusion=on"));
            assert_eq!(args.iter().filter(|x| *x == "--flash-attn").count(), 1);
            assert_eq!(args.iter().filter(|x| *x == "--vae-tiling").count(), 1);
            assert!(!args.iter().any(|x| x == "--diffusion-fa"));
        }
    }

    #[test]
    fn memory_placement_is_left_to_the_engines_auto_fit() {
        // The engine's default `--auto-fit on` places weights GPU → RAM → disk
        // itself. Any of these would override or disable it (`--params-backend`
        // turns it off outright) or shrink the model behind the user's back.
        for req in [sample(), edit_model()] {
            let args = build_args(&req, "/out/x.png", Some("vulkan1"), EngineOptions::default());
            for flag in [
                "--auto-fit",
                "--params-backend",
                "--offload-to-cpu",
                "--max-vram",
                "--stream-layers",
                "--tensor-type-rules",
                "--type",
            ] {
                assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
            }
        }
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
