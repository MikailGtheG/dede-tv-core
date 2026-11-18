//! Malformed Kodi channel identifiers must never update the boot-resume target.
//! JSON numbers that are not positive u32 channel IDs cannot represent PVR channels.
use dede_tv_core::{
    tv::{Source, TvSession},
    VirtualChannels,
};
use serde_json::json;

#[test]
fn malformed_pvr_ids_do_not_persist_boot_resume() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);

    for invalid_id in [
        json!(0),
        json!(-1),
        json!(17.5),
        json!("17"),
        json!(null),
        json!(4294967296_u64),
    ] {
        let request = json!({
            "jsonrpc": "2.0",
            "method": "Player.Open",
            "params": { "item": { "channelid": invalid_id } },
            "id": 1
        });
        assert!(tv.confirm_pvr_request(&db, "TRT 1", &request).is_err());
        assert_eq!(db.last_channel().unwrap(), None);
    }
}
