use anyhow::Result;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    widgets::Paragraph,
    Terminal,
};

use crate::api::TidalClient;
use crate::config;
use crate::preview;
use crate::utils::is_saved;

pub fn run(client: &mut TidalClient, seed_track_id: u64, debug: bool) -> Result<()> {
    // A fetch failure must not unwind through the caller's UI loop and kill
    // the app — Tidal 404s tracks without a radio, and timeouts/offline hit
    // the same path. Show a dismissable notice and unwind no further than
    // this call.
    let tracks = match client.track_radio(seed_track_id) {
        Ok(tracks) => tracks,
        Err(e) => {
            let msg = if e.to_string().contains("API error 404") {
                "This track has no radio."
            } else {
                "Couldn't load radio.\n\nCheck your connection and try again."
            };
            notice(&format!("{msg}\n\nPress any key to go back."))?;
            return Ok(());
        }
    };

    if tracks.is_empty() {
        notice("This track has no radio.\n\nPress any key to go back.")?;
        return Ok(());
    }

    let cfg = config::load();
    let volume = Arc::new(Mutex::new(cfg.volume));
    let mut idx: usize = 0;
    let mut direction: Option<&str> = None;

    loop {
        let track = &tracks[idx];
        let saved = is_saved(&cfg.output_path(), &track.artist_name, &track.title);
        let label = format!("Radio  {} / {}", idx + 1, tracks.len());

        let result = preview::run(
            client,
            track.id,
            debug,
            Some(label),
            Some(volume.clone()),
            saved,
            direction,
        )?;

        match result.as_str() {
            "prev" => {
                idx = (idx + tracks.len() - 1) % tracks.len();
                direction = Some("prev");
            }
            "quit" => break,
            r if r.starts_with("radio:") => {
                // User pressed r again — re-seed from the new track
                if let Ok(new_id) = r["radio:".len()..].parse::<u64>() {
                    run(client, new_id, debug)?;
                }
                break;
            }
            _ => {
                idx = (idx + 1) % tracks.len();
                direction = Some("next");
            }
        }
    }
    Ok(())
}

/// Full-screen message dismissed by any key — same pattern as the
/// fuzzy-select empty states in library.rs. Terminal is in cooked mode
/// here: preview::run tears down before returning its `radio:` action.
fn notice(msg: &str) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, Clear(ClearType::All), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    terminal.draw(|f| {
        let area = f.area();
        f.render_widget(Paragraph::new(msg), area);
    })?;
    loop {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    break;
                }
            }
        }
    }
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    Ok(())
}
