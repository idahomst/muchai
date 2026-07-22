### Tier 1: Ultra-Light (≤4 GB VRAM / CPU-Friendly)

*Targeting budget GPUs (GTX 1050/1650, Intel Arc, laptop chips) or pure CPU execution.*

* **SD (1.5):** [runwayml/stable-diffusion-v1-5](https://www.google.com/search?q=https://huggingface.co/runwayml/stable-diffusion-v1-5)
* *Format:* Standard `.safetensors` (~2 GB) or 4-bit GGUF (~1.8 GB). Highly efficient and lightweight.


* **SDXL:** [mzwing/SDXL-Lightning-GGUF](https://huggingface.co/mzwing/SDXL-Lightning-GGUF)
* *Quant:* `Q2_K` or `Q4_0` (~2.5 GB). Ultra-compressed SDXL that renders in 2–4 steps.


* **Flux.1:** [city96/FLUX.1-schnell-gguf](https://huggingface.co/city96/FLUX.1-schnell-gguf)
* *Quant:* `Q2_K` (~4.01 GB). Non-gated Apache-2.0 model heavily quantized for low-VRAM cards.


* **Flux.2:** [unsloth/FLUX.2-klein-4B-GGUF](https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF)
* *Quant:* `Q3_K` or `Q4_0` (~2.5–3.5 GB). The lightweight 4B parameter variant of Flux.2.


* **Qwen-Image / Z-Image:** [leejet/Z-Image-Turbo-GGUF](https://huggingface.co/leejet/Z-Image-Turbo-GGUF)
* *Quant:* `Q2_K` (~2.59 GB) or `Q3_K` (~3.5 GB). Converted by `stable-diffusion.cpp`'s author (`leejet`) specifically for 4GB systems.



---

### Tier 2: Light (4–8 GB VRAM)

*The sweet spot for budget gaming GPUs (RTX 3050/3060 8GB, GTX 1070/1080).*

* **SD (SD 3.5 Turbo):** [city96/stable-diffusion-3.5-large-turbo-gguf](https://huggingface.co/city96/stable-diffusion-3.5-large-turbo-gguf)
* *Quant:* `Q4_0` (~4.77 GB). High quality, fast sampling with a minimal VRAM footprint.


* **SDXL:** [silveroxides/sdxl-gguf](https://huggingface.co/silveroxides/sdxl-gguf) or [stabilityai/sdxl-turbo](https://huggingface.co/stabilityai/sdxl-turbo)
* *Quant:* `Q4_0` / `Q5_1` (~5.2 GB). Standard SDXL base capability running cleanly under 8GB VRAM.


* **Flux.1:** [leejet/FLUX.1-schnell-gguf](https://huggingface.co/leejet/FLUX.1-schnell-gguf)
* *Quant:* `Q4_0` (~6.88 GB). Apache 2.0 licensed, 4-step generation with excellent prompt adherence.


* **Flux.2:** [leejet/FLUX.2-klein-base-9B-GGUF](https://huggingface.co/leejet/FLUX.2-klein-base-9B-GGUF)
* *Quant:* `Q4_0` (~5.62 GB). The 9B parameter base Flux.2 model compressed down for mid-light GPUs.


* **Qwen-Image / Z-Image:** [leejet/Z-Image-Turbo-GGUF](https://huggingface.co/leejet/Z-Image-Turbo-GGUF)
* *Quant:* `Q4_0` (~3.68 GB) or `Q6_K` (~5.26 GB).



---

### Tier 3: Mid (8–12 GB VRAM)

*Standard modern desktop setups (RTX 4070 12GB, RX 6700 XT).*

* **SD (SD 3.5 Large):** [city96/stable-diffusion-3.5-large-gguf](https://huggingface.co/city96/stable-diffusion-3.5-large-gguf)
* *Quant:* `Q5_0` (~5.77 GB) or `Q8_0` (~8.78 GB).


* **SDXL:** [stabilityai/stable-diffusion-xl-base-1.0](https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0)
* *Format:* Native `.safetensors` single file (`fp16` ~6.9 GB). Uncompressed SDXL base model.


* **Flux.1:** [city96/FLUX.1-dev-gguf](https://huggingface.co/city96/FLUX.1-dev-gguf)
* *Quant:* `Q5_K_S` (~8.2 GB) or `Q6_K` (~9.86 GB). Non-commercial license; incredible detail at medium quantization loss.


* **Flux.2:** [leejet/FLUX.2-klein-base-9B-GGUF](https://huggingface.co/leejet/FLUX.2-klein-base-9B-GGUF)
* *Quant:* `Q8_0` (~9.98 GB). Near-lossless precision for the Flux.2 Klein 9B model.


* **Qwen-Image:** [Qwen/Qwen-Image](https://huggingface.co/collections/Qwen/qwen-image)
* *Format:* Quantized/Distilled weights (~8–10 GB operational memory footprint).



---

### Tier 4: High (12–16 GB VRAM)

*High-end consumer GPUs (RTX 4070 Ti Super, RTX 4080 16GB, RX 7800 XT).*

* **SD (SD 3.5 Large):** [stabilityai/stable-diffusion-3.5-large](https://huggingface.co/stabilityai/stable-diffusion-3.5-large)
* *Format:* Full precision `bfloat16` / `fp16` standard checkpoint (~16 GB VRAM peak).


* **SDXL:** Full precision SDXL Base + Refiner pipelines running simultaneously with heavy LoRAs.
* **Flux.1:** [city96/FLUX.1-dev-gguf](https://huggingface.co/city96/FLUX.1-dev-gguf)
* *Quant:* `Q8_0` (~12.7 GB) or [black-forest-labs/FLUX.1-schnell](https://huggingface.co/black-forest-labs/FLUX.1-schnell) in `fp8`.


* **Flux.2:** [black-forest-labs/FLUX.2-klein-base-9B](https://huggingface.co/black-forest-labs/FLUX.2-klein-base-9B)
* *Format:* Unquantized / `fp8` multi-file model (~14–16 GB VRAM required).


* **Qwen-Image:** [Qwen/Qwen-Image](https://huggingface.co/Qwen/Qwen-Image)
* *Format:* High-precision pipeline weights.



---

### Tier 5: Max (16–24 GB+ VRAM)

*Enthusiast & Workstation GPUs (RTX 3090/4090 24GB, RTX 6000 Ada).*

* **Flux.1:** [black-forest-labs/FLUX.1-dev](https://huggingface.co/black-forest-labs/FLUX.1-dev)
* *Format:* Full native precision `F16` / `BF16` (~23.8 GB). Maximum visual quality without quantization compromises.


* **Flux.2:** [black-forest-labs/FLUX.2-klein-base-9B](https://huggingface.co/black-forest-labs/FLUX.2-klein-base-9B) / Multi-file specifications from [drawthingsai/community-models](https://huggingface.co/drawthingsai).
* **Qwen-Image:** [Qwen/Qwen-Image-2512](https://huggingface.co/collections/Qwen/qwen-image)
* *Format:* Full 20B parameter multi-file release running in native precision.
