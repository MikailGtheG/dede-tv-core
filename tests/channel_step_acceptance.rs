//! TV-only channel stepping and fail-closed playback acceptance.
use dede_tv_core::{
    remote::{RemoteKey, Screen},
    tv::{Source, TvEvent, TvSession},
    VirtualChannels,
};

#[test]
fn channel_up_from_picker_tunes_next_channel_without_kodi_ui() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut session = TvSession::new(
        vec![
            ("TRT 1".into(), Source::Pvr(17)),
            ("ATV".into(), Source::Pvr(18)),
        ],
        vec![],
    );
    assert!(matches!(
        session.handle(RemoteKey::Ok, &db, 0).unwrap(),
        TvEvent::Picker(_)
    ));
    assert_eq!(session.picker.screen(), Screen::ChannelList);
    match session.handle(RemoteKey::ChannelUp, &db, 0).unwrap() {
        TvEvent::Playback { channel, request } => {
            assert_eq!(channel, "ATV");
            assert_eq!(request["method"], "Player.Open");
            assert_eq!(request["params"]["item"]["channelid"], 18);
        }
        _ => panic!("channel up must tune the next channel"),
    }
    assert_eq!(session.picker.screen(), Screen::LiveTv);
    assert_eq!(db.last_channel().unwrap(), None);
}

#[test]
fn invalid_channel_step_restores_open_picker_without_persisting() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut session = TvSession::new(
        vec![
            ("TRT 1".into(), Source::Pvr(17)),
            ("Invalid".into(), Source::Pvr(0)),
        ],
        vec![],
    );
    assert!(matches!(
        session.handle(RemoteKey::Ok, &db, 0).unwrap(),
        TvEvent::Picker(_)
    ));
    assert!(session.handle(RemoteKey::ChannelUp, &db, 0).is_err());
    assert_eq!(session.picker.screen(), Screen::ChannelList);
    assert_eq!(session.picker.selected(), Some("TRT 1"));
    assert_eq!(db.last_channel().unwrap(), None);
}
