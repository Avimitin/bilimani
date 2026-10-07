use super::*;

impl Menu {
    pub(super) fn live(
        &mut self,
        ui: &mut Ui,
        bridge: &Bridge,
        view: &View,
        height: f32,
        focus_queue: bool,
    ) {
        ui.columns(2, |columns| {
            self.request_queue(&mut columns[0], bridge, view, height, focus_queue);
            self.chat_history(&mut columns[1], view, height);
        });
    }

    fn chat_history(&mut self, ui: &mut Ui, view: &View, height: f32) {
        let theme = Theme::get(ui);
        Card::new().sm().show(ui, |ui| {
            let top = ui.cursor().top();
            let inner_height = height - 32.0;
            ui.set_min_height(inner_height);
            ui.horizontal(|ui| {
                Heading::new("实时弹幕").heading().show(ui);
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
                .auto_shrink([false, false])
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
                ui.spacing_mut().item_spacing.y = 6.0;
                for line in &view.chats {
                    egui::Frame::new()
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            theme.muted_foreground.gamma_multiply(0.55),
                        ))
                        .corner_radius(5)
                        .inner_margin(egui::Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.spacing_mut().interact_size.y = 0.0;
                            let mut message = egui::text::LayoutJob::default();
                            for text in [
                                RichText::new(format!("{}：", line.name))
                                    .color(theme.info)
                                    .strong(),
                                RichText::new(&line.text),
                            ] {
                                text.append_to(
                                    &mut message,
                                    ui.style(),
                                    egui::FontSelection::Default,
                                    egui::Align::Center,
                                );
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                ui.small(
                                    RichText::new(format!(
                                        "{:02}:{:02}",
                                        line.at / 60,
                                        line.at % 60
                                    ))
                                    .color(theme.muted_foreground),
                                );
                                ui.allocate_ui_with_layout(
                                    egui::vec2(ui.available_width(), 0.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.add(egui::Label::new(message).wrap());
                                    },
                                );
                            });
                        });
                }
            });
            self.chat_offset = history.state.offset.y;
        });
    }

    fn request_queue(
        &mut self,
        ui: &mut Ui,
        bridge: &Bridge,
        view: &View,
        height: f32,
        mut focus_queue: bool,
    ) {
        let theme = Theme::get(ui);
        Card::new().sm().show(ui, |ui| {
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
            // Leave room for the log heading, then give most of the remaining
            // space to requests. The log keeps roughly half its old height.
            let queue_height = ((inner_height - (ui.cursor().top() - top) - 48.0) * 0.66).max(60.0);
            ScrollArea::vertical()
                .id_salt("menu-queue")
                .max_height(queue_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if view.queue.is_empty() {
                        Text::new("队列为空，等待观众点歌").muted().wrap().show(ui);
                    }
                    for (i, row) in view.queue.iter().enumerate() {
                        // Keep focus tied to the request when earlier rows disappear.
                        ui.scope_builder(
                            egui::UiBuilder::new().id(egui::Id::new(("queue-row", row.token))),
                            |ui| {
                                ui.horizontal(|ui| {
                                    let width = (ui.available_width() - 40.0).max(32.0);
                                    let select = ui
                                        .add_enabled_ui(row.selectable, |ui| {
                                            ui.add_sized(
                                                egui::vec2(width, 32.0),
                                                egui::Button::new(format!(
                                                    "{:02}  {}{}",
                                                    i + 1,
                                                    row.text,
                                                    if row.removable {
                                                        ""
                                                    } else {
                                                        " · 正在定位…"
                                                    },
                                                ))
                                                .wrap(),
                                            )
                                        })
                                        .inner
                                        .on_hover_text("定位到这首歌（B6 确认）");
                                    if focus_queue && row.selectable {
                                        select.request_focus();
                                        focus_queue = false;
                                    }
                                    if select.has_focus() {
                                        select.scroll_to_me(None);
                                        ui.painter().rect_stroke(
                                            select.rect,
                                            5,
                                            egui::Stroke::new(2.0_f32, theme.ring),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                    if select.clicked() {
                                        self.send(
                                            bridge,
                                            Action::Select {
                                                token: row.token,
                                                epoch: view.epoch,
                                            },
                                        );
                                    }
                                    let remove = Button::new("")
                                        .icon_left(icons::TRASH)
                                        .icon_only()
                                        .ghost()
                                        .sm()
                                        .enabled(row.removable)
                                        .show(ui)
                                        .on_hover_text("删除这条点歌");
                                    if remove.has_focus() {
                                        remove.scroll_to_me(None);
                                    }
                                    if remove.clicked() {
                                        self.send(bridge, Action::Remove(row.token));
                                    }
                                });
                            },
                        );
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
                .auto_shrink([false, false])
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
                            "awaiting_selection" | "selection_page_changed" => theme.info,
                            s if s.starts_with("rejected_") || s == "ignored_catalog_not_ready" => {
                                theme.warning
                            }
                            _ => theme.muted_foreground,
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.small(
                                RichText::new(format!("{:02}:{:02}", line.at / 60, line.at % 60))
                                    .color(theme.muted_foreground),
                            );
                            ui.label(&line.name);
                            ui.label(RichText::new(processing_label(line.outcome)).color(color));
                        });
                        ui.small(RichText::new(&line.text).color(theme.muted_foreground));
                        ui.add_space(4.0);
                    }
                });
            self.processing_offset = log.state.offset.y;
        });
    }
}
