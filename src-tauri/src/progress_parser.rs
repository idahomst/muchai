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
}
