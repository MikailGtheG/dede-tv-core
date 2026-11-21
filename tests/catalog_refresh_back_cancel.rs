//! A cancelled selection must never become a playback request, even if an
//! administrator updates the channel catalog while the overlay is visible.
use dede_tv_core::{
    remote::{RemoteKey, Screen},
    tv::{Source, TvEvent, TvSession},
    VirtualChannels,
};

#[test]
fn back_after_catalog_refresh_cancels_without_tuning() {
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
    assert!(matches!(
        tv.handle(RemoteKey::Down, &db, 0).unwrap(),
        TvEvent::Picker(_)
    ));
    tv.replace_channels(vec![
        ("ATV".into(), Source::Pvr(19)),
        ("TRT 1".into(), Source::Pvr(17)),
    ]);
    assert_eq!(tv.picker.screen(), Screen::ChannelList);
    assert!(matches!(
        tv.handle(RemoteKey::Back, &db, 0).unwrap(),
        TvEvent::Picker(_)
    ));
    assert_eq!(tv.picker.screen(), Screen::LiveTv);
    assert_eq!(db.last_channel().unwrap(), None);
    assert!(matches!(
        tv.handle(RemoteKey::Menu, &db, 0).unwrap(),
        TvEvent::None
    ));
    assert_eq!(db.last_channel().unwrap(), None);
}
