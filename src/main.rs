//! Local administrative CLI for the Dede TV Core prototype.
//! Intentionally no HTTP listener. Only trusted administrators may modify this SQLite DB.
use dede_tv_core::{kodi, transport, Video, VirtualChannels};
use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> Result<u64, String> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    u64::try_from(elapsed.as_millis()).map_err(|e| e.to_string())
}

fn usage() -> &'static str {
    "Usage: dede-tv-core --db FILE COMMAND [ARGS]\n\
     Commands:\n\
       add NAME FILE DURATION_MS [FILE DURATION_MS ...]\n\
       select NAME\n\
       last\n\
       now NAME\n\
       request-pvr CHANNEL_ID\n\
       request-virtual NAME APPROVED_MEDIA_ROOT\n\
       play-pvr CHANNEL_ID\n\
       resume-last APPROVED_MEDIA_ROOT\n\
       play-virtual NAME APPROVED_MEDIA_ROOT\n\
     Note: play-* contacts Kodi strictly on 127.0.0.1:9090."
}

// Debian 13's Rust compiler predates is_multiple_of and as_chunks.
// Keep these two portable equivalents despite newer Clippy suggestions.
#[allow(clippy::manual_is_multiple_of, clippy::chunks_exact_to_as_chunks)]
fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 3 || args[0] != "--db" || args[1].is_empty() {
        return Err(usage().to_string());
    }
    let database = PathBuf::from(&args[1]);
    let command = args[2].as_str();
    let params = &args[3..];
    let mut channels = VirtualChannels::open(&database).map_err(|e| format!("{e:?}"))?;
    match command {
        "add" => {
            if params.len() < 3 || params.len() % 2 == 0 {
                return Err(usage().to_string());
            }
            let name = &params[0];
            let videos = params[1..]
                .chunks_exact(2)
                .map(|pair| {
                    let ms = pair[1]
                        .parse::<u64>()
                        .map_err(|_| "duration must be a positive integer in milliseconds")?;
                    Ok(Video {
                        path: pair[0].clone(),
                        duration_ms: ms,
                    })
                })
                .collect::<Result<Vec<_>, &str>>()?;
            channels
                .set_channel(name, &videos, now_ms()?)
                .map_err(|e| format!("{e:?}"))?;
            println!("Added {} videos to channel {:?}", videos.len(), name);
        }
        "list" if params.is_empty() => {
            for name in channels.list_channels().map_err(|e| format!("{e:?}"))? {
                println!("{name}");
            }
        }
        "remove" if params.len() == 1 => {
            let removed = channels
                .remove_channel(&params[0])
                .map_err(|e| format!("{e:?}"))?;
            if !removed {
                return Err(format!("Channel not found: {:?}", params[0]));
            }
            println!("Removed {:?}", params[0]);
        }
        "select" if params.len() == 1 => {
            // Never persist a target that boot-resume cannot resolve.
            // Bare names are retained for compatibility with early databases.
            let selected = &params[0];
            if let Some(id) = selected.strip_prefix("pvr:") {
                let parsed = id.parse::<u32>().map_err(|_| "invalid PVR channel ID")?;
                if parsed == 0 {
                    return Err("invalid PVR channel ID".into());
                }
            } else {
                let name = selected.strip_prefix("virtual:").unwrap_or(selected);
                if !channels
                    .list_channels()
                    .map_err(|e| format!("{e:?}"))?
                    .iter()
                    .any(|existing| existing == name)
                {
                    return Err(format!("Channel not found: {name:?}"));
                }
            }
            channels
                .set_last_channel(selected)
                .map_err(|e| format!("{e:?}"))?;
            println!("Selected {:?}", selected);
        }
        "last" if params.is_empty() => {
            println!(
                "{}",
                channels
                    .last_channel()
                    .map_err(|e| format!("{e:?}"))?
                    .unwrap_or_default()
            );
        }
        "now" if params.len() == 1 => {
            let play = channels
                .current(&params[0], now_ms()?)
                .map_err(|e| format!("{e:?}"))?;
            println!("{} @ {} ms", play.path, play.position_ms);
        }
        "request-pvr" if params.len() == 1 => {
            let id = params[0]
                .parse::<u32>()
                .map_err(|_| "invalid PVR channel ID")?;
            println!("{}", kodi::open_pvr_channel(id, 1));
        }
        "request-virtual" if params.len() == 2 => {
            let play = channels
                .current(&params[0], now_ms()?)
                .map_err(|e| format!("{e:?}"))?;
            let request = kodi::open_virtual_channel(&play, &[PathBuf::from(&params[1])], 1)
                .map_err(|e| format!("{e:?}"))?;
            println!("{request}");
        }
        "resume-last" if params.len() == 1 => {
            if let Some(last) = channels.last_channel().map_err(|e| format!("{e:?}"))? {
                let request = if let Some(id) = last.strip_prefix("pvr:") {
                    let id = id
                        .parse::<u32>()
                        .map_err(|_| "invalid saved PVR channel ID")?;
                    kodi::open_pvr_channel(id, 1)
                } else {
                    let name = last.strip_prefix("virtual:").unwrap_or(&last);
                    let play = channels
                        .current(name, now_ms()?)
                        .map_err(|e| format!("{e:?}"))?;
                    kodi::open_virtual_channel(&play, &[PathBuf::from(&params[0])], 1)
                        .map_err(|e| format!("{e:?}"))?
                };
                let reply = transport::send_playback(&request).map_err(|e| e.to_string())?;
                println!("{reply}");
            } else {
                println!("No last channel stored");
            }
        }
        "play-pvr" if params.len() == 1 => {
            let id = params[0]
                .parse::<u32>()
                .map_err(|_| "invalid PVR channel ID")?;
            let request = kodi::open_pvr_channel(id, 1);
            let reply = transport::send_playback(&request).map_err(|e| e.to_string())?;
            channels
                .set_last_channel(&format!("pvr:{id}"))
                .map_err(|e| format!("{e:?}"))?;
            println!("{reply}");
        }
        "play-virtual" if params.len() == 2 => {
            let play = channels
                .current(&params[0], now_ms()?)
                .map_err(|e| format!("{e:?}"))?;
            let request = kodi::open_virtual_channel(&play, &[PathBuf::from(&params[1])], 1)
                .map_err(|e| format!("{e:?}"))?;
            let reply = transport::send_playback(&request).map_err(|e| e.to_string())?;
            channels
                .set_last_channel(&format!("virtual:{}", params[0]))
                .map_err(|e| format!("{e:?}"))?;
            println!("{reply}");
        }
        _ => return Err(usage().to_string()),
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
