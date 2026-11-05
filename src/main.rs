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
       play-virtual NAME APPROVED_MEDIA_ROOT\n\
     Note: play-* contacts Kodi strictly on 127.0.0.1:9090."
}

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
            if params.len() < 3 || params.len().is_multiple_of(2) {
                return Err(usage().to_string());
            }
            let name = &params[0];
            let videos = params[1..]
                .as_chunks::<2>().0.iter()
                .map(|pair| {
                    let ms = pair[1]
                        .parse::<u64>()
                        .map_err(|_| "duration must be a positive integer in milliseconds")?;
                    Ok(Video { path: pair[0].clone(), duration_ms: ms })
                })
                .collect::<Result<Vec<_>, &str>>()?;
            channels
                .set_channel(name, &videos, now_ms()?)
                .map_err(|e| format!("{e:?}"))?;
            println!("Added {} videos to channel {:?}", videos.len(), name);
        }
        "select" if params.len() == 1 => {
            channels.set_last_channel(&params[0]).map_err(|e| format!("{e:?}"))?;
            println!("Selected {:?}", params[0]);
        }
        "last" if params.is_empty() => {
            println!("{}", channels.last_channel().map_err(|e| format!("{e:?}"))?.unwrap_or_default());
        }
        "now" if params.len() == 1 => {
            let play = channels.current(&params[0], now_ms()?)
                .map_err(|e| format!("{e:?}"))?;
            println!("{} @ {} ms", play.path, play.position_ms);
        }
        "request-pvr" if params.len() == 1 => {
            let id = params[0].parse::<u32>().map_err(|_| "invalid PVR channel ID")?;
            println!("{}", kodi::open_pvr_channel(id, 1));
        }
        "request-virtual" if params.len() == 2 => {
            let play = channels.current(&params[0], now_ms()?)
                .map_err(|e| format!("{e:?}"))?;
            let request = kodi::open_virtual_channel(&play, &[PathBuf::from(&params[1])], 1)
                .map_err(|e| format!("{e:?}"))?;
            println!("{request}");
        }
        "play-pvr" if params.len() == 1 => {
            let id = params[0].parse::<u32>().map_err(|_| "invalid PVR channel ID")?;
            let request = kodi::open_pvr_channel(id, 1);
            let reply = transport::send_playback(&request).map_err(|e| e.to_string())?;
            println!("{reply}");
        }
        "play-virtual" if params.len() == 2 => {
            let play = channels.current(&params[0], now_ms()?)
                .map_err(|e| format!("{e:?}"))?;
            let request = kodi::open_virtual_channel(&play, &[PathBuf::from(&params[1])], 1)
                .map_err(|e| format!("{e:?}"))?;
            let reply = transport::send_playback(&request).map_err(|e| e.to_string())?;
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
