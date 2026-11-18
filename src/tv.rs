//! Pure TV playback orchestration: remote actions never open Kodi UI windows.
use crate::{
    kodi,
    picker::{ChannelPicker, PickerEvent},
    remote::RemoteKey,
    VirtualChannels,
};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, PartialEq, Eq)]
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
    Playback { channel: String, request: Value },
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
    /// Replace the administrator-managed channel catalog atomically with the
    /// picker entries. Updating the picker alone would leave stale playback
    /// sources behind after channels are removed or reordered.
    pub fn replace_channels(&mut self, channels: Vec<(String, Source)>) -> PickerEvent {
        let source_changed = self.channels != channels;
        let names = channels.iter().map(|(name, _)| name.clone()).collect();
        self.channels = channels;
        let event = self.picker.replace_channels(names);
        // A source can change while its display name stays the same.
        // Redraw an open overlay so the UI does not show stale catalog data.
        if source_changed
            && matches!(event, PickerEvent::None)
            && self.picker.screen() == crate::remote::Screen::ChannelList
        {
            return PickerEvent::SelectionChanged {
                selected: self.picker.selected_index().unwrap_or(0),
            };
        }
        event
    }

    /// Record a channel only after Kodi acknowledges a successful Player.Open.
    /// Pass the channel identifier returned in TvEvent::Playback, not the
    /// current picker selection: the user may have navigated elsewhere while
    /// Kodi was processing the request.
    pub fn confirm_playback(&self, db: &VirtualChannels, name: &str) -> Result<(), String> {
        // Names are used as playback confirmation tokens. Ambiguous names
        // could persist the wrong source after Kodi acknowledges a request.
        let mut matches = self.channels.iter().filter(|(channel, _)| channel == name);
        let (_, source) = matches.next().ok_or("channel not found")?;
        if matches.next().is_some() {
            return Err("ambiguous channel name".into());
        }
        if name.trim().is_empty() || name.len() > 100 || name.chars().any(char::is_control) {
            return Err("channel name must not be blank or contain control characters".into());
        }
        if matches!(source, Source::Virtual)
            && (name.len() > 92 || name.starts_with("pvr:") || name.starts_with("virtual:"))
        {
            return Err("invalid virtual channel name for boot resume".into());
        }
        let key = match source {
            Source::Pvr(0) => return Err("PVR channel ID must be positive".into()),
            Source::Pvr(id) => format!("pvr:{id}"),
            Source::Virtual => format!("virtual:{name}"),
        };
        db.set_last_channel(&key).map_err(|e| format!("{e:?}"))
    }

    /// Confirm a PVR tune against the immutable Kodi request, not just its
    /// display name. Reject catalog changes that reuse a name for another ID.
    /// Call this after Kodi reports Player.Open success.
    pub fn confirm_pvr_request(
        &self,
        db: &VirtualChannels,
        name: &str,
        request: &Value,
    ) -> Result<(), String> {
        if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || request.get("method").and_then(Value::as_str) != Some("Player.Open")
        {
            return Err("not a Kodi JSON-RPC 2.0 playback request".into());
        }
        // Reject malformed envelopes instead of persisting a tune based on
        // a request that Kodi would not recognize as our generated call.
        let envelope = request
            .as_object()
            .ok_or("invalid Kodi playback request envelope")?;
        if envelope.len() != 4
            || !["jsonrpc", "method", "params", "id"]
                .iter()
                .all(|key| envelope.contains_key(*key))
            || request
                .get("id")
                .and_then(Value::as_u64)
                .filter(|id| *id > 0)
                .is_none()
        {
            return Err("invalid Kodi playback request envelope".into());
        }
        // A PVR acknowledgement must describe only a channel tune. A
        // mixed item (for example, both file and channelid) is not a
        // trustworthy confirmation of which source Kodi actually opened.
        let params = request
            .get("params")
            .and_then(Value::as_object)
            .ok_or("missing PVR playback parameters")?;
        if params.len() != 1 || !params.contains_key("item") {
            return Err("ambiguous PVR playback parameters".into());
        }
        let item = params
            .get("item")
            .and_then(Value::as_object)
            .ok_or("missing PVR channel item")?;
        if item.len() != 1 || !item.contains_key("channelid") {
            return Err("ambiguous PVR playback item".into());
        }
        let requested_id = item
            .get("channelid")
            .and_then(Value::as_u64)
            .ok_or("missing PVR channel ID")?;
        if requested_id == 0 || requested_id > u32::MAX as u64 {
            return Err("invalid PVR channel ID".into());
        }
        let mut matches = self.channels.iter().filter(|(channel, _)| channel == name);
        let (_, source) = matches.next().ok_or("channel not found")?;
        if matches.next().is_some() {
            return Err("ambiguous channel name".into());
        }
        match source {
            Source::Pvr(id) if u64::from(*id) == requested_id => self.confirm_playback(db, name),
            _ => Err("channel catalog changed since playback request".into()),
        }
    }

    pub fn handle(
        &mut self,
        key: RemoteKey,
        db: &VirtualChannels,
        now_ms: u64,
    ) -> Result<TvEvent, String> {
        // Keep the picker usable when an invalid source cannot be tuned.
        let previous_picker = self.picker.clone();
        match self.picker.handle(key) {
            PickerEvent::Tune(name) => {
                // The picker currently identifies channels by display name.
                // Never tune an arbitrary source when names are duplicated.
                let mut matches = self.channels.iter().filter(|(n, _)| *n == name);
                let (_, source) = match matches.next() {
                    Some(channel) => channel,
                    None => {
                        self.picker = previous_picker;
                        return Err("channel not found".into());
                    }
                };
                if matches.next().is_some() {
                    self.picker = previous_picker;
                    return Err("ambiguous channel name".into());
                }
                if name.trim().is_empty() || name.len() > 100 || name.chars().any(char::is_control)
                {
                    self.picker = previous_picker;
                    return Err(
                        "channel name must not be blank or contain control characters".into(),
                    );
                }
                if matches!(source, Source::Virtual)
                    && (name.len() > 92 || name.starts_with("pvr:") || name.starts_with("virtual:"))
                {
                    self.picker = previous_picker;
                    return Err("virtual channel name exceeds resumable limit".into());
                }
                let request = match source {
                    Source::Pvr(id) => {
                        if *id == 0 {
                            self.picker = previous_picker;
                            return Err("PVR channel ID must be positive".into());
                        }
                        kodi::open_pvr_channel(*id, 1)
                    }
                    Source::Virtual => {
                        let playback = match db.current(&name, now_ms) {
                            Ok(playback) => playback,
                            Err(error) => {
                                self.picker = previous_picker;
                                return Err(format!("{error:?}"));
                            }
                        };
                        match kodi::open_virtual_channel(&playback, &self.roots, 1) {
                            Ok(request) => request,
                            Err(error) => {
                                self.picker = previous_picker;
                                return Err(format!("{error:?}"));
                            }
                        }
                    }
                };
                // Building a request is not evidence that Kodi started playback.
                // The caller must send it and explicitly confirm success.
                Ok(TvEvent::Playback {
                    channel: name,
                    request,
                })
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
    fn source_refresh_preserves_second_duplicate_overlay_position() {
        let mut tv = TvSession::new(
            vec![
                ("Same".into(), Source::Pvr(17)),
                ("Same".into(), Source::Pvr(18)),
            ],
            vec![],
        );
        assert_eq!(tv.picker.handle(RemoteKey::Ok), PickerEvent::Show { selected: 0 });
        assert_eq!(
            tv.picker.handle(RemoteKey::Down),
            PickerEvent::SelectionChanged { selected: 1 }
        );
        assert_eq!(
            tv.replace_channels(vec![
                ("Same".into(), Source::Pvr(17)),
                ("Same".into(), Source::Pvr(19)),
            ]),
            PickerEvent::SelectionChanged { selected: 1 }
        );
    }

    #[test]
    fn remote_ok_generates_kodi_playback_without_opening_menus() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
            TvEvent::Playback { channel, request } => {
                assert_eq!(channel, "TRT 1");
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
    fn channel_step_generates_pvr_tune_without_opening_kodi_menus() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(
            vec![
                ("TRT 1".into(), Source::Pvr(17)),
                ("ATV".into(), Source::Pvr(18)),
            ],
            vec![],
        );
        match tv.handle(RemoteKey::ChannelUp, &db, 0).unwrap() {
            TvEvent::Playback { channel, request } => {
                assert_eq!(channel, "ATV");
                assert_eq!(request["method"], "Player.Open");
                assert_eq!(request["params"]["item"]["channelid"], 18);
            }
            _ => panic!("expected direct channel-up playback"),
        }
        assert_eq!(db.last_channel().unwrap(), None);
        match tv.handle(RemoteKey::ChannelDown, &db, 0).unwrap() {
            TvEvent::Playback { channel, request } => {
                assert_eq!(channel, "TRT 1");
                assert_eq!(request["params"]["item"]["channelid"], 17);
            }
            _ => panic!("expected direct channel-down playback"),
        }
        assert_eq!(tv.picker.screen(), crate::remote::Screen::LiveTv);
    }

    #[test]
    fn live_tv_arrow_keys_tune_pvr_channels_without_persisting_unconfirmed_state() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(
            vec![
                ("TRT 1".into(), Source::Pvr(17)),
                ("ATV".into(), Source::Pvr(18)),
            ],
            vec![],
        );
        for (key, expected_name, expected_id) in
            [(RemoteKey::Up, "ATV", 18), (RemoteKey::Down, "TRT 1", 17)]
        {
            match tv.handle(key, &db, 0).unwrap() {
                TvEvent::Playback { channel, request } => {
                    assert_eq!(channel, expected_name);
                    assert_eq!(request["method"], "Player.Open");
                    assert_eq!(request["params"]["item"]["channelid"], expected_id);
                }
                _ => panic!("expected arrow-key PVR playback"),
            }
            assert_eq!(tv.picker.screen(), crate::remote::Screen::LiveTv);
            assert_eq!(db.last_channel().unwrap(), None);
        }
    }

    #[test]
    fn invalid_channel_step_restores_selection_and_keeps_live_tv() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(
            vec![
                ("TRT 1".into(), Source::Pvr(17)),
                ("Broken".into(), Source::Pvr(0)),
            ],
            vec![],
        );
        assert!(tv.handle(RemoteKey::ChannelUp, &db, 0).is_err());
        assert_eq!(tv.picker.selected(), Some("TRT 1"));
        assert_eq!(tv.picker.screen(), crate::remote::Screen::LiveTv);
        assert_eq!(db.last_channel().unwrap(), None);
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
    fn reserved_virtual_prefixes_cannot_be_saved_as_resume_targets() {
        let db = VirtualChannels::open(":memory:").unwrap();
        for name in ["pvr:17", "virtual:Movies"] {
            let tv = TvSession::new(vec![(name.into(), Source::Virtual)], vec![]);
            assert!(tv.confirm_playback(&db, name).is_err());
            assert_eq!(db.last_channel().unwrap(), None);
        }
    }

    #[test]
    fn reserved_virtual_prefixes_cannot_generate_playback_requests() {
        let db = VirtualChannels::open(":memory:").unwrap();
        for name in ["pvr:17", "virtual:Movies"] {
            let mut tv = TvSession::new(vec![(name.into(), Source::Virtual)], vec![]);
            assert!(matches!(
                tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
                TvEvent::Picker(_)
            ));
            assert!(tv.handle(RemoteKey::Ok, &db, 0).is_err());
            assert_eq!(tv.picker.screen(), crate::remote::Screen::ChannelList);
            assert_eq!(db.last_channel().unwrap(), None);
        }
    }

    #[test]
    fn confirmation_uses_requested_channel_not_current_picker_selection() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(
            vec![
                ("TRT 1".into(), Source::Pvr(17)),
                ("ATV".into(), Source::Pvr(18)),
            ],
            vec![],
        );
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        let requested = match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
            TvEvent::Playback { channel, .. } => channel,
            _ => panic!("expected playback request"),
        };
        // The user can navigate while Kodi is processing the first request.
        tv.handle(RemoteKey::ChannelDown, &db, 0).unwrap();
        tv.confirm_playback(&db, &requested).unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:17"));
    }

    #[test]
    fn duplicate_names_cannot_confirm_the_wrong_playback_source() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let tv = TvSession::new(
            vec![
                ("Same".into(), Source::Pvr(17)),
                ("Same".into(), Source::Pvr(18)),
            ],
            vec![],
        );
        assert!(tv.confirm_playback(&db, "Same").is_err());
        assert_eq!(db.last_channel().unwrap(), None);
    }

    #[test]
    fn duplicate_names_cannot_tune_an_arbitrary_pvr_source() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(
            vec![
                ("Same".into(), Source::Pvr(17)),
                ("Same".into(), Source::Pvr(18)),
            ],
            vec![],
        );
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        assert!(tv.handle(RemoteKey::Ok, &db, 0).is_err());
        assert_eq!(tv.picker.screen(), crate::remote::Screen::ChannelList);
        assert_eq!(db.last_channel().unwrap(), None);
    }

    #[test]
    fn invalid_pvr_channel_id_never_generates_playback_request() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![("Invalid".into(), Source::Pvr(0))], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        assert!(tv.handle(RemoteKey::Ok, &db, 0).is_err());
        assert_eq!(tv.picker.screen(), crate::remote::Screen::ChannelList);
        assert!(tv.confirm_playback(&db, "Invalid").is_err());
        assert_eq!(db.last_channel().unwrap(), None);
    }

    #[test]
    fn replacing_catalog_updates_playback_sources_and_picker_together() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(PickerEvent::Show { selected: 0 })
        ));
        assert!(matches!(
            tv.replace_channels(vec![("ATV".into(), Source::Pvr(18))]),
            PickerEvent::SelectionChanged { selected: 0 }
        ));
        match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
            TvEvent::Playback { channel, request } => {
                assert_eq!(channel, "ATV");
                assert_eq!(request["params"]["item"]["channelid"], 18);
            }
            _ => panic!("expected playback from refreshed catalog"),
        }
        assert!(tv.confirm_playback(&db, "TRT 1").is_err());
        tv.confirm_playback(&db, "ATV").unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:18"));
    }

    #[test]
    fn stale_pvr_ack_cannot_persist_removed_channel_after_catalog_refresh() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        let request = kodi::open_pvr_channel(17, 1);
        tv.replace_channels(vec![("ATV".into(), Source::Pvr(18))]);
        assert!(tv.confirm_pvr_request(&db, "TRT 1", &request).is_err());
        assert_eq!(db.last_channel().unwrap(), None);
        let current = kodi::open_pvr_channel(18, 1);
        tv.confirm_pvr_request(&db, "ATV", &current).unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:18"));
    }

    #[test]
    fn stale_pvr_ack_cannot_persist_reassigned_channel_id() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        tv.handle(RemoteKey::Ok, &db, 0).unwrap();
        let request = match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
            TvEvent::Playback { request, .. } => request,
            _ => panic!("expected playback"),
        };
        tv.replace_channels(vec![("TRT 1".into(), Source::Pvr(18))]);
        assert!(tv.confirm_pvr_request(&db, "TRT 1", &request).is_err());
        assert_eq!(db.last_channel().unwrap(), None);
        let current = kodi::open_pvr_channel(18, 1);
        tv.confirm_pvr_request(&db, "TRT 1", &current).unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:18"));
    }

    #[test]
    fn mixed_kodi_playback_item_cannot_confirm_pvr_selection() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        let mixed = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "Player.Open",
            "params": { "item": { "channelid": 17, "file": "/other/video.mp4" } },
            "id": 1
        });
        assert!(tv.confirm_pvr_request(&db, "TRT 1", &mixed).is_err());
        assert_eq!(db.last_channel().unwrap(), None);
        let valid = kodi::open_pvr_channel(17, 1);
        tv.confirm_pvr_request(&db, "TRT 1", &valid).unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:17"));
    }

    #[test]
    fn extra_pvr_playback_parameters_cannot_confirm_selection() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        let mixed = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "Player.Open",
            "params": { "item": { "channelid": 17 }, "options": { "resume": true } },
            "id": 1
        });
        assert!(tv.confirm_pvr_request(&db, "TRT 1", &mixed).is_err());
        assert_eq!(db.last_channel().unwrap(), None);
        let valid = kodi::open_pvr_channel(17, 1);
        tv.confirm_pvr_request(&db, "TRT 1", &valid).unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:17"));
    }

    #[test]
    fn malformed_pvr_request_envelope_cannot_persist_selection() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        let valid = kodi::open_pvr_channel(17, 1);
        for key in ["id", "params"] {
            let mut malformed = valid.clone();
            malformed.as_object_mut().unwrap().remove(key);
            assert!(tv.confirm_pvr_request(&db, "TRT 1", &malformed).is_err());
            assert_eq!(db.last_channel().unwrap(), None);
        }
        for id in [
            serde_json::json!(0),
            serde_json::json!(-1),
            serde_json::json!("1"),
        ] {
            let mut malformed = valid.clone();
            malformed["id"] = id;
            assert!(tv.confirm_pvr_request(&db, "TRT 1", &malformed).is_err());
            assert_eq!(db.last_channel().unwrap(), None);
        }
        let mut extra = valid.clone();
        extra["unexpected"] = serde_json::json!(true);
        assert!(tv.confirm_pvr_request(&db, "TRT 1", &extra).is_err());
        assert_eq!(db.last_channel().unwrap(), None);
        tv.confirm_pvr_request(&db, "TRT 1", &valid).unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:17"));
    }

    #[test]
    fn malformed_jsonrpc_envelope_cannot_confirm_pvr_selection() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        let valid = kodi::open_pvr_channel(17, 1);
        for version in [
            serde_json::Value::Null,
            serde_json::json!("1.0"),
            serde_json::json!(2),
        ] {
            let mut malformed = valid.clone();
            malformed["jsonrpc"] = version;
            assert!(tv.confirm_pvr_request(&db, "TRT 1", &malformed).is_err());
            assert_eq!(db.last_channel().unwrap(), None);
        }
        tv.confirm_pvr_request(&db, "TRT 1", &valid).unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:17"));
    }

    #[test]
    fn non_integer_pvr_channel_ids_never_persist_resume_state() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        let valid = kodi::open_pvr_channel(17, 1);
        for invalid_id in [
            serde_json::json!(-17),
            serde_json::json!(17.0),
            serde_json::json!("17"),
            serde_json::json!(null),
            serde_json::json!(4294967296_u64),
        ] {
            let mut request = valid.clone();
            request["params"]["item"]["channelid"] = invalid_id;
            assert!(tv.confirm_pvr_request(&db, "TRT 1", &request).is_err());
            assert_eq!(db.last_channel().unwrap(), None);
        }
        tv.confirm_pvr_request(&db, "TRT 1", &valid).unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:17"));
    }

    #[test]
    fn blank_channel_name_cannot_tune_or_persist() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![("   ".into(), Source::Pvr(17))], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        assert!(tv.handle(RemoteKey::Ok, &db, 0).is_err());
        assert_eq!(tv.picker.screen(), crate::remote::Screen::ChannelList);
        assert!(tv.confirm_playback(&db, "   ").is_err());
        assert_eq!(db.last_channel().unwrap(), None);
    }

    #[test]
    fn control_characters_in_channel_names_cannot_tune_or_persist() {
        let db = VirtualChannels::open(":memory:").unwrap();
        for name in ["TRT\n1", "TRT\r1", "TRT\t1", "TRT\u{0000}1"] {
            let mut tv = TvSession::new(vec![(name.into(), Source::Pvr(17))], vec![]);
            assert!(matches!(
                tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
                TvEvent::Picker(_)
            ));
            assert!(tv.handle(RemoteKey::Ok, &db, 0).is_err());
            assert_eq!(tv.picker.screen(), crate::remote::Screen::ChannelList);
            assert!(tv.confirm_playback(&db, name).is_err());
            assert_eq!(db.last_channel().unwrap(), None);
        }
    }

    #[test]
    fn oversized_pvr_channel_name_is_rejected_before_kodi_request() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let name = "P".repeat(101);
        let mut tv = TvSession::new(vec![(name.clone(), Source::Pvr(17))], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        assert!(tv.handle(RemoteKey::Ok, &db, 0).is_err());
        assert_eq!(tv.picker.screen(), crate::remote::Screen::ChannelList);
        assert!(tv.confirm_playback(&db, &name).is_err());
        assert_eq!(db.last_channel().unwrap(), None);
    }

    #[test]
    fn multibyte_pvr_name_over_byte_limit_is_rejected_before_kodi_request() {
        let db = VirtualChannels::open(":memory:").unwrap();
        // 51 two-byte characters are 102 UTF-8 bytes, despite being
        // fewer than the 100-character persistence limit.
        let name = "é".repeat(51);
        assert_eq!(name.chars().count(), 51);
        assert_eq!(name.len(), 102);
        let mut tv = TvSession::new(vec![(name.clone(), Source::Pvr(17))], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        assert!(tv.handle(RemoteKey::Ok, &db, 0).is_err());
        assert_eq!(tv.picker.screen(), crate::remote::Screen::ChannelList);
        assert!(tv.confirm_playback(&db, &name).is_err());
        assert_eq!(db.last_channel().unwrap(), None);
    }

    #[test]
    fn pvr_channel_name_at_exact_byte_limit_remains_playable() {
        let db = VirtualChannels::open(":memory:").unwrap();
        // Fifty two-byte UTF-8 characters are exactly 100 bytes.
        let name = "é".repeat(50);
        assert_eq!(name.len(), 100);
        let mut tv = TvSession::new(vec![(name.clone(), Source::Pvr(17))], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
            TvEvent::Playback { channel, request } => {
                assert_eq!(channel, name);
                assert_eq!(request["params"]["item"]["channelid"], 17);
            }
            _ => panic!("expected playback at the accepted byte boundary"),
        }
        assert_eq!(db.last_channel().unwrap(), None);
        tv.confirm_playback(&db, &name).unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:17"));
    }

    #[test]
    fn oversized_virtual_channel_name_cannot_tune_or_persist() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let name = "V".repeat(93);
        let mut tv = TvSession::new(vec![(name.clone(), Source::Virtual)], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(_)
        ));
        assert!(tv.handle(RemoteKey::Ok, &db, 0).is_err());
        assert_eq!(tv.picker.screen(), crate::remote::Screen::ChannelList);
        assert!(tv.confirm_playback(&db, &name).is_err());
        assert_eq!(db.last_channel().unwrap(), None);
    }
    #[test]
    fn virtual_channel_utf8_name_respects_resumable_byte_boundary() {
        let db = VirtualChannels::open(":memory:").unwrap();
        // The persisted key includes the eight-byte "virtual:" prefix.
        // 46 two-byte characters fit exactly; 47 exceed the 100-byte key limit.
        let accepted = "é".repeat(46);
        let rejected = "é".repeat(47);
        assert_eq!(accepted.len(), 92);
        assert_eq!(rejected.len(), 94);

        let accepted_tv = TvSession::new(vec![(accepted.clone(), Source::Virtual)], vec![]);
        accepted_tv.confirm_playback(&db, &accepted).unwrap();
        assert_eq!(
            db.last_channel().unwrap().as_deref(),
            Some(format!("virtual:{accepted}").as_str())
        );

        let rejected_tv = TvSession::new(vec![(rejected.clone(), Source::Virtual)], vec![]);
        assert!(rejected_tv.confirm_playback(&db, &rejected).is_err());
        assert_eq!(
            db.last_channel().unwrap().as_deref(),
            Some(format!("virtual:{accepted}").as_str())
        );
    }

    #[test]
    fn channel_buttons_request_playback_without_persisting_before_ack() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(
            vec![
                ("TRT 1".into(), Source::Pvr(17)),
                ("ATV".into(), Source::Pvr(18)),
            ],
            vec![],
        );
        match tv.handle(RemoteKey::ChannelUp, &db, 0).unwrap() {
            TvEvent::Playback { channel, request } => {
                assert_eq!(channel, "ATV");
                assert_eq!(request["method"], "Player.Open");
                assert_eq!(request["params"]["item"]["channelid"], 18);
            }
            _ => panic!("expected channel-up playback"),
        }
        assert_eq!(db.last_channel().unwrap(), None);
        assert_eq!(tv.picker.screen(), crate::remote::Screen::LiveTv);
        tv.confirm_playback(&db, "ATV").unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:18"));
    }

    #[test]
    fn channel_down_from_picker_returns_to_live_tv_without_early_persistence() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(
            vec![
                ("TRT 1".into(), Source::Pvr(17)),
                ("ATV".into(), Source::Pvr(18)),
            ],
            vec![],
        );
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(PickerEvent::Show { selected: 0 })
        ));
        match tv.handle(RemoteKey::ChannelDown, &db, 0).unwrap() {
            TvEvent::Playback { channel, request } => {
                assert_eq!(channel, "ATV");
                assert_eq!(request["method"], "Player.Open");
                assert_eq!(request["params"]["item"]["channelid"], 18);
            }
            _ => panic!("expected channel-down playback"),
        }
        assert_eq!(tv.picker.screen(), crate::remote::Screen::LiveTv);
        assert_eq!(db.last_channel().unwrap(), None);
        tv.confirm_playback(&db, "ATV").unwrap();
        assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:18"));
    }

    #[test]
    fn single_channel_step_never_restarts_kodi_playback() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        for key in [RemoteKey::ChannelUp, RemoteKey::ChannelDown] {
            assert!(matches!(tv.handle(key, &db, 0).unwrap(), TvEvent::None));
        }
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(PickerEvent::Show { selected: 0 })
        ));
        for key in [RemoteKey::ChannelUp, RemoteKey::ChannelDown] {
            assert!(matches!(
                tv.handle(key, &db, 0).unwrap(),
                TvEvent::Picker(PickerEvent::Hide)
            ));
            assert_eq!(tv.picker.screen(), crate::remote::Screen::LiveTv);
            assert!(matches!(
                tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
                TvEvent::Picker(PickerEvent::Show { selected: 0 })
            ));
        }
        assert_eq!(db.last_channel().unwrap(), None);
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
    #[test]
    fn source_change_with_same_name_redraws_open_channel_list() {
        let db = VirtualChannels::open(":memory:").unwrap();
        let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
        assert!(matches!(
            tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
            TvEvent::Picker(PickerEvent::Show { selected: 0 })
        ));
        assert!(matches!(
            tv.replace_channels(vec![("TRT 1".into(), Source::Pvr(18))]),
            PickerEvent::SelectionChanged { selected: 0 }
        ));
        assert!(matches!(
            tv.replace_channels(vec![("TRT 1".into(), Source::Pvr(18))]),
            PickerEvent::None
        ));
        match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
            TvEvent::Playback { request, .. } => {
                assert_eq!(request["params"]["item"]["channelid"], 18);
            }
            _ => panic!("expected updated PVR source"),
        }
    }
}
