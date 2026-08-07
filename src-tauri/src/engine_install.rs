//! Downloading, verifying and installing an engine release.
//!
//! The store is `~/.local/share/muchai/engines/<tag>/`, holding the extracted
//! archive much as `scripts/fetch-engine.sh` lays it out: `sd-cli` plus its
//! sibling `.so` files, ~118 MB. (The script additionally deletes `sd-server`;
//! extraction here keeps it, so the store is slightly larger than `binaries/`.)
//!
//! Install is *download → verify → extract to `.staging-<tag>/` → validate →
//! `rename()`*. Verification precedes extraction deliberately, and that
//! ordering is load-bearing: it is the whole reason `extract_zip` decompresses
//! without a size cap. Do not reorder these.
//! Rename is atomic within a filesystem, so a directory named `<tag>` existing
//! is itself proof that a fully-downloaded, hash-verified, flag-checked engine
//! is inside. A crash or kill mid-install can only leave a `.staging-*`
//! directory, which is swept on next start. No partially-installed engine can
//! ever be selected, and there is no validity flag to drift.

use std::path::{Path, PathBuf};

/// Prefix marking an install still in progress. Never a valid tag, because
/// tags start with `master-`.
const STAGING_PREFIX: &str = ".staging-";

/// Where a finished install lives. Its existence is the completion proof.
pub fn install_dir(root: &Path, tag: &str) -> PathBuf {
    root.join(tag)
}

/// Where an install is assembled before the atomic rename.
pub fn staging_dir(root: &Path, tag: &str) -> PathBuf {
    root.join(format!("{STAGING_PREFIX}{tag}"))
}

/// Delete every entry named `.staging-*` under `root`. Called once at startup: a
/// crash or kill mid-install can only leave one of these behind, and a staging
/// directory is by definition incomplete. Best-effort — a failure here just
/// wastes disk, it cannot break anything.
pub fn sweep_staging(root: &Path) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(STAGING_PREFIX) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Installed engine tags, newest build first. Staging directories and anything
/// whose name is not a parseable tag are ignored.
pub fn installed_tags(root: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut tagged: Vec<(u32, String)> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let parsed = crate::engine_release::parse_tag(&name)?;
            Some((parsed.build, name))
        })
        .collect();
    tagged.sort_by(|a, b| b.0.cmp(&a.0));
    tagged.into_iter().map(|(_, name)| name).collect()
}

/// Keep the `keep` newest installed engines plus `protect`, delete the rest.
///
/// Two copies at ~113 MB each is a fair tax; an unbounded collection is not.
/// `protect` is the currently-selected tag: normally that is the newest and the
/// argument is redundant, but a user who deliberately went back to an older
/// build must not have it deleted out from under them. Directories that are not
/// parseable tags are never touched — they are not ours.
pub fn prune(root: &Path, keep: usize, protect: &str) {
    for (i, tag) in installed_tags(root).into_iter().enumerate() {
        if i < keep || tag == protect {
            continue;
        }
        let _ = std::fs::remove_dir_all(install_dir(root, &tag));
    }
}

/// Message shown for any structurally bad or hostile archive. Deliberately one
/// message for all of them: the user's action is identical (try again), and
/// distinguishing "corrupt" from "malicious" tells an attacker more than it
/// tells the user.
const BAD_ARCHIVE: &str = "The downloaded archive was damaged. Try again.";

/// Extract `zip_path` into `dest`. Every entry lands inside `dest` or the whole
/// extraction fails.
///
/// A zip is attacker-shaped input arriving over the network, and `../` in an
/// entry name is precisely how an archive writes outside the directory you
/// extracted it to. `enclosed_name()` is what guarantees containment, but note
/// what it actually does: it returns `None` for a name that climbs out via
/// `..`, and *strips* a leading `/` or drive prefix rather than rejecting it —
/// so `/etc/passwd` extracts to `dest/etc/passwd`. Either way the result is
/// always contained, which is the property this function needs. (`zip` changed
/// the absolute-path behaviour deliberately, to match other zip tools; do not
/// re-read this as "absolute paths are refused" on the next major bump.)
///
/// A refusal aborts the whole extraction rather than skipping the offending
/// entry — an archive containing a traversal entry is not one to trust the rest
/// of. It also aborts *before* writing anything from that entry, so a refused
/// archive leaves nothing behind.
pub fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file =
        std::fs::File::open(zip_path).map_err(|e| format!("Couldn't open the download: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| BAD_ARCHIVE.to_string())?;
    std::fs::create_dir_all(dest)
        .map_err(|e| format!("Couldn't create {}: {e}", dest.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|_| BAD_ARCHIVE.to_string())?;
        let rel = entry.enclosed_name().ok_or_else(|| BAD_ARCHIVE.to_string())?;
        // A name that resolves to nothing at all — a bare UNC prefix like
        // `//srv/share` parses entirely as a Windows prefix — would make `out`
        // equal `dest`, and the file branch below would then try to `File::create`
        // a directory. Refuse it as the malformed entry it is, so the user sees
        // BAD_ARCHIVE rather than a raw EISDIR quoting the staging path.
        if rel.as_os_str().is_empty() {
            return Err(BAD_ARCHIVE.to_string());
        }
        let out = dest.join(&rel);

        if entry.is_dir() {
            std::fs::create_dir_all(&out)
                .map_err(|e| format!("Couldn't create {}: {e}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
        }
        let mut w = std::fs::File::create(&out)
            .map_err(|e| format!("Couldn't write {}: {e}", out.display()))?;
        std::io::copy(&mut entry, &mut w).map_err(|_| BAD_ARCHIVE.to_string())?;

        // Engine archives carry the executable bit on sd-cli and the .so files;
        // losing it would make the freshly installed engine unspawnable. That
        // one bit is all we take: `unix_mode` returns the archive's mode
        // verbatim, file-type and setuid bits included, and this updater exists
        // to adopt whatever a third party's CI emits, unreviewed, forever. An
        // upstream umask slip shipping 0o777 would otherwise leave sd-cli
        // world-writable in the user's data directory — and MuchAI executes it
        // on every generation.
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let mode = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

/// SHA-256 of a file, lowercase hex. Streamed through `io::copy` rather than
/// read into a buffer — the archive is ~45 MB and there is no reason to hold it
/// in memory on top of the copy already on disk.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f =
        std::fs::File::open(path).map_err(|e| format!("Couldn't read the download: {e}"))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher).map_err(|e| format!("Couldn't read the download: {e}"))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Compare a downloaded file against the digest the API published next to its
/// URL.
///
/// `None` means nothing usable was published — an absent `digest`, or one in an
/// algorithm `parse_release_json` doesn't recognise — which is not a failure.
/// Skipping the check beats comparing a sha512 against a sha256 and rejecting
/// every good download if GitHub ever switches.
///
/// Be aware of what that costs, because more rests on this check than integrity
/// alone: `install_release` verifies *before* extracting, and that ordering is
/// why `extract_zip` decompresses without a size cap — a zip bomb would first
/// have to match a digest GitHub itself published. If GitHub ever stops
/// emitting `digest`, or emits it as `SHA256:` (the prefix strip is
/// case-sensitive), this returns `Ok(())` and unverified bytes reach the
/// extractor with that argument quietly gone. It is an accepted risk, not an
/// oversight: an upstream that can forge the digest can simply ship a malicious
/// `sd-cli` instead, which is strictly worse and no cap would help.
pub fn verify_hash(path: &Path, expected: Option<&str>) -> Result<(), String> {
    let Some(expected) = expected else { return Ok(()) };
    let actual = sha256_file(path)?;
    if actual == expected {
        Ok(())
    } else {
        Err("The download didn't match its published checksum. Try again.".to_string())
    }
}

/// Bytes that must be free before starting an install.
///
/// At the peak the archive and its extraction sit side by side in the staging
/// directory — the `.part` file is *renamed* into the archive rather than
/// copied, so those two are one file. Measured against the pinned asset: 45.0 MB
/// compressed, 117.8 MB extracted (`src-tauri/binaries/engine`, plus ~1.4 MB for
/// the `sd-server` that `fetch-engine.sh` deletes and extraction here keeps), so
/// extraction is ~2.6× and the peak is ~3.7×. 5× leaves real slack for an asset
/// that compresses better or ships more files; 4× left only ~9%, which is not
/// enough margin to be worth the ~45 MB saved on a guard this coarse.
///
/// The shared headroom is deliberately *not* added here. `diskspace::fits` is
/// the one place that reserves it — it requires `free - required >= HEADROOM` —
/// and both existing callers (`downloader.rs`, `commands.rs`) pass their raw
/// need and let `fits` apply the policy. Folding a second `HEADROOM_BYTES` in
/// here would demand ~2.2 GB free for a 45 MB engine instead of ~1.2 GB.
///
/// This is a UX guard, not a security guard. `asset_size` is whatever the API
/// declared, and the estimate bounds the *expected* download; it bounds nothing
/// about what extraction actually writes to disk. Its job is to say "not enough
/// room" up front instead of failing halfway through a 45 MB download.
///
/// Saturating, for the same reason `diskspace::fits` uses `checked_sub`: an
/// absurd declared size must report "does not fit", not wrap into a false pass
/// (or panic in a debug build, inside a Tauri command).
pub fn required_space(asset_size: u64) -> u64 {
    asset_size.saturating_mul(5)
}

/// The most missing flags to name in an error message. Beyond a handful the
/// list stops informing and starts overwhelming — and if this many are gone,
/// the one fact that matters is "this build is not compatible".
const MAX_REPORTED_FLAGS: usize = 3;

/// How long to wait for a probe before giving up on it. This runs a binary that
/// was downloaded from the internet minutes ago, and `install_release` holds the
/// generation lock while it does — an unbounded wait here wedges the app with no
/// cancel path. Ten seconds mirrors `devices::engine_version`, probing the same
/// binary for the same kind of answer.
#[cfg(not(test))]
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Shortened under test so the give-up path can actually be exercised; a
/// ten-second test would simply be deleted by the next person who saw it.
#[cfg(test)]
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);

/// Run a probe subcommand against the engine in `dir`, returning its combined
/// output.
///
/// The environment is deliberately left alone. The engine's `.so` files sit
/// beside the binary rather than on the system path, and `sd-cli` finds them
/// through its own `RUNPATH` of `$ORIGIN` — which is also how `run_generation`
/// and `devices::engine_version` run it, neither of which sets
/// `LD_LIBRARY_PATH`. Setting it here would make validation strictly more
/// permissive than production: the loader searches `LD_LIBRARY_PATH` before
/// `RUNPATH`, so a build that lost its `$ORIGIN` would probe clean, install,
/// and then fail every generation with "error while loading shared libraries".
/// Validation is only worth anything if it runs the binary the way we will.
///
/// The exit status is deliberately not consulted: what a probe *printed* is the
/// evidence, and a build that exits non-zero from `--help` (a common argument
/// parser habit) is not thereby a bad build. Same reasoning as the merge of
/// stderr into stdout below — `-h`/`--help` lands on one or the other depending
/// on the build, and reading only one is a live way to end up with a help
/// string `engine_flags` cannot parse.
fn probe(dir: &Path, arg: &str) -> Result<String, String> {
    use std::io::Read;
    let bin = dir.join(crate::commands::engine_binary_name());
    let mut child = crate::engine::retrying_while_busy(|| {
        std::process::Command::new(&bin)
            .arg(arg)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
    })
    .map_err(|e| format!("The downloaded engine didn't start: {e}"))?;

    // Both pipes are drained on their own threads: a probe that fills one while
    // we blocked on the other would deadlock, and that is a wedge no timeout on
    // a single stream would clear. Slot 0 is stdout, so the halves always
    // recombine in the same order however they arrive.
    let mut out = child.stdout.take().expect("stdout piped");
    let mut err = child.stderr.take().expect("stderr piped");
    let (tx, rx) = std::sync::mpsc::channel();
    let tx_err = tx.clone();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        let _ = tx.send((0, s));
    });
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        let _ = tx_err.send((1, s));
    });

    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    let mut parts = [String::new(), String::new()];
    let mut timed_out = false;
    for _ in 0..parts.len() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok((slot, s)) => parts[slot] = s,
            Err(_) => {
                timed_out = true;
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();

    if timed_out {
        return Err(format!("The downloaded engine didn't answer `{arg}` in time."));
    }
    Ok(format!("{}{}", parts[0], parts[1]))
}

/// Check that a freshly extracted engine can run and still speaks our dialect.
/// Returns its commit on success.
///
/// Milliseconds, no model load. This catches a build that won't start and a
/// build that dropped a flag MuchAI emits. It cannot catch a build that accepts
/// every flag and produces garbage output — the yellow-square failure that
/// motivated pinning the engine in the first place. There is no cheap probe for
/// that; the mitigation is the inline revert offered on first-run failure.
///
/// One more blind spot, for the next reader asking "why did validation pass?":
/// `extract_zip` writes a symlink entry as an ordinary file holding its target
/// as text. The pinned asset dereferences its sonames, so this is latent, but
/// were upstream to start packaging symlinks, a mangled `libstable-diffusion.so`
/// would fail to link and the identity probe would catch it — while a mangled
/// `libggml-cpu-*.so` is `dlopen`ed lazily, passes cleanly here, and would
/// surface only at generation time.
fn validate_engine(dir: &Path) -> Result<String, String> {
    let version_out = probe(dir, "--version")?;
    let commit = crate::devices::parse_engine_version(&version_out)
        .ok_or_else(|| "The downloaded engine didn't start properly.".to_string())?;

    // `probe` merges stdout and stderr deliberately — `engine_flags` documents
    // that contract, because a build that prints help to stderr would otherwise
    // parse as empty and come back `Unparseable`.
    let help = probe(dir, "--help")?;
    match crate::engine_flags::missing_flags(&help) {
        crate::engine_flags::FlagCheck::Compatible => {}
        crate::engine_flags::FlagCheck::Missing(missing) => {
            let shown: Vec<&str> = missing.iter().take(MAX_REPORTED_FLAGS).copied().collect();
            let more = missing.len().saturating_sub(shown.len());
            let tail = if more > 0 { format!(" and {more} more") } else { String::new() };
            return Err(format!(
                "This engine build isn't compatible with MuchAI — it no longer supports {}{}. Keeping your current engine.",
                shown.join(", "),
                tail
            ));
        }
        // Distinct wording on purpose: reporting all 22 flags as "missing" would
        // be false and would point the user at twenty-two dead ends instead of
        // the one real problem.
        crate::engine_flags::FlagCheck::Unparseable { .. } => {
            return Err(
                "MuchAI couldn't read this engine build's --help output, so it can't confirm the build is compatible. Keeping your current engine."
                    .to_string(),
            );
        }
    }
    Ok(commit)
}

/// Validate the staging tree, then move it into place atomically.
///
/// Everything destructive happens *after* validation passes, and the final move
/// is a single `rename()`. On any failure the staging tree is removed and the
/// previously installed engine is untouched.
fn finish_install(root: &Path, tag: &str) -> Result<String, String> {
    let staging = staging_dir(root, tag);
    let dest = install_dir(root, tag);

    let finish = || -> Result<String, String> {
        // sd-server ships in the archive and MuchAI never spawns it; ~1.4 MB of
        // dead weight per install. fetch-engine.sh drops it for the same reason.
        let _ = std::fs::remove_file(staging.join("sd-server"));

        // Belt and braces on the executable bit. Extraction preserves the mode
        // recorded in the archive, but an archive built on a filesystem that
        // does not carry the bit would produce an engine that cannot be
        // spawned — and validation below is the first thing that would try.
        // fetch-engine.sh chmods for the same reason.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let bin = staging.join(crate::commands::engine_binary_name());
            if let Ok(meta) = std::fs::metadata(&bin) {
                let mode = meta.permissions().mode() | 0o111;
                let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(mode));
            }
        }

        let commit = validate_engine(&staging)?;

        // Reinstalling the same tag must replace, not merge — a leftover file
        // from a previous attempt could shadow a renamed one.
        if dest.exists() {
            std::fs::remove_dir_all(&dest)
                .map_err(|e| format!("Couldn't replace the existing engine: {e}"))?;
        }
        std::fs::rename(&staging, &dest).map_err(|e| format!("Couldn't install the engine: {e}"))?;
        Ok(commit)
    };

    let result = finish();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// Pure space predicate, split out so it is testable without a real disk.
fn check_space_for(asset_size: u64, free: u64) -> Result<(), String> {
    let need = required_space(asset_size);
    if crate::diskspace::fits(free, need) {
        return Ok(());
    }
    Err(format!(
        "{}: installing this engine needs about {} free, but only {} is available.",
        crate::downloader::INSUFFICIENT_SPACE_PREFIX,
        crate::diskspace::fmt_bytes(need),
        crate::diskspace::fmt_bytes(free),
    ))
}

/// Download, verify, extract, validate and install a release. Returns the
/// engine's commit on success.
///
/// `on_progress` receives `(downloaded, total)` during the transfer only;
/// extraction and validation are fast enough not to need their own reporting.
pub fn install_release(
    root: &Path,
    release: &crate::engine_release::EngineRelease,
    free_bytes: u64,
    on_progress: impl FnMut(u64, Option<u64>),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<String, String> {
    // The tag names a directory under the engines root, so it must never be able
    // to climb out of it. `to_engine_release` is where that is enforced — it
    // refuses any tag `parse_tag` rejects — but `EngineRelease`'s fields are
    // `pub`, so a struct literal written elsewhere in the crate (a test fixture,
    // most plausibly) could still carry `../../evil` here. This documents the
    // invariant at the boundary that depends on it, and costs nothing in release.
    debug_assert!(crate::engine_release::parse_tag(&release.tag).is_some());

    check_space_for(release.asset.size, free_bytes)?;

    let staging = staging_dir(root, &release.tag);
    let _ = std::fs::remove_dir_all(&staging); // a previous attempt, swept early
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("Couldn't create {}: {e}", staging.display()))?;

    let cleanup = |e: String| {
        let _ = std::fs::remove_dir_all(&staging);
        e
    };

    let archive = staging.join("engine.zip");
    // Empty token: the engine archive is a public GitHub asset, unlike the
    // HuggingFace and Civitai paths that share this downloader.
    crate::downloader::download_to(&release.asset.url, "", &archive, on_progress, cancel)
        .map_err(|e| cleanup(e.message()))?;

    verify_hash(&archive, release.asset.sha256.as_deref()).map_err(cleanup)?;
    extract_zip(&archive, &staging).map_err(cleanup)?;
    std::fs::remove_file(&archive).map_err(|e| cleanup(format!("Couldn't tidy up: {e}")))?;

    finish_install(root, &release.tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("muchai-inst-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn mkdirs(root: &Path, names: &[&str]) {
        for n in names {
            std::fs::create_dir_all(root.join(n)).unwrap();
        }
    }

    fn entries(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    /// Build a zip from `(name, contents)` pairs using the `zip` CLI, run from a
    /// scratch `src/` directory so entry names are exactly the names given.
    fn make_zip(dir: &Path, entries: &[(&str, &str)]) -> PathBuf {
        let src = dir.join("src");
        for (name, body) in entries {
            let p = src.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, body).unwrap();
        }
        let zip_path = dir.join("a.zip");
        let names: Vec<&str> = entries.iter().map(|(n, _)| *n).collect();
        let status = std::process::Command::new("zip")
            .arg("-q")
            .arg(&zip_path)
            .args(&names)
            .current_dir(&src)
            .status()
            .expect("the `zip` CLI must be installed to run these tests");
        assert!(status.success());
        zip_path
    }

    /// Write an executable stand-in for `sd-cli` that echoes canned output.
    ///
    /// The canned text lands in sidecar files the script `cat`s, rather than
    /// inline in the script itself: the real help fixture is 252 lines
    /// containing backticks and `%`, and any attempt to embed it in a shell
    /// string has the shell re-read them. `cat` reproduces the bytes exactly,
    /// which is the whole point — the parser under test must see what the real
    /// engine prints.
    fn fake_engine(dir: &Path, version_out: &str, help_out: &str, exit_code: i32) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("version-out.txt"), version_out).unwrap();
        std::fs::write(dir.join("help-out.txt"), help_out).unwrap();
        let script = format!(
            "#!/bin/sh\n\
             d=$(dirname \"$0\")\n\
             case \"$1\" in\n\
             --version) cat \"$d/version-out.txt\" ;;\n\
             --help) cat \"$d/help-out.txt\" ;;\n\
             esac\n\
             exit {exit_code}\n"
        );
        let p = dir.join(crate::commands::engine_binary_name());
        std::fs::write(&p, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// As `fake_engine`, but the build prints its help to stderr — which real
    /// argument parsers commonly do, and which is the whole reason `probe`
    /// merges the two streams.
    fn fake_engine_talking_to_stderr(dir: &Path, version_out: &str, help_out: &str) {
        fake_engine(dir, version_out, help_out, 0);
        let script = "#!/bin/sh\n\
                      d=$(dirname \"$0\")\n\
                      case \"$1\" in\n\
                      --version) cat \"$d/version-out.txt\" ;;\n\
                      --help) cat \"$d/help-out.txt\" >&2 ;;\n\
                      esac\n\
                      exit 0\n";
        let p = dir.join(crate::commands::engine_binary_name());
        std::fs::write(&p, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The pinned engine's real `--help`, byte for byte.
    fn full_help() -> String {
        include_str!("../fixtures/sd-help.txt").to_string()
    }

    const GOOD_VERSION: &str = "stable-diffusion.cpp version unknown, commit 5ef4a75\n";

    #[test]
    fn extracts_a_normal_archive() {
        let root = tmp("unzip-ok");
        let zip_path =
            make_zip(&root, &[("sd-cli", "binary"), ("libggml.so", "lib"), ("nested/x.txt", "n")]);
        let dest = root.join("out");

        extract_zip(&zip_path, &dest).unwrap();

        assert_eq!(std::fs::read_to_string(dest.join("sd-cli")).unwrap(), "binary");
        assert_eq!(std::fs::read_to_string(dest.join("libggml.so")).unwrap(), "lib");
        assert_eq!(std::fs::read_to_string(dest.join("nested/x.txt")).unwrap(), "n");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Defensive, not a live fix: the currently pinned asset is flat — 36 plain
    /// file entries, no directory entries at all — but nothing upstream pins
    /// that. An archive built with `zip -r` emits an explicit entry per
    /// directory, and treating one as a file both loses an empty directory
    /// outright and makes the *next* entry under it unwritable, failing the
    /// whole extraction.
    #[test]
    fn extracts_directory_entries_including_empty_ones() {
        let root = tmp("unzip-dirs");
        let src = root.join("src");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::create_dir_all(src.join("emptydir")).unwrap();
        std::fs::write(src.join("nested/x.txt"), "n").unwrap();
        let zip_path = root.join("d.zip");
        let status = std::process::Command::new("zip")
            .args(["-q", "-r"])
            .arg(&zip_path)
            .args(["nested", "emptydir"])
            .current_dir(&src)
            .status()
            .expect("the `zip` CLI must be installed to run these tests");
        assert!(status.success());

        let dest = root.join("out");
        extract_zip(&zip_path, &dest).unwrap();

        assert_eq!(std::fs::read_to_string(dest.join("nested/x.txt")).unwrap(), "n");
        assert!(dest.join("emptydir").is_dir(), "an empty directory entry must survive");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An archive with no entries still has to leave `dest` behind: the caller's
    /// contract is "after this returns Ok, dest is a directory holding the
    /// archive's contents", and zero contents is not an excuse to skip it.
    #[test]
    fn creates_the_destination_for_an_empty_archive() {
        let root = tmp("unzip-empty");
        // A bare end-of-central-directory record: the smallest valid zip.
        let zip_path = root.join("empty.zip");
        let mut bytes = b"PK\x05\x06".to_vec();
        bytes.extend_from_slice(&[0u8; 18]);
        std::fs::write(&zip_path, &bytes).unwrap();

        let dest = root.join("out");
        extract_zip(&zip_path, &dest).unwrap();

        assert!(dest.is_dir(), "dest must exist even when the archive is empty");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_a_zip_slip_entry() {
        let root = tmp("unzip-slip");
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("ok.txt"), "fine").unwrap();
        std::fs::write(root.join("evil.txt"), "pwned").unwrap();
        let zip_path = root.join("evil.zip");
        // The decoy is re-stamped *after* zipping, below, so that its content
        // differs from the payload the archive carries. Zipping the decoy itself
        // and then asserting it still says "pwned" would pass whether the file
        // was left alone or overwritten with those same bytes.
        // `zip` normalises `../` away unless told not to; -y keeps the literal
        // name. If this ever stops producing a traversal entry the assertion
        // below will fail loudly rather than silently passing.
        let status = std::process::Command::new("zip")
            .args(["-q", "-y"])
            .arg(&zip_path)
            .args(["ok.txt", "../evil.txt"])
            .current_dir(&src)
            .status()
            .expect("the `zip` CLI must be installed to run these tests");
        assert!(status.success());
        // Now the archive says "pwned" and the file on disk says "untouched".
        std::fs::write(root.join("evil.txt"), "untouched").unwrap();

        let dest = root.join("out");
        let err = extract_zip(&zip_path, &dest).unwrap_err();

        assert!(err.contains("archive"), "unexpected message: {err}");
        // Refusing has to mean *nothing was written*, not "written, then an
        // error returned". A validate-as-you-go implementation still reports the
        // violation but leaves the escape behind; only comparing against content
        // the archive does not carry can tell the two apart.
        assert_eq!(std::fs::read_to_string(root.join("evil.txt")).unwrap(), "untouched");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A vanished or unreadable download is an error to report, not a panic:
    /// this runs inside a Tauri command, where unwinding would take the whole
    /// install with it instead of surfacing a message.
    #[test]
    fn reports_a_missing_archive_rather_than_panicking() {
        let root = tmp("unzip-missing");
        let err = extract_zip(&root.join("nope.zip"), &root.join("out")).unwrap_err();

        assert!(err.contains("Couldn't open"), "unexpected message: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An entry name that resolves to nothing must be refused, not carried into
    /// `dest.join()` where it would name `dest` itself. The `zip` CLI cannot
    /// produce such a name, so this one is built with Python — the same escape
    /// hatch the plan documents for hand-shaped archives.
    #[test]
    fn rejects_an_entry_whose_name_resolves_to_nothing() {
        let root = tmp("unzip-empty-name");
        std::fs::create_dir_all(&root).unwrap();
        let zip_path = root.join("unc.zip");
        // `//srv/share` parses entirely as a Windows UNC prefix, so
        // `enclosed_name()` returns Some(""), not None.
        let script = format!(
            "import zipfile; z=zipfile.ZipFile(r'{}','w'); z.writestr('//srv/share','x'); z.close()",
            zip_path.display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &script])
            .status()
            .expect("python3 is needed to build this archive");
        assert!(status.success());

        let err = extract_zip(&zip_path, &root.join("out")).unwrap_err();

        assert!(err.contains("archive"), "unexpected message: {err}");
        // Specifically not a raw EISDIR quoting the staging path at the user.
        assert!(!err.contains("os error"), "internal error leaked: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The likeliest real failure this function has to report: the central
    /// directory is intact so the archive opens and entries enumerate, but a
    /// payload is corrupt. That surfaces from `io::copy`, not from `by_index`,
    /// and it must be an error rather than a panic inside a Tauri command.
    #[test]
    fn reports_a_corrupt_payload_rather_than_panicking() {
        let root = tmp("unzip-corrupt");
        // Poorly-compressible content, so the deflate stream stays long enough
        // to damage in place. A highly repetitive body would compress to a few
        // bytes and leave no room between the local header and the central
        // directory, so the corruption would land in the latter and be caught
        // by a different clause than the one under test.
        let mut seed: u32 = 0x1234_5678;
        let body: String = (0..8000)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                char::from(b'a' + (seed >> 16) as u8 % 26)
            })
            .collect();
        let zip_path = make_zip(&root, &[("sd-cli", body.as_str())]);

        let mut bytes = std::fs::read(&zip_path).unwrap();
        // Damage the tail of the compressed stream: the bytes immediately before
        // the central directory. That leaves every header intact, so the archive
        // opens and the entry enumerates — the failure can only surface while
        // reading the payload.
        let cd = bytes
            .windows(4)
            .position(|w| w == b"PK\x01\x02")
            .expect("the central directory must be present");
        assert!(cd > 60, "the compressed stream is too short to damage in place");
        for b in bytes[cd - 16..cd].iter_mut() {
            *b ^= 0xff;
        }
        std::fs::write(&zip_path, &bytes).unwrap();

        let err = extract_zip(&zip_path, &root.join("out")).unwrap_err();

        assert!(err.contains("damaged"), "unexpected message: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_a_file_that_is_not_a_zip() {
        let root = tmp("unzip-junk");
        let not_zip = root.join("a.zip");
        std::fs::write(&not_zip, b"this is not a zip archive").unwrap();

        assert!(extract_zip(&not_zip, &root.join("out")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extraction_preserves_the_executable_bit() {
        let root = tmp("unzip-mode");
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("sd-cli"), "binary").unwrap();
        std::process::Command::new("chmod")
            .args(["+x", "sd-cli"])
            .current_dir(&src)
            .status()
            .unwrap();
        let zip_path = root.join("m.zip");
        std::process::Command::new("zip")
            .arg("-q")
            .arg(&zip_path)
            .arg("sd-cli")
            .current_dir(&src)
            .status()
            .unwrap();

        let dest = root.join("out");
        extract_zip(&zip_path, &dest).unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dest.join("sd-cli")).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "executable bits must survive extraction");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The archive's mode is a suggestion, not an instruction. Only "is it
    /// executable" is carried across; a world-writable or setuid mode out of
    /// upstream's CI must not reach a binary MuchAI then runs on every
    /// generation.
    #[test]
    fn extraction_normalises_hostile_modes() {
        let root = tmp("unzip-mode-hostile");
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("sd-cli"), "binary").unwrap();
        std::fs::write(src.join("model.txt"), "data").unwrap();
        // 4777: setuid and world-writable, on a file that is also executable.
        // 0666: world-writable, not executable.
        std::process::Command::new("chmod")
            .args(["4777", "sd-cli"])
            .current_dir(&src)
            .status()
            .unwrap();
        std::process::Command::new("chmod")
            .args(["0666", "model.txt"])
            .current_dir(&src)
            .status()
            .unwrap();
        let zip_path = root.join("h.zip");
        std::process::Command::new("zip")
            .arg("-q")
            .arg(&zip_path)
            .args(["sd-cli", "model.txt"])
            .current_dir(&src)
            .status()
            .unwrap();

        let dest = root.join("out");
        extract_zip(&zip_path, &dest).unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &str| {
            std::fs::metadata(dest.join(p)).unwrap().permissions().mode() & 0o7777
        };
        assert_eq!(mode("sd-cli"), 0o755, "executable entry must land as 0755, setuid dropped");
        assert_eq!(mode("model.txt"), 0o644, "non-executable entry must land as 0644");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn builds_install_and_staging_paths() {
        let root = Path::new("/data/engines");
        assert_eq!(install_dir(root, "master-797-5ef4a75"), root.join("master-797-5ef4a75"));
        assert_eq!(staging_dir(root, "master-797-5ef4a75"), root.join(".staging-master-797-5ef4a75"));
    }

    #[test]
    fn sweep_removes_only_staging_directories() {
        let root = tmp("sweep");
        // `.gitkeep` is dot-prefixed but not staging: it pins that the guard is
        // the `.staging-` prefix and not merely "starts with a dot".
        mkdirs(
            &root,
            &[".gitkeep", "master-797-5ef4a75", ".staging-master-799-abc1234", ".staging-junk"],
        );
        std::fs::write(root.join("a-file.txt"), b"x").unwrap();

        sweep_staging(&root);

        assert_eq!(entries(&root), vec![".gitkeep", "a-file.txt", "master-797-5ef4a75"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sweep_on_a_missing_root_is_a_noop() {
        sweep_staging(Path::new("/nonexistent/muchai/engines")); // must not panic
    }

    #[test]
    fn prune_keeps_the_highest_build_numbers() {
        let root = tmp("prune");
        mkdirs(&root, &["master-780-aaaaaaa", "master-797-5ef4a75", "master-791-b8bf676"]);

        prune(&root, 2, "master-797-5ef4a75");

        assert_eq!(entries(&root), vec!["master-791-b8bf676", "master-797-5ef4a75"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_never_deletes_the_selected_engine() {
        let root = tmp("prune-protect");
        mkdirs(&root, &["master-780-aaaaaaa", "master-797-5ef4a75", "master-791-b8bf676"]);

        // The user is deliberately running the oldest one.
        prune(&root, 1, "master-780-aaaaaaa");

        assert!(root.join("master-780-aaaaaaa").exists(), "the running engine must survive");
        assert!(root.join("master-797-5ef4a75").exists(), "the newest is the one kept");
        assert!(!root.join("master-791-b8bf676").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_leaves_unrecognised_directories_alone() {
        let root = tmp("prune-unknown");
        mkdirs(&root, &["master-797-5ef4a75", "master-791-b8bf676", "my-own-build"]);

        prune(&root, 1, "master-797-5ef4a75");

        assert_eq!(entries(&root), vec!["master-797-5ef4a75", "my-own-build"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_on_a_missing_root_is_a_noop() {
        prune(Path::new("/nonexistent/muchai/engines"), 2, "master-797-5ef4a75");
    }

    #[test]
    fn lists_installed_tags_newest_first() {
        let root = tmp("list");
        mkdirs(&root, &["master-780-aaaaaaa", "master-797-5ef4a75", ".staging-master-799-abc1234", "junk"]);
        // A *file* named like a tag is not an installed engine. Listing it would
        // burn a retention slot in `prune` and offer a selection that resolves to
        // nothing; `remove_dir_all` on it fails, so it would never self-heal.
        std::fs::write(root.join("master-999-fffffff"), b"x").unwrap();

        assert_eq!(installed_tags(&root), vec!["master-797-5ef4a75", "master-780-aaaaaaa"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hashes_a_file() {
        let root = tmp("hash");
        let f = root.join("x.bin");
        std::fs::write(&f, b"abc").unwrap();

        // Well-known SHA-256 of "abc".
        assert_eq!(
            sha256_file(&f).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The archive this hashes is 45 MB, but every other test here uses a
    /// three-byte file — so nothing pinned that the *whole* file is read. A
    /// hand-rolled read loop missing its `loop`, added later to report hashing
    /// progress, would digest only the first buffer and quietly accept any
    /// download truncated past it: exactly the failure this task exists to
    /// catch, reported to the user as a damaged archive instead.
    #[test]
    fn hashes_the_whole_file_not_just_the_first_buffer() {
        let root = tmp("hash-big");
        let f = root.join("big.bin");
        // Comfortably more than io::copy's 8 KiB buffer, and non-constant so a
        // partial read cannot coincidentally produce the same digest.
        let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&f, &bytes).unwrap();

        use sha2::{Digest, Sha256};
        let expected = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(sha256_file(&f).unwrap(), expected);

        // And it is genuinely distinguishable from hashing only the first
        // buffer, or this test would pass on a truncating implementation.
        let first_buffer = format!("{:x}", Sha256::digest(&bytes[..8192]));
        assert_ne!(expected, first_buffer);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hashing_a_missing_file_is_an_error() {
        assert!(sha256_file(Path::new("/nonexistent/x.bin")).is_err());
    }

    #[test]
    fn verify_accepts_a_matching_hash_and_rejects_a_mismatch() {
        let root = tmp("verify");
        let f = root.join("x.bin");
        std::fs::write(&f, b"abc").unwrap();
        let good = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        assert!(verify_hash(&f, Some(good)).is_ok());
        assert!(verify_hash(&f, None).is_ok(), "no digest published means nothing to check");

        let err = verify_hash(
            &f,
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
        )
        .unwrap_err();
        assert!(err.contains("didn't match"), "unexpected message: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn required_space_covers_archive_plus_extraction() {
        // Measured bytes, not a ratio: deriving `peak` from the same multiple
        // the implementation uses would let the bound ratify whatever the
        // implementation happens to say. These are the real numbers for the
        // pinned asset — its size from fixtures/gh-release-latest.json, its
        // extraction measured with `du -sb src-tauri/binaries/engine`, plus
        // sd-server, which that directory has had deleted but a staging
        // extraction keeps.
        let asset = 45_020_326u64;
        let extracted = 117_795_060u64 + 1_400_000;
        let peak = asset + extracted;
        let need = required_space(asset);

        assert!(
            need >= peak,
            "the estimate must cover archive + extraction, got {need} for a peak of {peak}"
        );
        // And with room to spare, not by a hair: an engine that compresses a
        // little better than this one must not tip the guard into failing.
        assert!(
            need >= peak + peak / 10,
            "the estimate must leave slack, got {need} for a peak of {peak}"
        );
        assert_eq!(need, asset * 5);
    }

    /// The shared 1 GiB headroom belongs to `fits`, which subtracts it from
    /// what is free. Adding it here as well would silently double it, so this
    /// pins that a machine with exactly headroom-plus-the-install free accepts
    /// the install rather than refusing it for want of a second gigabyte.
    #[test]
    fn required_space_does_not_double_count_the_headroom() {
        let asset = 45_000_000u64;
        // Stated in absolute bytes, deliberately: phrasing it relative to
        // `required_space(asset)` would hold whatever that function returns,
        // since it would only be re-checking `fits`'s own contract.
        let enough = crate::diskspace::HEADROOM_BYTES + asset * 5;

        assert!(
            crate::diskspace::fits(enough, required_space(asset)),
            "headroom plus the install's own bytes must be enough"
        );
    }

    /// An absurd declared size must come out as "does not fit" — `fits` refuses
    /// anything it cannot subtract — rather than wrapping into a false pass or
    /// panicking on overflow inside a Tauri command.
    #[test]
    fn required_space_saturates_instead_of_overflowing() {
        let need = required_space(u64::MAX);

        assert_eq!(need, u64::MAX);
        assert!(!crate::diskspace::fits(u64::MAX, need), "an absurd size must never fit");
    }

    #[test]
    fn validation_accepts_a_good_engine() {
        let root = tmp("val-ok");
        fake_engine(&root, GOOD_VERSION, &full_help(), 0);

        assert_eq!(validate_engine(&root).unwrap(), "5ef4a75");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_rejects_an_engine_that_will_not_run() {
        let root = tmp("val-norun");
        // No binary at all.
        std::fs::create_dir_all(&root).unwrap();

        let err = validate_engine(&root).unwrap_err();
        // The colon is what makes this the *spawn* failure and not the "ran but
        // printed no commit" one below; without it, a `probe` that swallowed
        // spawn errors and returned empty output would still pass this test.
        assert!(err.contains("didn't start:"), "unexpected message: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The engine's `.so` files sit beside the binary and are not on the system
    /// path, so a probe that does not point the loader at the staging directory
    /// fails on a perfectly good build — see the libcuda note in the project's
    /// memory. The stand-in refuses to identify itself unless the variable is
    /// set, exactly as the real loader would.
    /// `engine_flags` states the merge as a caller contract, and reading only
    /// stdout is the documented way to end up rejecting a perfectly good build
    /// as unparseable. Without this, deleting the merge changes no test.
    #[test]
    fn validation_reads_help_printed_to_stderr() {
        let root = tmp("val-stderr");
        fake_engine_talking_to_stderr(&root, GOOD_VERSION, &full_help());

        assert_eq!(validate_engine(&root).unwrap(), "5ef4a75");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A downloaded binary that never answers must not wedge the app: this runs
    /// under `install_release`'s generation lock, with no cancel path.
    #[test]
    fn validation_gives_up_on_an_engine_that_never_answers() {
        let root = tmp("val-hang");
        std::fs::create_dir_all(&root).unwrap();
        let p = root.join(crate::commands::engine_binary_name());
        std::fs::write(&p, "#!/bin/sh\nsleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();

        let started = std::time::Instant::now();
        let err = validate_engine(&root).unwrap_err();
        assert!(err.contains("in time"), "unexpected message: {err}");
        // The point is the bound, not the message: assert we returned on the
        // timeout rather than on the sleep finishing.
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "probe was not bounded");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_runs_the_engine_the_way_generation_will() {
        let root = tmp("val-ldpath");
        std::fs::create_dir_all(&root).unwrap();
        let p = root.join(crate::commands::engine_binary_name());
        // Refuses to answer if `LD_LIBRARY_PATH` has been aimed at its own
        // directory, which is what a probe more permissive than production
        // would do. Comparing against `$d` rather than requiring the variable
        // to be unset keeps this honest when the developer's own environment
        // sets it for unrelated reasons.
        std::fs::write(
            &p,
            "#!/bin/sh\n\
             d=$(dirname \"$0\")\n\
             [ \"$LD_LIBRARY_PATH\" = \"$d\" ] && { echo 'loader was given a leg up'; exit 127; }\n\
             case \"$1\" in\n\
             --version) cat \"$d/version-out.txt\" ;;\n\
             --help) cat \"$d/help-out.txt\" ;;\n\
             esac\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(root.join("version-out.txt"), GOOD_VERSION).unwrap();
        std::fs::write(root.join("help-out.txt"), full_help()).unwrap();

        assert_eq!(validate_engine(&root).unwrap(), "5ef4a75");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The live shape of "won't run": the binary spawns fine, the dynamic
    /// loader then fails to resolve a sibling `.so` and the process dies
    /// printing a linker error instead of a commit. This is the branch that
    /// catches a symlink entry extracted as an 11-byte text file — see
    /// `extract_zip` — and it is a *different* branch from the one above,
    /// where the spawn itself fails.
    #[test]
    fn validation_rejects_a_build_that_runs_but_prints_no_commit() {
        let root = tmp("val-noload");
        fake_engine(
            &root,
            "sd-cli: error while loading shared libraries: libstable-diffusion.so: \
             cannot open shared object file: No such file or directory\n",
            &full_help(),
            127,
        );

        let err = validate_engine(&root).unwrap_err();
        assert!(err.contains("didn't start properly"), "unexpected message: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A probe is judged by what it printed, never by its exit status. Plenty
    /// of argument parsers exit non-zero from `--help`, and refusing a build
    /// over that would reject a perfectly good engine — the same reasoning that
    /// makes `probe` merge stderr into stdout.
    #[test]
    fn validation_judges_the_output_not_the_exit_status() {
        let root = tmp("val-exit");
        fake_engine(&root, GOOD_VERSION, &full_help(), 1);

        assert_eq!(validate_engine(&root).unwrap(), "5ef4a75");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The engine is exec'd seconds after this process wrote it, and Linux
    /// refuses to exec a file that is open for writing anywhere — a fork on
    /// another thread mid-extraction is enough. Holding a writable descriptor
    /// here reproduces that exactly, and it must be waited out rather than
    /// reported as a broken build: this is the one failure mode where a
    /// perfectly good, hash-verified download would otherwise be discarded.
    /// (It is not hypothetical — `engine.rs`'s tests carry a mutex for the same
    /// race, and a parallel `cargo test` hits it several times an hour.)
    #[test]
    fn validation_waits_out_a_busy_executable() {
        let root = tmp("val-busy");
        fake_engine(&root, GOOD_VERSION, &full_help(), 0);

        let held = std::fs::OpenOptions::new()
            .write(true)
            .open(root.join(crate::commands::engine_binary_name()))
            .unwrap();
        // The hold has to sit strictly between "one attempt" and "the whole
        // budget": short enough that the retries outlast it, long enough that
        // the first attempt still lands inside it when this thread is competing
        // with four hundred others for a core. 80 ms against a 160 ms budget.
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            drop(held);
        });

        assert_eq!(validate_engine(&root).unwrap(), "5ef4a75");
        releaser.join().unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_rejects_a_build_missing_a_flag_muchai_uses() {
        let root = tmp("val-flags");
        let doctored = full_help().replace("--tensor-type-rules", "--type-rules");
        fake_engine(&root, GOOD_VERSION, &doctored, 0);

        let err = validate_engine(&root).unwrap_err();
        assert!(err.contains("--tensor-type-rules"), "the message must name the flag: {err}");
        assert!(err.contains("isn't compatible"), "unexpected message: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_reports_at_most_a_few_missing_flags() {
        let root = tmp("val-many");
        // A readable help that simply shares none of our flags. It must clear
        // `engine_flags`' plausibility floor, or this exercises the
        // `Unparseable` branch instead of the "missing" one it is testing.
        let help: String =
            (0..40).map(|i| format!("  --unrelated-{i} <string>    something else\n")).collect();
        fake_engine(&root, GOOD_VERSION, &help, 0);

        let err = validate_engine(&root).unwrap_err();
        assert!(err.contains("isn't compatible"), "unexpected message: {err}");
        // Naming the first few and counting the rest, not listing all 22. The
        // length bound alone would also pass on a message that named every
        // flag but abbreviated each one, so pin a specific flag that must not
        // appear: `--vae-tiling` is last in `REQUIRED_FLAGS`, so it is only
        // ever reached by an implementation that skipped the cap.
        assert!(!err.contains("--vae-tiling"), "the list must be truncated: {err}");
        assert!(err.contains("19 more"), "the message must account for the rest: {err}");
        assert!(err.len() < 300, "the message must stay readable, got {} chars", err.len());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_says_it_could_not_read_the_help_rather_than_naming_every_flag() {
        let root = tmp("val-unreadable");
        // Help we cannot parse at all — reporting all 22 flags as missing would
        // be false and would send the user chasing twenty-two dead ends.
        fake_engine(&root, GOOD_VERSION, "usage: sd [options]\n", 0);

        let err = validate_engine(&root).unwrap_err();
        assert!(err.contains("couldn't read"), "unexpected message: {err}");
        assert!(!err.contains("--backend"), "must not name flags it cannot vouch for: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_renames_staging_into_place() {
        let root = tmp("install-rename");
        let engines = root.join("engines");
        let staging = staging_dir(&engines, "master-797-5ef4a75");
        fake_engine(&staging, GOOD_VERSION, &full_help(), 0);
        std::fs::write(staging.join("sd-server"), b"unused").unwrap();

        assert_eq!(finish_install(&engines, "master-797-5ef4a75").unwrap(), "5ef4a75");

        let dest = install_dir(&engines, "master-797-5ef4a75");
        assert!(dest.join(crate::commands::engine_binary_name()).exists());
        assert!(!dest.join("sd-server").exists(), "sd-server is dead weight and must be dropped");
        assert!(!staging.exists(), "staging must be gone after the rename");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Extraction carries the archive's executable bit across, but an archive
    /// built where that bit does not exist would produce an engine that cannot
    /// be spawned — and validation is the first thing that would try to spawn
    /// it, so the repair has to happen before then, not at first generation.
    #[test]
    fn install_restores_a_missing_executable_bit_before_validating() {
        let root = tmp("install-chmod");
        let engines = root.join("engines");
        let staging = staging_dir(&engines, "master-797-5ef4a75");
        fake_engine(&staging, GOOD_VERSION, &full_help(), 0);
        use std::os::unix::fs::PermissionsExt;
        let bin = staging.join(crate::commands::engine_binary_name());
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o644)).unwrap();

        finish_install(&engines, "master-797-5ef4a75").unwrap();

        let dest = install_dir(&engines, "master-797-5ef4a75");
        let mode = std::fs::metadata(dest.join(crate::commands::engine_binary_name()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the engine must be executable after install");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_replaces_an_existing_directory_for_the_same_tag() {
        let root = tmp("install-replace");
        let engines = root.join("engines");
        let dest = install_dir(&engines, "master-797-5ef4a75");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("stale.txt"), b"old").unwrap();
        let staging = staging_dir(&engines, "master-797-5ef4a75");
        fake_engine(&staging, GOOD_VERSION, &full_help(), 0);

        finish_install(&engines, "master-797-5ef4a75").unwrap();

        assert!(!dest.join("stale.txt").exists(), "a reinstall must not merge with the old tree");
        assert!(dest.join(crate::commands::engine_binary_name()).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_validation_leaves_nothing_installed() {
        let root = tmp("install-badval");
        let engines = root.join("engines");
        let staging = staging_dir(&engines, "master-799-bad");
        fake_engine(&staging, GOOD_VERSION, "usage: sd [--foo]\n", 0);

        assert!(finish_install(&engines, "master-799-bad").is_err());

        assert!(!install_dir(&engines, "master-799-bad").exists(), "nothing may be installed");
        assert!(!staging.exists(), "the failed staging tree must be cleaned up");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The rename is the only destructive step and it happens *after*
    /// validation passes. An implementation that cleared the destination first,
    /// or that renamed and then rolled back, would still satisfy "nothing may
    /// be installed" above — while having destroyed the engine the user was
    /// running in between. That is the whole atomicity claim in the module
    /// header, and only an existing install can witness it.
    #[test]
    fn a_failed_validation_leaves_the_previous_install_untouched() {
        let root = tmp("install-badval-keep");
        let engines = root.join("engines");
        let dest = install_dir(&engines, "master-797-5ef4a75");
        fake_engine(&dest, GOOD_VERSION, &full_help(), 0);
        std::fs::write(dest.join("marker.txt"), b"the engine the user is running").unwrap();

        let staging = staging_dir(&engines, "master-797-5ef4a75");
        fake_engine(&staging, GOOD_VERSION, "usage: sd [--foo]\n", 0);

        assert!(finish_install(&engines, "master-797-5ef4a75").is_err());

        assert_eq!(
            std::fs::read_to_string(dest.join("marker.txt")).unwrap(),
            "the engine the user is running",
            "a failed install must not disturb the engine already installed"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_refuses_when_the_disk_is_too_full() {
        // 45 MB asset needs 225 MB. (The plan said "~1.2 GB"; that was written
        // when `required_space` folded the headroom in, which it no longer
        // does — `fits` owns that.)
        let err = check_space_for(45_000_000, 100_000_000).unwrap_err();
        assert!(err.starts_with(crate::downloader::INSUFFICIENT_SPACE_PREFIX), "unexpected: {err}");
        // Human units, and the right way round — the two figures are quoted
        // with their surrounding words so that swapping them is caught.
        assert!(err.contains("needs about 225 MB"), "must state what is needed: {err}");
        assert!(err.contains("only 100 MB"), "must state what is free: {err}");
        assert!(!err.contains("225000000"), "raw byte counts are not for users: {err}");
    }

    /// Room for the install alone is not room enough: `fits` reserves a
    /// gigabyte behind every download, and this check has to defer to it rather
    /// than compare against the bare requirement.
    #[test]
    fn install_refuses_when_it_would_eat_the_reserve() {
        let need = required_space(45_000_000);
        let free = need + crate::diskspace::HEADROOM_BYTES / 2;

        assert!(free > need, "the install fits on its own; only the reserve is short");
        assert!(check_space_for(45_000_000, free).is_err());
    }

    #[test]
    fn install_proceeds_when_there_is_room() {
        assert!(check_space_for(45_000_000, 50_000_000_000).is_ok());
    }
}
