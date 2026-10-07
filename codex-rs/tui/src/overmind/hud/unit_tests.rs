use std::time::Duration;

use pretty_assertions::assert_eq;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::overmind::hud::activity::HudEvent;
use crate::overmind::hud::activity::Phase;
use crate::overmind::hud::activity::ToolCounts;
use crate::overmind::hud::activity::TurnActivity;
use crate::overmind::hud::activity::display_command;
use crate::overmind::hud::activity::format_elapsed;
use crate::overmind::hud::config::HudConfig;
use crate::overmind::hud::config::ModelPrice;
use crate::overmind::hud::config::load_hud_config;
use crate::overmind::hud::config::parse_hud_config;
use crate::overmind::hud::meter::Glyphs;
use crate::overmind::hud::meter::Palette;
use crate::overmind::hud::meter::Tone;
use crate::overmind::hud::meter::bar;
use crate::overmind::hud::meter::progress;
use crate::overmind::hud::meter::truncate;
use crate::overmind::hud::meter::usage_tone;
use crate::overmind::hud::segments::Align;
use crate::overmind::hud::segments::Segment;
use crate::overmind::hud::segments::fit_line;
use crate::overmind::hud::summary::format_usd;

fn text(spans: &[Span<'_>]) -> String {
    spans.iter().map(|span| span.content.as_ref()).collect()
}

fn line_text(line: &Line<'_>) -> String {
    text(&line.spans)
}

fn unicode_bar(ratio: f64, cells: u16) -> String {
    text(&bar(
        ratio,
        cells,
        Tone::Calm,
        Glyphs::Unicode,
        Palette::MONO,
    ))
}

#[test]
fn bar_uses_half_cell_resolution() {
    assert_eq!(unicode_bar(0.0, 4), "────");
    assert_eq!(unicode_bar(0.5, 4), "━━──");
    assert_eq!(unicode_bar(0.375, 4), "━╸──");
    assert_eq!(unicode_bar(1.0, 4), "━━━━");
}

#[test]
fn bar_never_looks_empty_or_full_when_it_is_not() {
    assert_eq!(unicode_bar(0.001, 4), "╸───");
    assert_eq!(unicode_bar(0.999, 4), "━━━╸");
    assert_eq!(unicode_bar(f64::NAN, 3), "───");
    assert_eq!(unicode_bar(7.0, 3), "━━━");
    assert_eq!(unicode_bar(-1.0, 3), "───");
}

#[test]
fn ascii_bar_is_bracketed() {
    let spans = bar(0.625, 4, Tone::Calm, Glyphs::Ascii, Palette::MONO);
    assert_eq!(text(&spans), "[##=-]");
    assert!(text(&spans).is_ascii());
}

#[test]
fn mono_palette_has_no_colors_but_keeps_warning_emphasis() {
    for tone in [
        Tone::Calm,
        Tone::Good,
        Tone::Accent,
        Tone::Warn,
        Tone::Critical,
    ] {
        let style = Palette::MONO.style(tone);
        assert_eq!(style.fg, None, "{tone:?}");
    }
    assert_eq!(Palette::MONO.style(Tone::Calm), Style::default());
    assert_ne!(Palette::MONO.style(Tone::Critical), Style::default());
    assert!(Palette::COLOR.style(Tone::Critical).fg.is_some());
}

#[test]
fn usage_tone_thresholds() {
    assert_eq!(usage_tone(0.0), Tone::Calm);
    assert_eq!(usage_tone(0.59), Tone::Calm);
    assert_eq!(usage_tone(0.60), Tone::Warn);
    assert_eq!(usage_tone(0.849), Tone::Warn);
    assert_eq!(usage_tone(0.85), Tone::Critical);
    assert_eq!(usage_tone(f64::NAN), Tone::Calm);
}

#[test]
fn progress_marks_completion() {
    let spans = progress("plan", 2, 2, Some(4), Glyphs::Unicode, Palette::COLOR);
    assert_eq!(text(&spans), "plan ━━━━ 2/2");
    assert_eq!(
        spans.last().and_then(|span| span.style.fg),
        Palette::COLOR.style(Tone::Good).fg
    );
    assert_eq!(
        text(&progress("", 0, 0, None, Glyphs::Ascii, Palette::MONO)),
        "0/0"
    );
}

#[test]
fn truncate_respects_display_width() {
    assert_eq!(truncate("short", 10, Glyphs::Unicode), "short");
    assert_eq!(
        truncate("cargo test --workspace", 10, Glyphs::Unicode),
        "cargo tes…"
    );
    assert_eq!(
        truncate("cargo test --workspace", 10, Glyphs::Ascii),
        "cargo t..."
    );
    assert_eq!(truncate("日本語のテキスト", 7, Glyphs::Unicode), "日本語…");
}

fn seg(priority: u8, align: Align, variants: &[&str]) -> Segment {
    Segment::new(
        priority,
        align,
        variants
            .iter()
            .map(|variant| vec![Span::raw((*variant).to_string())])
            .collect(),
    )
}

fn fit(segments: &[Segment], width: u16) -> Option<String> {
    fit_line(segments, width, "", " · ", Style::default()).map(|line| line_text(&line))
}

#[test]
fn fit_line_degrades_lowest_priority_first() {
    let segments = [
        seg(90, Align::Left, &["important-full", "imp"]),
        seg(10, Align::Left, &["minor-full", "min"]),
    ];
    assert_eq!(
        fit(&segments, 40).as_deref(),
        Some("important-full · minor-full")
    );
    assert_eq!(fit(&segments, 20).as_deref(), Some("important-full · min"));
    // Spare columns flow back to lower-priority segments once higher ones cannot grow.
    assert_eq!(fit(&segments, 16).as_deref(), Some("imp · minor-full"));
    assert_eq!(fit(&segments, 5).as_deref(), Some("imp"));
    assert_eq!(fit(&segments, 2).as_deref(), Some("imp"));
    assert_eq!(fit(&[], 20), None);
    assert_eq!(fit(&segments, 0), None);
}

#[test]
fn fit_line_right_aligns_the_right_group() {
    let segments = [
        seg(90, Align::Left, &["left"]),
        seg(50, Align::Right, &["right"]),
    ];
    assert_eq!(fit(&segments, 20).as_deref(), Some("left           right"));
    assert_eq!(fit(&segments, 11).as_deref(), Some("left  right"));
    assert_eq!(fit(&segments, 10).as_deref(), Some("left"));
    let right_only = [seg(50, Align::Right, &["r"])];
    assert_eq!(fit(&right_only, 5).as_deref(), Some("    r"));
}

#[test]
fn turn_activity_tracks_phase_and_counts() {
    let t0 = std::time::Instant::now();
    let mut turn = TurnActivity::new(t0, Default::default());
    assert_eq!(turn.phase, Phase::Starting);
    turn.apply(HudEvent::Thinking);
    assert_eq!(turn.phase, Phase::Thinking);
    turn.apply(HudEvent::CommandStarted("bash -lc \"just fmt\""));
    assert_eq!(turn.phase, Phase::Running("just fmt".to_string()));
    turn.apply(HudEvent::ToolFinished);
    turn.apply(HudEvent::PatchStarted { files: 3 });
    assert_eq!(turn.phase.detail().as_deref(), Some("3 files"));
    turn.apply(HudEvent::McpStarted {
        server: "github",
        tool: "search",
    });
    assert_eq!(turn.phase, Phase::Calling("github.search".to_string()));
    turn.apply(HudEvent::DynamicToolStarted("browser"));
    turn.apply(HudEvent::ImageGenerationStarted);
    turn.apply(HudEvent::WebSearchStarted);
    turn.apply(HudEvent::ApprovalRequested);
    assert!(turn.phase.needs_attention());
    assert_eq!(turn.phase.short_verb(), "approve?");
    turn.apply(HudEvent::Responding);
    assert_eq!(turn.phase, Phase::Responding);
    assert_eq!(
        turn.counts,
        ToolCounts {
            commands: 1,
            edits: 1,
            mcp: 1,
            web: 1,
            other: 2,
        }
    );
    assert_eq!(turn.counts.total(), 6);
    assert_eq!(
        turn.elapsed(t0 + Duration::from_secs(5)),
        Duration::from_secs(5)
    );
}

#[test]
fn tool_counts_breakdown_skips_zeroes() {
    let counts = ToolCounts {
        commands: 2,
        edits: 0,
        mcp: 1,
        web: 0,
        other: 0,
    };
    assert_eq!(counts.breakdown(), vec![(2, "sh"), (1, "mcp")]);
}

#[test]
fn display_command_strips_shell_wrappers() {
    assert_eq!(
        display_command("bash -lc 'cargo test  -p x'"),
        "cargo test -p x"
    );
    assert_eq!(display_command("/bin/zsh -lc \"ls\""), "ls");
    assert_eq!(display_command("rg -n foo\nbar"), "rg -n foo bar");
}

#[test]
fn format_elapsed_is_compact() {
    assert_eq!(format_elapsed(Duration::from_millis(300)), "<1s");
    assert_eq!(format_elapsed(Duration::from_secs(42)), "42s");
    assert_eq!(format_elapsed(Duration::from_secs(187)), "3m 07s");
    assert_eq!(format_elapsed(Duration::from_secs(3_725)), "1h 02m");
}

#[test]
fn format_usd_scales_precision() {
    assert_eq!(format_usd(1.234), "~$1.23");
    assert_eq!(format_usd(0.0412), "~$0.041");
    assert_eq!(format_usd(0.00421), "~$0.0042");
    assert_eq!(format_usd(-1.0), "~$0.0000");
}

#[test]
fn config_defaults_and_toggles() {
    assert_eq!(parse_hud_config(""), Ok(HudConfig::default()));
    assert_eq!(
        parse_hud_config("[pipelines]\nx = 1\n"),
        Ok(HudConfig::default())
    );
    let parsed = parse_hud_config(
        "[tui]\nrate_limits = false\nascii = true\n[tui.prices.\"composer-2.5\"]\ninput = 0.5\noutput = 2.5\n",
    )
    .expect("valid");
    assert!(!parsed.rate_limits);
    assert!(parsed.ascii);
    assert!(parsed.context_gauge);
    assert_eq!(
        parsed.price_for("composer-2.5"),
        Some(&ModelPrice {
            input: 0.5,
            cached_input: None,
            output: 2.5,
        })
    );
    assert!(!HudConfig::off().row_enabled());
    assert!(HudConfig::default().row_enabled());
}

#[test]
fn config_rejects_unknown_tui_keys() {
    let err = parse_hud_config("[tui]\ncontext_guage = false\n").expect_err("typo");
    assert!(err.contains("context_guage"), "{err}");
}

#[test]
fn load_config_handles_missing_and_invalid_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(load_hud_config(dir.path()), (HudConfig::default(), None));
    std::fs::write(dir.path().join("overmind.toml"), "[tui]\nhud = false\n").expect("write");
    assert!(!load_hud_config(dir.path()).0.hud);
    std::fs::write(dir.path().join("overmind.toml"), "[tui\n").expect("write");
    let (config, warning) = load_hud_config(dir.path());
    assert_eq!(config, HudConfig::default());
    assert!(warning.is_some_and(|w| w.contains("overmind.toml")));
}

#[test]
fn model_price_cost() {
    let price = ModelPrice {
        input: 2.0,
        cached_input: None,
        output: 8.0,
    };
    assert!((price.cost_usd(1_000_000, 250_000, 500_000) - 6.0).abs() < 1e-9);
}
