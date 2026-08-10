// Plain-language explanations shown in ⓘ tooltips (and reused by the welcome
// flow). Keep each string short and jargon-free — the audience is
// non-technical first-time users.
export const HELP = {
  // PromptPanel
  prompt:
    "Describe the image you want — subject, style, colors, mood. Be specific, e.g. 'a red fox in a snowy forest, watercolor'.",
  instruction:
    "Say what to CHANGE about the image, not what the finished picture contains. 'Make the sky stormy' works; 'a house under a stormy sky' will redraw the whole scene.",
  negativePrompt:
    "Things you DON'T want in the image (e.g. 'blurry, text, extra fingers'). Optional. It only works on models that use guidance — Stable Diffusion (1.5 / XL) and Qwen-Image. The fast 'turbo' models (FLUX, Z-Image) ignore it, so you can leave it empty for those.",
  // SettingsPanel
  steps:
    "How many passes the AI makes to refine the image. More can add detail but takes longer. 20 is a good starting point.",
  cfg:
    "How strictly the image follows your prompt. Lower = more creative, higher = more literal. Around 7 works well.",
  width:
    "Image width in pixels. Bigger is sharper but slower and uses more memory. 512 is safe; many newer models prefer 1024.",
  height:
    "Image height in pixels. Bigger is sharper but slower and uses more memory. 512 is safe; many newer models prefer 1024.",
  sampler:
    "The method used to build the image. Different recipes, similar results — 'Euler a' is a fine default.",
  batch:
    "How many images to make in one run. Each extra image adds time and memory.",
  format:
    "File type for saved images. PNG keeps the best quality; JPEG makes smaller files.",
  seed:
    "The random starting point. −1 makes a new random image each time; set a fixed number to reproduce the exact same image.",
  // LoraPanel
  lora:
    "Small add-on files that nudge a model toward a particular style, character, or subject — like a filter for the AI. Turn one on and slide its strength up or down. If a LoRA lists trigger words, click one to add it to your prompt.",
  // RefImagePanel
  refImage:
    "The picture you want to change. Drop one here or choose a file, then describe the change you want — for example 'make the sky stormy'. MuchAI matches the output size to your image.",
  // ModelLibrary / DevicePicker
  model:
    "The AI model that turns your words into images. Download one to get started — different models produce different styles.",
  device:
    "The hardware that runs the AI. A graphics card (GPU) is much faster than CPU. 'Default' lets MuchAI choose for you.",
} as const;
