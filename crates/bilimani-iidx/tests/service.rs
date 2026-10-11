use bilimani_api::{ApiError, Command, SelectionInput};
use bilimani_iidx::{
    catalog::{Catalog, CatalogError},
    game::{GameUpdate, Phase, SelectionResult, Snapshot, Song},
    iidx::{self, SP},
    service::{GameAction, Service},
    versions,
};
fn service() -> Service {
    let mut service = Service::default();
    service.catalog = Some(
        Catalog::new(vec![Song {
            id: 100,
            title: "AA".into(),
            search_terms: vec!["えーえー".into()],
            charts: iidx::available_charts([0, 5, 10, 12, 0, 0, 5, 9, 12, 0]),
        }])
        .unwrap(),
    );
    service
}
fn update(phase: Phase, epoch: u64) -> GameUpdate {
    GameUpdate {
        snapshot: Snapshot {
            can_open_menu: true,
            phase,
            mode: Some(SP),
            epoch,
        },
        ..Default::default()
    }
}
fn submit(service: &mut Service, now: u64) -> u64 {
    service
        .command(
            Command::Select(SelectionInput {
                song_id: 100,
                mode: Some("SP".into()),
                chart: Some("SPA".into()),
            }),
            now,
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap()
}
#[test]
fn operation_waits_for_native_ack_and_preserves_scene_epoch() {
    let mut service = service();
    let id = submit(&mut service, 0);
    assert!(service.update(update(Phase::Playing, 5), 100).is_none());
    let Some(GameAction::Submit(selection)) = service.update(update(Phase::Select, 6), 200) else {
        panic!("missing native selection")
    };
    assert_eq!(
        (selection.token, selection.epoch, selection.song_id),
        (id, 6, 100)
    );
    assert_eq!(
        service.command(Command::Selection(id), 200).unwrap()["status"],
        "executing"
    );
    assert!(matches!(
        service.command(
            Command::Select(SelectionInput {
                song_id: 100,
                mode: None,
                chart: None
            }),
            200
        ),
        Err(ApiError::Busy)
    ));
    let mut ack = update(Phase::Select, 6);
    ack.selection_result = Some(SelectionResult {
        token: id,
        result: Some(Ok(())),
    });
    service.update(ack, 300);
    assert_eq!(
        service.command(Command::Selection(id), 300).unwrap()["status"],
        "selected"
    );
    assert_eq!(service.plays, 0, "locating a song does not imply gameplay");
    assert_ne!(submit(&mut service, 400), id);
}
#[test]
fn deferred_selection_retries_with_new_epoch_and_timeout_cancels_mailbox() {
    let mut service = service();
    let id = submit(&mut service, 0);
    service.update(update(Phase::Select, 1), 0);
    let mut deferred = update(Phase::Other, 2);
    deferred.selection_result = Some(SelectionResult {
        token: id,
        result: None,
    });
    assert!(service.update(deferred, 50).is_none());
    let Some(GameAction::Submit(selection)) = service.update(update(Phase::Select, 3), 100) else {
        panic!()
    };
    assert_eq!(selection.epoch, 3);
    assert!(matches!(
        service.update(update(Phase::Other, 4), 30_000),
        Some(GameAction::Cancel)
    ));
    let failed = service.command(Command::Selection(id), 30_000).unwrap();
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"]["code"], "expired");
}
#[test]
fn validation_and_search_have_no_viewer_state() {
    let mut empty = Service::default();
    assert!(matches!(
        empty.command(
            Command::Search {
                query: "AA".into(),
                limit: 5
            },
            0
        ),
        Err(ApiError::NotReady)
    ));
    let mut service = service();
    let result = service
        .command(
            Command::Search {
                query: "ＡＡ".into(),
                limit: 5,
            },
            0,
        )
        .unwrap();
    assert_eq!(result["songs"][0]["id"], 100);
    assert!(matches!(
        service.command(
            Command::Select(SelectionInput {
                song_id: 100,
                mode: Some("SP".into()),
                chart: Some("DPA".into())
            }),
            0
        ),
        Err(ApiError::InvalidRequest(_))
    ));
    assert!(matches!(
        service.command(
            Command::Select(SelectionInput {
                song_id: 999,
                mode: None,
                chart: None
            }),
            0
        ),
        Err(ApiError::NotFound)
    ));
    let snapshots = service.snapshots();
    let state = &snapshots
        .iter()
        .find(|(path, _)| path.ends_with("/state"))
        .unwrap()
        .1;
    assert!(state.get("queue").is_some());
    assert!(state.get("connected").is_none());
}
#[test]
fn search_index_and_typed_catalog_errors() {
    assert!(matches!(Catalog::new(vec![]), Err(CatalogError::Empty)));
    let mut service = service();
    let catalog = service.catalog.as_mut().unwrap();
    catalog.set_search_index(&[(100, "native synonym".into())]);
    assert_eq!(catalog.search("ntv syn", 5)[0].id, 100);
    assert!(catalog.search("absent", 5).is_empty());
    assert!(versions::resolve("unverified").is_err());
    assert_eq!(
        versions::resolve(versions::v33::b20260812::SUPPORTED_SHA256).unwrap(),
        versions::Build::V33_20260812
    );
}

#[test]
fn api_and_menu_commands_share_authoritative_queue_and_failed_priority() {
    let mut service = service();
    service.update(update(Phase::Select, 1), 0);
    service.menu_open = true;
    let enqueue = || {
        Command::Enqueue(bilimani_api::QueueInput {
            song_id: 100,
            mode: Some("SP".into()),
            chart: Some("SPA".into()),
            requester: "Viewer".into(),
        })
    };
    let id = service.command(enqueue(), 1).unwrap()["id"]
        .as_u64()
        .unwrap();
    assert!(service.update(update(Phase::Select, 1), 2).is_none());
    service.command(Command::Priority(id), 3).unwrap();
    let Some(GameAction::Submit(selection)) = service.update(update(Phase::Select, 1), 4) else {
        panic!("explicit priority must execute")
    };
    assert!(matches!(
        service.command(Command::Remove(id), 5),
        Err(ApiError::Busy)
    ));
    let mut ack = update(Phase::Select, 1);
    ack.selection_result = Some(SelectionResult {
        token: selection.token,
        result: Some(Err(bilimani_iidx::game::SelectionError::Unavailable)),
    });
    service.update(ack, 6);
    assert!(!service.close_menu);
    assert!(service.queue.get(id).unwrap().error.is_some());
    service.command(Command::Priority(id), 7).unwrap();
    let Some(GameAction::Submit(selection)) = service.update(update(Phase::Select, 2), 8) else {
        panic!()
    };
    assert_eq!(selection.epoch, 2);
    let mut ack = update(Phase::Select, 2);
    ack.selection_result = Some(SelectionResult {
        token: selection.token,
        result: Some(Ok(())),
    });
    service.update(ack, 9);
    assert!(service.close_menu);
    assert!(
        service
            .menu_view(std::path::Path::new("aliases.csv"), "")
            .queue
            .items
            .is_empty()
    );
    let snapshots = service.snapshots();
    let queue = &snapshots
        .iter()
        .find(|(path, _)| path.ends_with("/queue"))
        .unwrap()
        .1;
    assert_eq!(queue["current"]["id"], id);
    service.command(Command::Remove(id), 10).unwrap();
    assert!(service.queue.view(10).current.is_none());
}
