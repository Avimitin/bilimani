use chart_requester::{
    config::Config,
    gui::{QueueRow, View, record_chat, record_processing},
    platforms::Chat,
};

pub fn fixture() -> View {
    let mut view = View::new(Config::default());
    view.config.bilibili.enabled = false;
    view.connection.connected = true;
    view.connection.text = "预览模式 · 模拟弹幕，不连接直播间".into();
    view.room = Some(chart_requester::platforms::RoomInfo {
        room_id: 123456,
        name: "示例主播".into(),
        title: "IIDX 点歌练习 · 今晚继续挑战未通过的曲目，欢迎大家来点歌".into(),
    });
    view.ready = true;
    view.epoch = 1;
    view.config
        .aliases
        .insert("向日葵".into(), "ヒマワリ".into());
    view.config
        .aliases
        .insert("冰淇淋".into(), "STARLIGHT DANCEHALL".into());
    for (i, text) in [
        "晚上好！",
        "点歌 AA SPA",
        "今天的手感怎么样",
        "点歌 ヒマワリ",
        "这首好听",
        "下一首安排冥吗？",
    ]
    .into_iter()
    .enumerate()
    {
        record_chat(
            &mut view.chats,
            &Chat {
                user: i.to_string(),
                name: format!("观众 {}", i + 1),
                text: text.into(),
            },
            80 + i as u64,
        );
        record_processing(
            &mut view.processing,
            &Chat {
                user: i.to_string(),
                name: format!("观众 {}", i + 1),
                text: text.into(),
            },
            match i {
                1 => "awaiting_selection",
                3 => "enqueued",
                _ => "ignored_not_a_request",
            },
            80 + i as u64,
        );
    }
    view.current = Some(QueueRow {
        token: 1,
        text: "AA [SPA] — 观众 2".into(),
        removable: true,
        selectable: false,
    });
    view.queue = vec![
        QueueRow {
            token: 2,
            text: "ヒマワリ [SP] — 观众 4".into(),
            removable: true,
            selectable: true,
        },
        QueueRow {
            token: 3,
            text: "冥 [SPA] — 观众 6".into(),
            removable: true,
            selectable: true,
        },
    ];
    view
}
