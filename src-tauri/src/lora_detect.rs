//! Which base model a LoRA was trained for, inferred from its tensor names.
//!
//! There is no field in a LoRA that states its base model, so this reads the
//! naming convention. There is no single convention — the four real files
//! surveyed below use four different ones:
//!
//! - **kohya** (SD1.5, SDXL) — `lora_unet_down_blocks_0_…`, `.lora_down.weight`
//! - **XLabs** (FLUX.1) — `double_blocks.0.processor.qkv_lora1.down.weight`
//! - **diffusers/kohya hybrid** (Qwen-Image) — `transformer_blocks.0.attn.…`
//!   with kohya's `.lora_down` / `.lora_up` / `.alpha` leaves
//! - **diffusers** — `unet.down_blocks.0.…`, `text_encoder_2.…`, `.lora_A/.lora_B`
//!
//! All four are handled. The rules are pinned against real files rather than
//! documentation; see `fixtures/lora-headers/` and the tests at the bottom of
//! this file. Survey run 2026-07-27 — count of tensor names containing each
//! substring:
//!
//! ```text
//! substring                     sd15   sdxl   flux1   qwen-image
//! (total tensors)                834   2364     152         2160
//! lora_te2_                        0      0       0            0
//! text_encoder_2                   0      0       0            0
//! lora_te_                         0      0       0            0
//! text_encoder.                    0      0       0            0
//! lora_unet_                     834   2364       0            0
//! unet.                            0      0       0            0
//! double_blocks                    0      0     152            0
//! single_blocks                    0      0       0            0
//! single_transformer_blocks        0      0       0            0
//! transformer_blocks             480   2100       0         2160
//! down_blocks_3                   18      0       0            0
//! down_blocks.3                    0      0       0            0
//! ```
//!
//! Three readings of that table drive the rule order below:
//!
//! - **No real fixture has text-encoder tensors.** Modern LoRAs are
//!   UNet/transformer-only, so the `lora_te2_` → SDXL rule fires for older
//!   files only and is covered by a synthetic test, not by a fixture. SDXL is
//!   really decided by the absence of `down_blocks_3`.
//! - **`transformer_blocks` is a false positive on kohya SD names**, which
//!   embed `_transformer_blocks_` mid-string. Qwen-Image is only distinguishable
//!   because the rule is guarded on there being no UNet marker at all.
//! - **`down_blocks_3` is the entire SD1.5-vs-SDXL signal**: four down-block
//!   stages against three.
//!
//! Returning a *list* rather than one family is deliberate: FLUX.1 and FLUX.2
//! share their block naming exactly, so the honest output for a FLUX LoRA is
//! "one of these two" and the add dialog asks. One candidate means confident;
//! zero or several means ask the user.

use std::path::Path;

/// Families whose LoRA naming convention matches this safetensors header.
///
/// Exactly one element → confident. Empty (nothing matched, or the header
/// isn't a tensor map) or several (the conventions can't be told apart) → the
/// caller must ask the user.
///
/// Rule order matters and is not arbitrary:
/// 1. FLUX block naming first — kohya FLUX keys also start with `lora_unet_`,
///    and diffusers FLUX keys also contain `transformer_blocks`, so checking
///    either of those generic prefixes first would swallow every FLUX LoRA.
/// 2. A second text encoder is unique to SDXL among the SD families. This is a
///    legacy path: no surveyed file has text-encoder tensors at all.
/// 3. `transformer_blocks` is Qwen-Image **only when no UNet marker is
///    present**. The guard is load-bearing, not defensive — kohya SD names
///    embed `_transformer_blocks_` mid-string (480 hits in the SD1.5 fixture,
///    2100 in the SDXL one), so dropping it files every SD LoRA as Qwen-Image.
/// 4. A UNet LoRA is then split by depth, which is the real SD1.5-vs-SDXL
///    signal: SD1.5's UNet has four down blocks (0-3), SDXL's has three (0-2).
pub fn candidate_families(header_json: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(header_json) else {
        return Vec::new();
    };
    let Some(obj) = value.as_object() else {
        return Vec::new();
    };
    let names: Vec<&str> =
        obj.keys().filter(|k| k.as_str() != "__metadata__").map(String::as_str).collect();
    if names.is_empty() {
        return Vec::new();
    }
    let any = |needles: &[&str]| names.iter().any(|n| needles.iter().any(|x| n.contains(x)));

    if any(&["double_blocks", "single_blocks", "single_transformer_blocks"]) {
        return vec!["flux1".into(), "flux2".into()];
    }
    if any(&["lora_te2_", "text_encoder_2"]) {
        return vec!["sdxl".into()];
    }
    let unet = any(&["lora_unet_", "unet.", "lora_te_", "text_encoder."]);
    if !unet && any(&["transformer_blocks"]) {
        return vec!["qwen-image".into()];
    }
    if unet {
        return if any(&["down_blocks_3", "down_blocks.3"]) {
            vec!["sd15".into()]
        } else {
            vec!["sdxl".into()]
        };
    }
    Vec::new()
}

/// `candidate_families` for a file on disk. Empty for anything that isn't a
/// readable safetensors container (including GGUF LoRAs, which the engine can
/// load but whose headers this build does not parse).
pub fn detect_family(path: &Path) -> Vec<String> {
    crate::weights::read_header(path).map(|h| candidate_families(&h)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_sd15_lora_detects_sd15() {
        let h = include_str!("../fixtures/lora-headers/sd15.json");
        assert_eq!(candidate_families(h), vec!["sd15".to_string()]);
    }

    #[test]
    fn real_sdxl_lora_detects_sdxl() {
        let h = include_str!("../fixtures/lora-headers/sdxl.json");
        assert_eq!(candidate_families(h), vec!["sdxl".to_string()]);
    }

    #[test]
    fn real_flux1_lora_is_ambiguous_between_flux_generations() {
        // FLUX.1 and FLUX.2 share the double/single-block transformer naming,
        // so the honest answer is "one of these two" — the add dialog asks.
        let h = include_str!("../fixtures/lora-headers/flux1.json");
        assert_eq!(candidate_families(h), vec!["flux1".to_string(), "flux2".to_string()]);
    }

    #[test]
    fn real_qwen_image_lora_detects_qwen_image() {
        let h = include_str!("../fixtures/lora-headers/qwen-image.json");
        assert_eq!(candidate_families(h), vec!["qwen-image".to_string()]);
    }

    #[test]
    fn kohya_and_diffusers_conventions_both_resolve_to_sdxl() {
        // Two naming schemes ship in the wild for the same base model. Both
        // inputs are synthetic on purpose: the survey found that none of the
        // four real fixtures carries text-encoder tensors at all, so this is
        // the only coverage the second-text-encoder branch gets.
        let kohya = r#"{"lora_te2_text_model_encoder_layers_0_mlp_fc1.lora_down.weight":{"dtype":"F16","shape":[8,1280],"data_offsets":[0,20480]}}"#;
        let diffusers = r#"{"text_encoder_2.text_model.encoder.layers.0.mlp.fc1.lora_A.weight":{"dtype":"F16","shape":[8,1280],"data_offsets":[0,20480]}}"#;
        assert_eq!(candidate_families(kohya), vec!["sdxl".to_string()]);
        assert_eq!(candidate_families(diffusers), vec!["sdxl".to_string()]);
    }

    #[test]
    fn a_kohya_name_containing_transformer_blocks_is_not_qwen_image() {
        // kohya embeds `_transformer_blocks_` inside its SD UNet names. The
        // survey counted 480 such names in the SD1.5 fixture and 2100 in the
        // SDXL one. Without the "no UNet marker" guard on the Qwen-Image rule,
        // every Stable Diffusion LoRA ever published is misfiled.
        let h = r#"{"lora_unet_down_blocks_3_attentions_0_transformer_blocks_0_attn1_to_k.lora_down.weight":{"dtype":"F16","shape":[4,320],"data_offsets":[0,2560]}}"#;
        assert_eq!(candidate_families(h), vec!["sd15".to_string()]);
    }

    #[test]
    fn unet_only_lora_is_split_by_down_block_depth() {
        // No text-encoder tensors to key on. SD1.5's UNet has four down blocks
        // (0-3); SDXL's has three (0-2), so `down_blocks_3` is the tell.
        let sd15 = r#"{"lora_unet_down_blocks_3_attentions_0_proj_in.lora_down.weight":{"dtype":"F16","shape":[4,320],"data_offsets":[0,2560]}}"#;
        let sdxl = r#"{"lora_unet_down_blocks_2_attentions_1_proj_in.lora_down.weight":{"dtype":"F16","shape":[4,1280],"data_offsets":[0,10240]}}"#;
        assert_eq!(candidate_families(sd15), vec!["sd15".to_string()]);
        assert_eq!(candidate_families(sdxl), vec!["sdxl".to_string()]);
    }

    #[test]
    fn flux_wins_over_the_generic_unet_prefix() {
        // kohya FLUX keys start with `lora_unet_` too; the block naming has to
        // be checked first or every FLUX LoRA would be filed as sd15/sdxl.
        let h = r#"{"lora_unet_double_blocks_0_img_attn_proj.lora_down.weight":{"dtype":"F16","shape":[16,3072],"data_offsets":[0,98304]}}"#;
        assert_eq!(candidate_families(h), vec!["flux1".to_string(), "flux2".to_string()]);
    }

    #[test]
    fn flux_wins_over_the_generic_transformer_blocks_prefix() {
        // Diffusers FLUX carries BOTH `transformer_blocks` and
        // `single_transformer_blocks`; Qwen-Image carries only the former.
        let h = r#"{
            "transformer.transformer_blocks.0.attn.to_q.lora_A.weight":{"dtype":"F16","shape":[16,3072],"data_offsets":[0,98304]},
            "transformer.single_transformer_blocks.0.attn.to_q.lora_A.weight":{"dtype":"F16","shape":[16,3072],"data_offsets":[98304,196608]}
        }"#;
        assert_eq!(candidate_families(h), vec!["flux1".to_string(), "flux2".to_string()]);
    }

    #[test]
    fn unrecognised_and_malformed_headers_yield_no_candidates() {
        assert!(candidate_families(r#"{"mystery.weight":{"dtype":"F16","shape":[1],"data_offsets":[0,2]}}"#).is_empty());
        assert!(candidate_families(r#"{"__metadata__":{"ss_network_dim":"8"}}"#).is_empty());
        assert!(candidate_families("[1,2,3]").is_empty());
        assert!(candidate_families("not json").is_empty());
        assert!(candidate_families("").is_empty());
    }

    #[test]
    fn detect_family_reads_a_file_and_none_for_junk() {
        let dir = std::env::temp_dir().join(format!("muchai-loradet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let header = r#"{"lora_unet_double_blocks_0_img_attn_proj.lora_down.weight":{"dtype":"F16","shape":[2],"data_offsets":[0,4]}}"#;
        let p = dir.join("flux-lora.safetensors");
        let mut f = std::fs::File::create(&p).unwrap();
        std::io::Write::write_all(&mut f, &(header.len() as u64).to_le_bytes()).unwrap();
        std::io::Write::write_all(&mut f, header.as_bytes()).unwrap();
        std::io::Write::write_all(&mut f, &[0u8; 4]).unwrap();
        drop(f);
        assert_eq!(detect_family(&p), vec!["flux1".to_string(), "flux2".to_string()]);

        let junk = dir.join("notes.txt");
        std::fs::write(&junk, b"hello").unwrap();
        assert!(detect_family(&junk).is_empty());
        assert!(detect_family(std::path::Path::new("/nonexistent/muchai/x.safetensors")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
