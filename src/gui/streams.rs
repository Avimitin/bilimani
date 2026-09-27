use super::*;
use crate::profiles::{self, CardId, GLOBAL, StreamProfile};

impl Menu {
    pub(super) fn create_profile(&mut self, view: &View) {
        let Some(card) = &view.player_card else {
            return;
        };
        if profiles::for_card(&self.draft, Some(card)).is_some() {
            return;
        }
        let mut profile = StreamProfile::new(card.clone());
        // Application credentials may be shared; never copy the previous room.
        profile.bilibili.relay_url = self.draft.bilibili.relay_url.clone();
        profile.bilibili.app_id = self.draft.bilibili.app_id;
        profile.bilibili.access_key_id = self.draft.bilibili.access_key_id.clone();
        profile.bilibili.access_key_secret = self.draft.bilibili.access_key_secret.clone();
        self.profile = profile.id.clone();
        self.draft.profiles.push(profile);
        self.bind_card = Some(card.clone());
        self.page = Page::Bilibili;
        self.delete_profile = false;
        self.feedback = "填写身份码或直播间号后点击「应用并保存」，即会绑定当前登录卡号".into();
    }

    fn add_card(&mut self, card: CardId) -> anyhow::Result<()> {
        anyhow::ensure!(
            profiles::for_card(&self.draft, Some(&card)).is_none(),
            "该卡号已绑定档案，请先解除原绑定"
        );
        let profile = self
            .draft
            .profiles
            .iter_mut()
            .find(|p| p.id == self.profile)
            .ok_or_else(|| anyhow::anyhow!("请选择个人直播档案"))?;
        profile.cards.push(card);
        Ok(())
    }

    pub(super) fn streams(&mut self, ui: &mut Ui, view: &View) {
        Heading::new("直播连接与档案").h2().show(ui);
        let active = view
            .config
            .profiles
            .iter()
            .find(|p| p.id == view.active_profile)
            .map_or("全局档案", |p| p.name.as_str());
        ui.label(format!("当前使用：{active}"));
        if let Some(card) = &view.player_card {
            ui.label(format!("已登录卡号：{}", card.masked()));
            if profiles::for_card(&self.draft, Some(card)).is_none()
                && Button::new("创建新直播间").sm().show(ui).clicked()
            {
                self.create_profile(view);
            }
        } else {
            ui.weak("登录游戏后可为当前卡号创建直播间；游客和未绑定卡号使用全局档案。");
        }
        Text::new(&view.connection.text)
            .caption()
            .muted()
            .wrap()
            .show(ui);
        ui.separator();
        ui.label("编辑档案");
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_value(&mut self.profile, GLOBAL.into(), "全局档案")
                .clicked()
            {
                self.delete_profile = false;
                self.card_input.clear();
            }
            for profile in &self.draft.profiles {
                if ui
                    .selectable_value(&mut self.profile, profile.id.clone(), &profile.name)
                    .clicked()
                {
                    self.delete_profile = false;
                    self.card_input.clear();
                }
            }
        });
        ui.weak("选择档案可编辑设置；实际连接根据登录卡号自动切换。切换档案会清空点歌和弹幕。");
        ui.separator();
        let index = self
            .draft
            .profiles
            .iter()
            .position(|p| p.id == self.profile);
        if let Some(index) = index {
            field(ui, "档案名称", &mut self.draft.profiles[index].name, false);
        } else {
            self.profile = GLOBAL.into();
            ui.weak("全局档案供未绑定直播间的账户使用，退出登录后也会恢复到这里。");
        }
        let connection = if let Some(index) = index {
            &mut self.draft.profiles[index].bilibili
        } else {
            &mut self.draft.bilibili
        };
        connection_fields(ui, connection);
        if let Some(index) = index {
            ui.separator();
            Heading::new("绑定卡号").heading().show(ui);
            let mut remove = None;
            for (i, card) in self.draft.profiles[index].cards.iter().enumerate() {
                ui.push_id(card.as_str(), |ui| {
                    ui.horizontal(|ui| {
                        ui.monospace(card.masked());
                        if view.player_card.as_ref() == Some(card) {
                            ui.weak("当前登录");
                        }
                        if Button::new("解除绑定").ghost().sm().show(ui).clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                self.draft.profiles[index].cards.remove(i);
            }
            if let Some(card) = view
                .player_card
                .as_ref()
                .filter(|c| profiles::for_card(&self.draft, Some(c)).is_none())
                && Button::new("绑定当前登录卡号")
                    .outline()
                    .sm()
                    .show(ui)
                    .clicked()
            {
                match self.add_card(card.clone()) {
                    Ok(()) => self.bind_card = Some(card.clone()),
                    Err(e) => self.feedback = e.to_string(),
                }
            }
            field(ui, "添加其他卡号（16 位）", &mut self.card_input, true);
            ui.weak("填写读卡器或 Spice2x 使用的 16 位卡号。可将多张卡绑定到同一直播间。");
            if Button::new("添加卡号").outline().sm().show(ui).clicked() {
                let result = CardId::parse(&self.card_input).and_then(|card| self.add_card(card));
                match result {
                    Ok(()) => {
                        self.card_input.clear();
                        self.feedback = "卡号已加入草稿，点击「应用并保存」生效".into();
                    }
                    Err(e) => self.feedback = e.to_string(),
                }
            }
            ui.separator();
            if self.delete_profile {
                ui.label("删除此档案后，绑定卡号将使用全局档案。保存后生效。");
                ui.horizontal(|ui| {
                    if Button::new("确认删除档案").sm().show(ui).clicked() {
                        self.draft.profiles.remove(index);
                        self.profile = GLOBAL.into();
                        self.delete_profile = false;
                    }
                    if Button::new("取消").ghost().sm().show(ui).clicked() {
                        self.delete_profile = false;
                    }
                });
            } else if Button::new("删除此档案").ghost().sm().show(ui).clicked() {
                self.delete_profile = true;
            }
        }
    }
}

fn connection_fields(ui: &mut Ui, c: &mut crate::config::Bilibili) {
    toggle(ui, &mut c.enabled, "启用弹幕连接");
    Text::new("连接方式").label().show(ui);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut c.mode, "open_live".into(), "主播身份码");
        ui.selectable_value(&mut c.mode, "web".into(), "直播间网页");
    });
    if c.mode == "open_live" {
        field(ui, "身份码", &mut c.auth_code, true);
        field(ui, "会话接口地址", &mut c.relay_url, false);
        ui.collapsing("自有开放平台应用（直连）", |ui| {
            number(ui, "App ID", &mut c.app_id, 0..=u64::MAX);
            field(ui, "Access Key ID", &mut c.access_key_id, true);
            field(ui, "Access Key Secret", &mut c.access_key_secret, true);
        });
    } else {
        number(ui, "直播间号", &mut c.room_id, 0..=u64::MAX);
        field(ui, "SESSDATA", &mut c.sessdata, true);
        field(ui, "buvid3", &mut c.buvid3, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(menu: &mut Menu, ctx: &egui::Context, bridge: &Bridge, event: Option<Navigation>) {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        menu.controller_input(ctx, &mut input, event);
        let _ = ctx.run_ui(input, |root| menu.show(root.ctx(), bridge));
    }
    #[test]
    fn controller_creates_a_login_bound_draft_and_busy_channel_keeps_it_editable() {
        let mut view = View::new(Config::default());
        let card = CardId::parse("E0040123456789AB").unwrap();
        view.player_card = Some(card.clone());
        let (bridge, rx) = Bridge::new(view.clone(), design::fonts());
        let ctx = egui::Context::default();
        ctx.set_fonts(design::fonts());
        ctx.set_global_style(design::style());
        let mut menu = Menu::new(&view);
        bridge.visible.store(true, Ordering::Release);
        for _ in 0..3 {
            frame(&mut menu, &ctx, &bridge, None);
        }
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Up)); // Close button.
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Up)); // Create room.
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Confirm));
        assert!(menu.page == Page::Bilibili);
        assert_eq!(menu.draft.profiles.len(), 1);
        assert_eq!(menu.draft.profiles[0].cards, std::slice::from_ref(&card));
        assert_eq!(menu.bind_card, Some(card.clone()));
        assert!(bridge.snapshot().config.profiles.is_empty());
        assert!(rx.try_recv().is_err()); // Only a draft until Save.
        menu.draft.profiles[0].bilibili.auth_code = "ui-test-room".into();
        menu.add_card(CardId::parse("E0040123456789CD").unwrap())
            .unwrap();
        assert!(menu.add_card(card.clone()).is_err());
        for _ in 0..8 {
            bridge
                .commands
                .try_send(Command {
                    id: 900,
                    action: Action::Reload,
                })
                .unwrap();
        }
        menu.send(
            &bridge,
            Action::Apply {
                config: Box::new(menu.draft.clone()),
                revision: 0,
                bind_card: menu.bind_card.clone(),
            },
        );
        assert!(menu.pending.is_none());
        assert!(menu.feedback.contains("忙"));
        assert_eq!(menu.draft.profiles[0].cards.len(), 2);
        while rx.try_recv().is_ok() {}
        menu.send(
            &bridge,
            Action::Apply {
                config: Box::new(menu.draft.clone()),
                revision: 0,
                bind_card: menu.bind_card.clone(),
            },
        );
        let command = rx.try_recv().unwrap();
        assert!(
            matches!(command.action, Action::Apply { bind_card: Some(ref c), .. } if c == &card)
        );
        view.reply = (command.id, "未应用：登录卡号已变化".into());
        view.player_card = None;
        bridge.publish(view.clone());
        frame(&mut menu, &ctx, &bridge, None);
        assert!(menu.pending.is_none());
        assert!(menu.feedback.contains("登录卡号已变化"));
        assert_eq!(menu.draft.profiles.len(), 1); // Failed save retains the editable draft.
        menu.reset(&view);
        assert!(menu.draft.profiles.is_empty() && menu.bind_card.is_none());
    }
}
