# Onboarding & Help for Non-Technical Users — Design Spec

**Date:** 2026-07-01
**Status:** Approved (brainstorm complete; ready for implementation plan)
**Feature branch:** `feat/onboarding-help`

## Problem

fridAI is about to go to non-technical test users (friends) who have no image-generation or model-settings experience. Two frictions block them:

1. **Cold start** — the app opens empty (no model). "What do I even do?"
2. **Jargon** — Steps, CFG, Sampler, Seed, negative prompt, Model, Device are meaningless to a beginner.

## Goal

A brand-new user goes from empty app to their first image without outside help, and can understand any control they're curious about. Ship as **one plan** (not phased).

## Non-Goals (YAGNI)

- No interactive spotlight/coach-mark tour (fragile against layout changes).
- No internationalization — English copy only.
- No per-model dynamic hints or "recommended values per model family" (that's the future "Simple vs Advanced GUI" idea, out of scope here).
- No analytics/telemetry on help usage.

## Approach

A lightweight help layer built from small reusable pieces, leaning on patterns already in the codebase so there is little new machinery:

- Modal reuses **DownloadDialog**'s overlay/dialog styling + theme tokens.
- Persistence + dismiss reuse the **ThemeToggle / ParamsPanel** optimistic-set-then-`setSettings`-revert pattern.
- New config flag uses the existing `#[serde(default)]` convention (mirrors `theme`, `params_expanded`).

Three parts: (A) a one-time **Welcome dialog** + a permanent **"?" Help button** to reopen it, (B) **empty-state text** that teaches in context, and (C) **ⓘ tooltips** on every control.

---

## Components

### `InfoHint.svelte` (new, reusable)
The ⓘ trigger + popover used by every tooltip.

- **Props:** `text: string` (the explanation); `label?: string` (aria-label, default `"More info"`).
- **Trigger:** a `<button class="info">ⓘ</button>` — never a bare `<span>`, so it is keyboard-focusable and screen-reader-announced.
- **Open on:** hover (mouseenter), focus, and click/tap — so mouse, keyboard, and touch all work.
- **Close on:** mouseleave, blur, Escape, and a second click.
- **Popover:** absolutely-positioned small card, `max-width` ~ 220px, styled with theme tokens (`--surface`, `--text`, `--border`, `--overlay` for shadow). `role="tooltip"`, referenced by the button's `aria-describedby`.
- **State:** local `open = $state(false)`; no global state.
- Renders inline next to a label; must not disrupt the existing label flex/grid layout.

### `helpText.ts` (new)
A single central map of all plain-language copy, so wording is consistent and the Welcome dialog can reuse strings.

```ts
export const HELP = {
  // PromptPanel
  prompt: "Describe the image you want — subject, style, colors, mood. Be specific, e.g. 'a red fox in a snowy forest, watercolor'.",
  negativePrompt: "Things you DON'T want in the image (e.g. 'blurry, text, extra fingers'). Optional — fine to leave empty.",
  // SettingsPanel
  steps: "How many passes the AI makes to refine the image. More can add detail but takes longer. 20 is a good starting point.",
  cfg: "How strictly the image follows your prompt. Lower = more creative, higher = more literal. Around 7 works well.",
  width: "Image width in pixels. Bigger is sharper but slower and uses more memory. 512 is safe; many newer models prefer 1024.",
  height: "Image height in pixels. Bigger is sharper but slower and uses more memory. 512 is safe; many newer models prefer 1024.",
  sampler: "The method used to build the image. Different recipes, similar results — 'Euler a' is a fine default.",
  batch: "How many images to make in one run. Each extra image adds time and memory.",
  format: "File type for saved images. PNG keeps the best quality; JPEG makes smaller files.",
  seed: "The random starting point. −1 makes a new random image each time; set a fixed number to reproduce the exact same image.",
  // ModelLibrary / DevicePicker
  model: "The AI model that turns your words into images. Download one to get started — different models produce different styles.",
  device: "The hardware that runs the AI. A graphics card (GPU) is much faster than CPU. 'Default' lets fridAI choose for you.",
} as const;
```

### `WelcomeDialog.svelte` (new)
One-time modal.

- Title: **"Welcome to fridAI 👋"**
- Intro: "Make images from text in three steps:"
- Steps:
  1. **Download a model** — the AI that creates images. Use the **Download…** button under "Model".
  2. **Describe your image** in the Prompt box — the more specific, the better.
  3. **Press Generate** and wait a few moments for your image.
- Footer note: "Hover the ⓘ icons anytime to learn what each setting does."
- Single button: **"Got it"**.
- Overlay click and Escape also dismiss (same as "Got it").
- Props: `onclose: () => void` (mirrors DownloadDialog's callback style).

### Header "?" Help button
In the `.brandbar` in `+page.svelte`, alongside `ThemeToggle`. A `<button>` with `aria-label="Help"` that sets `showWelcome = true`. Reopening via "?" does **not** change config (it's just viewing help again).

### Empty states
- **`ImagePreview.svelte`** — when there is no current image, show placeholder text instead of blank: "Your image will appear here.\nPick a model, write a prompt, then press Generate." (Styled muted, centered.)
- **`ModelLibrary.svelte`** — the existing `{#if $models.length === 0}` hint ("No models found. Click Download… to get one.") already covers the no-model case; keep it. No change required beyond confirming it reads well.

---

## Persistence & Data Flow

### Config
- **Rust** `src-tauri/src/types.rs` — add to `AppConfig`:
  ```rust
  #[serde(default)]
  pub onboarded: bool,
  ```
- **Rust** `src-tauri/src/config.rs` — `default_config()` sets `onboarded: false`.
- **TS** `src/lib/types.ts` — add `onboarded: boolean;` to `AppConfig`.

`onboarded` defaults to `false`, so every existing/legacy config (and every friend's fresh install) shows the Welcome dialog exactly once.

### Wiring (`+page.svelte`)
- After `settings.set(cfg)`, initialize local `let showWelcome = $state(!cfg.onboarded)`.
- Render `{#if showWelcome}<WelcomeDialog onclose={dismissWelcome} />{/if}`.
- `dismissWelcome()`:
  - Set `showWelcome = false`.
  - If `!$settings.onboarded`: optimistically `settings.set({ ...$settings, onboarded: true })`, then `await setSettings(next)`; on error revert the store (the dialog stays closed for this session regardless — we don't re-nag mid-session).
- Header "?" handler: `showWelcome = true` (no config write).

This matches the existing optimistic-persist pattern; no new store or API is introduced (`getSettings`/`setSettings` already exist).

---

## Tooltip Placement

ⓘ via `InfoHint` next to each control label:

| File | Controls |
|------|----------|
| `PromptPanel.svelte` | Prompt, Negative prompt |
| `SettingsPanel.svelte` | Steps, CFG, Width, Height, Sampler, Batch, Format, Seed |
| `ModelLibrary.svelte` | Model |
| `DevicePicker.svelte` | Device |

The ⓘ sits inside/next to the existing `<label>`; layout (the two-column `.grid` in SettingsPanel, the `.field` rows elsewhere) must be preserved.

---

## Error Handling

- `setSettings` failure on dismiss: revert the `onboarded` store value; the dialog still closes for this session (no error surfaced to the user — onboarding is non-critical). Worst case: the friend sees the welcome once more next launch. Acceptable.
- `InfoHint` has no failure modes (pure UI). Popover must not overflow off-screen awkwardly; acceptable to let it clip at panel edges for beta (no collision-detection library).

---

## Testing

- **Rust** (`cargo test --lib`, from `src-tauri/`):
  - `old_config_without_onboarded_defaults_to_false` — legacy JSON without the key loads with `onboarded == false`.
  - `onboarded_round_trips` — `true` survives save→load.
  - (Mirrors the existing `theme` / `params_expanded` tests.)
- **Type check** (`npm run check`): 0 errors / 0 warnings.
- **No frontend unit-test harness** exists → the Welcome dialog, "?" reopen, empty states, and every tooltip are verified in manual E2E on the dev box.

## Manual E2E checklist (for implementation handoff)

- [ ] Fresh config (or `onboarded:false`) → Welcome dialog appears once; "Got it" dismisses; relaunch → does not reappear.
- [ ] Header "?" reopens the Welcome dialog anytime.
- [ ] Empty preview shows the placeholder; it disappears after first generate.
- [ ] Every ⓘ works via mouse hover, keyboard focus (Tab + read), and click; Escape closes.
- [ ] Tooltips render correctly in both light and dark themes.
- [ ] Layout of the controls panel is unchanged (no wrapping/overflow regressions).

## Implementation Order (natural task sequence)

1. Config flag (`onboarded`) — Rust types + default + 2 tests; TS mirror.
2. `WelcomeDialog.svelte` + wire into `+page.svelte` + header "?" button.
3. Empty-state text in `ImagePreview.svelte`.
4. `InfoHint.svelte` reusable component.
5. `helpText.ts` + place ⓘ tooltips across PromptPanel, SettingsPanel, ModelLibrary, DevicePicker.
6. Final review + manual E2E.
