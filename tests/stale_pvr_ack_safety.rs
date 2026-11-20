use dede_tv_core::{
    remote::RemoteKey,
    tv::{Source, TvEvent, TvSession},
    VirtualChannels,
};

#[test]
fn delayed_pvr_ack_cannot_persist_a_reassigned_channel() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);

    assert!(matches!(
        tv.handle(RemoteKey::Ok, &db, 0).unwrap(),
        TvEvent::Picker(_)
    ));
    let old_request = match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
        TvEvent::Playback { request, .. } => request,
        _ => panic!("expected PVR tune"),
    };

    // The administrator can reuse a display name for another Kodi channel
    // while the original Player.Open acknowledgement is still in flight.
    tv.replace_channels(vec![("TRT 1".into(), Source::Pvr(18))]);
    assert!(tv.confirm_pvr_request(&db, "TRT 1", &old_request).is_err());
    assert_eq!(db.last_channel().unwrap(), None);

    let new_request = match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
        TvEvent::Playback { request, .. } => request,
        _ => panic!("expected new PVR tune"),
    };
    assert_eq!(new_request["params"]["item"]["channelid"], 18);
    tv.confirm_pvr_request(&db, "TRT 1", &new_request).unwrap();
    assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:18"));
}
