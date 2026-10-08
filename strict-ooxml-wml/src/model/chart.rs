//! Cached chart data read from a DrawingML chart part (`c:chartSpace`).
//!
//! A `c:chart` reference in the document points at a separate chart part. The
//! parser follows that reference and keeps what a renderer needs to draw the
//! chart from the producer's *cached* values (`c:strCache`, `c:numCache`): the
//! chart type, the title, the category labels, the series values and colours
//! and the legend. Formulas, axes options, data labels, trendlines and the
//! embedded workbook are not modelled; what was seen but not kept is listed in
//! [`ChartData::unsupported`] and in the document's support report.
//!
//! Every value is finite: the parser drops `NaN`, infinities and unparsable
//! numbers as missing points, which is what makes the manual [`Eq`] below
//! honest.

/// Most series kept per chart; the rest are dropped and reported.
pub const MAX_CHART_SERIES: usize = 256;

/// Most points (categories) kept per series; later indices are dropped and
/// reported.
pub const MAX_CHART_POINTS: usize = 4096;

/// How the bars of a bar chart are grouped (`c:grouping`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BarGrouping {
    /// Bars side by side (`clustered`, also `standard`).
    #[default]
    Clustered,
    /// Bars stacked on each other (`stacked`).
    Stacked,
    /// Bars stacked and scaled to 100 % (`percentStacked`).
    PercentStacked,
}

/// The kind of chart, from the first chart-type element of `c:plotArea`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChartKind {
    /// `c:barChart` / `c:bar3DChart`.
    Bar {
        /// `c:barDir val="bar"`: bars run horizontally.
        horizontal: bool,
        /// `c:grouping`.
        grouping: BarGrouping,
    },
    /// `c:lineChart` / `c:line3DChart`.
    Line,
    /// `c:areaChart` / `c:area3DChart`.
    Area,
    /// `c:pieChart` / `c:pie3DChart` / `c:ofPieChart` / `c:doughnutChart`.
    Pie {
        /// `c:doughnutChart`: a ring rather than a disc.
        doughnut: bool,
    },
    /// `c:scatterChart` (and `c:bubbleChart`, drawn without bubble sizes).
    Scatter,
    /// Any other chart type, by its element local name (`surfaceChart`...).
    Other(String),
}

/// Where the legend sits (`c:legendPos`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LegendPosition {
    /// Right of the plot area (`r`, the schema default; also `tr`).
    #[default]
    Right,
    /// Left of the plot area (`l`).
    Left,
    /// Above the plot area (`t`).
    Top,
    /// Below the plot area (`b`).
    Bottom,
}

/// One data series (`c:ser`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ChartSeries {
    /// Series name from `c:tx` (cached string or literal `c:v`).
    pub name: Option<String>,
    /// Cached values (`c:val` or, for scatter, `c:yVal`) by point index.
    ///
    /// The length is the highest cached index plus one (never more than
    /// `c:ptCount` or [`MAX_CHART_POINTS`]); an index with no cached point is
    /// `None`, and so is any index past the end.
    pub values: Vec<Option<f64>>,
    /// Cached x values of a scatter series (`c:xVal`), by point index.
    pub x_values: Vec<Option<f64>>,
    /// Explicit series colour as `RRGGBB` (`c:spPr` fill, else its line).
    ///
    /// A scheme colour `accent1`..`accent6` is mapped to the Office default
    /// theme's accent; any other colour is left `None`, and the renderer's
    /// palette applies.
    pub color: Option<String>,
    /// Explicit per-point colours (`c:dPt`), as `(index, RRGGBB)`.
    pub point_colors: Vec<(usize, String)>,
}

impl Eq for ChartSeries {}

/// A chart's cached data (`c:chartSpace/c:chart`).
#[derive(Clone, Debug, PartialEq)]
pub struct ChartData {
    /// Chart type.
    pub kind: ChartKind,
    /// Title text (`c:title` rich text or cached string), if a title is shown.
    pub title: Option<String>,
    /// Category labels from the first series that carries `c:cat`, by index.
    pub categories: Vec<String>,
    /// Data series in document order (at most [`MAX_CHART_SERIES`]).
    pub series: Vec<ChartSeries>,
    /// Whether a legend is shown (`c:legend` present).
    pub legend: bool,
    /// Legend placement.
    pub legend_position: LegendPosition,
    /// Feature ids of what the chart part carries but the model does not
    /// (`c:surfaceChart`, `c:bar3DChart`, `limit.chart_points`...).
    pub unsupported: Vec<String>,
}

impl Eq for ChartData {}

impl ChartData {
    /// Number of category slots: the longer of the labels and any series.
    #[must_use]
    pub fn category_count(&self) -> usize {
        self.series
            .iter()
            .map(|series| series.values.len())
            .chain(std::iter::once(self.categories.len()))
            .max()
            .unwrap_or(0)
    }
}
