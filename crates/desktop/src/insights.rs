use super::*;

impl App {
    pub(super) fn insights(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        if self.insights.is_empty() {
            if self.queries.waiting() {
                kit::skeleton(ui, 6);
            } else {
                kit::empty_state(
                    ui,
                    icons::LIGHTBULB,
                    "No findings in this scope",
                    "Scan development folders or collect more observations. No findings is not a guarantee of system health.",
                );
            }
            return;
        }
        let mut kinds = BTreeMap::<String, usize>::new();
        for insight in &self.insights {
            *kinds.entry(insight.kind.clone()).or_default() += 1;
        }
        ui.horizontal_wrapped(|ui| {
            for (kind, count) in &kinds {
                let (icon, color) = kit::insight_style(&p, kind, "info");
                kit::badge_icon(
                    ui,
                    Some(icon),
                    &format!("{count} · {}", humanize(kind)),
                    color,
                );
            }
            kit::caption(ui, "Largest estimated impact first");
        });
        let insights: Vec<_> = self.insights.iter().take(100).cloned().collect();
        for insight in &insights {
            self.finding_card(ui, insight, false);
        }
        if self.insights.len() > 100 {
            kit::caption(
                ui,
                "Showing the 100 largest findings. The full list is available through the API.",
            );
        }
    }
}
