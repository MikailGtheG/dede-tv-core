//! Repeated remote key events must not expose Kodi menus or persist a tune.
use dede_tv_core::{
    remote::{RemoteKey, Screen},
    tv::{Source, TvEvent, TvSession},
    VirtualChannels,
};

#[test]
fn repeated_menu_home_and_back_are_idempotent_in_live_tv() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
    for _ in 0..100 {
        for key in [RemoteKey::Menu, RemoteKey::Home, RemoteKey::Back, RemoteKey::Unknown] {
            assert!(matches!(tv.handle(key, &db, 0).unwrap(), TvEvent::None));
            assert_eq!(tv.picker.screen(), Screen::LiveTv);
        }
    }
    assert_eq!(db.last_channel().unwrap(), None);
}

#[test]
fn repeated_menu_events_do_not_dismiss_channel_list_or_tune() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
    assert!(matches!(tv.handle(RemoteKey::Ok, &db, 0).unwrap(), TvEvent::Picker(_)));
    for _ in 0..100 {
        for key in [RemoteKey::Menu, RemoteKey::Unknown] {
            assert!(matches!(tv.handle(key, &db, 0).unwrap(), TvEvent::None));
            assert_eq!(tv.picker.screen(), Screen::ChannelList);
        }
    }
    assert_eq!(db.last_channel().unwrap(), None);
}
