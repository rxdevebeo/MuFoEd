//! A chat about one open document.
//!
//! The model reads and proposes. The editor locates the exact places, shows
//! each proposal, and applies a proposal as one undo step. A live model is
//! optional: the loop also runs on a scripted queue, and the ordinary tests
//! never call Ollama.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use eframe::egui;
use serde_json::{json, Value};

use crate::ai::Kind;

/// How many model calls one turn may spend.
pub(crate) const STEP_LIMIT: usize = 8;

/// Who wrote a history entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Instructions for the model.
    System,
    /// The person at the keyboard.
    User,
    /// The model.
    Assistant,
    /// The editor's answer to a tool call.
    Tool,
}

/// One call the model asked for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolCall {
    /// Tool name.
    pub(crate) name: String,
    /// Arguments, as JSON.
    pub(crate) arguments: Value,
}

/// One history entry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Turn {
    /// Who wrote it.
    pub(crate) role: Role,
    /// Text. Empty when the entry only carries tool calls.
    pub(crate) content: String,
    /// Tool name, when `role` is [`Role::Tool`].
    pub(crate) name: String,
    /// Calls, when the model asked for tools.
    pub(crate) calls: Vec<ToolCall>,
}

impl Turn {
    fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            name: String::new(),
            calls: Vec::new(),
        }
    }

    /// A system instruction.
    pub(crate) fn system(text: impl Into<String>) -> Self {
        Self::new(Role::System, text)
    }

    /// The user's message.
    pub(crate) fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, text)
    }

    /// The model's text.
    pub(crate) fn assistant(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, text)
    }

    /// A tool result.
    pub(crate) fn tool(name: impl Into<String>, content: impl Into<String>) -> Self {
        let mut turn = Self::new(Role::Tool, content);
        turn.name = name.into();
        turn
    }
}

/// What the model returned.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ModelReply {
    /// A text answer. The turn stops.
    Text(String),
    /// Tool calls to run.
    Calls(Vec<ToolCall>),
}

/// Why a turn stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The model answered in text.
    Answer,
    /// [`STEP_LIMIT`] calls were spent.
    Limited,
    /// The user cancelled.
    Cancelled,
    /// The model failed. The message is also in the history.
    Failed(String),
}

/// Something that answers a history.
pub(crate) trait Model {
    /// The next reply.
    fn complete(&mut self, history: &[Turn]) -> Result<ModelReply, String>;
}

/// A queue of replies, for tests and for a turn that must not touch the network.
#[derive(Clone, Debug, Default)]
pub(crate) struct Scripted {
    /// Replies, in order.
    pub(crate) replies: Vec<Result<ModelReply, String>>,
    next: usize,
}

impl Model for Scripted {
    fn complete(&mut self, _history: &[Turn]) -> Result<ModelReply, String> {
        let Some(reply) = self.replies.get(self.next) else {
            return Err("no scripted reply".to_owned());
        };
        self.next += 1;
        reply.clone()
    }
}

/// Instructions for the model, in the language the interface speaks.
pub(crate) fn system_prompt(language: &str) -> String {
    let language = if language == "ru" {
        "Russian"
    } else {
        "English"
    };
    format!(
        "You revise one open document by calling tools. Read with outline, \
         read_paragraphs, read_selection, find_all, list_formulas and list_tables \
         before proposing a change. propose queues a change the user applies; it \
         does not change the document, except kind write, which the editor inserts. \
         To add new prose, call propose with kind write and put the whole piece in \
         text, with a blank line between paragraphs. write adds paragraphs and does \
         not replace text that is already there. Do not use report to deliver new \
         prose, and do not claim the piece is in the document unless write was queued. \
         Fragments written as ⟦…⟧ are formulas and pictures: copy every such fragment \
         unchanged. Do not invent character offsets or object numbers. When you are \
         finished, answer in {language}, briefly."
    )
}

/// The tools Ollama is offered.
pub(crate) fn tool_schema() -> Value {
    json!([
        schema_tool("outline", "Heading outline: index, level and text.", json!({"type": "object", "properties": {}})),
        schema_tool("read_paragraphs", "Paragraph text. index is the paragraph's own index.", json!({
            "type": "object",
            "properties": {"start": {"type": "integer"}, "count": {"type": "integer"}},
            "required": ["start", "count"]
        })),
        schema_tool("read_selection", "The selected text, if any.", json!({"type": "object", "properties": {}})),
        schema_tool("find_all", "Every occurrence of a string, as paragraph indexes. Does not change the document.", json!({
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"]
        })),
        schema_tool("list_formulas", "Displayed formulas in document order: id, paragraph, LaTeX, current number.", json!({"type": "object", "properties": {}})),
        schema_tool("list_tables", "Tables as rows of cell text, and which pictures there are.", json!({"type": "object", "properties": {}})),
        schema_tool("propose", "Queue one change. kind is replace, range, forms, underline, kinds, numbers, comment, bookmark, join, split, move, cite, report, table_header, chart, caption or write. write adds the text of a new piece; separate its paragraphs with a blank line.", json!({
            "type": "object",
            "properties": {
                "kind": {"type": "string"},
                "items": {"type": "array"},
                "forms": {"type": "array"},
                "pairs": {"type": "array"},
                "ids": {"type": "array"},
                "index": {"type": "integer"},
                "start": {"type": "integer"},
                "to": {"type": "integer"},
                "level": {"type": "integer"},
                "span": {"type": "string"},
                "text": {"type": "string"},
                "name": {"type": "string"},
                "before": {"type": "string"},
                "formula": {"type": "string"},
                "title": {"type": "string"},
                "old": {"type": "string"},
                "new": {"type": "string"},
                "find": {"type": "string"}
            },
            "required": ["kind"]
        }))
    ])
}

fn schema_tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {"name": name, "description": description, "parameters": parameters}
    })
}

/// The JSON body of one `/api/chat` call.
pub(crate) fn request_body(model: &str, history: &[Turn], num_ctx: u32) -> Value {
    json!({
        "model": model,
        "stream": false,
        "messages": history.iter().map(message_json).collect::<Vec<_>>(),
        "tools": tool_schema(),
        "options": {"temperature": 0.2, "num_ctx": num_ctx},
        "think": false,
    })
}

fn message_json(turn: &Turn) -> Value {
    let role = match turn.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut value = json!({"role": role, "content": turn.content});
    if turn.role == Role::Tool {
        value["name"] = json!(turn.name);
    }
    if !turn.calls.is_empty() {
        value["tool_calls"] = json!(turn
            .calls
            .iter()
            .map(|call| json!({
                "function": {"name": call.name, "arguments": call.arguments}
            }))
            .collect::<Vec<_>>());
    }
    value
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(900)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Posts one chat request and returns Ollama's JSON.
pub(crate) fn post(
    endpoint: &str,
    model: &str,
    history: &[Turn],
    num_ctx: u32,
) -> Result<Value, String> {
    let url = format!("{}/api/chat", endpoint.trim_end_matches('/'));
    let send = |body: Value| -> Result<(u16, Value), String> {
        let mut response = agent()
            .post(&url)
            .header("content-type", "application/json")
            .send(body.to_string())
            .map_err(|error| error.to_string())?;
        let status = response.status().as_u16();
        let value: Value = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))?;
        Ok((status, value))
    };
    let (mut status, mut value) = send(request_body(model, history, num_ctx))?;
    if status != 200 && error_text(&value).contains("think") {
        let mut body = request_body(model, history, num_ctx);
        if let Some(map) = body.as_object_mut() {
            map.remove("think");
        }
        (status, value) = send(body)?;
    }
    if status != 200 {
        return Err(format!("HTTP {status}: {}", error_text(&value)));
    }
    Ok(value)
}

fn error_text(value: &Value) -> String {
    value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// A model that calls Ollama.
pub(crate) struct Remote {
    /// Ollama's address.
    pub(crate) endpoint: String,
    /// Model name.
    pub(crate) model: String,
    /// Measured once, then reused for every step of the turn.
    pub(crate) num_ctx: Option<u32>,
}

impl Model for Remote {
    fn complete(&mut self, history: &[Turn]) -> Result<ModelReply, String> {
        let num_ctx = if let Some(num_ctx) = self.num_ctx {
            num_ctx
        } else {
            let num_ctx = crate::context_window::num_ctx(&self.endpoint, &self.model);
            self.num_ctx = Some(num_ctx);
            num_ctx
        };
        interpret(&post(&self.endpoint, &self.model, history, num_ctx)?)
    }
}

/// Reads a model message: native tool calls, else a JSON tool call in the text.
pub(crate) fn interpret(value: &Value) -> Result<ModelReply, String> {
    let message = value.get("message").unwrap_or(value);
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        if !calls.is_empty() {
            let parsed: Vec<ToolCall> = calls.iter().filter_map(parse_call).collect();
            if parsed.is_empty() {
                return Err("unreadable tool call".to_owned());
            }
            return Ok(ModelReply::Calls(parsed));
        }
    }
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let content = without_thinking(content).trim();
    if content.is_empty() {
        return Err("empty answer".to_owned());
    }
    match calls_in_text(content) {
        Some(calls) if !calls.is_empty() => Ok(ModelReply::Calls(calls)),
        Some(_) => Err(format!("unreadable tool call: {content}")),
        None if content.starts_with('{') || content.starts_with('[') => {
            Err(format!("unreadable tool call: {content}"))
        }
        None => Ok(ModelReply::Text(content.to_owned())),
    }
}

fn without_thinking(text: &str) -> &str {
    match text.find("</think>") {
        Some(end) => text.get(end + "</think>".len()..).unwrap_or_default(),
        None => text,
    }
}

fn parse_call(value: &Value) -> Option<ToolCall> {
    let function = value.get("function").unwrap_or(value);
    let name = function.get("name").and_then(Value::as_str)?.to_owned();
    if name.is_empty() {
        return None;
    }
    Some(ToolCall {
        name,
        arguments: arguments(function.get("arguments")),
    })
}

fn arguments(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or(Value::Null),
        Some(other) => other.clone(),
        None => json!({}),
    }
}

fn calls_in_text(text: &str) -> Option<Vec<ToolCall>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let list = value.as_array().cloned().unwrap_or_else(|| vec![value]);
    if list.iter().all(|item| {
        item.get("tool").and_then(Value::as_str).is_none()
            && item.get("name").and_then(Value::as_str).is_none()
    }) {
        return None;
    }
    Some(
        list.into_iter()
            .filter_map(|item| {
                let name = item
                    .get("tool")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)?
                    .to_owned();
                let arguments = item
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| item.get("payload").cloned().unwrap_or(Value::Null));
                (!name.is_empty()).then_some(ToolCall { name, arguments })
            })
            .collect(),
    )
}

/// Runs the tool loop until the model answers, the step limit, or a cancel.
pub(crate) fn run<M, H>(
    model: &mut M,
    history: &mut Vec<Turn>,
    mut host: H,
    cancel: &AtomicBool,
) -> Stop
where
    M: Model + ?Sized,
    H: FnMut(&str, &Value) -> String,
{
    if !history.iter().any(|turn| turn.role == Role::System) {
        history.insert(0, Turn::system(system_prompt("en")));
    }
    for _ in 0..STEP_LIMIT {
        if cancel.load(Ordering::Relaxed) {
            return Stop::Cancelled;
        }
        match model.complete(history) {
            Ok(ModelReply::Text(text)) => {
                history.push(Turn::assistant(text));
                return Stop::Answer;
            }
            Ok(ModelReply::Calls(calls)) => {
                if calls.is_empty() {
                    let error = "empty tool call".to_owned();
                    history.push(Turn::assistant(error.clone()));
                    return Stop::Failed(error);
                }
                let mut turn = Turn::assistant(String::new());
                turn.calls.clone_from(&calls);
                history.push(turn);
                for call in calls {
                    if cancel.load(Ordering::Relaxed) {
                        return Stop::Cancelled;
                    }
                    let result = host(&call.name, &call.arguments);
                    history.push(Turn::tool(call.name, result));
                }
            }
            Err(error) => {
                history.push(Turn::assistant(error.clone()));
                return Stop::Failed(error);
            }
        }
    }
    history.push(Turn::assistant("step limit"));
    Stop::Limited
}

/// One paragraph the tools can read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Paragraph {
    /// Block index in the body.
    pub(crate) index: usize,
    /// Text, with formulas and pictures kept as `⟦…⟧` fragments.
    pub(crate) text: String,
    /// Outline level, when the paragraph is a heading.
    pub(crate) heading: Option<u8>,
    /// Direct paragraph style id.
    pub(crate) style: String,
}

/// A formula the numbering tool can see.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Formula {
    /// `f1`, `f2`, … in document order. Displayed formulas only.
    pub(crate) id: String,
    /// Block index.
    pub(crate) paragraph: usize,
    /// A displayed equation. An inline formula is recorded and never numbered.
    pub(crate) display: bool,
    /// LaTeX of the body, without a `#` number.
    pub(crate) latex: String,
    /// The current `#` number, when there is one.
    pub(crate) number: Option<String>,
}

/// A table, as cell text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Table {
    /// Rows of cells.
    pub(crate) rows: Vec<Vec<String>>,
    /// The first row repeats as a header.
    pub(crate) header: bool,
}

/// How one run is drawn, so a test can see an underline beside bold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RunMark {
    /// The run's text.
    pub(crate) text: String,
    /// Direct bold.
    pub(crate) bold: bool,
    /// Direct underline.
    pub(crate) underline: bool,
}

/// A comment the snapshot can see.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CommentNote {
    /// Block index of the anchored paragraph.
    pub(crate) paragraph: usize,
    /// The comment's own text.
    pub(crate) text: String,
}

/// A field the snapshot can see.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FieldNote {
    /// The field instruction.
    pub(crate) instruction: String,
    /// The cached result.
    pub(crate) text: String,
}

/// Where a search landed. No character offsets: the editor finds those itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    /// Block index.
    pub(crate) paragraph: usize,
    /// The matched text.
    pub(crate) text: String,
}

/// The document as the tools read it. Built by the worker; never written back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct View {
    /// Body paragraphs.
    pub(crate) paragraphs: Vec<Paragraph>,
    /// The selection, when there is one.
    pub(crate) selection: Option<String>,
    /// Formulas, displayed and inline.
    pub(crate) formulas: Vec<Formula>,
    /// Tables.
    pub(crate) tables: Vec<Table>,
    /// Block indexes of paragraphs that hold a picture.
    pub(crate) pictures: Vec<usize>,
    /// Charts in the body.
    pub(crate) charts: usize,
    /// Comments.
    pub(crate) comments: Vec<CommentNote>,
    /// Fields.
    pub(crate) fields: Vec<FieldNote>,
    /// Direct run marks, one list per paragraph.
    pub(crate) runs: Vec<Vec<RunMark>>,
    /// Caret block index and scalar offset.
    pub(crate) caret: Option<(usize, usize)>,
}

/// One line of a proposal: what it was, what it would become.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    /// The text now.
    pub(crate) before: String,
    /// The text after the change.
    pub(crate) after: String,
}

/// One queued change. Applying it is a single undo step.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Batch {
    /// A short name for the pane.
    pub(crate) title: String,
    /// What the pane shows.
    pub(crate) lines: Vec<Line>,
    /// What the worker applies.
    pub(crate) ops: Vec<Op>,
}

/// A change the worker can apply.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    /// The whole paragraph, if it still reads `old`.
    Replace {
        /// Block index.
        index: usize,
        /// Text the paragraph must still have.
        old: String,
        /// Replacement.
        new: String,
    },
    /// One exact stretch inside a paragraph that still reads `old`.
    Range {
        /// Block index.
        index: usize,
        /// Text the paragraph must still have.
        old: String,
        /// Scalar start.
        start: usize,
        /// Scalar end.
        end: usize,
        /// Replacement.
        new: String,
    },
    /// Word-boundary forms, if the paragraph still reads `old`.
    Forms {
        /// Block index.
        index: usize,
        /// Text the paragraph must still have.
        old: String,
        /// Each surface form and what replaces it.
        pairs: Vec<(String, String)>,
    },
    /// Underline these scalar spans, if the paragraph still reads `old`.
    Underline {
        /// Block index.
        index: usize,
        /// Text the paragraph must still have.
        old: String,
        /// Half-open scalar spans.
        spans: Vec<(usize, usize)>,
    },
    /// A paragraph style.
    Kind {
        /// Block index.
        index: usize,
        /// The kind.
        kind: Kind,
    },
    /// Numbers for these displayed-formula ids, in the order the model named.
    Number {
        /// Ids such as `f2`.
        ids: Vec<String>,
    },
    /// A comment on a span, if the paragraph still reads `old`.
    Comment {
        /// Block index.
        index: usize,
        /// Text the paragraph must still have.
        old: String,
        /// Scalar start.
        start: usize,
        /// Scalar end.
        end: usize,
        /// The comment.
        text: String,
    },
    /// A bookmark on a span, if the paragraph still reads `old`.
    Bookmark {
        /// Block index.
        index: usize,
        /// Text the paragraph must still have.
        old: String,
        /// Scalar start.
        start: usize,
        /// Scalar end.
        end: usize,
        /// Bookmark name.
        name: String,
    },
    /// Join with the next paragraph.
    Join {
        /// Block index of the left paragraph.
        index: usize,
    },
    /// Split a paragraph.
    Split {
        /// Block index.
        index: usize,
        /// Scalar offset.
        at: usize,
    },
    /// Move a run of paragraphs and, when `level` is set, restyle the first.
    Move {
        /// First block index.
        start: usize,
        /// How many blocks.
        count: usize,
        /// Insertion index, in the document as it is.
        to: usize,
        /// New heading level of the first paragraph.
        level: Option<u8>,
    },
    /// A field pointing at a formula's bookmark, if the paragraph still reads `old`.
    Cite {
        /// Block index.
        index: usize,
        /// Text the paragraph must still have.
        old: String,
        /// Formula id (`f1`).
        formula: String,
    },
    /// The first row of the first table repeats as a header.
    TableHeader,
    /// A chart of the first table's numbers.
    Chart {
        /// Chart title.
        title: String,
    },
    /// A caption paragraph after a picture.
    Caption {
        /// Block index of the picture's paragraph.
        index: usize,
        /// Caption text.
        text: String,
    },
    /// New paragraphs, added without replacing text that is already there.
    Write {
        /// Plain paragraphs, in order. None of them contains a line break.
        paragraphs: Vec<String>,
    },
}

/// One row of a consistency report. It changes nothing until it is chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReportItem {
    /// What the pane shows.
    pub(crate) text: String,
    /// Paragraph the row is about.
    pub(crate) paragraph: Option<usize>,
    /// That paragraph's text when the report was made.
    pub(crate) paragraph_text: String,
    /// A replacement for the whole paragraph, when the row can become an edit.
    pub(crate) suggestion: Option<String>,
}

/// What the user did with a report row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReportChoice {
    /// Leave it as a comment.
    Comment,
    /// Turn the suggestion into a replacement.
    Edit,
}

/// Proposals collected while the tools run.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Book {
    /// Queued changes.
    pub(crate) batches: Vec<Batch>,
    /// A report, which is not a change.
    pub(crate) report: Vec<ReportItem>,
}

/// Every occurrence of `query`. The document is not involved.
pub(crate) fn find_all(view: &View, query: &str) -> Vec<Hit> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for paragraph in &view.paragraphs {
        for _ in 0..paragraph.text.matches(query).count() {
            hits.push(Hit {
                paragraph: paragraph.index,
                text: query.to_owned(),
            });
        }
    }
    hits
}

/// Spans where `form` is a whole word. A form inside a longer word is not one.
pub(crate) fn word_spans(text: &str, form: &str) -> Vec<(usize, usize)> {
    if form.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let needle: Vec<char> = form.chars().collect();
    let mut spans = Vec::new();
    let mut index = 0;
    while index + needle.len() <= chars.len() {
        let end = index + needle.len();
        let boundary = (index == 0 || !chars[index - 1].is_alphanumeric())
            && (end == chars.len() || !chars[end].is_alphanumeric());
        if boundary && chars[index..end] == needle[..] {
            spans.push((index, end));
            index = end;
        } else {
            index += 1;
        }
    }
    spans
}

/// Displayed formulas named by the model, numbered in document order from 1.
pub(crate) fn assign_numbers(formulas: &[Formula], ids: &[String]) -> Vec<(String, u32)> {
    let mut chosen: Vec<&Formula> = formulas
        .iter()
        .filter(|formula| formula.display && ids.iter().any(|id| id == &formula.id))
        .collect();
    chosen.sort_by_key(|formula| formula.paragraph);
    chosen
        .into_iter()
        .enumerate()
        .map(|(index, formula)| {
            (
                formula.id.clone(),
                u32::try_from(index).unwrap_or(u32::MAX - 1) + 1,
            )
        })
        .collect()
}

/// How many blocks a section at `start` covers: the heading and the body under it.
pub(crate) fn section_count(paragraphs: &[Paragraph], start: usize) -> usize {
    let Some(level) = paragraphs
        .iter()
        .find(|paragraph| paragraph.index == start)
        .and_then(|paragraph| paragraph.heading)
    else {
        return 1;
    };
    let mut count = 1;
    for paragraph in paragraphs {
        if paragraph.index != start + count {
            continue;
        }
        if paragraph.heading.is_some_and(|next| next <= level) {
            break;
        }
        count += 1;
    }
    count
}

/// Moves that take `count` blocks from `start` and insert them before `to`.
///
/// Addresses are what the editor expects when each move sees the document the
/// previous move left: a later destination stays put because removal adjusts
/// it, and an earlier destination walks forward with the inserted blocks.
pub(crate) fn plan_moves(start: usize, count: usize, to: usize) -> Vec<(usize, usize)> {
    if count == 0 || (to > start && to < start + count) {
        return Vec::new();
    }
    if to > start {
        return vec![(start, to); count];
    }
    (0..count)
        .map(|index| (start + index, to + index))
        .collect()
}

/// Runs one tool. Unknown names and a proposal the view cannot carry become
/// text in the history; `book` gains a batch only for a proposal that has work.
pub(crate) fn tool(view: &View, book: &mut Book, name: &str, args: &Value) -> String {
    match name {
        "outline" => outline(view),
        "read_paragraphs" => read_paragraphs(view, args),
        "read_selection" => json!({"selection": view.selection}).to_string(),
        "find_all" => {
            let query = string(args, "query");
            let hits: Vec<Value> = find_all(view, &query)
                .into_iter()
                .map(|hit| json!({"paragraph": hit.paragraph, "text": hit.text}))
                .collect();
            json!({"hits": hits}).to_string()
        }
        "list_formulas" => formulas(view),
        "list_tables" => tables(view),
        "propose" => propose(view, book, args),
        _ => json!({"error": "unknown tool", "name": name}).to_string(),
    }
}

fn outline(view: &View) -> String {
    let items: Vec<Value> = view
        .paragraphs
        .iter()
        .filter_map(|paragraph| {
            paragraph.heading.map(|level| {
                json!({
                    "index": paragraph.index,
                    "level": level,
                    "text": paragraph.text,
                })
            })
        })
        .collect();
    json!({"outline": items}).to_string()
}

fn read_paragraphs(view: &View, args: &Value) -> String {
    let start = usize_field(args, "start").unwrap_or(0);
    let count = usize_field(args, "count").unwrap_or(1).clamp(1, 40);
    let items: Vec<Value> = view
        .paragraphs
        .iter()
        .filter(|paragraph| paragraph.index >= start)
        .take(count)
        .map(|paragraph| {
            json!({
                "index": paragraph.index,
                "text": paragraph.text,
                "heading": paragraph.heading,
            })
        })
        .collect();
    json!({"paragraphs": items}).to_string()
}

fn formulas(view: &View) -> String {
    let items: Vec<Value> = view
        .formulas
        .iter()
        .filter(|formula| formula.display)
        .map(|formula| {
            json!({
                "id": formula.id,
                "paragraph": formula.paragraph,
                "latex": formula.latex,
                "number": formula.number,
            })
        })
        .collect();
    json!({"formulas": items}).to_string()
}

fn tables(view: &View) -> String {
    json!({
        "tables": view.tables.iter().map(|table| &table.rows).collect::<Vec<_>>(),
        "pictures": view.pictures,
    })
    .to_string()
}

fn propose(view: &View, book: &mut Book, args: &Value) -> String {
    let kind = string(args, "kind");
    let mut batch = Batch {
        title: kind.clone(),
        lines: Vec::new(),
        ops: Vec::new(),
    };
    let mut skipped = 0_u32;
    match kind.as_str() {
        "replace" => propose_replace(view, args, &mut batch, &mut skipped),
        "range" => propose_range(view, args, &mut batch, &mut skipped),
        "forms" => propose_forms(view, args, &mut batch),
        "underline" => propose_underline(view, args, &mut batch),
        "kinds" => propose_kinds(view, args, &mut batch, &mut skipped),
        "numbers" => propose_numbers(view, args, &mut batch),
        "comment" => propose_mark(view, args, &mut batch, true, &mut skipped),
        "bookmark" => propose_mark(view, args, &mut batch, false, &mut skipped),
        "join" => propose_join(view, args, &mut batch, &mut skipped),
        "split" => propose_split(view, args, &mut batch, &mut skipped),
        "move" => propose_move(view, args, &mut batch, &mut skipped),
        "cite" => propose_cite(view, args, &mut batch, &mut skipped),
        "report" => return propose_report(view, book, args),
        "table_header" => {
            batch.ops.push(Op::TableHeader);
            batch.lines.push(Line {
                before: String::new(),
                after: "header row".to_owned(),
            });
        }
        "chart" => {
            let title = string(args, "title");
            batch.lines.push(Line {
                before: String::new(),
                after: title.clone(),
            });
            batch.ops.push(Op::Chart { title });
        }
        "caption" => propose_caption(view, args, &mut batch, &mut skipped),
        "write" => propose_write(args, &mut batch, &mut skipped),
        _ => return json!({"error": "unknown proposal", "kind": kind}).to_string(),
    }
    let queued = !batch.ops.is_empty();
    if queued {
        book.batches.push(batch);
    }
    json!({"queued": queued, "skipped": skipped}).to_string()
}

fn propose_replace(view: &View, args: &Value, batch: &mut Batch, skipped: &mut u32) {
    for item in array(args, "items") {
        let Some(paragraph) = paragraph(view, item) else {
            *skipped += 1;
            continue;
        };
        let old = string(item, "old");
        let new = string(item, "new");
        if paragraph.text != old {
            *skipped += 1;
            continue;
        }
        // An empty paragraph cannot hold a line break. New prose is inserted.
        if old.is_empty() {
            let paragraphs = paragraphs_of(&new);
            if paragraphs.is_empty() {
                *skipped += 1;
                continue;
            }
            push_write(batch, paragraphs);
            continue;
        }
        batch.lines.push(Line {
            before: old.clone(),
            after: new.clone(),
        });
        batch.ops.push(Op::Replace {
            index: paragraph.index,
            old,
            new,
        });
    }
}

fn propose_range(view: &View, args: &Value, batch: &mut Batch, skipped: &mut u32) {
    for item in array(args, "items") {
        let Some(paragraph) = paragraph(view, item) else {
            *skipped += 1;
            continue;
        };
        let old = string(item, "old");
        let find = string(item, "find");
        let new = string(item, "new");
        if find.is_empty() {
            *skipped += 1;
            continue;
        }
        let Some(start) = char_find(&paragraph.text, &find) else {
            *skipped += 1;
            continue;
        };
        if paragraph.text != old {
            *skipped += 1;
            continue;
        }
        let end = start + find.chars().count();
        batch.lines.push(Line {
            before: find.clone(),
            after: new.clone(),
        });
        batch.ops.push(Op::Range {
            index: paragraph.index,
            old,
            start,
            end,
            new,
        });
    }
}

fn propose_forms(view: &View, args: &Value, batch: &mut Batch) {
    let pairs: Vec<(String, String)> = array(args, "pairs")
        .iter()
        .map(|item| (string(item, "from"), string(item, "to")))
        .filter(|(from, _)| !from.is_empty())
        .collect();
    for paragraph in &view.paragraphs {
        let hits = pairs
            .iter()
            .any(|(from, _)| !word_spans(&paragraph.text, from).is_empty());
        if !hits {
            continue;
        }
        let mut after = paragraph.text.clone();
        for (from, to) in &pairs {
            after = replace_words(&after, from, to);
        }
        batch.lines.push(Line {
            before: paragraph.text.clone(),
            after,
        });
        batch.ops.push(Op::Forms {
            index: paragraph.index,
            old: paragraph.text.clone(),
            pairs: pairs.clone(),
        });
    }
}

fn replace_words(text: &str, from: &str, to: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut cursor = 0;
    for (start, end) in word_spans(text, from) {
        out.extend(chars.get(cursor..start).unwrap_or_default());
        out.push_str(to);
        cursor = end;
    }
    out.extend(chars.get(cursor..).unwrap_or_default());
    out
}

fn propose_underline(view: &View, args: &Value, batch: &mut Batch) {
    let forms: Vec<String> = args
        .get("forms")
        .and_then(Value::as_array)
        .map(|forms| {
            forms
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    for paragraph in &view.paragraphs {
        let mut spans = Vec::new();
        for form in &forms {
            spans.extend(word_spans(&paragraph.text, form));
        }
        spans.sort_unstable();
        spans.dedup();
        if spans.is_empty() {
            continue;
        }
        batch.lines.push(Line {
            before: paragraph.text.clone(),
            after: forms.join(", "),
        });
        batch.ops.push(Op::Underline {
            index: paragraph.index,
            old: paragraph.text.clone(),
            spans,
        });
    }
}

fn propose_kinds(view: &View, args: &Value, batch: &mut Batch, skipped: &mut u32) {
    for item in array(args, "items") {
        let Some(paragraph) = paragraph(view, item) else {
            *skipped += 1;
            continue;
        };
        let Some(kind) = parse_kind(&string(item, "kind")) else {
            *skipped += 1;
            continue;
        };
        batch.lines.push(Line {
            before: paragraph.text.clone(),
            after: string(item, "kind"),
        });
        batch.ops.push(Op::Kind {
            index: paragraph.index,
            kind,
        });
    }
}

fn propose_numbers(view: &View, args: &Value, batch: &mut Batch) {
    let ids: Vec<String> = args
        .get("ids")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let numbers = assign_numbers(&view.formulas, &ids);
    if numbers.is_empty() {
        return;
    }
    for (id, number) in &numbers {
        batch.lines.push(Line {
            before: id.clone(),
            after: number.to_string(),
        });
    }
    batch.ops.push(Op::Number { ids });
}

fn propose_mark(view: &View, args: &Value, batch: &mut Batch, comment: bool, skipped: &mut u32) {
    let Some(paragraph) = paragraph_at(view, usize_field(args, "index")) else {
        *skipped += 1;
        return;
    };
    let span = string(args, "span");
    let Some(start) = char_find(&paragraph.text, &span) else {
        *skipped += 1;
        return;
    };
    let end = start + span.chars().count();
    let note = if comment {
        string(args, "text")
    } else {
        string(args, "name")
    };
    if note.is_empty() || (!comment && !bookmark_name(&note)) {
        *skipped += 1;
        return;
    }
    batch.lines.push(Line {
        before: span,
        after: note.clone(),
    });
    batch.ops.push(if comment {
        Op::Comment {
            index: paragraph.index,
            old: paragraph.text.clone(),
            start,
            end,
            text: note,
        }
    } else {
        Op::Bookmark {
            index: paragraph.index,
            old: paragraph.text.clone(),
            start,
            end,
            name: note,
        }
    });
}

fn bookmark_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().next().is_some_and(char::is_alphabetic)
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && name.chars().count() <= 40
}

fn propose_join(view: &View, args: &Value, batch: &mut Batch, skipped: &mut u32) {
    let Some(index) = usize_field(args, "index") else {
        *skipped += 1;
        return;
    };
    if paragraph_at(view, Some(index)).is_none() || paragraph_at(view, Some(index + 1)).is_none() {
        *skipped += 1;
        return;
    }
    batch.ops.push(Op::Join { index });
    batch.lines.push(Line {
        before: index.to_string(),
        after: "join".to_owned(),
    });
}

fn propose_split(view: &View, args: &Value, batch: &mut Batch, skipped: &mut u32) {
    let Some(paragraph) = paragraph_at(view, usize_field(args, "index")) else {
        *skipped += 1;
        return;
    };
    let before = string(args, "before");
    let Some(at) = char_find(&paragraph.text, &before) else {
        *skipped += 1;
        return;
    };
    if at == 0 {
        *skipped += 1;
        return;
    }
    batch.ops.push(Op::Split {
        index: paragraph.index,
        at,
    });
    batch.lines.push(Line {
        before: paragraph.text.clone(),
        after: before,
    });
}

fn propose_move(view: &View, args: &Value, batch: &mut Batch, skipped: &mut u32) {
    let Some(start) = usize_field(args, "start") else {
        *skipped += 1;
        return;
    };
    let Some(to) = usize_field(args, "to") else {
        *skipped += 1;
        return;
    };
    if paragraph_at(view, Some(start)).is_none() {
        *skipped += 1;
        return;
    }
    let count = section_count(&view.paragraphs, start);
    if plan_moves(start, count, to).is_empty() {
        *skipped += 1;
        return;
    }
    let level = usize_field(args, "level").and_then(|level| u8::try_from(level).ok());
    batch.lines.push(Line {
        before: start.to_string(),
        after: to.to_string(),
    });
    batch.ops.push(Op::Move {
        start,
        count,
        to,
        level,
    });
}

fn propose_cite(view: &View, args: &Value, batch: &mut Batch, skipped: &mut u32) {
    let Some(paragraph) = paragraph_at(view, usize_field(args, "index")) else {
        *skipped += 1;
        return;
    };
    let formula = string(args, "formula");
    if !view
        .formulas
        .iter()
        .any(|item| item.display && item.id == formula)
    {
        *skipped += 1;
        return;
    }
    batch.lines.push(Line {
        before: paragraph.text.clone(),
        after: format!("REF {formula}"),
    });
    batch.ops.push(Op::Cite {
        index: paragraph.index,
        old: paragraph.text.clone(),
        formula,
    });
}

fn propose_write(args: &Value, batch: &mut Batch, skipped: &mut u32) {
    let mut paragraphs = paragraphs_of(&string(args, "text"));
    for item in array(args, "items") {
        let text = item
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| string(item, "text"));
        paragraphs.extend(paragraphs_of(&text));
    }
    if paragraphs.is_empty() {
        *skipped += 1;
        return;
    }
    push_write(batch, paragraphs);
}

/// Plain paragraphs of a piece. Line breaks separate them; an empty line is
/// not a paragraph, and a paragraph cannot hold a tab or a line break.
pub(crate) fn paragraphs_of(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| {
            line.trim_matches('\r')
                .replace('\t', " ")
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

fn push_write(batch: &mut Batch, paragraphs: Vec<String>) {
    for paragraph in &paragraphs {
        batch.lines.push(Line {
            before: String::new(),
            after: paragraph.clone(),
        });
    }
    if let Some(Op::Write {
        paragraphs: existing,
    }) = batch.ops.last_mut()
    {
        existing.extend(paragraphs);
        return;
    }
    batch.ops.push(Op::Write { paragraphs });
}

/// On a document with no text, a finished answer that is the piece itself
/// becomes a write. A short status line does not.
pub(crate) fn keep_blank_answer(view: &View, history: &[Turn], book: &mut Book) {
    if view
        .paragraphs
        .iter()
        .any(|paragraph| !paragraph.text.trim().is_empty())
    {
        return;
    }
    if book
        .batches
        .iter()
        .any(|batch| batch.ops.iter().any(|op| matches!(op, Op::Write { .. })))
    {
        return;
    }
    let from_answer = history.iter().rev().find_map(|turn| {
        (turn.role == Role::Assistant && !turn.content.trim().is_empty())
            .then(|| turn.content.clone())
    });
    let mut paragraphs = from_answer
        .as_deref()
        .map(paragraphs_of)
        .unwrap_or_default();
    if paragraphs
        .iter()
        .map(|line| line.chars().count())
        .sum::<usize>()
        < 50
    {
        paragraphs = book
            .report
            .iter()
            .flat_map(|item| paragraphs_of(&item.text))
            .collect();
        if paragraphs
            .iter()
            .map(|line| line.chars().count())
            .sum::<usize>()
            < 50
        {
            return;
        }
        book.report.clear();
    }
    book.batches.push(Batch {
        title: "write".to_owned(),
        lines: Vec::new(),
        ops: Vec::new(),
    });
    if let Some(batch) = book.batches.last_mut() {
        push_write(batch, paragraphs);
    }
}

fn propose_caption(view: &View, args: &Value, batch: &mut Batch, skipped: &mut u32) {
    let Some(index) = usize_field(args, "index") else {
        *skipped += 1;
        return;
    };
    if !view.pictures.contains(&index) {
        *skipped += 1;
        return;
    }
    let text = string(args, "text");
    if text.is_empty() {
        *skipped += 1;
        return;
    }
    batch.lines.push(Line {
        before: String::new(),
        after: text.clone(),
    });
    batch.ops.push(Op::Caption { index, text });
}

fn propose_report(view: &View, book: &mut Book, args: &Value) -> String {
    let mut items = Vec::new();
    for item in array(args, "items") {
        let paragraph = usize_field(item, "paragraph");
        let paragraph_text = paragraph_at(view, paragraph)
            .map(|paragraph| paragraph.text.clone())
            .unwrap_or_default();
        let suggestion = item
            .get("suggestion")
            .and_then(Value::as_str)
            .map(str::to_owned);
        items.push(ReportItem {
            text: string(item, "text"),
            paragraph,
            paragraph_text,
            suggestion,
        });
    }
    let count = items.len();
    book.report.extend(items);
    json!({"report": count}).to_string()
}

fn paragraph<'a>(view: &'a View, item: &Value) -> Option<&'a Paragraph> {
    paragraph_at(view, usize_field(item, "index"))
}

fn paragraph_at(view: &View, index: Option<usize>) -> Option<&Paragraph> {
    let index = index?;
    view.paragraphs
        .iter()
        .find(|paragraph| paragraph.index == index)
}

fn array<'a>(args: &'a Value, key: &str) -> &'a [Value] {
    args.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn usize_field(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|number| usize::try_from(number).ok())
}

fn char_find(text: &str, needle: &str) -> Option<usize> {
    text.find(needle).map(|byte| text[..byte].chars().count())
}

fn parse_kind(text: &str) -> Option<Kind> {
    match text.trim().to_lowercase().as_str() {
        "title" => Some(Kind::Title),
        "h1" | "heading1" | "heading 1" => Some(Kind::Heading(1)),
        "h2" | "heading2" | "heading 2" => Some(Kind::Heading(2)),
        "h3" | "heading3" | "heading 3" => Some(Kind::Heading(3)),
        "body" | "normal" | "text" => Some(Kind::Body),
        "quote" => Some(Kind::Quote),
        "caption" => Some(Kind::Caption),
        _ => None,
    }
}

/// The batches a pane click produces. Skip and no click produce none.
pub(crate) fn apply_action(batch: Batch, clicked: Option<bool>) -> Vec<Batch> {
    match clicked {
        Some(true) => vec![batch],
        Some(false) | None => Vec::new(),
    }
}

/// The one batch a chosen report row becomes.
pub(crate) fn report_action(item: &ReportItem, choice: Option<ReportChoice>) -> Vec<Batch> {
    let Some(choice) = choice else {
        return Vec::new();
    };
    let Some(index) = item.paragraph else {
        return Vec::new();
    };
    let op = match choice {
        ReportChoice::Comment => Op::Comment {
            index,
            old: item.paragraph_text.clone(),
            start: 0,
            end: item.paragraph_text.chars().count(),
            text: item.text.clone(),
        },
        ReportChoice::Edit => Op::Replace {
            index,
            old: item.paragraph_text.clone(),
            new: item.suggestion.clone().unwrap_or_default(),
        },
    };
    vec![Batch {
        title: "report".to_owned(),
        lines: vec![Line {
            before: item.paragraph_text.clone(),
            after: item.text.clone(),
        }],
        ops: vec![op],
    }]
}

/// Apply and Skip for one proposal.
///
/// `Some(true)` applies it and `Some(false)` skips it. The rectangle is the
/// Apply button, so a test can click it.
pub(crate) fn batch_controls(
    ui: &mut egui::Ui,
    title: &str,
    lines: &[Line],
    apply: &str,
    skip: &str,
) -> (Option<bool>, egui::Rect) {
    let mut choice = None;
    let mut apply_rect = egui::Rect::ZERO;
    ui.group(|ui| {
        ui.label(egui::RichText::new(title).strong());
        for line in lines {
            ui.label(line.before.as_str());
            ui.label(line.after.as_str());
        }
        ui.horizontal(|ui| {
            let response = ui.button(apply);
            apply_rect = response.rect;
            if response.clicked() {
                choice = Some(true);
            }
            if ui.button(skip).clicked() {
                choice = Some(false);
            }
        });
    });
    (choice, apply_rect)
}

/// A report row. Nothing is chosen until a button is clicked.
pub(crate) fn report_controls(
    ui: &mut egui::Ui,
    item: &ReportItem,
    comment: &str,
    edit: &str,
) -> Option<ReportChoice> {
    let mut choice = None;
    ui.group(|ui| {
        ui.label(item.text.as_str());
        ui.horizontal(|ui| {
            if ui.button(comment).clicked() {
                choice = Some(ReportChoice::Comment);
            }
            if item.suggestion.is_some() && ui.button(edit).clicked() {
                choice = Some(ReportChoice::Edit);
            }
        });
    });
    choice
}

#[cfg(test)]
mod tests {
    use super::{
        apply_action, assign_numbers, batch_controls, find_all, interpret, keep_blank_answer,
        paragraphs_of, plan_moves, post, report_action, report_controls, request_body, run,
        section_count, system_prompt, tool, word_spans, Batch, Book, Formula, Hit, Line,
        ModelReply, Op, Paragraph, ReportChoice, ReportItem, Role, Scripted, Stop, ToolCall, Turn,
        View,
    };
    use eframe::egui;
    use std::sync::atomic::AtomicBool;

    fn call(name: &str) -> ModelReply {
        ModelReply::Calls(vec![ToolCall {
            name: name.to_owned(),
            arguments: serde_json::json!({}),
        }])
    }

    #[test]
    fn two_reads_then_a_text_answer() {
        let mut model = Scripted {
            replies: vec![
                Ok(call("outline")),
                Ok(call("read_paragraphs")),
                Ok(ModelReply::Text("done".to_owned())),
            ],
            next: 0,
        };
        let mut history = vec![Turn::user("shorten the introduction")];
        let mut seen = Vec::new();
        let stop = run(
            &mut model,
            &mut history,
            |name, _| {
                seen.push(name.to_owned());
                "{\"ok\":true}".to_owned()
            },
            &AtomicBool::new(false),
        );
        assert_eq!(stop, Stop::Answer);
        assert_eq!(seen, ["outline", "read_paragraphs"]);
        assert_eq!(
            history.last().map(|turn| turn.content.as_str()),
            Some("done")
        );
        assert!(history.iter().any(|turn| turn.role == Role::Tool));
    }

    #[test]
    fn the_loop_stops_at_the_step_limit() {
        let mut model = Scripted {
            replies: vec![Ok(call("outline")); 12],
            next: 0,
        };
        let mut history = vec![Turn::user("keep going")];
        let mut calls = 0;
        let stop = run(
            &mut model,
            &mut history,
            |_, _| {
                calls += 1;
                "{}".to_owned()
            },
            &AtomicBool::new(false),
        );
        assert_eq!(stop, Stop::Limited);
        assert_eq!(calls, super::STEP_LIMIT);
        assert_eq!(
            history.last().map(|turn| turn.content.as_str()),
            Some("step limit")
        );
    }

    #[test]
    fn cancel_between_steps_stops_the_loop() {
        let flag = AtomicBool::new(false);
        let mut model = Scripted {
            replies: vec![Ok(call("outline")); 4],
            next: 0,
        };
        let mut history = vec![Turn::user("go")];
        let stop = run(
            &mut model,
            &mut history,
            |_, _| {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                "{}".to_owned()
            },
            &flag,
        );
        assert_eq!(stop, Stop::Cancelled);
        assert!(history.len() < 6);
    }

    #[test]
    fn an_unknown_tool_and_bad_json_stay_in_the_history() {
        let mut model = Scripted {
            replies: vec![
                Ok(call("not_a_tool")),
                Err("unreadable tool call: {".to_owned()),
            ],
            next: 0,
        };
        let mut history = vec![Turn::user("go")];
        let mut book = Book::default();
        let view = View::default();
        let stop = run(
            &mut model,
            &mut history,
            |name, args| tool(&view, &mut book, name, args),
            &AtomicBool::new(false),
        );
        assert_eq!(stop, Stop::Failed("unreadable tool call: {".to_owned()));
        assert!(book.batches.is_empty());
        assert!(history
            .iter()
            .any(|turn| turn.content.contains("unknown tool")));
        assert!(history
            .iter()
            .any(|turn| turn.content.contains("unreadable tool call")));
    }

    #[test]
    fn tool_calls_and_json_in_the_text_both_parse() {
        let native = interpret(&serde_json::json!({
            "message": {
                "content": "",
                "tool_calls": [{"function": {"name": "outline", "arguments": {}}}]
            }
        }))
        .unwrap();
        assert!(matches!(native, ModelReply::Calls(calls) if calls[0].name == "outline"));
        let fallback = interpret(&serde_json::json!({
            "message": {"content": "{\"tool\":\"find_all\",\"arguments\":{\"query\":\"alpha\"}}"}
        }))
        .unwrap();
        match fallback {
            ModelReply::Calls(calls) => {
                assert_eq!(calls[0].name, "find_all");
                assert_eq!(calls[0].arguments["query"], "alpha");
            }
            ModelReply::Text(_) => panic!("json tool call"),
        }
        assert!(interpret(&serde_json::json!({"message": {"content": "{not json"}})).is_err());
    }

    #[test]
    fn the_chat_request_carries_tools_and_the_previous_result() {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let body = loop {
                let mut buffer = [0; 4096];
                let read = stream.read(&mut buffer).unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buffer[..read]);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break request[end + 4..end + 4 + length].to_vec();
                    }
                }
            };
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(json.get("tools").is_some());
            let messages = json["messages"].as_array().unwrap();
            assert!(messages.iter().any(|message| {
                message["role"] == "tool" && message["content"] == "previous-result-token"
            }));
            let response = serde_json::json!({"message": {"content": "ok"}}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len()
            )
            .unwrap();
        });
        let history = vec![
            Turn::user("look"),
            Turn::tool("outline", "previous-result-token"),
        ];
        let body = request_body("test-model", &history, 4096);
        assert_eq!(body["options"]["num_ctx"], 4096);
        assert!(body.get("tools").is_some());
        let value = post(&format!("http://{address}"), "test-model", &history, 4096).unwrap();
        server.join().unwrap();
        assert_eq!(value["message"]["content"], "ok");
    }

    #[test]
    fn find_all_returns_both_hits_without_a_document() {
        let view = View {
            paragraphs: vec![
                Paragraph {
                    index: 0,
                    text: "alpha and alpha".to_owned(),
                    ..Paragraph::default()
                },
                Paragraph {
                    index: 2,
                    text: "beta".to_owned(),
                    ..Paragraph::default()
                },
            ],
            ..View::default()
        };
        assert_eq!(
            find_all(&view, "alpha"),
            vec![
                Hit {
                    paragraph: 0,
                    text: "alpha".to_owned(),
                },
                Hit {
                    paragraph: 0,
                    text: "alpha".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_form_inside_a_longer_word_is_not_a_hit() {
        let spans = word_spans("штатный штат", "штат");
        assert_eq!(spans, vec![(8, 12)]);
        let replaced = super::replace_words("Штате и штате и штатный", "штате", "регионе");
        assert_eq!(replaced, "Штате и регионе и штатный");
        let titled = super::replace_words(&replaced, "Штате", "Регионе");
        assert_eq!(titled, "Регионе и регионе и штатный");
    }

    #[test]
    fn formula_ids_are_numbered_in_document_order() {
        let formulas = vec![
            Formula {
                id: "f1".to_owned(),
                paragraph: 0,
                display: true,
                ..Formula::default()
            },
            Formula {
                id: "f2".to_owned(),
                paragraph: 2,
                display: true,
                ..Formula::default()
            },
            Formula {
                id: "i1".to_owned(),
                paragraph: 3,
                display: false,
                ..Formula::default()
            },
        ];
        assert_eq!(
            assign_numbers(
                &formulas,
                &["f2".to_owned(), "f1".to_owned(), "i1".to_owned()]
            ),
            vec![("f1".to_owned(), 1), ("f2".to_owned(), 2)]
        );
    }

    #[test]
    fn a_section_moves_without_changing_its_text() {
        let paragraphs = vec![
            Paragraph {
                index: 0,
                heading: Some(0),
                text: "Alpha".to_owned(),
                ..Paragraph::default()
            },
            Paragraph {
                index: 1,
                text: "body a".to_owned(),
                ..Paragraph::default()
            },
            Paragraph {
                index: 2,
                heading: Some(0),
                text: "Beta".to_owned(),
                ..Paragraph::default()
            },
            Paragraph {
                index: 3,
                text: "body b".to_owned(),
                ..Paragraph::default()
            },
            Paragraph {
                index: 4,
                text: "End".to_owned(),
                ..Paragraph::default()
            },
        ];
        assert_eq!(section_count(&paragraphs, 0), 2);
        let moves = plan_moves(0, 2, 4);
        let mut doc: Vec<char> = "ABCDE".chars().collect();
        for (at, to) in moves {
            let item = doc.remove(at);
            let dest = if to > at { to - 1 } else { to };
            doc.insert(dest, item);
        }
        assert_eq!(doc.iter().collect::<String>(), "CDABE");
    }

    #[test]
    fn apply_sends_one_batch_and_skip_sends_none() {
        let batch = Batch {
            title: "replace".to_owned(),
            lines: vec![Line {
                before: "Two".to_owned(),
                after: "Dos".to_owned(),
            }],
            ops: Vec::new(),
        };
        assert_eq!(apply_action(batch.clone(), Some(true)).len(), 1);
        assert!(apply_action(batch.clone(), Some(false)).is_empty());
        assert!(apply_action(batch, None).is_empty());
        let lines = [Line {
            before: "Two".to_owned(),
            after: "Dos".to_owned(),
        }];
        let ctx = egui::Context::default();
        let mut apply_at = egui::Pos2::ZERO;
        let mut clicked = None;
        for frame in 0..4 {
            let mut events = vec![egui::Event::PointerMoved(apply_at)];
            if frame == 2 || frame == 3 {
                events.push(egui::Event::PointerButton {
                    pos: apply_at,
                    button: egui::PointerButton::Primary,
                    pressed: frame == 2,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    time: Some(f64::from(frame) * 0.1),
                    ..Default::default()
                },
                |ui| {
                    let (choice, rect) = batch_controls(ui, "replace", &lines, "Apply", "Skip");
                    if frame == 0 {
                        assert_eq!(choice, None);
                    }
                    apply_at = rect.center();
                    if choice == Some(true) {
                        clicked = Some(true);
                    }
                },
            );
        }
        assert_eq!(clicked, Some(true));
    }

    #[test]
    fn a_report_sends_nothing_until_a_row_is_chosen() {
        let item = ReportItem {
            text: "repeated".to_owned(),
            paragraph: Some(1),
            paragraph_text: "repeated word".to_owned(),
            suggestion: Some("word".to_owned()),
        };
        assert!(report_action(&item, None).is_empty());
        assert_eq!(report_action(&item, Some(ReportChoice::Edit)).len(), 1);
        assert_eq!(report_action(&item, Some(ReportChoice::Comment)).len(), 1);
        let ctx = egui::Context::default();
        let mut choice = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                ..Default::default()
            },
            |ui| {
                choice = report_controls(ui, &item, "Comment", "Edit");
            },
        );
        assert_eq!(choice, None);
        assert!(system_prompt("ru").contains("Russian"));
    }

    #[test]
    fn a_blank_document_keeps_a_written_answer() {
        let blank = View {
            paragraphs: vec![Paragraph {
                index: 0,
                ..Paragraph::default()
            }],
            ..View::default()
        };
        let mut book = Book::default();
        let wrote = tool(
            &blank,
            &mut book,
            "propose",
            &serde_json::json!({
                "kind": "write",
                "text": "Осень.\n\nЛистья падают на дорожки парка и шуршат под ногами."
            }),
        );
        assert!(wrote.contains("\"queued\":true"));
        match &book.batches[0].ops[0] {
            Op::Write { paragraphs } => {
                assert_eq!(
                    paragraphs,
                    &[
                        "Осень.".to_owned(),
                        "Листья падают на дорожки парка и шуршат под ногами.".to_owned()
                    ]
                );
            }
            other => panic!("{other:?}"),
        }
        let mut reported = Book::default();
        tool(
            &blank,
            &mut reported,
            "propose",
            &serde_json::json!({
                "kind": "report",
                "items": [{
                    "text": "Катет — сторона прямоугольного треугольника, образующая прямой угол.",
                    "paragraph": 0
                }]
            }),
        );
        let history = vec![Turn::assistant("Добавлено в документ.".to_owned())];
        keep_blank_answer(&blank, &history, &mut reported);
        assert!(reported.report.is_empty());
        match &reported.batches[0].ops[0] {
            Op::Write { paragraphs } => {
                assert_eq!(paragraphs.len(), 1);
                assert!(paragraphs[0].contains("Катет"));
            }
            other => panic!("{other:?}"),
        }
        let mut status = Book::default();
        keep_blank_answer(&blank, &history, &mut status);
        assert!(status.batches.is_empty());
        assert_eq!(
            paragraphs_of("  one \n\n\ttwo\r\n"),
            vec!["one".to_owned(), "two".to_owned()]
        );
    }
}
