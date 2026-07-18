# Rename FridAI → MuchAI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebrand the application from FridAI (Frida Kahlo) to MuchAI (Alfons Mucha) across code, config, and docs — for trademark reasons — while preserving each existing install's saved tokens, settings, gallery, and models via a one-time data-directory migration, and preserving git history (the old name stays in past commits).

**Architecture:** The name appears in five distinct roles, each with its own correct mapping: (1) lowercase machine identifiers — Cargo package/lib name, npm package name, Tauri `identifier`, on-disk data dirs → `muchai`; (2) the on-disk data directory, which is derived via `directories::ProjectDirs::from("cz","mst","fridai")` and holds `config.json` (HF/Civitai tokens + all settings), `gallery/`, and `models/` — this changes to `muchai` **plus** a one-time startup migration that renames the legacy dirs so no user loses data; (3) the display/brand name shown in the UI and user-facing strings → `MuchAI`; (4) cosmetic strings (doc comments, per-test temp-dir prefixes) → `muchai`/`MuchAI` to match their casing; (5) internal docs (specs + plans) → blanket rename for consistency. Repo, git remote, and the working-directory folder are handled as manual ops in the final task, with the folder rename done **last** because it is this session's working directory.

**Tech Stack:** Rust (Tauri v2 backend, crate `fridai`→`muchai`, lib `fridai_lib`→`muchai_lib`, `directories` crate), Svelte 5 frontend, `svelte-check`, `cargo test`.

**Casing map (apply case-sensitively):**
- `fridai` → `muchai` (all-lowercase identifiers, paths, temp-dir prefixes)
- `FridAI` → `MuchAI` (display brand)
- `fridAI` → `MuchAI` (display brand — collapse the mixed-case styling to the canonical `MuchAI`)

**Global constraints for every task:**
- Rust gate: `cargo test --manifest-path src-tauri/Cargo.toml --lib` (0 failures) and `cargo build --manifest-path src-tauri/Cargo.toml --lib` (clean).
- Frontend gate: `npm run check` (0 errors / 0 warnings).
- One commit per task. Keep the old name in history (no history rewrite). The lead commit body must state the trademark reason; later commits may reference it briefly.
- Do NOT touch `.git/`. Do NOT hand-edit `src-tauri/Cargo.lock` — let `cargo build` regenerate it.
- **Security (unchanged):** tokens live in plaintext in `config.json` (an approved decision). Never `{:?}`/log the whole `AppConfig` or a raw token. The migration must move the config file, never read/print its contents.

**Out of scope (note, don't do):** branding artwork/icons (Frida→Mucha art is a separate design task; current `icons/` are generic sizes), and any GitHub repo *description* text (a web action for the user).

---

### Task 1: Rename the Rust crate (package + lib)

**Files:**
- Modify: `src-tauri/Cargo.toml:2` and `:14`
- Modify: `src-tauri/src/main.rs:5`
- Regenerated: `src-tauri/Cargo.lock` (by cargo, do not hand-edit)

No test change — the existing suite is the regression check that the rename didn't break the build.

- [ ] **Step 1: Rename the package and lib targets in `Cargo.toml`**

Change line 2 from `name = "fridai"` to:
```toml
name = "muchai"
```
Change line 14 from `name = "fridai_lib"` to:
```toml
name = "muchai_lib"
```

- [ ] **Step 2: Update the binary entry point**

In `src-tauri/src/main.rs:5`, change `fridai_lib::run()` to:
```rust
    muchai_lib::run()
```

- [ ] **Step 3: Build (regenerates Cargo.lock) and run tests**

Run: `cargo build --manifest-path src-tauri/Cargo.toml --lib && cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: clean build; `Cargo.lock` package entry now reads `name = "muchai"`; all tests pass (143 at time of writing).

- [ ] **Step 4: Commit (this is the lead commit — full reason in the body)**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/main.rs
git commit -m "$(cat <<'EOF'
refactor: rename Rust crate fridai → muchai (rebrand step 1)

Begin rebranding FridAI → MuchAI. "FridAI" references Frida Kahlo, whose
name carries trademark protection we cannot rely on; "MuchAI" references
Alfons Mucha instead. Historical commits keep the old name intentionally —
only current sources are renamed, no history is rewritten.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Rename the data directory + one-time migration

**Files:**
- Modify: `src-tauri/src/config.rs` (line 7 comment, line 9 `ProjectDirs`, line 27 fallback filename; add migration functions)
- Modify: `src-tauri/src/lib.rs:24` (call migration before loading config)
- Test: `src-tauri/src/config.rs` (in the existing `#[cfg(test)] mod tests`)

**Why this task exists:** `config.rs` resolves storage via `ProjectDirs::from("cz","mst","fridai")` → `~/.config/fridai/config.json` (holds HF/Civitai tokens + all settings) and `~/.local/share/fridai/{gallery,models}`. Changing the app name to `muchai` moves these paths, so an existing install would start fresh — losing tokens and showing an empty gallery. Additionally, an existing `config.json` stores **absolute** `gallery_dir`/`models_dir` strings pointing under `~/.local/share/fridai/`, so even after moving the directory those strings must be prefix-rewritten. This task changes the name to `muchai` and adds a one-time startup migration that moves the legacy dirs and fixes the stored paths. The migration never reads or logs `config.json` contents beyond deserializing to rewrite two path fields (no token logging).

- [ ] **Step 1: Write failing tests for the two pure helpers**

Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/config.rs`:
```rust
#[test]
fn migrate_dir_moves_when_new_absent_and_legacy_present() {
    let base = std::env::temp_dir().join(format!("muchai-mig-{}", std::process::id()));
    let legacy = base.join("share/fridai");
    let new = base.join("share/muchai");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("marker.txt"), b"keep me").unwrap();

    let moved = migrate_dir(&legacy, &new).unwrap();

    assert!(moved);
    assert!(!legacy.exists());
    assert_eq!(std::fs::read(new.join("marker.txt")).unwrap(), b"keep me");
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn migrate_dir_is_noop_when_new_already_exists() {
    let base = std::env::temp_dir().join(format!("muchai-mig2-{}", std::process::id()));
    let legacy = base.join("fridai");
    let new = base.join("muchai");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(legacy.join("old.txt"), b"x").unwrap();

    let moved = migrate_dir(&legacy, &new).unwrap();

    assert!(!moved);
    assert!(legacy.exists()); // untouched
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn migrate_dir_is_noop_when_legacy_absent() {
    let base = std::env::temp_dir().join(format!("muchai-mig3-{}", std::process::id()));
    let legacy = base.join("fridai");
    let new = base.join("muchai");
    let moved = migrate_dir(&legacy, &new).unwrap();
    assert!(!moved);
    assert!(!new.exists());
}

#[test]
fn rewrite_data_paths_rehomes_paths_under_legacy_and_leaves_others() {
    let legacy = Path::new("/home/u/.local/share/fridai");
    let new = Path::new("/home/u/.local/share/muchai");
    let mut cfg = default_config();
    cfg.gallery_dir = "/home/u/.local/share/fridai/gallery".into();
    cfg.models_dir = "/mnt/big/models".into(); // custom, outside legacy prefix

    rewrite_data_paths(&mut cfg, legacy, new);

    assert_eq!(cfg.gallery_dir, "/home/u/.local/share/muchai/gallery");
    assert_eq!(cfg.models_dir, "/mnt/big/models"); // unchanged
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib config::tests::migrate 2>&1; cargo test --manifest-path src-tauri/Cargo.toml --lib config::tests::rewrite 2>&1`
Expected: compile error / FAIL — `migrate_dir` and `rewrite_data_paths` don't exist yet.

- [ ] **Step 3: Implement the helpers and switch the app name to `muchai`**

In `src-tauri/src/config.rs`, replace the `project_dirs` function (lines 5-10) with the updated name + comment:
```rust
fn project_dirs() -> Option<ProjectDirs> {
    // On Linux the qualifier/organization are ignored — XDG paths use only the
    // app name ("muchai"), so ~/.config/muchai and ~/.local/share/muchai are
    // stable regardless of the qualifier/org values passed here.
    ProjectDirs::from("cz", "mst", "muchai")
}
```

Change the fallback on line 27 from `PathBuf::from("./fridai-config.json")` to:
```rust
        .unwrap_or_else(|| PathBuf::from("./muchai-config.json"))
```

Add these functions after `save_config_to` (after line 68, before the `#[cfg(test)]` block):
```rust
/// Move `legacy` to `new` if `new` doesn't exist yet and `legacy` does.
/// Best-effort and idempotent: returns Ok(false) (no-op) if `new` already
/// exists or `legacy` is absent. Never inspects directory contents.
fn migrate_dir(legacy: &Path, new: &Path) -> std::io::Result<bool> {
    if new.exists() || !legacy.exists() {
        return Ok(false);
    }
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(legacy, new)?;
    Ok(true)
}

/// Rehome an absolute path that lives under `legacy` onto `new`, preserving the
/// relative tail. Returns None if the path isn't under `legacy`.
fn rewrite_prefix(p: &str, legacy: &Path, new: &Path) -> Option<String> {
    let rel = Path::new(p).strip_prefix(legacy).ok()?;
    Some(new.join(rel).to_string_lossy().into_owned())
}

/// Fix up stored gallery/models paths that pointed under the old data dir so
/// they follow the renamed directory. User-chosen paths outside the old data
/// dir are left untouched.
pub fn rewrite_data_paths(cfg: &mut AppConfig, legacy_data: &Path, new_data: &Path) {
    if let Some(g) = rewrite_prefix(&cfg.gallery_dir, legacy_data, new_data) {
        cfg.gallery_dir = g;
    }
    if let Some(m) = rewrite_prefix(&cfg.models_dir, legacy_data, new_data) {
        cfg.models_dir = m;
    }
}

/// One-time rebrand migration: move ~/.config/fridai → ~/.config/muchai and
/// ~/.local/share/fridai → ~/.local/share/muchai, then rehome the absolute
/// gallery/models paths inside the migrated config. Safe to call on every
/// startup: it no-ops once the new dirs exist. Does not log config contents.
pub fn migrate_legacy_data_dirs() {
    let (Some(legacy), Some(current)) = (
        ProjectDirs::from("cz", "mst", "fridai"),
        ProjectDirs::from("cz", "mst", "muchai"),
    ) else {
        return;
    };
    let _ = migrate_dir(legacy.config_dir(), current.config_dir());
    let _ = migrate_dir(legacy.data_dir(), current.data_dir());

    let cfg_path = current.config_dir().join("config.json");
    if cfg_path.exists() {
        let mut cfg = load_config_from(&cfg_path);
        rewrite_data_paths(&mut cfg, legacy.data_dir(), current.data_dir());
        let _ = save_config_to(&cfg_path, &cfg);
    }
}
```

- [ ] **Step 4: Run tests to confirm they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: all pass, including the four new tests.

- [ ] **Step 5: Wire the migration into startup (before config load)**

In `src-tauri/src/lib.rs`, in `run()`, add the migration call immediately before line 24 so it runs before the config is loaded:
```rust
    config::migrate_legacy_data_dirs();
    let initial = config::load_config_from(&config::config_file_path());
```

- [ ] **Step 6: Build to confirm wiring compiles**

Run: `cargo build --manifest-path src-tauri/Cargo.toml --lib`
Expected: clean (no unused-function warning — `migrate_legacy_data_dirs` is now called).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/config.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat: migrate ~/.config/fridai + data dirs to muchai on startup

Part of the FridAI→MuchAI rebrand. Switches the app-name used for XDG
paths to "muchai" and adds a one-time, idempotent startup migration that
renames the legacy fridai config/data directories and rehomes the stored
absolute gallery/models paths, so existing installs keep their tokens,
settings, gallery, and models.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Tauri config (product name, identifier, window title, CSP scope)

**Files:**
- Modify: `src-tauri/tauri.conf.json` lines 3, 5, 15, 24

**Note on `productName`:** keep it lowercase `muchai` (matching the Cargo bin name from Task 1 and the previous lowercase `fridai`) to avoid bundle/binary-naming surprises. The visible brand comes from the window `title` and the frontend `<h1>` (Task 4), not `productName`. The `identifier` change is safe: this app resolves its data dir via the `directories` crate (Task 2), not Tauri's path API, so changing `identifier` moves no user data.

- [ ] **Step 1: Edit the four values**

- Line 3: `"productName": "fridai",` → `"productName": "muchai",`
- Line 5: `"identifier": "cz.mst.fridai",` → `"identifier": "cz.mst.muchai",`
- Line 15: `"title": "FridAI",` → `"title": "MuchAI",`
- Line 24: `"scope": ["$APPDATA/**", "$HOME/.local/share/fridai/**"]` → `"scope": ["$APPDATA/**", "$HOME/.local/share/muchai/**"]`

- [ ] **Step 2: Build to validate the config**

Run: `cargo build --manifest-path src-tauri/Cargo.toml --lib`
Expected: clean — `generate_context!` parses `tauri.conf.json` at compile time, so a malformed edit would fail here.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "chore: rebrand Tauri config to MuchAI (identifier, title, CSP scope)"
```

---

### Task 4: Frontend brand strings

**Files:**
- Modify: `package.json:2`
- Modify: `src/routes/+page.svelte:64`
- Modify: `src/lib/components/WelcomeDialog.svelte:16`
- Modify: `src/lib/components/PreferencesDialog.svelte:76`
- Modify: `src/lib/helpText.ts:31`

- [ ] **Step 1: Edit each occurrence**

- `package.json:2`: `"name": "fridai",` → `"name": "muchai",`
- `src/routes/+page.svelte:64`: `<h1 class="brand">FridAI</h1>` → `<h1 class="brand">MuchAI</h1>`
- `WelcomeDialog.svelte:16`: `<h2 id="welcome-title">Welcome to FridAI 👋</h2>` → `<h2 id="welcome-title">Welcome to MuchAI 👋</h2>`
- `PreferencesDialog.svelte:76`: `...token is all FridAI needs...` → `...token is all MuchAI needs...`
- `helpText.ts:31`: `'Default' lets FridAI choose for you.` → `'Default' lets MuchAI choose for you.`

- [ ] **Step 2: Frontend gate**

Run: `npm run check`
Expected: 0 errors / 0 warnings.

- [ ] **Step 3: Commit**

```bash
git add package.json src/routes/+page.svelte src/lib/components/WelcomeDialog.svelte src/lib/components/PreferencesDialog.svelte src/lib/helpText.ts
git commit -m "chore(ui): rebrand user-facing strings FridAI → MuchAI"
```

---

### Task 5: Rust cosmetic strings (doc comments, panic message, test temp-dir prefixes)

**Files:**
- Modify: all remaining `fridai`/`fridAI`/`FridAI` occurrences under `src-tauri/src/**/*.rs`

After Tasks 1–2, every remaining occurrence in `src-tauri/src` is cosmetic: the `.expect("...fridAI")` panic string (`lib.rs:99`), doc comments (`devices.rs:115,131`, `types.rs:279`), the `/no/such/fridai/dir` test path (`models.rs:94`), and per-test temp-dir name prefixes (`fridai-*` in `commands.rs`, `config.rs`, `devices.rs`, `downloader.rs`, `engine.rs`, `gallery.rs`, `models.rs`, `sysmon/providers.rs`). A blanket case-sensitive replace is safe because all real identifiers were already changed to `muchai`.

- [ ] **Step 1: Apply the case-sensitive replacements**

```bash
git grep -lE 'fridai|fridAI|FridAI' -- src-tauri/src \
  | xargs sed -i -e 's/fridai/muchai/g' -e 's/fridAI/MuchAI/g' -e 's/FridAI/MuchAI/g'
```
(The three patterns are disjoint strings, so `-e` order doesn't matter. `-- src-tauri/src` is a recursive directory pathspec — all files there are `.rs`, and only `.rs` files contain the name.)

- [ ] **Step 2: Verify nothing remains and the suite is green**

Run: `git grep -nE 'fridai|fridAI|FridAI' -- src-tauri/src || echo "clean (no matches)"`
Expected: prints `clean (no matches)`.
Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib && cargo build --manifest-path src-tauri/Cargo.toml --lib`
Expected: all tests pass, clean build.

- [ ] **Step 3: Commit**

```bash
git add -A src-tauri/src
git commit -m "chore: rename residual fridai strings in Rust sources to muchai"
```

---

### Task 6: AppImage build script

**Files:**
- Modify: `scripts/build-appimage.sh` (lines 2, 37, 58, 60, 63)

Tauri generates the AppDir name from `productName`, now `muchai` (Task 3), so the script's hardcoded `fridai.AppDir` and output filename must follow, or the repack step won't find the AppDir.

- [ ] **Step 1: Apply the replacements**

```bash
sed -i -e 's/fridai/muchai/g' -e 's/fridAI/MuchAI/g' scripts/build-appimage.sh
```
This updates the header comment (`fridAI`→`MuchAI`), `APPDIR=".../muchai.AppDir"`, `OUTPUT="muchai_0.1.0_amd64.AppImage"`, `--appdir muchai.AppDir`, and the final `ls` path.

- [ ] **Step 2: Syntax-check the script**

Run: `bash -n scripts/build-appimage.sh ; echo "exit=$?"`
Expected: exit=0 (no syntax error). NOTE: a full AppImage build (`npm run tauri build`) needs the prebuilt Vulkan engine bundle and the linuxdeploy plugin, so end-to-end AppImage verification is a manual step for the maintainer, out of scope for this task's automated gate.

- [ ] **Step 3: Commit**

```bash
git add scripts/build-appimage.sh
git commit -m "chore: update AppImage build script for the muchai rename"
```

---

### Task 7: Rename internal docs (specs + plans)

**Files:**
- Modify (content): all `docs/superpowers/**/*.md` containing the name — **except** this plan file, `docs/superpowers/plans/2026-07-17-rename-fridai-to-muchai.md` (renaming its own text would corrupt the fridai→muchai explanation).
- Rename (git mv): the two docs whose filenames contain `fridai`.

- [ ] **Step 1: Rewrite content (excluding this plan file)**

```bash
git grep -lE 'fridai|fridAI|FridAI' -- docs \
  | grep -v 'plans/2026-07-17-rename-fridai-to-muchai.md' \
  | xargs sed -i -e 's/fridai/muchai/g' -e 's/fridAI/MuchAI/g' -e 's/FridAI/MuchAI/g'
```

- [ ] **Step 2: Rename the two fridai-named files**

```bash
git mv docs/superpowers/plans/2026-06-25-fridai-beta-image-generation.md \
       docs/superpowers/plans/2026-06-25-muchai-beta-image-generation.md
git mv docs/superpowers/specs/2026-06-25-fridai-beta-image-generation-design.md \
       docs/superpowers/specs/2026-06-25-muchai-beta-image-generation-design.md
```

- [ ] **Step 3: Verify only this plan file still mentions the old name**

Run: `git grep -lE 'fridai|fridAI|FridAI' -- docs`
Expected: only `docs/superpowers/plans/2026-07-17-rename-fridai-to-muchai.md` (by design).

- [ ] **Step 4: Commit**

```bash
git add -A docs/
git commit -m "docs: rebrand specs and plans FridAI → MuchAI"
```

---

### Task 8: Manual ops — GitHub repo, git remote, working folder (CONTROLLER/HUMAN, not a subagent)

**Do NOT dispatch this to an implementer subagent.** These steps affect resources outside the repo and the session's own working directory. Run them **after** Tasks 1–7 are merged to `main` (via `superpowers:finishing-a-development-branch`).

- [ ] **Step 1: Rename the GitHub repository**

Either in the GitHub UI (Settings → Repository name → `muchai`), or:
```bash
gh repo rename muchai
```
GitHub keeps redirects from the old `fridai` URL (web, git, API), so existing clones keep working until the remote is updated.

- [ ] **Step 2: Update the local git remote**

```bash
git remote set-url origin https://github.com/idahomst/muchai.git
git remote -v   # confirm
```

- [ ] **Step 3: Rename the working folder — LAST, and note the caveats**

This is the session's current working directory, so renaming it mid-session breaks tool paths. Do it as the very last action, from a shell outside the folder:
```bash
mv ~/g/mst/fridai ~/g/mst/muchai
```
Caveats to tell the user:
- Any open editor/terminal/Claude Code session rooted at the old path must be reopened at `~/g/mst/muchai`.
- Claude Code keys project history/memory to the folder path (`-home-idaho-g-mst-fridai`); after the move it starts a fresh project context at the new path — prior session history/memory won't auto-follow.

---

## Post-implementation

After Tasks 1–7 pass their reviews:
1. Dispatch a final whole-branch code reviewer (focus: no stray `fridai` in shipped code, migration correctness, no token logging).
2. Use `superpowers:finishing-a-development-branch` to merge `rename/fridai-to-muchai` → `main` (verify `cargo test` + `npm run check` on the merged result).
3. Then perform Task 8 (manual ops).

