use bilimani_queue::*;
fn request(id: u32, mode: &str) -> SongRequest {
    SongRequest {
        song_id: id,
        title: format!("Song {id}"),
        mode: mode.into(),
        chart: None,
        requester: "Viewer".into(),
    }
}
#[test]
fn menu_pauses_automatic_dispatch_but_priority_preserves_fifo_until_ack() {
    let mut queue = Queue::new(3, 5000);
    let a = queue.add(request(1, "SP")).unwrap().id;
    let b = queue.add(request(2, "SP")).unwrap().id;
    assert!(queue.next("SP", true).is_none());
    queue.prioritize(b).unwrap();
    assert_eq!(queue.next("SP", true).unwrap().entry.id, b);
    assert_eq!(queue.remove(b), Err(QueueError::Busy));
    assert_eq!(queue.prioritize(a), Err(QueueError::Busy));
    assert_eq!(queue.view(0).items.len(), 2);
    assert!(queue.finish(b, Completion::Selected, 0));
    assert_eq!(queue.view(0).items[0].entry.id, a);
    assert_eq!(queue.view(0).current.unwrap().entry.id, b);
    assert!(queue.next("SP", false).is_none());
    queue.remove(b).unwrap();
    assert_eq!(queue.next("SP", false).unwrap().entry.id, a);
}
#[test]
fn failed_replacement_retains_current_and_waiting_item_for_manual_retry() {
    let mut queue = Queue::default();
    let a = queue.add(request(1, "SP")).unwrap().id;
    queue.next("SP", false).unwrap();
    queue.finish(a, Completion::Selected, 0);
    let b = queue.add(request(2, "SP")).unwrap().id;
    queue.prioritize(b).unwrap();
    queue.next("SP", true).unwrap();
    assert!(!queue.finish(b, Completion::Failed("unavailable".into()), 10));
    assert_eq!(queue.view(10).current.unwrap().entry.id, a);
    assert_eq!(queue.get(b).unwrap().error.as_deref(), Some("unavailable"));
    queue.remove(a).unwrap();
    assert!(queue.next("SP", false).is_none());
    queue.prioritize(b).unwrap();
    queue.next("SP", true).unwrap();
    assert!(queue.finish(b, Completion::Selected, 20));
    assert!(queue.view(20).current.unwrap().entry.error.is_none());
}
#[test]
fn mode_mismatch_waits_and_play_counter_handles_ack_after_gameplay() {
    let mut queue = Queue::new(2, 2000);
    let dp = queue.add(request(1, "DP")).unwrap().id;
    let sp = queue.add(request(2, "SP")).unwrap().id;
    assert!(matches!(queue.add(request(3, "SP")), Err(QueueError::Full)));
    assert_eq!(queue.next("SP", false).unwrap().entry.id, sp);
    queue.observe(1, 500);
    queue.finish(sp, Completion::Selected, 1000);
    assert!(queue.view(1000).current.is_none());
    assert!(queue.next("SP", false).is_none());
    assert_eq!(queue.next("DP", false).unwrap().entry.id, dp);
    queue.finish(dp, Completion::Selected, 1000);
    assert_eq!(queue.view(2001).current.unwrap().remaining_seconds, 1);
    queue.observe(1, 3000);
    assert!(queue.view(3000).current.is_none());
    assert_eq!(
        queue.view(3000).events.last().unwrap().kind,
        EventKind::Expired
    );
}
