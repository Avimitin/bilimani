use chart_requester::{
    catalog::Catalog,
    config::Config,
    engine::{Chat, Engine, Phase, Snapshot},
    game::Song,
    games::iidx::{self, v33::catalog::parse_database},
};
use std::collections::BTreeMap;
fn song(id: u32, title: &str) -> Song {
    Song {
        id,
        title: title.into(),
        search_terms: vec![],
        charts: iidx::available_charts([0, 4, 7, 10, 12, 0, 4, 7, 10, 12]),
    }
}
fn engine() -> Engine {
    let catalog = Catalog::new(
        vec![
            song(1, "AA"),
            song(2, "AA -rebuild-"),
            song(3, "冥"),
            song(4, "雪月花"),
        ],
        &BTreeMap::new(),
    )
    .unwrap();
    let mut e = Engine::new(Config::default(), catalog, &iidx::RULES);
    e.observe(
        Snapshot {
            phase: Phase::Select,
            mode: Some(iidx::SP),
            epoch: 1,
            can_skip: true,
        },
        0,
    );
    e
}

#[test]
fn menu_deletion_rejects_inflight_and_stale_ids_without_dropping_other_requests() {
    let mut e = engine();
    chat(&mut e, "a", "点歌 冥", 0);
    chat(&mut e, "b", "点歌 雪月花", 0);
    let second = e.queue[1].token;
    let j = e.next_jump(0).unwrap();
    assert!(!e.remove_queued(j.request.token, 1));
    assert!(e.remove_queued(second, 1));
    assert!(!e.remove_queued(second, 1));
    e.jump_result(j.request.token, Some(Ok(())), 2);
    assert!(!e.dismiss_current(j.request.token, 99, 2));
    assert!(e.dismiss_current(j.request.token, 1, 2));
    assert!(e.current.is_none());
}

#[test]
fn picking_a_queued_song_preserves_order_until_ack_and_replaces_current_on_success() {
    let mut e = engine();
    chat(&mut e, "current", "点歌 冥", 0);
    let current = jump(&mut e, 0);
    chat(&mut e, "a", "点歌 雪月花", 1);
    chat(&mut e, "b", "点歌 冥", 1);
    chat(&mut e, "c", "点歌 雪月花", 1);
    let tokens: Vec<_> = e.queue.iter().map(|r| r.token).collect();
    let picked = e.select_queued(tokens[1], 1).unwrap();
    assert_eq!(picked.selection().token, tokens[1]);
    assert_eq!(e.current.as_ref().unwrap().request.token, current);
    assert_eq!(e.queue.iter().map(|r| r.token).collect::<Vec<_>>(), tokens);
    assert!(!e.remove_queued(tokens[1], 2));
    assert!(e.next_jump(2).is_none());
    assert!(e.select_queued(tokens[2], 1).is_none());
    e.jump_result(tokens[0], Some(Ok(())), 2); // Unrelated/stale acknowledgement.
    assert_eq!(e.queue.len(), 3);
    e.jump_result(tokens[1], Some(Ok(())), 2);
    assert_eq!(e.current.as_ref().unwrap().request.token, tokens[1]);
    assert_eq!(
        e.queue.iter().map(|r| r.token).collect::<Vec<_>>(),
        [tokens[0], tokens[2]],
    );
    e.jump_result(tokens[1], Some(Ok(())), 3);
    assert_eq!(e.queue.len(), 2);
    assert!(e.select_queued(tokens[1], 1).is_none());
}

#[test]
fn picking_a_queued_song_rejects_stale_scene_mode_and_inflight_requests() {
    let mut e = engine();
    chat(&mut e, "a", "点歌 冥", 0);
    let token = e.queue[0].token;
    assert!(e.select_queued(token + 1, 1).is_none());
    assert!(e.select_queued(token, 2).is_none());
    for phase in [Phase::Playing, Phase::Other] {
        e.snapshot.phase = phase;
        assert!(!e.can_select_queued(token));
        assert!(e.select_queued(token, 1).is_none());
    }
    e.snapshot.phase = Phase::Select;
    for mode in [None, Some(iidx::DP)] {
        e.snapshot.mode = mode;
        assert!(e.select_queued(token, 1).is_none());
    }
    e.snapshot.mode = Some(iidx::SP);
    e.next_jump(0).unwrap();
    assert!(e.select_queued(token, 1).is_none());
    e.jump_result(token, None, 1);
    // Manual picking does not depend on the opposite-Start shortcut being enabled.
    e.snapshot.can_skip = false;
    e.config.controls.skip_enabled = false;
    assert!(e.select_queued(token, 1).is_some());
    assert_eq!(e.queue.len(), 1);
}

#[test]
fn cancelled_or_failed_manual_pick_keeps_current_and_all_waiting_requests() {
    let mut e = engine();
    chat(&mut e, "current", "点歌 冥", 0);
    let current = jump(&mut e, 0);
    chat(&mut e, "a", "点歌 雪月花", 1);
    chat(&mut e, "b", "点歌 冥", 1);
    let tokens: Vec<_> = e.queue.iter().map(|r| r.token).collect();
    for result in [None, Some(Err("locked".into()))] {
        e.select_queued(tokens[1], 1).unwrap();
        e.jump_result(tokens[1], result, 2);
        assert_eq!(e.current.as_ref().unwrap().request.token, current);
        assert_eq!(e.queue.iter().map(|r| r.token).collect::<Vec<_>>(), tokens);
        assert!(e.can_select_queued(tokens[1]));
    }
}

#[test]
fn live_alias_update_keeps_native_keywords_pending_and_queue() {
    let mut e = engine();
    e.catalog
        .set_search_index(&[(3, "special native reading".into())]);
    chat(&mut e, "a", "点歌 雪月花", 0);
    chat(&mut e, "b", "点歌 AA", 0);
    let token = e.queue[0].token;
    let until = e.pending["b"].until;
    e.catalog
        .update_aliases(&BTreeMap::from([("new alias".into(), "3".into())]))
        .unwrap();
    assert_eq!(e.catalog.search("special native reading", 5)[0].id, 3);
    assert_eq!(e.catalog.search("new alias", 5)[0].id, 3);
    assert!(
        e.catalog
            .update_aliases(&BTreeMap::from([("bad".into(), "absent".into())]))
            .is_err()
    );
    assert_eq!(e.catalog.search("new alias", 5)[0].id, 3);
    assert_eq!(e.queue[0].token, token);
    assert_eq!(e.pending["b"].until, until);
}
fn chat(e: &mut Engine, user: &str, text: &str, now: u64) {
    e.chat(
        Chat {
            user: user.into(),
            name: format!("viewer {user}"),
            text: text.into(),
        },
        now,
    );
}
fn jump(e: &mut Engine, now: u64) -> u64 {
    let j = e.next_jump(now).unwrap();
    e.jump_result(j.request.token, Some(Ok(())), now);
    j.request.token
}

#[test]
fn manual_skip_advances_once_and_preserves_notice_after_jump_ack() {
    let mut e = engine();
    chat(&mut e, "a", "点歌 冥", 0);
    chat(&mut e, "b", "点歌 雪月花", 0);
    let first = jump(&mut e, 0);
    assert!(e.skip_current(first, 1, 1));
    assert!(!e.skip_current(first, 1, 1));
    assert!(e.current.is_none());
    assert_eq!(e.queue.len(), 1);
    let second = jump(&mut e, 1);
    assert_ne!(first, second);
    assert_eq!(e.current.as_ref().unwrap().request.song.title, "雪月花");
    let message = &e.messages.back().unwrap().1;
    assert!(message.contains("已跳过：冥"));
    assert!(message.contains("已定位：雪月花"));
    assert!(!e.skip_current(first, 1, 2));
    assert!(e.skip_current(second, 1, 2)); // Last item can be dismissed too.
    assert!(e.next_jump(2).is_none());
}

#[test]
fn manual_skip_rejects_stale_context_disabled_controls_and_inflight_jump() {
    let mut e = engine();
    chat(&mut e, "a", "点歌 冥", 0);
    let j = e.next_jump(0).unwrap();
    assert!(!e.skip_current(j.request.token, 1, 0));
    e.jump_result(j.request.token, Some(Ok(())), 0);
    let token = j.request.token;
    assert!(!e.skip_current(token, 2, 1));
    assert!(!e.skip_current(token + 1, 1, 1));
    e.snapshot.phase = Phase::Other;
    assert!(!e.skip_current(token, 1, 1));
    e.snapshot.phase = Phase::Playing;
    assert!(!e.skip_current(token, 1, 1));
    e.snapshot.phase = Phase::Select;
    e.snapshot.mode = Some(iidx::DP);
    e.snapshot.can_skip = false;
    assert!(!e.skip_current(token, 1, 1));
    e.snapshot.mode = Some(iidx::SP);
    e.snapshot.can_skip = true;
    e.config.controls.skip_enabled = false;
    assert!(!e.skip_current(token, 1, 1));
    assert_eq!(e.current.as_ref().unwrap().request.token, token);
}

#[test]
fn controls_defaults_and_window_validation() {
    let mut c: Config = toml::from_str("").unwrap();
    assert!(c.controls.skip_enabled);
    assert_eq!(c.controls.double_tap_ms, 400);
    for ms in [0, 99, 2001] {
        c.controls.double_tap_ms = ms;
        assert!(c.validate().is_err());
    }
    for ms in [100, 400, 2000] {
        c.controls.double_tap_ms = ms;
        c.validate().unwrap();
    }
}

#[test]
fn all_ten_difficulty_tokens() {
    for mode in ["SP", "DP"] {
        for (i, d) in ["B", "N", "H", "A", "L"].iter().enumerate() {
            let token = format!("{mode}{d}");
            let c = iidx::parse_chart(&token.to_lowercase()).unwrap();
            assert_eq!(c.label(), token);
            assert_eq!(iidx::difficulty(c).unwrap(), i as u32);
        }
    }
    for bad in ["A", "SP", "DPAx", "SPX", "DPＡ"] {
        assert!(iidx::parse_chart(bad).is_none());
    }
}
#[test]
fn fuzzy_uses_native_index_ids_and_deduplicates_keyword_matches() {
    let mut e = engine();
    let entries = vec![
        (3, "meimei".into()),
        (3, "ＭＥＩＭＥＩ".into()),
        (3, "mei alternate".into()),
        (999, "nonexistent".into()),
    ];
    assert_eq!(e.catalog.set_search_index(&entries), 2);
    let found = e.catalog.search("mem", 5);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, 3);
    assert!(e.catalog.search("nonexistent", 5).is_empty());
    chat(&mut e, "1", "点歌 mem SPA", 0);
    assert_eq!(e.queue[0].song.id, 3);
    assert_eq!(e.queue[0].chart.unwrap().label(), "SPA");
    e.catalog.set_search_index(&[]);
    assert!(e.catalog.search("mem", 5).is_empty());
    assert_eq!(e.catalog.search("冥", 5)[0].id, 3);
}
#[test]
fn command_boundary_and_optional_difficulty() {
    let mut e = engine();
    chat(&mut e, "1", "普通聊天 AA", 0);
    chat(&mut e, "1", "点歌AA", 0);
    assert!(e.queue.is_empty());
    chat(&mut e, "1", "点歌 冥", 1);
    assert_eq!(e.queue[0].chart, None);
    assert_eq!(e.queue[0].mode, iidx::SP);
    chat(&mut e, "2", "点歌 雪月花 spa", 2);
    assert_eq!(e.queue[1].chart.unwrap().label(), "SPA");
}
#[test]
fn choices_are_per_sender_and_exact_title_still_has_candidates() {
    let mut e = engine();
    chat(&mut e, "123456", "点歌 AA SPA", 0);
    chat(&mut e, "654321", "点歌 AA", 0);
    assert_eq!(e.pending["123456"].songs[0].title, "AA");
    chat(&mut e, "stranger", "1", 1);
    assert!(e.queue.is_empty());
    chat(&mut e, "123456", "2", 2);
    assert_eq!(e.queue[0].song.title, "AA -rebuild-");
    assert_eq!(e.queue[0].chart.unwrap().label(), "SPA");
    assert!(e.pending.contains_key("654321"));
    chat(&mut e, "654321", "1", 3);
    assert_eq!(e.queue[1].song.title, "AA");
}
#[test]
fn pending_replaced_timeout_and_invalid_selection() {
    let mut e = engine();
    chat(&mut e, "1", "点歌 AA", 0);
    chat(&mut e, "1", "9", 1);
    assert!(e.pending.contains_key("1"));
    chat(&mut e, "1", "点歌 AA SPA", 5);
    assert_eq!(e.pending["1"].until, 65);
    chat(&mut e, "1", "1", 65);
    assert!(e.pending.is_empty());
    assert!(e.queue.is_empty());
    chat(&mut e, "1", "点歌 AA", 70);
    chat(&mut e, "1", "点歌 冥", 71);
    assert!(e.pending.is_empty());
    assert_eq!(e.queue.len(), 1);
}
#[test]
fn mode_and_nonexistent_chart_rejected() {
    let mut e = engine();
    chat(&mut e, "1", "点歌 冥 DPA", 0);
    chat(&mut e, "1", "点歌 冥 SPB", 0);
    assert!(e.queue.is_empty());
    chat(&mut e, "1", "点歌 AA", 0);
    e.observe(
        Snapshot {
            mode: Some(iidx::DP),
            ..e.snapshot
        },
        1,
    );
    chat(&mut e, "1", "1", 2);
    assert!(e.queue.is_empty());
    chat(&mut e, "1", "点歌 冥 DPL", 3);
    assert_eq!(e.queue.len(), 1);
}
#[test]
fn cooldown_only_on_acceptance() {
    let mut e = engine();
    e.config.requests.cooldown_seconds = 300;
    chat(&mut e, "1", "点歌 no-such-song", 0);
    chat(&mut e, "1", "点歌 冥", 1);
    assert_eq!(e.queue.len(), 1);
    chat(&mut e, "1", "点歌 雪月花", 300);
    assert_eq!(e.queue.len(), 1);
    chat(&mut e, "1", "点歌 雪月花", 301);
    assert_eq!(e.queue.len(), 2);
}
#[test]
fn capacity_rechecked_when_resolving_pending_choice() {
    let mut e = engine();
    e.config.requests.queue_capacity = 1;
    chat(&mut e, "1", "点歌 AA", 0);
    chat(&mut e, "2", "点歌 冥", 0);
    chat(&mut e, "1", "1", 1);
    assert_eq!(e.queue.len(), 1);
    assert_eq!(e.queue[0].user, "2");
}
#[test]
fn queue_is_popped_only_after_success_and_does_not_drain() {
    let mut e = engine();
    chat(&mut e, "1", "点歌 冥", 0);
    chat(&mut e, "2", "点歌 雪月花", 0);
    let j = e.next_jump(1).unwrap();
    assert_eq!(e.queue.len(), 2);
    assert!(e.next_jump(1).is_none());
    e.jump_result(j.request.token, None, 2);
    assert_eq!(e.queue.len(), 2);
    let j = e.next_jump(3).unwrap();
    e.jump_result(j.request.token + 99, Some(Ok(())), 3);
    assert_eq!(e.queue.len(), 2);
    e.jump_result(j.request.token, Some(Ok(())), 3);
    assert_eq!(e.queue.len(), 1);
    assert!(e.next_jump(4).is_none());
    assert!(e.next_jump(602).is_none());
    assert!(e.next_jump(603).is_some());
}
#[test]
fn no_jumps_in_gameplay_or_menus_then_advance_on_return() {
    let mut e = engine();
    chat(&mut e, "1", "点歌 冥", 0);
    chat(&mut e, "2", "点歌 雪月花", 0);
    jump(&mut e, 1);
    e.observe(
        Snapshot {
            phase: Phase::Playing,
            ..e.snapshot
        },
        2,
    );
    assert!(e.current.is_none());
    assert!(e.next_jump(1000).is_none());
    e.observe(
        Snapshot {
            phase: Phase::Other,
            ..e.snapshot
        },
        1001,
    );
    assert!(e.next_jump(1001).is_none());
    e.observe(
        Snapshot {
            phase: Phase::Select,
            epoch: 2,
            ..e.snapshot
        },
        1002,
    );
    assert_eq!(e.next_jump(1002).unwrap().request.song.title, "雪月花");
}
#[test]
fn timeout_while_outside_selection_never_jumps() {
    let mut e = engine();
    chat(&mut e, "1", "点歌 冥", 0);
    chat(&mut e, "2", "点歌 雪月花", 0);
    jump(&mut e, 0);
    e.observe(
        Snapshot {
            phase: Phase::Other,
            ..e.snapshot
        },
        600,
    );
    assert!(e.current.is_none());
    assert!(e.next_jump(600).is_none());
}
#[test]
fn unavailable_native_chart_skipped_without_becoming_current() {
    let mut e = engine();
    chat(&mut e, "1", "点歌 冥", 0);
    let j = e.next_jump(0).unwrap();
    e.jump_result(j.request.token, Some(Err("locked".into())), 1);
    assert!(e.queue.is_empty());
    assert!(e.current.is_none());
}

#[test]
fn gameplay_clears_a_jump_acknowledged_after_the_phase_transition() {
    let mut e = engine();
    chat(&mut e, "1", "点歌 冥", 0);
    let j = e.next_jump(0).unwrap();
    let playing = Snapshot {
        phase: Phase::Playing,
        ..e.snapshot
    };
    e.observe(playing, 1);
    e.jump_result(j.request.token, Some(Ok(())), 2);
    assert!(e.current.is_some());
    e.observe(playing, 2);
    assert!(e.current.is_none());
    assert!(e.queue.is_empty());
}
#[test]
fn aliases_are_exact_and_fullwidth_search_normalizes() {
    let aliases = BTreeMap::from([("黑白".into(), "1".into())]);
    let mut c = Catalog::new(vec![song(1, "AA"), song(2, "AA -rebuild-")], &aliases).unwrap();
    assert_eq!(c.search("黑白", 5).len(), 1);
    assert_eq!(c.search("ＡＡ", 5)[0].id, 1);
    assert!(
        Catalog::new(
            vec![song(1, "AA")],
            &BTreeMap::from([("a".into(), "missing".into())])
        )
        .is_err()
    );
}
#[test]
fn separate_obs_contents() {
    let mut e = engine();
    chat(&mut e, "1", "点歌 AA", 0);
    chat(&mut e, "2", "点歌 冥", 0);
    jump(&mut e, 1);
    let (q, i) = e.render(2);
    assert!(q.contains("冥"));
    assert!(!q.contains("rebuild"));
    assert!(i.contains("1. AA\n2. AA -rebuild-"));
    assert!(i.contains("viewer 1 [1]"));
}
#[test]
fn malformed_database_is_rejected() {
    for bytes in [vec![], b"IIDX".to_vec(), vec![0; 64]] {
        assert!(parse_database(&bytes).is_err());
    }
}
#[test]
fn unknown_mode_does_not_guess_sp() {
    let mut e = engine();
    e.snapshot = Snapshot::default();
    chat(&mut e, "1", "点歌 冥", 0);
    assert!(e.queue.is_empty());
}
#[test]
fn legacy_config_is_valid() {
    let c: Config = toml::from_str(include_str!("fixtures/legacy-config.toml")).unwrap();
    c.validate().unwrap();
}
#[test]
fn fuzzy_search_does_not_span_title_and_reading_or_parse_title_operators() {
    let mut a = song(10, "A");
    a.search_terms = vec!["A".into()];
    let mut c = Catalog::new(
        vec![a, song(11, "AA"), song(12, "!Viva!")],
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(
        c.search("AA", 5).iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![11]
    );
    assert_eq!(c.search("!Viva!", 5)[0].id, 12);
}

#[test]
fn database_ignores_unused_lookup_entries_pointing_at_record_zero() {
    let mut data = vec![0u8; 16 + 8 * 4 + 0x7f8];
    data[..4].copy_from_slice(b"IIDX");
    data[4..8].copy_from_slice(&33u32.to_le_bytes());
    data[8..12].copy_from_slice(&1u32.to_le_bytes());
    data[12..16].copy_from_slice(&8u32.to_le_bytes());
    let record = &mut data[48..];
    record[0..2].copy_from_slice(&('冥' as u16).to_le_bytes());
    record[0x67c..0x680].copy_from_slice(&5u32.to_le_bytes());
    record[0x3ef] = 12;
    let songs = parse_database(&data).unwrap();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].id, 5);
    assert_eq!(songs[0].title, "冥");
    data[16 + 5 * 4..16 + 6 * 4].copy_from_slice(&2u32.to_le_bytes());
    assert!(parse_database(&data).is_err());
}

#[test]
fn invalid_configuration_does_not_expose_secret_values() {
    let path = std::env::temp_dir().join(format!("chart-requester-{}.toml", uuid::Uuid::new_v4()));
    std::fs::write(&path, "[bilibili]\nauth_code = 12345678901234\n").unwrap();
    let error = format!("{:#}", Config::load(&path).unwrap_err());
    assert!(!error.contains("12345678901234"));
    assert!(error.contains("byte"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn diagnostics_distinguish_ignored_pending_rejected_and_enqueued_messages() {
    let mut e = engine();
    let send = |e: &mut Engine, user: &str, text: &str| {
        e.chat(
            Chat {
                user: user.into(),
                name: "viewer".into(),
                text: text.into(),
            },
            0,
        )
    };
    assert_eq!(send(&mut e, "1", "hello"), "ignored_not_a_request");
    assert_eq!(send(&mut e, "1", "点歌 AA"), "awaiting_selection");
    assert_eq!(send(&mut e, "2", "1"), "ignored_no_pending_selection");
    assert_eq!(send(&mut e, "1", "99"), "rejected_invalid_selection");
    assert_eq!(send(&mut e, "1", "1"), "enqueued");
    assert_eq!(e.activity(), "ready_to_jump");
    assert!(e.take_diagnostics().any(|s| s.contains("已加入队列")));
    let j = e.next_jump(0).unwrap();
    assert_eq!(e.activity(), "waiting_for_game_ack");
    e.jump_result(j.request.token, Some(Ok(())), 1);
    assert_eq!(e.activity(), "waiting_for_play_or_timeout");
}
