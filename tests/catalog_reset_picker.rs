use dede_tv_core::picker::{ChannelPicker, PickerEvent};
use dede_tv_core::remote::{RemoteKey, Screen};

#[test]
fn catalog_removal_closes_overlay_and_new_catalog_starts_clean() {
    let mut picker = ChannelPicker::new(
        vec!["TRT 1".into(), "ATV".into()],
        Some("TRT 1"),
    );
    assert_eq!(picker.handle(RemoteKey::Ok), PickerEvent::Show { selected: 0 });
    assert_eq!(
        picker.handle(RemoteKey::Down),
        PickerEvent::SelectionChanged { selected: 1 }
    );

    assert_eq!(picker.replace_channels(vec![]), PickerEvent::Hide);
    assert_eq!(picker.screen(), Screen::LiveTv);
    assert_eq!(picker.selected(), None);
    assert_eq!(picker.handle(RemoteKey::Ok), PickerEvent::None);
    assert_eq!(picker.handle(RemoteKey::Back), PickerEvent::None);

    assert_eq!(picker.replace_channels(vec!["New".into()]), PickerEvent::None);
    assert_eq!(picker.handle(RemoteKey::Ok), PickerEvent::Show { selected: 0 });
    assert_eq!(picker.handle(RemoteKey::Back), PickerEvent::Hide);
    assert_eq!(picker.selected(), Some("New"));
}
