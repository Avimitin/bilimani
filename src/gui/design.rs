//! Shared Ouroboros theme for the DLL, standalone preview and render checks.
use egui::{FontDefinitions, FontId, Stroke, TextStyle};
use ouroboros_ui::{Theme, theme::typography, tokens::core as tokens};

pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    typography::register(&mut fonts);
    fonts
}

pub fn style() -> egui::Style {
    let theme = Theme::dark();
    let mut style = egui::Style {
        visuals: egui::Visuals::dark(),
        ..Default::default()
    };
    style.visuals.override_text_color = Some(theme.foreground);
    style.visuals.weak_text_color = Some(theme.muted_foreground);
    style.visuals.panel_fill = theme.background;
    style.visuals.window_fill = theme.background;
    style.visuals.extreme_bg_color = theme.muted;
    style.visuals.faint_bg_color = theme.card;
    style.visuals.window_stroke = Stroke::new(tokens::BORDER_THIN, theme.border);
    style.visuals.window_corner_radius = egui::CornerRadius::same(tokens::RADIUS_LG as u8);
    style.visuals.selection.bg_fill = theme.info_bg;
    style.visuals.selection.stroke = Stroke::new(tokens::BORDER_THIN, theme.foreground);
    for widget in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.fg_stroke = Stroke::new(tokens::BORDER_THIN, theme.foreground);
        widget.bg_fill = theme.muted;
        widget.weak_bg_fill = theme.card;
        widget.bg_stroke = Stroke::new(tokens::BORDER_THIN, theme.border);
        widget.corner_radius = egui::CornerRadius::same(tokens::RADIUS_MD as u8);
    }
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(tokens::BORDER_THIN, theme.border_strong);
    style.visuals.widgets.active.bg_stroke = Stroke::new(tokens::BORDER_FOCUS, theme.ring);
    style.spacing.item_spacing = egui::vec2(tokens::SPACE_2, tokens::SPACE_2);
    style.spacing.button_padding = egui::vec2(tokens::SPACE_3, tokens::SPACE_2);
    style.spacing.interact_size.y = 32.0;
    style.spacing.window_margin = egui::Margin::same(16);
    for (kind, size) in [
        (TextStyle::Body, 15.0),
        (TextStyle::Button, 15.0),
        (TextStyle::Small, 12.0),
        (TextStyle::Heading, 22.0),
    ] {
        style.text_styles.insert(kind, FontId::proportional(size));
    }
    style.visuals.window_shadow = egui::epaint::Shadow::NONE;
    style
}
