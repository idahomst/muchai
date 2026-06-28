use crate::types::ProgressUpdate;

/// Parse one line of engine output into a progress update, if present.
/// stable-diffusion.cpp prints a sampling bar like:
///   "  |==========>            | 5/30 - 2.34it/s"
/// We only treat a line as progress if it contains a '|' bar, then read the
/// LAST "<digits>/<digits>" pair on the line. Returns None otherwise.
pub fn parse_progress_line(line: &str) -> Option<ProgressUpdate> {
    if !line.contains('|') {
        return None;
    }
    let bytes = line.as_bytes();
    let mut best: Option<(u32, u32)> = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'/' {
                let slash = i;
                i += 1;
                let tstart = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i > tstart {
                    if let (Ok(c), Ok(t)) =
                        (line[start..slash].parse::<u32>(), line[tstart..i].parse::<u32>())
                    {
                        if t > 0 {
                            best = Some((c, t));
                        }
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    best.map(|(current_step, total_steps)| ProgressUpdate { current_step, total_steps })
}

/// One parsed "generating image: i/N - seed S" line from the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSeed {
    /// 1-based image index within the batch, as the engine reports it.
    pub index: u32,
    pub seed: i64,
}

/// Parse stable-diffusion.cpp's per-image seed announcement, e.g.
///   "[INFO ] stable-diffusion.cpp:4561 - generating image: 2/3 - seed 43"
/// This is how we learn the *actual* seed of each batch image (each image uses
/// base_seed + offset, or a random seed when base is -1), so the user can pick
/// one and reproduce it. Returns None for any other line.
pub fn parse_image_seed_line(line: &str) -> Option<ImageSeed> {
    let after = line.split("generating image:").nth(1)?;
    // `after` looks like: " 2/3 - seed 43"
    let (idx_part, _) = after.split_once('/')?;
    let index: u32 = idx_part.trim().parse().ok()?;
    let seed_part = after.split("seed").nth(1)?.trim();
    let tok: String = seed_part
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    let seed: i64 = tok.parse().ok()?;
    Some(ImageSeed { index, seed })
}

/// Parse stable-diffusion.cpp's resolved base-seed echo from its parameter dump,
/// whose trimmed form is `seed: 1648302913,`. The engine prints this for *every*
/// run (single or batch) with the random seed already resolved to a concrete
/// value, so it lets us recover the actual seed even when the per-image
/// "generating image: i/N - seed S" lines are absent. Returns None otherwise.
pub fn parse_resolved_seed_line(line: &str) -> Option<i64> {
    let rest = line.trim().strip_prefix("seed:")?;
    let tok: String = rest
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    tok.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_progress_bar_line() {
        let line = "  |==========>            | 5/30 - 2.34it/s";
        assert_eq!(
            parse_progress_line(line),
            Some(ProgressUpdate { current_step: 5, total_steps: 30 })
        );
    }

    #[test]
    fn ignores_non_bar_lines() {
        assert_eq!(parse_progress_line("[INFO] loading model 1/1"), None);
        assert_eq!(parse_progress_line("done"), None);
    }

    #[test]
    fn takes_last_pair_on_the_line() {
        // a bar that also mentions another ratio earlier
        let line = "batch 1/2 |######| 30/30 - 1.0it/s";
        assert_eq!(
            parse_progress_line(line),
            Some(ProgressUpdate { current_step: 30, total_steps: 30 })
        );
    }

    #[test]
    fn bar_line_without_any_ratio_returns_none() {
        // SPEC implies: a '|' bar with no "<digits>/<digits>" pair is not progress.
        assert_eq!(parse_progress_line("  |===========>            |"), None);
    }

    #[test]
    fn parses_image_seed_line() {
        let line = "[INFO ] stable-diffusion.cpp:4561 - generating image: 2/3 - seed 43";
        assert_eq!(parse_image_seed_line(line), Some(ImageSeed { index: 2, seed: 43 }));
    }

    #[test]
    fn image_seed_ignores_other_lines() {
        assert_eq!(parse_image_seed_line("  |####| 5/30 - 2.3it/s"), None);
        assert_eq!(parse_image_seed_line("[INFO] save result image 0"), None);
    }

    #[test]
    fn parses_resolved_base_seed_line() {
        assert_eq!(parse_resolved_seed_line("  seed: 1648302913,"), Some(1648302913));
        assert_eq!(parse_resolved_seed_line("seed: 42"), Some(42));
    }

    #[test]
    fn resolved_seed_ignores_other_lines() {
        // the per-image line uses "seed " (no colon) and must not match here
        assert_eq!(parse_resolved_seed_line("generating image: 1/1 - seed 43"), None);
        assert_eq!(parse_resolved_seed_line("  |####| 5/30"), None);
        assert_eq!(parse_resolved_seed_line("output_path: \"/tmp/x.png\","), None);
    }
}
