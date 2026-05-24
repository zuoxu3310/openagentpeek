// Model -> context-window size, plus compact token formatting.
// Ported from `../mvp/src/contextWindow.js`.
//
// Caveat that drove this table's shape: the `model` string in the transcript
// (e.g. "claude-opus-4-7") does NOT encode which window variant is active — the
// 1M-context tier and the 200k default share the same model name. So we can't read
// the real limit; we pick the family's common ceiling instead. Best-effort and WILL
// drift as models change; update it when a new family ships.

use serde::Serialize;

/// What the row shows for context usage.
///   `{ text: "162k / 1M", ratio: Some(0.162) }`  when the window is known
///   `{ text: "162k",      ratio: None }`          when it isn't
#[derive(Serialize, Clone)]
pub struct Usage {
    pub text: String,
    pub ratio: Option<f64>,
}

/// The 1M tier for 4.x Opus/Sonnet, 200k for Haiku 4.x and the whole 3.x family.
/// Unrecognized -> None, so we show the bare token count with no "/limit".
fn context_limit(model: Option<&str>) -> Option<u64> {
    let m = model?;
    if m.contains("opus-4") || m.contains("sonnet-4") {
        Some(1_000_000)
    } else if m.contains("haiku-4") {
        Some(200_000)
    } else if m.contains("claude-3") {
        Some(200_000)
    } else {
        None
    }
}

/// Compact human token count: 840 -> "840", 162046 -> "162k", 1000000 -> "1M".
pub fn format_tokens(n: u64) -> String {
    if n < 1000 {
        return n.to_string();
    }
    // Cut over to "M" at 999.5k so 999999 rounds to "1M", not the ugly "1000k".
    if n < 999_500 {
        return format!("{}k", ((n as f64) / 1000.0).round() as u64);
    }
    let m = n as f64 / 1_000_000.0;
    if m.fract() == 0.0 {
        format!("{}M", m as u64)
    } else {
        // One decimal, matching JS `Number(m.toFixed(1))` (no trailing-zero padding).
        format!("{}M", (m * 10.0).round() / 10.0)
    }
}

/// `window_override` lets a provider that reports its own window size (Codex ships
/// `model_context_window` in every token_count) skip the model-name guess entirely.
pub fn format_usage(tokens: u64, model: Option<&str>, window_override: Option<u64>) -> Usage {
    let limit = match window_override {
        Some(w) if w > 0 => Some(w),
        _ => context_limit(model),
    };
    match limit {
        None => Usage {
            text: format_tokens(tokens),
            ratio: None,
        },
        Some(limit) => Usage {
            text: format!("{} / {}", format_tokens(tokens), format_tokens(limit)),
            ratio: Some((tokens as f64 / limit as f64).clamp(0.0, 1.0)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_tokens_small_prints_raw() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(840), "840");
    }

    #[test]
    fn format_tokens_thousands_round_to_k() {
        assert_eq!(format_tokens(88000), "88k");
        assert_eq!(format_tokens(162046), "162k");
        assert_eq!(format_tokens(200000), "200k");
        assert_eq!(format_tokens(410000), "410k");
    }

    #[test]
    fn format_tokens_millions_trim_trailing_zeros() {
        assert_eq!(format_tokens(1_000_000), "1M");
        assert_eq!(format_tokens(1_200_000), "1.2M");
    }

    #[test]
    fn format_tokens_just_under_1m_rounds_to_1m() {
        assert_eq!(format_tokens(999_999), "1M");
        assert_eq!(format_tokens(999_499), "999k");
    }

    #[test]
    fn context_limit_4x_families_assume_1m() {
        assert_eq!(context_limit(Some("claude-opus-4-7")), Some(1_000_000));
        assert_eq!(context_limit(Some("claude-sonnet-4-5")), Some(1_000_000));
    }

    #[test]
    fn context_limit_haiku4_and_3x_are_200k() {
        assert_eq!(context_limit(Some("claude-haiku-4-5")), Some(200_000));
        assert_eq!(context_limit(Some("claude-3-5-sonnet-20241022")), Some(200_000));
    }

    #[test]
    fn context_limit_unknown_or_missing_is_none() {
        assert_eq!(context_limit(Some("some-future-model")), None);
        assert_eq!(context_limit(None), None);
    }

    #[test]
    fn format_usage_known_model_shows_ratio() {
        let u = format_usage(162046, Some("claude-opus-4-7"), None);
        assert_eq!(u.text, "162k / 1M");
        assert!((u.ratio.unwrap() - 0.162046).abs() < 1e-6);
    }

    #[test]
    fn format_usage_unknown_model_absolute_only() {
        let u = format_usage(5000, Some("mystery-model"), None);
        assert_eq!(u.text, "5k");
        assert_eq!(u.ratio, None);
    }

    #[test]
    fn format_usage_ratio_clamped_past_limit() {
        let u = format_usage(250000, Some("claude-haiku-4-5"), None); // 250k > 200k
        assert_eq!(u.ratio, Some(1.0));
    }

    #[test]
    fn format_usage_zero_tokens_clean() {
        let u = format_usage(0, Some("claude-opus-4-7"), None);
        assert_eq!(u.text, "0 / 1M");
        assert_eq!(u.ratio, Some(0.0));
    }

    #[test]
    fn format_usage_explicit_override_beats_table() {
        let u = format_usage(116022, Some("gpt-5.5"), Some(258400));
        assert_eq!(u.text, "116k / 258k");
        assert!((u.ratio.unwrap() - 116022.0 / 258400.0).abs() < 1e-6);
    }

    #[test]
    fn format_usage_nonpositive_override_falls_back() {
        assert_eq!(format_usage(5000, Some("claude-opus-4-7"), None).text, "5k / 1M");
        assert_eq!(format_usage(5000, Some("claude-opus-4-7"), Some(0)).text, "5k / 1M");
    }
}
