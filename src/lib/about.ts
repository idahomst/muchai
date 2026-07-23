export const APP_TAGLINE = "Make images from text, right on your own machine.";

export interface CreditItem {
  label: string;
  url?: string;
  note?: string;
}

export interface CreditSection {
  heading: string;
  items: CreditItem[];
}

export const CREDITS: CreditSection[] = [
  {
    heading: "Image engine",
    items: [
      {
        label: "stable-diffusion.cpp",
        url: "https://github.com/leejet/stable-diffusion.cpp",
        note: "by leejet & contributors — the Vulkan/ggml engine that renders every image.",
      },
    ],
  },
  {
    heading: "Built with",
    items: [
      { label: "Tauri", url: "https://tauri.app" },
      { label: "Svelte", url: "https://svelte.dev" },
      { label: "Vite", url: "https://vite.dev" },
      { label: "Rust", url: "https://www.rust-lang.org" },
    ],
  },
  {
    heading: "Models",
    items: [
      {
        label: "Hugging Face and its community",
        url: "https://huggingface.co",
        note: "for the open models MuchAI can download and run.",
      },
    ],
  },
  {
    heading: "Inspired by",
    items: [
      { label: "Draw Things", url: "https://drawthings.ai" },
      { label: "Neural-Pixel" },
    ],
  },
  {
    heading: "Developed by",
    items: [
      {
        label: "Claude Opus (Anthropic)",
        note: "designed & built with Claude, for Martin Stepanek.",
      },
    ],
  },
];
