use super::*;

impl Menu {
    pub(super) fn live(&mut self, ui: &mut Ui, bridge: &Bridge, view: &View, height: f32) {
        let theme = Theme::get(ui);
        ui.columns(2, |columns| {
            Card::new().sm().show(&mut columns[0], |ui| {
                let top = ui.cursor().top();
                let inner_height = height - 32.0;
                ui.set_min_height(inner_height);
                ui.horizontal(|ui| {
                    Heading::new("实时弹幕").heading().show(ui);
                    Badge::new(view.chats.len().to_string())
                        .secondary()
                        .sm()
                        .show(ui);
                });
                Text::new("直播间的每一条发言").caption().muted().show(ui);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    toggle(ui, &mut self.follow, "跟随最新");
                    if Button::new("")
                        .icon_left(icons::CARET_UP)
                        .icon_only()
                        .ghost()
                        .sm()
                        .show(ui)
                        .on_hover_text("上翻弹幕")
                        .clicked()
                    {
                        self.follow = false;
                        self.chat_offset = (self.chat_offset - 180.0).max(0.0);
                    }
                    if Button::new("")
                        .icon_left(icons::CARET_DOWN)
                        .icon_only()
                        .ghost()
                        .sm()
                        .show(ui)
                        .on_hover_text("下翻弹幕")
                        .clicked()
                    {
                        self.follow = false;
                        self.chat_offset += 180.0;
                    }
                });
                ui.separator();
                let mut history = ScrollArea::vertical()
                    .id_salt("chat-history")
                    .max_height((inner_height - (ui.cursor().top() - top)).max(32.0))
                    .stick_to_bottom(self.follow);
                if !self.follow {
                    history = history.vertical_scroll_offset(self.chat_offset);
                }
                let history = history.show(ui, |ui| {
                    if view.chats.is_empty() {
                        ui.add_space(32.0);
                        Text::new("等待第一条弹幕").muted().show(ui);
                        Text::new("连接直播间后，观众发言会显示在这里")
                            .caption()
                            .muted()
                            .wrap()
                            .show(ui);
                    }
                    for line in &view.chats {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&line.name).color(theme.info).strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.small(
                                        RichText::new(format!(
                                            "{:02}:{:02}",
                                            line.at / 60,
                                            line.at % 60
                                        ))
                                        .color(theme.muted_foreground),
                                    );
                                },
                            );
                        });
                        ui.label(&line.text);
                        ui.add_space(8.0);
                    }
                });
                self.chat_offset = history.state.offset.y;
            });
            Card::new().sm().show(&mut columns[1], |ui| {
                let top = ui.cursor().top();
                let inner_height = height - 32.0;
                ui.set_min_height(inner_height);
                ui.horizontal(|ui| {
                    Heading::new("点歌队列").heading().show(ui);
                    Badge::new(format!(
                        "{} / {}",
                        view.queue.len(),
                        view.config.requests.queue_capacity
                    ))
                    .secondary()
                    .sm()
                    .show(ui);
                });
                Surface::new()
                    .muted()
                    .border_none()
                    .pad(8.0)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        Text::new("当前点歌").caption().muted().show(ui);
                        if let Some(row) = &view.current {
                            ui.label(RichText::new(&row.text).strong());
                            if Button::new("跳过当前")
                                .icon_left(icons::SKIP_FORWARD)
                                .outline()
                                .sm()
                                .enabled(row.removable)
                                .show(ui)
                                .clicked()
                            {
                                self.send(
                                    bridge,
                                    Action::Skip {
                                        token: row.token,
                                        epoch: view.epoch,
                                    },
                                );
                            }
                        } else {
                            ui.label("暂无正在进行的点歌");
                        }
                    });
                Text::new("接下来").caption().muted().show(ui);
                ScrollArea::vertical()
                    .id_salt("menu-queue")
                    .max_height((height * 0.22).max(60.0))
                    .show(ui, |ui| {
                        if view.queue.is_empty() {
                            Text::new("队列为空，等待观众点歌").muted().wrap().show(ui);
                        }
                        for (i, row) in view.queue.iter().enumerate() {
                            ui.push_id(row.token, |ui| {
                                ui.horizontal(|ui| {
                                    Text::new(format!("{:02}", i + 1))
                                        .caption()
                                        .muted()
                                        .show(ui);
                                    if Button::new("")
                                        .icon_left(icons::TRASH)
                                        .icon_only()
                                        .ghost()
                                        .sm()
                                        .enabled(row.removable)
                                        .show(ui)
                                        .on_hover_text("删除这条点歌")
                                        .clicked()
                                    {
                                        self.send(bridge, Action::Remove(row.token));
                                    }
                                    ui.vertical(|ui| {
                                        ui.label(&row.text);
                                        if !row.removable {
                                            Text::new("正在定位…").caption().muted().show(ui);
                                        }
                                    });
                                });
                            });
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    Heading::new("处理日志").heading().show(ui);
                    toggle(ui, &mut self.follow_processing, "最新");
                    if Button::new("")
                        .icon_left(icons::CARET_UP)
                        .icon_only()
                        .ghost()
                        .sm()
                        .show(ui)
                        .on_hover_text("较新记录")
                        .clicked()
                    {
                        self.follow_processing = false;
                        self.processing_offset = (self.processing_offset - 180.0).max(0.0);
                    }
                    if Button::new("")
                        .icon_left(icons::CARET_DOWN)
                        .icon_only()
                        .ghost()
                        .sm()
                        .show(ui)
                        .on_hover_text("较早记录")
                        .clicked()
                    {
                        self.follow_processing = false;
                        self.processing_offset += 180.0;
                    }
                });
                let log = ScrollArea::vertical()
                    .id_salt("processing-log")
                    .max_height((inner_height - (ui.cursor().top() - top)).max(32.0))
                    .vertical_scroll_offset(if self.follow_processing {
                        0.0
                    } else {
                        self.processing_offset
                    })
                    .show(ui, |ui| {
                        if view.processing.is_empty() {
                            Text::new("收到弹幕后显示处理结果")
                                .caption()
                                .muted()
                                .wrap()
                                .show(ui);
                        }
                        for line in view.processing.iter().rev() {
                            let color = match line.outcome {
                                "enqueued" => theme.success,
                                "awaiting_selection" => theme.info,
                                s if s.starts_with("rejected_")
                                    || s == "ignored_catalog_not_ready" =>
                                {
                                    theme.warning
                                }
                                _ => theme.muted_foreground,
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.small(
                                    RichText::new(format!(
                                        "{:02}:{:02}",
                                        line.at / 60,
                                        line.at % 60
                                    ))
                                    .color(theme.muted_foreground),
                                );
                                ui.label(&line.name);
                                ui.label(
                                    RichText::new(processing_label(line.outcome)).color(color),
                                );
                            });
                            ui.small(RichText::new(&line.text).color(theme.muted_foreground));
                            ui.add_space(4.0);
                        }
                    });
                self.processing_offset = log.state.offset.y;
            });
        });
    }
}
