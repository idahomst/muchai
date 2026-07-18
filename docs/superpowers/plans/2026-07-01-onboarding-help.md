# Onboarding & Help Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give non-technical first-time users a one-time welcome guide, contextual empty states, and ⓘ tooltips on every control so they can go from empty app to first image and understand each setting.

**Architecture:** A small help layer reusing existing patterns — a persisted `onboarded` config flag (like `theme`/`params_expanded`), a `WelcomeDialog` modal (reusing `DownloadDialog`'s backdrop/dialog styling + theme tokens), a reusable `InfoHint` tooltip component, and a central `helpText.ts` copy map wired into the control panels.

**Tech Stack:** Tauri v2 (Rust), SvelteKit + Svelte 5 runes (`$state`/`$derived`/`$props`), semantic CSS custom properties.

**Spec:** `docs/superpowers/specs/2026-07-01-onboarding-help-design.md`

**Gates (run from the stated dir):**
- Rust: `cd src-tauri && cargo test --lib`
- Types: `npm run check` (repo root) — must be **0 errors / 0 warnings**

---

## File Structure

- **Create** `src/lib/components/WelcomeDialog.svelte` — one-time welcome modal.
- **Create** `src/lib/components/InfoHint.svelte` — reusable ⓘ trigger + popover.
- **Create** `src/lib/helpText.ts` — central map of all tooltip/help copy.
- **Modify** `src-tauri/src/types.rs` — add `onboarded` field to `AppConfig`.
- **Modify** `src-tauri/src/config.rs` — default + 2 tests.
- **Modify** `src/lib/types.ts` — TS mirror of `onboarded`.
- **Modify** `src/routes/+page.svelte` — show WelcomeDialog when `!onboarded`, header "?" button, dismiss persist.
- **Modify** `src/lib/components/ImagePreview.svelte` — richer empty-state copy.
- **Modify** `src/lib/components/PromptPanel.svelte` — ⓘ on Prompt, Negative prompt.
- **Modify** `src/lib/components/SettingsPanel.svelte` — ⓘ on Steps, CFG, Width, Height, Sampler, Batch, Format, Seed.
- **Modify** `src/lib/components/ModelLibrary.svelte` — ⓘ on Model.
- **Modify** `src/lib/components/DevicePicker.svelte` — ⓘ on Device.

---

## Task 1: `onboarded` config flag (Rust + TS)

**Files:**
- Modify: `src-tauri/src/types.rs:217` (add field after `theme`)
- Modify: `src-tauri/src/config.rs` (default in `default_config`, 2 tests in `mod tests`)
- Modify: `src/lib/types.ts` (AppConfig interface)

- [ ] **Step 1: Write the failing tests** in `src-tauri/src/config.rs`, inside `mod tests { ... }`, right after the existing `theme_round_trips` test:

```rust
    #[test]
    fn old_config_without_onboarded_defaults_to_false() {
        let dir = std::env::temp_dir().join(format!("muchai-cfg-onb-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no onboarded key.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model_path":"","prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert!(!cfg.onboarded, "missing onboarded must default to false");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn onboarded_round_trips() {
        let dir = std::env::temp_dir().join(format!("muchai-cfg-onb2-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.onboarded = true;
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert!(back.onboarded);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib onboarded`
Expected: FAIL — compile error `no field 'onboarded' on type 'AppConfig'`.

- [ ] **Step 3: Add the field** to `AppConfig` in `src-tauri/src/types.rs`. Insert between the `theme` field (line 217) and `pub last_request` (line 218):

```rust
    /// Whether the user has dismissed the one-time welcome dialog. Defaults to
    /// `false`; pre-feature config files lack this key and deserialize as false.
    #[serde(default)]
    pub onboarded: bool,
```

- [ ] **Step 4: Set the default** in `src-tauri/src/config.rs`, in `default_config()`. Insert `onboarded: false,` immediately before `last_request: GenerationRequest::default(),`:

```rust
        theme: Theme::Dark,
        onboarded: false,
        last_request: GenerationRequest::default(),
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib`
Expected: PASS — all tests green (includes the 2 new ones).

- [ ] **Step 6: Add the TS mirror** in `src/lib/types.ts`. In the `AppConfig` interface, add after the `theme` line:

```ts
  // Whether the one-time welcome dialog has been dismissed. Mirrors the Rust
  // AppConfig.onboarded (#[serde(default)] → false for old configs).
  onboarded: boolean;
```

- [ ] **Step 7: Type check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/config.rs src/lib/types.ts
git commit -m "feat(onboarding): add persisted onboarded config flag"
```

---

## Task 2: WelcomeDialog + header "?" button + wiring

**Files:**
- Create: `src/lib/components/WelcomeDialog.svelte`
- Modify: `src/routes/+page.svelte`

- [ ] **Step 1: Create `src/lib/components/WelcomeDialog.svelte`** with this exact content (backdrop/dialog styling mirrors `DownloadDialog.svelte`; Escape and backdrop-click both dismiss):

```svelte
<script lang="ts">
  let { onclose }: { onclose: () => void } = $props();

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window {onkeydown} />

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="welcome-title">
    <h2 id="welcome-title">Welcome to MuchAI 👋</h2>
    <p class="intro">Make images from text in three steps:</p>
    <ol class="steps">
      <li><strong>Download a model</strong> — the AI that creates images. Use the <em>Download…</em> button under “Model”.</li>
      <li><strong>Describe your image</strong> in the Prompt box — the more specific, the better.</li>
      <li><strong>Press Generate</strong> and wait a few moments for your image.</li>
    </ol>
    <p class="tipnote">Hover the ⓘ icons anytime to learn what each setting does.</p>
    <div class="row">
      <button class="btn-primary" onclick={onclose}>Got it</button>
    </div>
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:var(--backdrop); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--dialog-bg); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(420px, 92vw); max-height:88vh; overflow-y:auto;
    display:flex; flex-direction:column; gap:.7rem; }
  h2 { margin:0; font-size:1.1rem; }
  .intro { margin:0; font-size:.85rem; opacity:.85; }
  .steps { margin:0; padding-left:1.2rem; display:flex; flex-direction:column; gap:.5rem;
    font-size:.82rem; line-height:1.4; }
  .tipnote { margin:0; font-size:.76rem; opacity:.7; padding:.4rem .5rem;
    background:var(--accent-tint); border:1px solid var(--border); border-radius:6px; }
  .row { display:flex; justify-content:flex-end; }
  button { font:inherit; font-size:.85rem; padding:.4rem .9rem; cursor:pointer; }
</style>
```

- [ ] **Step 2: Verify it type-checks** (component exists but not yet used — check no syntax errors)

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Wire into `src/routes/+page.svelte`.** Update the `<script>` block. (a) Add `setSettings` to the api import; (b) import `WelcomeDialog`; (c) add `showWelcome` state and `dismissWelcome`; (d) set `showWelcome` after loading config.

Change the api import line from:
```ts
  import { getSettings, listHistory, onSystemStats, listModels, listGpuDevices } from "$lib/api";
```
to:
```ts
  import { getSettings, setSettings, listHistory, onSystemStats, listModels, listGpuDevices } from "$lib/api";
```

Add with the other component imports:
```ts
  import WelcomeDialog from "$lib/components/WelcomeDialog.svelte";
```

Add above `onMount(`:
```ts
  let showWelcome = $state(false);

  // Persist dismissal optimistically; the dialog stays closed this session even
  // if the write fails (onboarding is non-critical — worst case it shows once
  // more next launch).
  async function dismissWelcome() {
    showWelcome = false;
    const cur = $settings;
    if (!cur || cur.onboarded) return;
    const next = { ...cur, onboarded: true };
    settings.set(next);
    try {
      await setSettings(next);
    } catch {
      settings.set(cur);
    }
  }
```

Inside the `onMount` async IIFE, immediately after `applyTheme(cfg.theme);`, add:
```ts
      showWelcome = !cfg.onboarded;
```

- [ ] **Step 4: Add the header "?" button and render the dialog** in `src/routes/+page.svelte` markup. Replace the `<header class="brandbar">…</header>` block with:

```svelte
    <header class="brandbar">
      <h1 class="brand">MuchAI</h1>
      <div class="hdr-actions">
        <button class="help-btn" aria-label="Help" title="Help" onclick={() => (showWelcome = true)}>?</button>
        <ThemeToggle />
      </div>
    </header>
```

At the very end of the markup, after `<ResourceMonitor />`, add:
```svelte
{#if showWelcome}
  <WelcomeDialog onclose={dismissWelcome} />
{/if}
```

- [ ] **Step 5: Add styles** in `src/routes/+page.svelte` `<style>` block, after the `.brand` rule:

```css
  .hdr-actions { display:flex; align-items:center; gap:.4rem; }
  .help-btn { font:inherit; font-size:.85rem; line-height:1; width:1.5rem; height:1.5rem;
    display:flex; align-items:center; justify-content:center; cursor:pointer;
    border:1px solid var(--border); border-radius:50%;
    background:var(--surface); color:var(--text); }
  .help-btn:hover { color:var(--accent-bright); border-color:var(--accent-bright); }
```

- [ ] **Step 6: Type check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 7: Commit**

```bash
git add src/lib/components/WelcomeDialog.svelte src/routes/+page.svelte
git commit -m "feat(onboarding): one-time welcome dialog + header help button"
```

---

## Task 3: Richer empty state in ImagePreview

**Files:**
- Modify: `src/lib/components/ImagePreview.svelte`

- [ ] **Step 1: Replace the empty-state markup.** In `src/lib/components/ImagePreview.svelte`, change the `{:else}` branch from:

```svelte
  {:else}
    <p class="empty">Your generated image will appear here.</p>
  {/if}
```
to:
```svelte
  {:else}
    <div class="empty">
      <p class="empty-title">Your image will appear here.</p>
      <p class="empty-sub">Pick a model, write a prompt, then press Generate.</p>
    </div>
  {/if}
```

- [ ] **Step 2: Update the styles.** In the same file's `<style>`, replace the `.empty { opacity:.5; }` rule with:

```css
  .empty { opacity:.55; text-align:center; padding:1rem; }
  .empty-title { margin:0 0 .3rem; font-size:.95rem; }
  .empty-sub { margin:0; font-size:.8rem; opacity:.85; }
```

- [ ] **Step 3: Type check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/ImagePreview.svelte
git commit -m "feat(onboarding): guidance text in empty image preview"
```

---

## Task 4: Reusable InfoHint tooltip component

**Files:**
- Create: `src/lib/components/InfoHint.svelte`

- [ ] **Step 1: Create `src/lib/components/InfoHint.svelte`** with this exact content. The trigger is a real `<button>` (keyboard-focusable, screen-reader-announced); the popover opens on hover, focus, and click, and closes on mouseleave, blur, Escape, or a second click. The module-scoped counter gives each popover a unique id for `aria-describedby`.

```svelte
<script module lang="ts">
  let counter = 0;
</script>

<script lang="ts">
  let { text, label = "More info" }: { text: string; label?: string } = $props();
  let open = $state(false);
  const tipId = `hint-${counter++}`;

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") open = false;
  }
</script>

<span class="wrap" onmouseenter={() => (open = true)} onmouseleave={() => (open = false)} role="presentation">
  <button
    type="button"
    class="info"
    aria-label={label}
    aria-expanded={open}
    aria-describedby={open ? tipId : undefined}
    onclick={() => (open = !open)}
    onfocus={() => (open = true)}
    onblur={() => (open = false)}
    {onkeydown}
  >ⓘ</button>
  {#if open}
    <span class="tip" role="tooltip" id={tipId}>{text}</span>
  {/if}
</span>

<style>
  .wrap { position:relative; display:inline-flex; }
  .info { font:inherit; font-size:.7rem; line-height:1; padding:0 .15rem; margin:0;
    background:none; border:none; color:var(--text-muted); cursor:help; opacity:.75; }
  .info:hover, .info:focus-visible { color:var(--accent-bright); opacity:1; }
  .tip { position:absolute; z-index:20; top:calc(100% + 4px); left:0;
    width:max-content; max-width:220px; padding:.4rem .55rem;
    background:var(--surface); color:var(--text);
    border:1px solid var(--border); border-radius:6px;
    box-shadow:0 4px 14px var(--overlay);
    font-size:.72rem; line-height:1.35; font-weight:normal; text-align:left;
    white-space:normal; pointer-events:none; }
</style>
```

- [ ] **Step 2: Type check** (component compiles standalone)

Run: `npm run check`
Expected: 0 errors, 0 warnings. (If svelte-check warns that the `<span class="wrap">` mouse handlers need a role, the `role="presentation"` already present satisfies it — do not add a keyboard handler to the span; keyboard is handled on the button.)

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/InfoHint.svelte
git commit -m "feat(onboarding): reusable InfoHint tooltip component"
```

---

## Task 5: Central help copy + wire tooltips into all controls

**Files:**
- Create: `src/lib/helpText.ts`
- Modify: `src/lib/components/PromptPanel.svelte`
- Modify: `src/lib/components/SettingsPanel.svelte`
- Modify: `src/lib/components/ModelLibrary.svelte`
- Modify: `src/lib/components/DevicePicker.svelte`

- [ ] **Step 1: Create `src/lib/helpText.ts`** with this exact content:

```ts
// Plain-language explanations shown in ⓘ tooltips (and reused by the welcome
// flow). Keep each string short and jargon-free — the audience is
// non-technical first-time users.
export const HELP = {
  // PromptPanel
  prompt:
    "Describe the image you want — subject, style, colors, mood. Be specific, e.g. 'a red fox in a snowy forest, watercolor'.",
  negativePrompt:
    "Things you DON'T want in the image (e.g. 'blurry, text, extra fingers'). Optional — fine to leave empty.",
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
  // ModelLibrary / DevicePicker
  model:
    "The AI model that turns your words into images. Download one to get started — different models produce different styles.",
  device:
    "The hardware that runs the AI. A graphics card (GPU) is much faster than CPU. 'Default' lets MuchAI choose for you.",
} as const;
```

- [ ] **Step 2: Wire PromptPanel.** In `src/lib/components/PromptPanel.svelte`, add to the `<script>`:

```ts
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";
```

Change the Prompt label row from:
```svelte
    <label class="label" for="prompt">Prompt</label>
```
to:
```svelte
    <span class="lbl-wrap"><label class="label" for="prompt">Prompt</label><InfoHint text={HELP.prompt} label="About the prompt" /></span>
```

Change the Negative prompt label row from:
```svelte
    <label class="label" for="neg">Negative prompt</label>
```
to:
```svelte
    <span class="lbl-wrap"><label class="label" for="neg">Negative prompt</label><InfoHint text={HELP.negativePrompt} label="About the negative prompt" /></span>
```

Add to the `<style>` block:
```css
  .lbl-wrap { display:inline-flex; align-items:center; gap:.2rem; }
```

- [ ] **Step 3: Wire SettingsPanel.** In `src/lib/components/SettingsPanel.svelte`, add to the `<script>`:

```ts
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../types";
```
⚠️ Correction — import HELP from the help module, not types:
```ts
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";
```

Each label currently looks like `<label class="label">Steps\n    <input …/></label>`. Because `.label` is `flex-direction:column`, wrap the text + ⓘ in a `.lbl-row` span so they stay on one line above the input. Replace each label's opening text accordingly:

```svelte
  <label class="label"><span class="lbl-row">Steps <InfoHint text={HELP.steps} /></span>
    <input type="number" min="1" max="150" bind:value={$request.steps} />
  </label>
  <label class="label"><span class="lbl-row">CFG <InfoHint text={HELP.cfg} /></span>
    <input type="number" min="1" max="30" step="0.5" bind:value={$request.cfg_scale} />
  </label>
  <label class="label"><span class="lbl-row">Width <InfoHint text={HELP.width} /></span>
    <input type="number" min="64" max="2048" step="64" bind:value={$request.width} />
  </label>
  <label class="label"><span class="lbl-row">Height <InfoHint text={HELP.height} /></span>
    <input type="number" min="64" max="2048" step="64" bind:value={$request.height} />
  </label>
  <label class="label"><span class="lbl-row">Sampler <InfoHint text={HELP.sampler} /></span>
    <select bind:value={$request.sampler}>
      {#each SAMPLERS as s}<option value={s.value}>{s.label}</option>{/each}
    </select>
  </label>
  <label class="label"><span class="lbl-row">Batch <InfoHint text={HELP.batch} /></span>
    <input type="number" min="1" max="8" bind:value={$request.batch_count} />
  </label>
  <label class="label"><span class="lbl-row">Format <InfoHint text={HELP.format} /></span>
    <select bind:value={$request.output_format}>
      {#each FORMATS as f}<option value={f.value}>{f.label}</option>{/each}
    </select>
  </label>
  <label class="label seed"><span class="lbl-row">Seed (-1 = random) <InfoHint text={HELP.seed} /></span>
    <input type="number" bind:value={$request.seed} />
  </label>
```

Add to the `<style>` block:
```css
  .lbl-row { display:inline-flex; align-items:center; gap:.15rem; }
```

- [ ] **Step 4: Wire ModelLibrary.** In `src/lib/components/ModelLibrary.svelte`, add to the `<script>`:

```ts
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";
```

Change the Model label from:
```svelte
  <span class="label">Model</span>
```
to:
```svelte
  <span class="label lbl-wrap">Model <InfoHint text={HELP.model} label="About models" /></span>
```

Add to the `<style>` block:
```css
  .lbl-wrap { display:inline-flex; align-items:center; gap:.2rem; }
```

- [ ] **Step 5: Wire DevicePicker.** In `src/lib/components/DevicePicker.svelte`, add to the `<script>`:

```ts
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";
```

Change the header label from:
```svelte
    <span class="lbl">Device</span>
```
to:
```svelte
    <span class="lbl-wrap"><span class="lbl">Device</span><InfoHint text={HELP.device} label="About the device" /></span>
```

Add to the `<style>` block:
```css
  .lbl-wrap { display:inline-flex; align-items:center; gap:.2rem; }
```

- [ ] **Step 6: Type check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 7: Commit**

```bash
git add src/lib/helpText.ts src/lib/components/PromptPanel.svelte src/lib/components/SettingsPanel.svelte src/lib/components/ModelLibrary.svelte src/lib/components/DevicePicker.svelte
git commit -m "feat(onboarding): ⓘ tooltips on all controls via central help copy"
```

---

## Task 6: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Full Rust test suite**

Run: `cd src-tauri && cargo test --lib`
Expected: PASS — all tests green (84 prior + 2 new = 86).

- [ ] **Step 2: Full type check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Confirm no stray references.** Verify `HELP` is imported from `"../helpText"` (never from `"../types"`) in all four control components:

Run: `grep -rn "import { HELP }" src/lib/components/`
Expected: four lines, all `from "../helpText"`.

- [ ] **Step 4: Manual E2E handoff.** Report to the controller that automated gates pass and the following require the user's manual E2E on the GPU box (no frontend unit harness exists):
  - Fresh/`onboarded:false` config → Welcome dialog appears once; "Got it" dismisses; relaunch → does not reappear.
  - Header "?" reopens the Welcome dialog anytime.
  - Empty preview shows the two-line placeholder; disappears after first generate.
  - Every ⓘ works via mouse hover, keyboard focus (Tab), and click; Escape closes.
  - Tooltips readable in both light and dark themes.
  - Controls-panel layout unchanged (no wrapping/overflow regressions).

---

## Self-Review

**Spec coverage:**
- Welcome dialog + one-time + reopenable "?" → Task 2. ✅
- Empty states (preview) → Task 3; ModelLibrary no-model hint already exists (spec said keep) → no task needed. ✅
- ⓘ tooltips on Prompt/Negative + 8 settings + Model + Device → Task 5 (matches the 12 `HELP` keys). ✅
- `InfoHint` accessible (button, hover/focus/click, Escape) → Task 4. ✅
- `helpText.ts` central copy → Task 5. ✅
- `onboarded` `#[serde(default)]` + TS mirror + 2 tests → Task 1. ✅
- Optimistic dismiss with revert → Task 2 Step 3. ✅
- Testing (2 Rust tests, svelte-check 0/0, manual E2E) → Tasks 1 & 6. ✅

**Placeholder scan:** No TBD/TODO; every code step shows complete code. The Task 5 Step 3 intentionally shows a wrong import then corrects it with a ⚠️ note — the final code block is the authoritative one (`from "../helpText"`).

**Type/name consistency:** `onboarded: bool`/`onboarded: boolean` consistent across Rust/TS/dismiss. `HELP` keys (`prompt`, `negativePrompt`, `steps`, `cfg`, `width`, `height`, `sampler`, `batch`, `format`, `seed`, `model`, `device`) match every `HELP.<key>` usage in Task 5. `InfoHint` prop names (`text`, `label`) match all call sites. `WelcomeDialog` `onclose` prop matches `dismissWelcome` wiring.
