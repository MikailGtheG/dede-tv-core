use dede_tv_core::{
    remote::RemoteKey,
    tv::{Source, TvEvent, TvSession},
    VirtualChannels,
};

#[test]
fn removing_last_channel_while_picker_open_never_tunes_stale_source() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);

    assert!(matches!(
        tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
        TvEvent::Picker(_)
    ));
    tv.replace_channels(vec![]);
    assert!(tv.picker.channels().is_empty());

    for key in [
        RemoteKey::Ok,
        RemoteKey::ChannelUp,
        RemoteKey::ChannelDown,
        RemoteKey::Menu,
        RemoteKey::Home,
        RemoteKey::Ok,
    ] {
        assert!(!matches!(
            tv.handle(key, &db, 0).unwrap(),
            TvEvent::Playback { .. }
        ));
    }
    assert_eq!(db.last_channel().unwrap(), None);
}
