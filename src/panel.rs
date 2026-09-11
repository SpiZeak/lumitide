use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::spectrum;
use crate::utils::fmt_time;

/// Max rows shown in a menu popup before it starts scrolling.
pub const MENU_MAX_VISIBLE: usize = 12;

/// A centered modal list rendered on top of the player (e.g. add-to menu).
pub struct MenuOverlay<'a> {
    pub title: &'a str,
    pub items: &'a [String],
    pub cursor: usize,
    pub scroll: usize,
}

/// All state the panel needs to render one frame.
pub struct PanelState<'a> {
    pub cover_lines: &'a [Line<'static>],
    pub track_name: &'a str,
    pub artist_name: &'a str,
    pub album_name: &'a str,
    pub track_label: Option<&'a str>,
    /// Actual audio quality of the stream/file (e.g. "FLAC 16-bit 44.1 kHz"),
    /// probed from the decoded audio rather than what was requested.
    pub quality_label: Option<&'a str>,
    pub elapsed: f64,
    pub total: f64,
    pub volume: f32,
    pub paused: bool,
    pub dl_status: &'a str,
    /// Transient status message (e.g. "✓ Added to favorites"); shown instead of dl_status.
    pub flash_msg: Option<&'a str>,
    pub bar_color: Option<(u8, u8, u8)>,
    /// Pre-rendered spectrum or download-progress lines (BAR_HEIGHT rows).
    pub vis_lines: &'a [Line<'static>],
    pub is_local: bool,
    pub show_controls: bool,
    pub show_controls_hint: bool,
    pub queue_status: Option<String>,
    pub menu: Option<MenuOverlay<'a>>,
}

pub fn render(frame: &mut Frame, state: &PanelState) {
    let terminal = frame.area();

    // ── Calculate content-fitted size ─────────────────────────────────────────
    let cover_col_w = (state.cover_lines.first().map(|l| l.width()).unwrap_or(0) + 1) as u16;
    let right_col_w: u16 = 50;

    // rows: 1 top-pad + title + artist + album + [label] + time + empty + vis
    let right_rows = 1 + 3
        + if state.track_label.is_some() || state.quality_label.is_some() { 1 } else { 0 }
        + 1 + 1
        + state.vis_lines.len() as u16;
    let cover_rows = state.cover_lines.len() as u16 + 1; // +1 top-pad

    let dim = Style::new().fg(Color::DarkGray);

    // ── "? controls" hint pinned to terminal bottom-right ────────────────────
    // Hidden while a download is active so the two don't overlap
    if state.show_controls_hint && state.queue_status.is_none() {
        let hint_text = " Press ? for ctrl ";
        let hint_w = hint_text.len() as u16;
        let hint_area = Rect::new(
            terminal.x + terminal.width.saturating_sub(hint_w),
            terminal.y + terminal.height.saturating_sub(1),
            hint_w.min(terminal.width),
            1,
        );
        frame.render_widget(Paragraph::new(Line::styled(hint_text, dim)), hint_area);
    }

    // Queue download status pinned to bottom-right (replaces the controls hint row)
    if let Some(ref qs) = state.queue_status {
        let qs_text = format!(" {} ", qs);
        let qs_w = qs_text.chars().count() as u16;
        let qs_area = Rect::new(
            terminal.x + terminal.width.saturating_sub(qs_w),
            terminal.y + terminal.height.saturating_sub(1),
            qs_w.min(terminal.width),
            1,
        );
        frame.render_widget(Paragraph::new(Line::styled(qs_text, dim)), qs_area);
    }

    // ── Border + controls (only when show_controls) ───────────────────────────
    let inner = if state.show_controls {
        let block_w = cover_col_w + right_col_w + 2; // +2 borders
        let block_h = right_rows.max(cover_rows) + 2; // +2 borders
        let x = terminal.x + terminal.width.saturating_sub(block_w) / 2;
        let y = terminal.y + terminal.height.saturating_sub(block_h) / 2;
        let area = Rect::new(x, y, block_w.min(terminal.width), block_h.min(terminal.height));

        let hint_text = if state.is_local {
            "← prev  Spc pause  → next  ↑↓ vol  q/Esc quit"
        } else {
            "← prev  Spc pause  → next  ↑↓ vol  a add  d download  r radio  q/Esc quit"
        };
        let outer = Block::default()
            .borders(Borders::ALL)
            .border_style(dim)
            .title_top(Line::styled(hint_text, dim).centered());
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        inner
    } else {
        // No border — centre content directly
        let content_w = cover_col_w + right_col_w;
        let content_h = right_rows.max(cover_rows);
        let x = terminal.x + terminal.width.saturating_sub(content_w) / 2;
        let y = terminal.y + terminal.height.saturating_sub(content_h) / 2;
        Rect::new(x, y, content_w.min(terminal.width), content_h.min(terminal.height))
    };

    // Two columns: cover | info+spectrum
    let cols = Layout::horizontal([
        Constraint::Length((state.cover_lines.first().map(|l| l.width()).unwrap_or(0) + 1) as u16),
        Constraint::Min(0),
    ])
    .split(inner);

    // ── Left: album cover ─────────────────────────────────────────────────────
    let mut cover_lines = vec![Line::raw("")];
    cover_lines.extend(state.cover_lines.iter().cloned());
    frame.render_widget(Paragraph::new(Text::from(cover_lines)), cols[0]);

    // ── Right: track info + visualisation ────────────────────────────────────
    let title_style = match state.bar_color {
        Some((r, g, b)) => Style::new().fg(Color::Rgb(r, g, b)).add_modifier(Modifier::BOLD),
        None => Style::new().add_modifier(Modifier::BOLD),
    };
    let dim = Style::new().fg(Color::DarkGray);

    let mut right: Vec<Line<'static>> = Vec::new();

    right.push(Line::raw(""));
    right.push(Line::from(Span::styled(state.track_name.to_string(), title_style)));
    right.push(Line::from(Span::styled(state.artist_name.to_string(), dim)));
    right.push(Line::from(Span::styled(state.album_name.to_string(), dim)));
    let label_spans: Vec<Span<'static>> = match (state.track_label, state.quality_label) {
        (Some(label), Some(quality)) => vec![
            Span::styled(label.to_string(), dim),
            Span::styled("  ·  ".to_string(), dim),
            Span::styled(quality.to_string(), dim),
        ],
        (Some(label), None) => vec![Span::styled(label.to_string(), dim)],
        (None, Some(quality)) => vec![Span::styled(quality.to_string(), dim)],
        (None, None) => vec![],
    };
    if !label_spans.is_empty() {
        right.push(Line::from(label_spans));
    }

    // Time + volume line
    let vol_pct = (state.volume * 100.0) as u32;
    let pause_str = if state.paused { "  ⏸" } else { "" };
    let time_str = fmt_time(state.elapsed);
    let mut time_line: Vec<Span<'static>> = vec![
        Span::styled(time_str, Style::new().add_modifier(Modifier::BOLD)),
    ];
    if state.total > 0.0 {
        time_line.push(Span::styled(
            format!(" / {}", fmt_time(state.total)),
            dim,
        ));
    }

    // A flash message (add-to result) takes priority over the download status
    let status_line = state.flash_msg.filter(|s| !s.is_empty()).or_else(|| {
        let s = state.dl_status;
        if !s.is_empty() && !s.starts_with('⬇') { Some(s) } else { None }
    });
    if let Some(status) = status_line {
        // Show status ("✓ Saved", "✓ Saving...", "✗ Error") in the info line
        let status_style = if status.starts_with('✓') {
            match state.bar_color {
                Some((r, g, b)) => Style::new().fg(Color::Rgb(r, g, b)).add_modifier(Modifier::BOLD),
                None => Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
            }
        } else {
            dim // error states
        };
        time_line.push(Span::styled(
            format!("  vol {}%{}", vol_pct, pause_str),
            Style::new().add_modifier(Modifier::BOLD),
        ));
        time_line.push(Span::styled(
            format!("  {}", status),
            status_style,
        ));
    } else {
        time_line.push(Span::styled(
            format!("  vol {}%{}", vol_pct, pause_str),
            Style::new(),
        ));
    }
    right.push(Line::from(time_line));
    right.push(Line::raw(""));

    // Visualisation lines
    for line in state.vis_lines {
        right.push(line.clone());
    }

    frame.render_widget(Paragraph::new(Text::from(right)), cols[1]);

    // ── Modal menu overlay (add to favorites / playlist) ─────────────────────
    if let Some(menu) = &state.menu {
        let item_w = menu.items.iter().map(|i| i.chars().count()).max().unwrap_or(0)
            .max(menu.title.chars().count());
        let w = (item_w as u16 + 6).min(terminal.width); // "> " + borders + padding
        let h = (MENU_MAX_VISIBLE.min(menu.items.len()) as u16 + 2).min(terminal.height);
        let x = terminal.x + terminal.width.saturating_sub(w) / 2;
        let y = terminal.y + terminal.height.saturating_sub(h) / 2;
        let area = Rect::new(x, y, w, h);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(dim)
            .title_top(Line::styled(format!(" {} ", menu.title), dim).centered());
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        let items: Vec<Line> = menu.items.iter().enumerate().skip(menu.scroll)
            .take(inner.height as usize)
            .map(|(i, item)| {
                if i == menu.cursor {
                    Line::styled(format!("> {}", item), Style::new().add_modifier(Modifier::BOLD))
                } else {
                    Line::styled(format!("  {}", item), dim)
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(items), inner);
    }
}

/// Build pre-rendered visualisation lines for the current frame.
/// Returns either spectrum or download-progress lines.
pub fn build_vis_lines(
    spec_buf: &[f32],
    band_edges: &[usize],
    bar_peaks: &mut Vec<f32>,
    bar_peak_hold: &mut Vec<u32>,
    bar_color: Option<(u8, u8, u8)>,
    dl_status: &str,
    dl_bytes: u64,
    dl_total: u64,
    calm: bool,
) -> Vec<Line<'static>> {
    if dl_status.starts_with('⬇') {
        spectrum::render_dl_progress(dl_bytes, dl_total, bar_color)
    } else {
        let normalized: Vec<f32> = if calm {
            spectrum::CALM_SPECTRUM.to_vec()
        } else {
            spectrum::compute_spectrum(spec_buf, band_edges)
        };
        spectrum::render_spectrum(&normalized, bar_peaks, bar_peak_hold, bar_color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn rendered_text(state: &PanelState) -> String {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, state)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .chunks(100)
            .map(|row| row.concat())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn base_state<'a>() -> PanelState<'a> {
        PanelState {
            cover_lines: &[],
            track_name: "Escape",
            artist_name: "Netsky",
            album_name: "2",
            track_label: None,
            quality_label: None,
            elapsed: 42.0,
            total: 240.0,
            volume: 0.5,
            paused: false,
            dl_status: "",
            flash_msg: None,
            bar_color: None,
            vis_lines: &[],
            is_local: false,
            show_controls: false,
            show_controls_hint: false,
            queue_status: None,
            menu: None,
        }
    }

    #[test]
    fn quality_label_renders_on_its_own_line() {
        let mut state = base_state();
        state.quality_label = Some("FLAC 16-bit 44.1 kHz");
        let text = rendered_text(&state);
        assert!(text.contains("FLAC 16-bit 44.1 kHz"), "quality missing:\n{text}");
    }

    #[test]
    fn quality_label_joins_track_label_on_one_line() {
        let mut state = base_state();
        state.track_label = Some("Mix 3/12");
        state.quality_label = Some("AAC 44.1 kHz");
        let text = rendered_text(&state);
        let line = text.lines().find(|l| l.contains("Mix 3/12")).expect("label line");
        assert!(line.contains("AAC 44.1 kHz"), "quality not on label line:\n{line}");
    }

    #[test]
    fn no_quality_label_renders_no_extra_line() {
        let state = base_state();
        let without = rendered_text(&state);
        assert!(!without.contains("kHz"));
    }
}
