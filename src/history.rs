use anyhow::{anyhow, Result};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{List, ListItem, Paragraph},
    Terminal,
};

use crate::api::{RecentItem, RecentKind, TidalClient, TrackInfo};
use crate::config;
use crate::preview;
use crate::radio;
use crate::utils::is_saved;

enum Action { Play(usize), Queue(usize), Back }

/// Fetch the track list behind a recently-played entry.
fn tracks_for(client: &mut TidalClient, item: &RecentItem) -> Result<Vec<TrackInfo>> {
    match item.kind {
        RecentKind::Mix => client.mix_tracks(&item.id),
        RecentKind::Album => {
            let id: u64 = item.id.parse().map_err(|_| anyhow!("invalid album id {}", item.id))?;
            client.album_tracks(id)
        }
        RecentKind::Playlist => client.playlist_tracks(&item.id),
    }
}

fn label_for(item: &RecentItem) -> String {
    match &item.subtitle {
        Some(s) if !s.is_empty() => format!("{} · {} — {}", item.kind.label(), item.title, s),
        _ => format!("{} · {}", item.kind.label(), item.title),
    }
}

pub fn run(client: &mut TidalClient, debug: bool) -> Result<()> {
    let items = client.recently_played()?;

    if items.is_empty() {
        use std::io::Write;
        print!("\x1B[2J\x1B[H");
        let _ = std::io::stdout().flush();
        println!("Nothing in your Tidal recently played yet.\n");
        println!("Albums, mixes, and playlists you play will appear here.\n");
        dialoguer::Select::new()
            .items(&["Back"])
            .default(0)
            .report(false)
            .interact_opt()?;
        return Ok(());
    }

    let labels: Vec<String> = items.iter().map(label_for).collect();

    let cfg = config::load();
    let show_hint = cfg.show_controls_hint;
    let mut cursor: usize = 0;
    let mut scroll: usize = 0;

    loop {
        enable_raw_mode()?;
        let mut terminal = {
            let mut stdout = io::stdout();
            execute!(stdout, Clear(ClearType::All), EnterAlternateScreen)?;
            Terminal::new(CrosstermBackend::new(stdout))?
        };
        while event::poll(Duration::ZERO)? {
            let _ = event::read();
        }

        let action = loop {
            terminal.draw(|f| {
                let area = f.area();
                let visible = area.height as usize;

                if cursor < scroll { scroll = cursor; }
                if visible > 0 && cursor >= scroll + visible { scroll = cursor + 1 - visible; }

                let list_items: Vec<ListItem> = labels
                    .iter()
                    .enumerate()
                    .skip(scroll)
                    .take(visible)
                    .map(|(i, label)| {
                        if i == cursor {
                            ListItem::new(Line::from(format!("> {}", label)))
                                .style(Style::default().add_modifier(Modifier::BOLD))
                        } else {
                            ListItem::new(format!("  {}", label))
                        }
                    })
                    .collect();
                f.render_widget(List::new(list_items), area);

                if let Some(txt) = crate::DOWNLOAD_QUEUE
                    .get()
                    .and_then(|q| q.status())
                    .or_else(|| show_hint.then(|| " ↵ play  d · download ".to_string()))
                {
                    let w = txt.chars().count() as u16;
                    let qs_area = Rect::new(
                        area.width.saturating_sub(w),
                        area.height.saturating_sub(1),
                        w.min(area.width),
                        1,
                    );
                    f.render_widget(
                        Paragraph::new(txt).style(Style::default().add_modifier(Modifier::DIM)),
                        qs_area,
                    );
                }
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press { continue; }
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => { if cursor > 0 { cursor -= 1; } }
                        KeyCode::Down | KeyCode::Char('j') => { if cursor + 1 < items.len() { cursor += 1; } }
                        KeyCode::Enter => break Action::Play(cursor),
                        KeyCode::Char('d') | KeyCode::Char('D') => break Action::Queue(cursor),
                        KeyCode::Esc | KeyCode::Char('q') => break Action::Back,
                        _ => {}
                    }
                }
            }
        };

        let _ = disable_raw_mode();
        let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
        drop(terminal);

        match action {
            Action::Back => return Ok(()),
            Action::Queue(idx) => {
                let tracks = tracks_for(client, &items[idx])?;
                if let Some(queue) = crate::DOWNLOAD_QUEUE.get() {
                    queue.push_tracks(tracks, client.session.clone(), cfg.output_path());
                }
            }
            Action::Play(idx) => {
                let tracks = tracks_for(client, &items[idx])?;
                if tracks.is_empty() { continue; }

                let volume = Arc::new(Mutex::new(cfg.volume));
                let mut play_idx: usize = 0;
                let mut direction: Option<&str> = None;

                loop {
                    let track = &tracks[play_idx];
                    let saved = is_saved(&cfg.output_path(), &track.artist_name, &track.title);
                    let label = format!("{} / {}", play_idx + 1, tracks.len());

                    let result = preview::run(client, track.id, debug, Some(label), Some(volume.clone()), saved, direction)?;

                    match result.as_str() {
                        "prev" => { play_idx = (play_idx + tracks.len() - 1) % tracks.len(); direction = Some("prev"); }
                        "quit" => break,
                        r if r.starts_with("radio:") => {
                            if let Ok(id) = r["radio:".len()..].parse::<u64>() {
                                radio::run(client, id, debug)?;
                            }
                            break;
                        }
                        _ => { play_idx = (play_idx + 1) % tracks.len(); direction = Some("next"); }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_includes_kind_title_and_subtitle() {
        let item = RecentItem {
            kind: RecentKind::Mix,
            id: "abc".into(),
            title: "Drunk Tonight".into(),
            subtitle: Some("DAWNLINE".into()),
        };
        assert_eq!(label_for(&item), "Mix · Drunk Tonight — DAWNLINE");
    }

    #[test]
    fn label_omits_missing_subtitle() {
        let item = RecentItem {
            kind: RecentKind::Playlist,
            id: "uuid-1".into(),
            title: "Lily - Favoriter".into(),
            subtitle: None,
        };
        assert_eq!(label_for(&item), "Playlist · Lily - Favoriter");
    }

    #[test]
    fn label_omits_empty_subtitle() {
        let item = RecentItem {
            kind: RecentKind::Album,
            id: "42".into(),
            title: "Drakens dans".into(),
            subtitle: Some(String::new()),
        };
        assert_eq!(label_for(&item), "Album · Drakens dans");
    }
}
