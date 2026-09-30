use anyhow::Result;
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

use crate::api::{PlaylistInfo, TidalClient};
use crate::config;
use crate::preview;
use crate::radio;
use crate::utils::is_saved;

enum Action { Play(usize), Queue(usize), Create, Back }

pub fn run(client: &mut TidalClient, debug: bool) -> Result<()> {
    let mut playlists = client.playlists()?;

    if playlists.is_empty() {
        use std::io::Write;
        print!("\x1B[2J\x1B[H");
        let _ = std::io::stdout().flush();
        println!("You don't have any playlists yet.\n");
        let choice = dialoguer::Select::new()
            .items(&["Create a playlist", "Back"])
            .default(0)
            .report(false)
            .interact_opt()?;
        if let Some(0) = choice {
            if let Some(pl) = prompt_create(client)? {
                playlists.push(pl);
            }
        }
        if playlists.is_empty() {
            return Ok(());
        }
    }

    let show_hint = config::load().show_controls_hint;
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

                let items: Vec<ListItem> = playlists
                    .iter()
                    .enumerate()
                    .skip(scroll)
                    .take(visible)
                    .map(|(i, pl)| {
                        if i == cursor {
                            ListItem::new(Line::from(format!("> {}", pl.title)))
                                .style(Style::default().add_modifier(Modifier::BOLD))
                        } else {
                            ListItem::new(format!("  {}", pl.title))
                        }
                    })
                    .collect();
                f.render_widget(List::new(items), area);

                if let Some(txt) = crate::DOWNLOAD_QUEUE
                    .get()
                    .and_then(|q| q.status())
                    .or_else(|| show_hint.then(|| " ↵ play  d · download  c · new ".to_string()))
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
                        KeyCode::Down | KeyCode::Char('j') => { if cursor + 1 < playlists.len() { cursor += 1; } }
                        KeyCode::Enter => break Action::Play(cursor),
                        KeyCode::Char('d') | KeyCode::Char('D') => break Action::Queue(cursor),
                        KeyCode::Char('c') | KeyCode::Char('C') => break Action::Create,
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
            Action::Create => {
                if let Some(pl) = prompt_create(client)? {
                    playlists.push(pl);
                    cursor = playlists.len() - 1;
                }
            }
            Action::Queue(idx) => {
                let tracks = client.playlist_tracks(&playlists[idx].id)?;
                if let Some(queue) = crate::DOWNLOAD_QUEUE.get() {
                    let cfg = config::load();
                    queue.push_tracks(tracks, client.session.clone(), cfg.output_path());
                }
            }
            Action::Play(idx) => {
                let tracks = client.playlist_tracks(&playlists[idx].id)?;
                if tracks.is_empty() { continue; }

                let cfg = config::load();
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

/// Prompt for a name (caller must have torn the TUI down) and create the
/// playlist on Tidal. Returns None when cancelled or on API failure.
fn prompt_create(client: &mut TidalClient) -> Result<Option<PlaylistInfo>> {
    let name: String = dialoguer::Input::new()
        .with_prompt("New playlist name")
        .allow_empty(true)
        .interact_text()?;
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    match client.create_playlist(name) {
        Ok(pl) => Ok(Some(pl)),
        Err(e) => {
            println!("\n✗ Could not create playlist: {e}");
            dialoguer::Select::new()
                .items(&["Back"])
                .default(0)
                .report(false)
                .interact_opt()?;
            Ok(None)
        }
    }
}
