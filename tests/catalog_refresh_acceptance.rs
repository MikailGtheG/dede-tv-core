use dede_tv_core::{
    remote::{RemoteKey, Screen},
    tv::{Source, TvEvent, TvSession},
    VirtualChannels,
};

#[test]
fn removing_all_channels_while_browsing_closes_overlay_and_prevents_tuning() {
    let db = VirtualChannels::open(":memory:").expect("in-memory DB");
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
    assert_eq!(tv.picker.screen(), Screen::ChannelList);

    // An administrator may replace the catalogue while a remote is browsing.
    tv.replace_channels(vec![]);
    assert_eq!(tv.picker.screen(), Screen::LiveTv);
    assert!(tv.picker.channels().is_empty());

    for key in [
        RemoteKey::Ok,
        RemoteKey::ChannelUp,
        RemoteKey::ChannelDown,
        RemoteKey::Up,
        RemoteKey::Down,
    ] {
        assert!(matches!(tv.handle(key, &db, 0).unwrap(), TvEvent::None));
    }
    assert_eq!(db.last_channel().unwrap(), None);

    // A subsequent catalogue refresh must restore normal remote navigation.
    tv.replace_channels(vec![("TRT 1".into(), Source::Pvr(17))]);
    assert!(matches!(
        tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
        TvEvent::Picker(_)
    ));
    match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
        TvEvent::Playback { channel, request } => {
            assert_eq!(channel, "TRT 1");
            assert_eq!(request["params"]["item"]["channelid"], 17);
        }
        _ => panic!("expected playback request after catalogue recovery"),
    }
    assert_eq!(db.last_channel().unwrap(), None);
}
