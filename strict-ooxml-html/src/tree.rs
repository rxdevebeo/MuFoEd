use crate::{HtmlError, HtmlOptions};
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use std::cell::RefCell;

#[derive(Default)]
pub(crate) struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<usize>,
}
impl Node {
    pub(crate) fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}
pub(crate) struct Tree {
    pub nodes: Vec<Node>,
    stack: Vec<usize>,
    limit: usize,
    depth: usize,
    error: Option<&'static str>,
}
struct Sink(RefCell<Tree>);
impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut t = self.0.borrow_mut();
        if t.error.is_some() {
            return TokenSinkResult::Continue;
        }
        match token {
            Token::CharacterTokens(text) => {
                let parent = *t.stack.last().unwrap();
                if let Some(last) = t.nodes[parent]
                    .children
                    .last()
                    .copied()
                    .filter(|n| t.nodes[*n].name == "#text")
                {
                    t.nodes[last].text.push_str(&text);
                } else {
                    t.add(
                        Node {
                            name: "#text".into(),
                            text: text.to_string(),
                            ..Default::default()
                        },
                        false,
                    );
                }
            }
            Token::TagToken(tag) => {
                let name = tag.name.to_string();
                if tag.kind == TagKind::EndTag {
                    if let Some(i) = t.stack.iter().rposition(|n| t.nodes[*n].name == name) {
                        t.stack.truncate(i.max(1));
                    }
                } else {
                    // HTML's common omitted end tags: adjacent paragraphs,
                    // list items, rows and cells must not become nested peers.
                    let closes: &[&str] = match name.as_str() {
                        "p" => &["p"],
                        "li" => &["li"],
                        "tr" => &["tr"],
                        "td" | "th" => &["td", "th"],
                        _ => &[],
                    };
                    if let Some(i) = t
                        .stack
                        .iter()
                        .rposition(|n| closes.contains(&t.nodes[*n].name.as_str()))
                    {
                        let boundary = t
                            .stack
                            .iter()
                            .rposition(|n| {
                                ["ul", "ol", "table"].contains(&t.nodes[*n].name.as_str())
                            })
                            .unwrap_or(0);
                        if i > boundary {
                            t.stack.truncate(i.max(1));
                        }
                    }
                    let raw = match name.as_str() {
                        "script" => Some(html5ever::tokenizer::states::RawKind::ScriptData),
                        "style" => Some(html5ever::tokenizer::states::RawKind::Rawtext),
                        _ => None,
                    };
                    let void = tag.self_closing
                        || [
                            "img", "br", "hr", "meta", "link", "input", "source", "wbr", "area",
                            "base", "col", "embed", "param", "track",
                        ]
                        .contains(&name.as_str());
                    t.add(
                        Node {
                            name,
                            attrs: tag
                                .attrs
                                .into_iter()
                                .map(|a| (a.name.local.to_string(), a.value.to_string()))
                                .collect(),
                            ..Default::default()
                        },
                        !void,
                    );
                    if let Some(raw) = raw {
                        return TokenSinkResult::RawData(raw);
                    }
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}
impl Tree {
    fn add(&mut self, node: Node, push: bool) {
        if self.nodes.len() >= self.limit {
            self.error = Some("nodes");
            return;
        }
        if push && self.stack.len() > self.depth {
            self.error = Some("nesting");
            return;
        }
        let index = self.nodes.len();
        self.nodes.push(node);
        let parent = *self.stack.last().unwrap();
        self.nodes[parent].children.push(index);
        if push {
            self.stack.push(index);
        }
    }
}
pub(crate) fn parse(html: &str, options: &HtmlOptions) -> Result<Tree, HtmlError> {
    if options.max_nodes == 0 {
        return Err(HtmlError::Limit("nodes"));
    }
    let sink = Sink(RefCell::new(Tree {
        nodes: vec![Node::default()],
        stack: vec![0],
        limit: options.max_nodes,
        depth: options.max_depth,
        error: None,
    }));
    let input = BufferQueue::default();
    input.push_back(StrTendril::from(html));
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let tree = tokenizer.sink.0.into_inner();
    if let Some(error) = tree.error {
        return Err(HtmlError::Limit(error));
    }
    Ok(tree)
}
