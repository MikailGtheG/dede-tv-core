//! Regression: administrator catalog updates must never tune a channel by themselves.
use dede_tv_core::{
    picker::{PickerEvent},
    remote::{RemoteKey, Screen},
    tv::{Source, TvEvent, TvSession},
    VirtualChannels,
};

#[test]
fn catalog_refresh_during_picker_navigation_never_autoplays() {
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
    assert!(matches!(
        tv.handle(RemoteKey::Down, &db, 0).unwrap(),
        TvEvent::Picker(PickerEvent::SelectionChanged { selected: 1 })
    ));

    // An administrative reorder changes the highlighted index, not playback.
    assert!(matches!(
        tv.replace_channels(vec![
            ("ATV".into(), Source::Pvr(18)),
            ("TRT 1".into(), Source::Pvr(17)),
        ]),
        PickerEvent::SelectionChanged { selected: 0 }
    ));
    assert_eq!(tv.picker.screen(), Screen::ChannelList);
    assert_eq!(db.last_channel().unwrap(), None);

    // Back cancels browsing, including the selection made before the refresh.
    assert!(matches!(
        tv.handle(RemoteKey::Back, &db, 0).unwrap(),
        TvEvent::Picker(PickerEvent::Hide)
    ));
    assert_eq!(tv.picker.screen(), Screen::LiveTv);
    assert_eq!(tv.picker.selected(), Some("TRT 1"));
    assert_eq!(db.last_channel().unwrap(), None);
}
