use crate::types::{GenerationRequest, ModelRef};

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
            if let Some(v) = &c.vae_format {
                push("--vae-format", v.clone());
            }
            if let Some(v) = &c.prediction {
                push("--prediction", v.clone());
            }
        }
    }
    push("-p", req.prompt.clone());
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
    if let Some(t) = &opts.weight_type {
        a.push("--tensor-type-rules".into());
        let rules: Vec<String> =
            DIFFUSION_TENSOR_RULES.iter().map(|p| format!("{p}={t}")).collect();
        a.push(rules.join(","));
    }
    if opts.low_vram {
        // Weights paged from RAM, tiled VAE decode, flash attention — the
        // low-VRAM/high-headroom bundle so models larger than VRAM can run.
        a.push("--offload-to-cpu".into());
        a.push("--vae-tiling".into());
        a.push("--diffusion-fa".into());
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
            vae: Some("/m/ae.safetensors".into()),
            vae_format: Some("flux".into()),
            prediction: Some("flux_flow".into()),
            ..Default::default()
        });
        let args = build_args(&req, "/out/x.png", None, EngineOptions::default());
        assert_eq!(val_after(&args, "--diffusion-model"), Some("/m/flux1-dev.safetensors"));
        assert_eq!(val_after(&args, "--t5xxl"), Some("/m/t5xxl.safetensors"));
        assert_eq!(val_after(&args, "--clip_l"), Some("/m/clip_l.safetensors"));
        assert_eq!(val_after(&args, "--clip_g"), Some("/m/clip_g.safetensors"));
        assert_eq!(val_after(&args, "--llm"), Some("/m/llm.safetensors"));
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
        for flag in ["--vae", "--clip_l", "--clip_g", "--t5xxl", "--llm", "--vae-format", "--prediction"] {
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
        for flag in ["--offload-to-cpu", "--vae-tiling", "--diffusion-fa", "--max-vram", "--stream-layers"] {
            assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
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
}
