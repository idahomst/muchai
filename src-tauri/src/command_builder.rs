use crate::types::{GenerationRequest, ModelRef};

/// Build the argument vector for stable-diffusion.cpp's CLI.
/// Pure function (no I/O) so it is fully unit-testable.
/// Flag spellings are confirmed against `fixtures/sd-help.txt`.
pub fn build_args(req: &GenerationRequest, output_path: &str, backend: Option<&str>) -> Vec<String> {
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
        let args = build_args(&sample(), "/out/x.png", None);
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
        let args = build_args(&sample(), "/out/x.png", None);
        assert_eq!(val_after(&args, "-M"), Some("img_gen"));
    }

    #[test]
    fn omits_negative_prompt_when_empty() {
        let mut req = sample();
        req.negative_prompt = "".into();
        let args = build_args(&req, "/out/x.png", None);
        assert!(!args.iter().any(|x| x == "-n"));
    }

    #[test]
    fn appends_backend_when_some() {
        let args = build_args(&sample(), "/out/x.png", Some("vulkan1"));
        assert_eq!(val_after(&args, "--backend"), Some("vulkan1"));
    }

    #[test]
    fn omits_backend_when_none() {
        let args = build_args(&sample(), "/out/x.png", None);
        assert!(!args.iter().any(|x| x == "--backend"));
    }

    #[test]
    fn single_file_emits_dash_m_and_no_diffusion_model() {
        let args = build_args(&sample(), "/out/x.png", None);
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
        let args = build_args(&req, "/out/x.png", None);
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
        let args = build_args(&req, "/out/x.png", None);
        assert_eq!(val_after(&args, "--diffusion-model"), Some("/m/d.safetensors"));
        for flag in ["--vae", "--clip_l", "--clip_g", "--t5xxl", "--llm", "--vae-format", "--prediction"] {
            assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
        }
    }

    #[test]
    fn output_path_extension_passes_through_verbatim() {
        // build_args is format-agnostic: whatever extension the caller chose on
        // the -o path is forwarded unchanged (the engine infers format from it).
        let args = build_args(&sample(), "/out/x.jpg", None);
        assert_eq!(val_after(&args, "-o"), Some("/out/x.jpg"));
    }
}
