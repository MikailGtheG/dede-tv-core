//! Pure TV playback orchestration: remote actions never open Kodi UI windows.
use crate::{
    kodi,
    picker::{ChannelPicker, PickerEvent},
    remote::RemoteKey,
    VirtualChannels,
};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone)]
pub enum Source {
    Pvr(u32),
    Virtual,
}
pub struct TvSession {
    pub picker: ChannelPicker,
    channels: Vec<(String, Source)>,
    roots: Vec<PathBuf>,
}
pub enum TvEvent {
    None,
    Picker(PickerEvent),
    Playback(Value),
}
impl TvSession {
    pub fn new(channels: Vec<(String, Source)>, roots: Vec<PathBuf>) -> Self {
        let picker = ChannelPicker::new(channels.iter().map(|(n, _)| n.clone()).collect(), None);
        Self {
            picker,
            channels,
            roots,
        }
    }
    /// Record a channel only after Kodi acknowledges a successful Player.Open.
    pub fn confirm_playback(&self, db: &VirtualChannels, name: &str) -> Result<(), String> {
        let (_, source) = self
            .channels
            .iter()
            .find(|(channel, _)| channel == name)
            .ok_or("channel not found")?;
        let key = match source {
            Source::Pvr(id) => format!("pvr:{id}"),
            Source::Virtual => format!("virtual:{name}"),
        };
        db.set_last_channel(&key).map_err(|e| format!("{e:?}"))
    }

    pub fn handle(
        &mut self,
        key: RemoteKey,
        db: &VirtualChannels,
        now_ms: u64,
    ) -> Result<TvEvent, String> {
        match self.picker.handle(key) {
            PickerEvent::Tune(name) => {
                let (_, source) = self
                    .channels
                    .iter()
                    .find(|(n, _)| *n == name)
                    .ok_or("channel not found")?;
                let request = match source {
                    Source::Pvr(id) => kodi::open_pvr_channel(*id, 1),
                    Source::Virtual => {
                        let playback = db.current(&name, now_ms).map_err(|e| format!("{e:?}"))?;
                        kodi::open_virtual_channel(&playback, &self.roots, 1)
                            .map_err(|e| format!("{e:?}"))?
                    }
                };
                // Building a request is not evidence that Kodi started playback.
                // The caller must send it and explicitly confirm success.
                Ok(TvEvent::Playback(request))
            }
            PickerEvent::None => Ok(TvEvent::None),
            event => Ok(TvEvent::Picker(event)),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_ok_generates_kodi_playback_without_opening_menus() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
            TvEvent::Playback(request) => {
                assert_eq!(request["method"], "Player.Open");
                assert_eq!(request["params"]["item"]["channelid"], 17);
            }
            _ => panic!("expected playback"),
        }
        assert_eq!(db.last_channel().unwrap(), None);
        assert!(tv.confirm_playback(&db, "missing").is_err());
        assert_eq!(db.last_channel().unwrap(), None);
        tv.confirm_playback(&db, "TRT 1").unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:17"));
        assert!(matches!(
            tv.handle(RemoteKey::Menu, &db, 0).unwrap(),
            TvEvent::None
        ));
    }
    #[test]
    fn confirmed_virtual_channel_uses_resumable_identity() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let tv = TvSession::new(vec![("Movies".into(), Source::Virtual)], vec![]);
        tv.confirm_playback(&db, "Movies").unwrap();
        assert_eq!(
            db.last_channel().unwrap().as_deref(),
            Some("virtual:Movies")
        );
    }

    #[test]
    fn empty_channel_list_is_safe() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::ChannelUp, &db, 0).unwrap(),
            TvEvent::None
        ));
    }
}
