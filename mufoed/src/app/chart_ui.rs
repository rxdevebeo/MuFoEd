//! The chart dialog: the kind of chart, its title and legend, and its data
//! as a small table, with a preview drawn as the document draws charts.

use eframe::egui::{self, vec2, Color32, RichText, Stroke};
use strict_ooxml::model::chart::{BarGrouping, ChartData, ChartKind, ChartSeries, LegendPosition};

use super::{Action, App};
use crate::worker::Request;

/// The kinds the dialog offers: locale key and model kind.
const KINDS: [(&str, ChartKind); 6] = [
    (
        "chart.column",
        ChartKind::Bar {
            horizontal: false,
            grouping: BarGrouping::Clustered,
        },
    ),
    (
        "chart.bar",
        ChartKind::Bar {
            horizontal: true,
            grouping: BarGrouping::Clustered,
        },
    ),
    ("chart.line", ChartKind::Line),
    ("chart.area", ChartKind::Area),
    ("chart.pie", ChartKind::Pie { doughnut: false }),
    ("chart.doughnut", ChartKind::Pie { doughnut: true }),
];

/// What the dialog edits.
#[derive(Clone)]
pub(crate) struct ChartDialog {
    kind: usize,
    grouping: BarGrouping,
    title: String,
    legend: bool,
    legend_position: LegendPosition,
    categories: Vec<String>,
    /// Each series: its name and its values as typed.
    series: Vec<(String, Vec<String>)>,
    /// It replaces the selected chart.
    replace: bool,
    /// The data the preview shows, and the preview.
    shown: Option<ChartData>,
    preview: Option<egui::TextureHandle>,
}

impl std::fmt::Debug for ChartDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartDialog")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(String::new, |v| {
        let text = format!("{v}");
        text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
    })
}

impl ChartDialog {
    /// A dialog for `data`, or for a sample chart.
    pub(crate) fn new(data: Option<ChartData>, sample: (&[String], &[String])) -> Self {
        let replace = data.is_some();
        let data = data.unwrap_or_else(|| ChartData {
            kind: ChartKind::Bar {
                horizontal: false,
                grouping: BarGrouping::Clustered,
            },
            title: None,
            categories: sample.0.to_vec(),
            series: sample
                .1
                .iter()
                .enumerate()
                .map(|(index, name)| ChartSeries {
                    name: Some(name.clone()),
                    values: [4.3, 2.5, 3.5, 4.5]
                        .iter()
                        .map(|v| Some(v + f64::from(u8::try_from(index).unwrap_or(0)) * 0.7))
                        .collect(),
                    ..ChartSeries::default()
                })
                .collect(),
            legend: true,
            legend_position: LegendPosition::Bottom,
            unsupported: Vec::new(),
        });
        let (kind, grouping) = match &data.kind {
            ChartKind::Bar {
                horizontal,
                grouping,
            } => (usize::from(*horizontal), *grouping),
            ChartKind::Line => (2, BarGrouping::Clustered),
            ChartKind::Area => (3, BarGrouping::Clustered),
            ChartKind::Pie { doughnut } => (4 + usize::from(*doughnut), BarGrouping::Clustered),
            ChartKind::Scatter | ChartKind::Other(_) => (0, BarGrouping::Clustered),
        };
        let count = data.category_count().max(1);
        let mut categories = data.categories.clone();
        categories.resize(count, String::new());
        Self {
            kind,
            grouping,
            title: data.title.clone().unwrap_or_default(),
            legend: data.legend,
            legend_position: data.legend_position,
            categories,
            series: data
                .series
                .iter()
                .map(|series| {
                    let mut values: Vec<String> =
                        series.values.iter().map(|v| number(*v)).collect();
                    values.resize(count, String::new());
                    (series.name.clone().unwrap_or_default(), values)
                })
                .collect(),
            replace,
            shown: None,
            preview: None,
        }
    }

    /// The chart the dialog describes.
    fn data(&self) -> ChartData {
        let mut kind = KINDS
            .get(self.kind)
            .map_or(ChartKind::Line, |(_, kind)| kind.clone());
        if let ChartKind::Bar { grouping, .. } = &mut kind {
            *grouping = self.grouping;
        }
        ChartData {
            kind,
            title: Some(self.title.trim().to_owned()).filter(|t| !t.is_empty()),
            categories: self.categories.clone(),
            series: self
                .series
                .iter()
                .map(|(name, values)| ChartSeries {
                    name: Some(name.clone()).filter(|n| !n.trim().is_empty()),
                    values: values
                        .iter()
                        .map(|v| {
                            v.trim()
                                .replace(',', ".")
                                .parse::<f64>()
                                .ok()
                                .filter(|v| v.is_finite())
                        })
                        .collect(),
                    ..ChartSeries::default()
                })
                .collect(),
            legend: self.legend,
            legend_position: self.legend_position,
            unsupported: Vec::new(),
        }
    }
}

impl App {
    /// Draws the chart dialog; `false` once it closes.
    pub(crate) fn chart_dialog_ui(
        &mut self,
        ctx: &egui::Context,
        dialog: &mut ChartDialog,
        actions: &mut Vec<Action>,
    ) -> bool {
        let t = |key: &str| self.i18n.t(key).to_owned();
        let data = dialog.data();
        if dialog.shown.as_ref() != Some(&data) {
            let image = crate::formula::preview_inline(strict_ooxml::model::Inline::Drawing(
                strict_ooxml::edit::chart_drawing(
                    "rIdPreview",
                    std::sync::Arc::new(data.clone()),
                    strict_ooxml::model::Emu(4_572_000),
                    strict_ooxml::model::Emu(2_743_200),
                    1,
                    "Chart",
                ),
            ));
            dialog.preview = image.map(|(w, h, rgba)| {
                ctx.load_texture(
                    "chart-preview",
                    egui::ColorImage::from_rgba_premultiplied([w, h], &rgba),
                    egui::TextureOptions::LINEAR,
                )
            });
            dialog.shown = Some(data.clone());
        }
        let mut put = false;
        let response = crate::dialog::modal(ctx, "chart").show(ctx, |ui| {
            crate::dialog::title(ui, &t("chart.title"));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(420.0);
                    ui.horizontal_wrapped(|ui| {
                        for (index, (key, _)) in KINDS.iter().enumerate() {
                            ui.selectable_value(&mut dialog.kind, index, t(key));
                        }
                    });
                    if dialog.kind < 2 {
                        ui.horizontal(|ui| {
                            for (grouping, key) in [
                                (BarGrouping::Clustered, "chart.clustered"),
                                (BarGrouping::Stacked, "chart.stacked"),
                                (BarGrouping::PercentStacked, "chart.percent"),
                            ] {
                                ui.radio_value(&mut dialog.grouping, grouping, t(key));
                            }
                        });
                    }
                    crate::dialog::form("chart-form").show(ui, |ui| {
                        ui.label(t("chart.heading"));
                        ui.add(egui::TextEdit::singleline(&mut dialog.title).desired_width(260.0));
                        ui.end_row();
                        ui.checkbox(&mut dialog.legend, t("chart.legend"));
                        ui.add_enabled_ui(dialog.legend, |ui| {
                            ui.horizontal(|ui| {
                                for (position, key) in [
                                    (LegendPosition::Bottom, "chart.bottom"),
                                    (LegendPosition::Right, "chart.right"),
                                    (LegendPosition::Top, "chart.top"),
                                    (LegendPosition::Left, "chart.left"),
                                ] {
                                    ui.radio_value(&mut dialog.legend_position, position, t(key));
                                }
                            });
                        });
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new(t("chart.data")).strong());
                    egui::ScrollArea::both().max_height(220.0).show(ui, |ui| {
                        egui::Grid::new("chart-data")
                            .spacing(vec2(4.0, 4.0))
                            .show(ui, |ui| {
                                ui.label("");
                                for (name, _) in &mut dialog.series {
                                    ui.add(egui::TextEdit::singleline(name).desired_width(80.0));
                                }
                                ui.end_row();
                                for (row, category) in dialog.categories.iter_mut().enumerate() {
                                    ui.add(
                                        egui::TextEdit::singleline(category).desired_width(90.0),
                                    );
                                    for (_, values) in &mut dialog.series {
                                        if let Some(value) = values.get_mut(row) {
                                            ui.add(
                                                egui::TextEdit::singleline(value)
                                                    .desired_width(80.0)
                                                    .horizontal_align(egui::Align::RIGHT),
                                            );
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        if ui.small_button(t("chart.add_row")).clicked() {
                            dialog.categories.push(String::new());
                            for (_, values) in &mut dialog.series {
                                values.push(String::new());
                            }
                        }
                        if ui
                            .add_enabled(
                                dialog.categories.len() > 1,
                                egui::Button::new(t("chart.remove_row")).small(),
                            )
                            .clicked()
                        {
                            dialog.categories.pop();
                            for (_, values) in &mut dialog.series {
                                values.pop();
                            }
                        }
                        if ui.small_button(t("chart.add_series")).clicked() {
                            let name = self
                                .i18n
                                .tf("chart.series_n", &[("n", &(dialog.series.len() + 1))]);
                            dialog
                                .series
                                .push((name, vec![String::new(); dialog.categories.len()]));
                        }
                        if ui
                            .add_enabled(
                                dialog.series.len() > 1,
                                egui::Button::new(t("chart.remove_series")).small(),
                            )
                            .clicked()
                        {
                            dialog.series.pop();
                        }
                    });
                });
                let frame = egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, Color32::from_gray(200)))
                    .corner_radius(4)
                    .inner_margin(8);
                frame.show(ui, |ui| {
                    ui.set_min_size(vec2(360.0, 240.0));
                    if let Some(texture) = &dialog.preview {
                        let size = texture.size_vec2();
                        let scale = (360.0 / size.x).min(240.0 / size.y).min(1.0);
                        ui.vertical_centered(|ui| {
                            ui.add(egui::Image::new((texture.id(), size * scale)));
                        });
                    }
                });
            });
            crate::dialog::buttons(ui, |ui| {
                let label = if dialog.replace {
                    t("formula.update")
                } else {
                    t("button.insert")
                };
                if crate::dialog::primary(ui, &label).clicked() {
                    put = true;
                    ui.close();
                }
                if crate::dialog::button(ui, &t("button.cancel")).clicked() {
                    ui.close();
                }
            });
        });
        if put {
            actions.push(Action::Doc(Request::PutChart(Box::new(dialog.data()))));
        }
        !(put || response.should_close())
    }
}
