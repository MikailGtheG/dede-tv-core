//! End-to-end core acceptance: remote events may only request TV playback,
//! never Kodi menus or an administrator surface.
use dede_tv_core::{
    remote::{RemoteKey, Screen},
    tv::{Source, TvEvent, TvSession},
    VirtualChannels,
};

#[test]
fn grandparent_ok_flow_opens_channel_list_then_requests_tv() {
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
    assert!(matches!(
        session.handle(RemoteKey::Down, &db, 0).unwrap(),
        TvEvent::Picker(_)
    ));
    match session.handle(RemoteKey::Ok, &db, 0).unwrap() {
        TvEvent::Playback { channel, request } => {
            assert_eq!(channel, "ATV");
            assert_eq!(request["method"], "Player.Open");
            assert_eq!(request["params"]["item"]["channelid"], 18);
        }
        _ => panic!("OK must request the highlighted TV channel"),
    }
    assert_eq!(session.picker.screen(), Screen::LiveTv);
    // A request is not proof of actual playback; persistence awaits Kodi ACK.
    assert_eq!(db.last_channel().unwrap(), None);
}

#[test]
fn menu_and_unknown_remote_keys_never_request_kodi_or_admin() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut session = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
    for key in [RemoteKey::Menu, RemoteKey::Unknown, RemoteKey::Home] {
        assert!(matches!(
            session.handle(key, &db, 0).unwrap(),
            TvEvent::None
        ));
        assert_eq!(session.picker.screen(), Screen::LiveTv);
    }
    session.handle(RemoteKey::Ok, &db, 0).unwrap();
    for key in [RemoteKey::Menu, RemoteKey::Unknown] {
        assert!(matches!(
            session.handle(key, &db, 0).unwrap(),
            TvEvent::None
        ));
        assert_eq!(session.picker.screen(), Screen::ChannelList);
    }
    assert_eq!(db.last_channel().unwrap(), None);
}
