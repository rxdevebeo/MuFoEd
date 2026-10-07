//! Markup Compatibility resolution (`TZ-STRICT-OOXML-RUST.md` §10.7, T6).
//!
//! `mc:AlternateContent` is markup a producer writes when it has two
//! representations of one thing and does not know which its consumer understands:
//! an `mc:Choice` per alternative with a `Requires` list of namespace prefixes, and
//! one `mc:Fallback` for everyone else. **Strict defines conformance on the
//! post-MCE part** — ECMA-376 Part 1 §2.1 clause (ii) — so a Strict document is
//! not one that never carries the markup; it is the part a processor leaves
//! behind. Resolving it here rather than leaving it to the consumer is what makes
//! the claim this project makes about its output true.
//!
//! # What "understood" means, and why it is not "is it in ECMA-376"
//!
//! MCE says a consumer selects an `mc:Choice` whose every `Requires` prefix it
//! understands. For *this* project that has one honest answer and it is not the
//! ECMA set: [`tables::REPRODUCED_EXTENSION_NAMESPACES`] is the set of vendor
//! namespaces the writer reproduces from the model (`wps`, `wpg`, `wp14`, `a14`,
//! … — ADR-0014's `XS-18`/`XS-19` debt, kept on purpose), so an
//! `mc:Choice Requires="wps"` names content this project reads and writes back.
//! Six of the corpus's nine blocks are exactly that, and choosing their
//! `mc:Fallback` instead would downgrade a shape we can render to a picture.
//!
//! Two sets are deliberately **not** understood:
//!
//! - [`tables::IGNORABLE_EXTENSION_NAMESPACES`] — `w14`/`w15`. We drop those, so
//!   a `Choice` requiring one would select content we are about to delete. The
//!   `mc:Fallback` is the producer's own statement of what an unaware consumer
//!   should get, which is the better answer than selecting-and-then-removing.
//! - anything the namespace registry does not know. That is not "vendor", it is
//!   unknown, and MCE's answer for unknown is the fallback.
//!
//! # The three policies
//!
//! | [`McePolicy`] | Behaviour |
//! |---|---|
//! | [`ProcessChoice`](McePolicy::ProcessChoice) | the first `mc:Choice` whose every `Requires` prefix is understood; otherwise the `mc:Fallback` |
//! | [`PreferFallback`](McePolicy::PreferFallback) | the `mc:Fallback`, whatever the choices say |
//! | [`Report`](McePolicy::Report) | the block is left exactly as it was and named |
//!
//! A block that resolves to **nothing** — no understood choice and no
//! `mc:Fallback` — removes its content, which is what MCE says and what the corpus
//! exercises: three `word/settings.xml` files carry
//! `<mc:Choice Requires="wpsCustomData"/>` with no fallback, and that element is a
//! Word spelling-version flag this project does not model. Removing it is correct
//! **and it is a removal**, so it is recorded; dropping it silently would be the
//! §6 lesson all over again.
//!
//! # The resolved branch goes back through T1–T5
//!
//! Per §10.7 step 4, and it matters here: six of the nine corpus blocks have an
//! `mc:Fallback` that is a `w:pict`, and a branch that bypassed the stages would
//! have its picture dropped by T7 rather than converted by it — the same picture,
//! from the same element, that a `w:pict` outside an `mc:AlternateContent` keeps.

use std::collections::VecDeque;

use quick_xml::events::attributes::Attribute;
use quick_xml::events::Event;
use quick_xml::XmlVersion;

use crate::error::SourceLocation;
use crate::normalize::report::{LossRecord, Severity};
use crate::normalize::tables;
use crate::normalize::transitional::{McePolicy, PartContext};

/// The MCE namespace. The same in Transitional and Strict, so [`map_uri`] leaves
/// it alone; the *markup* is removed even so, because conformance is defined after
/// MCE.
pub(crate) const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// How a block resolved, for the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// A `mc:Choice` was selected: the prefixes it requires, all understood.
    Choice(Vec<String>),
    /// No choice was understood and the `mc:Fallback` was taken.
    Fallback,
    /// Nothing was understood and there was no fallback, so the block resolved to
    /// nothing.
    Empty,
    /// [`McePolicy::Report`] left the block alone.
    Reported,
}

/// One branch of a block: an `mc:Choice` or the `mc:Fallback`.
pub(crate) struct Branch {
    /// The element's own local name.
    pub(crate) local: String,
    /// The `Requires` value, for a `Choice`.
    pub(crate) requires: String,
    /// The events between this element's tags.
    pub(crate) content: Vec<Event<'static>>,
}

/// Splits a buffered `mc:AlternateContent` into its branches.
///
/// A nested `mc:AlternateContent` inside a branch is **not** resolved here: the
/// branch's events go to the event loop, which meets them like any other element.
/// One level per pass is deliberate — a block that nests these is not a document to
/// be clever about, and resolving the inner one from here would mean re-entering
/// the whole pipeline inside a function that already has the reader borrowed.
pub(crate) fn branches(subtree: &[Event<'static>], context: &PartContext) -> Vec<Branch> {
    let mut out: Vec<Branch> = Vec::new();
    let mut index = 1usize; // 0 is the `mc:AlternateContent` itself
    while index < subtree.len() {
        let Event::Start(start) = &subtree[index] else {
            index += 1;
            continue;
        };
        let raw = String::from_utf8_lossy(start.name().as_ref()).into_owned();
        let Some((prefix, local)) = raw.split_once(':') else {
            index += 1;
            continue;
        };
        if context.uri_for(prefix.as_bytes()) != Some(MC_NS)
            || !matches!(local, "Choice" | "Fallback")
        {
            index += 1;
            continue;
        }
        let requires = requires_of(start.attributes().flatten());
        // Collect up to this element's matching end tag.
        let mut inner_events = Vec::new();
        let mut inner = 1usize;
        let mut scan = index + 1;
        while scan < subtree.len() && inner > 0 {
            match &subtree[scan] {
                Event::Start(_) => inner += 1,
                Event::End(_) => inner -= 1,
                Event::Eof => break,
                _ => {}
            }
            if inner > 0 {
                inner_events.push(subtree[scan].clone());
            }
            scan += 1;
        }
        out.push(Branch {
            local: local.to_owned(),
            requires,
            content: inner_events,
        });
        index = scan;
    }
    out
}

/// The `Requires` value of a start tag, by **local** attribute name.
fn requires_of<'a>(mut attributes: impl Iterator<Item = Attribute<'a>>) -> String {
    attributes
        .find_map(|attribute| {
            let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
            (key.rsplit(':').next() == Some("Requires"))
                .then(|| {
                    attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .ok()
                        .map(std::borrow::Cow::into_owned)
                })
                .flatten()
        })
        .unwrap_or_default()
}

/// Whether every prefix in a `Requires` list is one this project handles.
///
/// An empty `Requires` is **not** satisfied: MCE's `Requires` is the list of
/// namespaces an alternative needs, and an alternative that needs none is not an
/// alternative. Reading it as satisfied would select an `mc:Choice` the producer
/// meant to be conditional. A `mc:Choice` with no `Requires` is malformed MCE, and
/// the safe reading of malformed MCE is the fallback.
pub(crate) fn understood(requires: &str, context: &PartContext) -> Option<Vec<String>> {
    let prefixes: Vec<String> = requires
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect();
    let all = !prefixes.is_empty()
        && prefixes.iter().all(|prefix| {
            context
                .uri_for(prefix.as_bytes())
                .is_some_and(understood_namespace)
        });
    all.then_some(prefixes)
}

/// Whether this project handles a namespace at all: the registry knows it, or the
/// writer reproduces it from the model.
///
/// The second half is the one that is not obvious. `wps` is neither in ECMA-376 nor
/// in the registry, and an `mc:Choice Requires="wps"` still names content we can
/// read — it is the shape ADR-0014 keeps as debt on purpose. A project that
/// answered "unknown" here would take the `mc:Fallback` for a shape it can render,
/// which is a downgrade the producer never asked for.
pub(crate) fn understood_namespace(uri: &str) -> bool {
    !tables::is_ignorable_extension(uri)
        && (crate::ns::registry::strict_form(uri).is_some()
            || tables::REPRODUCED_EXTENSION_NAMESPACES.contains(&uri))
}

/// Picks the branch the policy asks for, and says what happened.
pub(crate) fn resolve(
    policy: McePolicy,
    branches: &[Branch],
    context: &PartContext,
) -> (Resolution, Vec<Event<'static>>) {
    if matches!(policy, McePolicy::Report) {
        return (Resolution::Reported, Vec::new());
    }
    let fallback = branches.iter().find(|branch| branch.local == "Fallback");
    if !matches!(policy, McePolicy::PreferFallback) {
        for choice in branches.iter().filter(|b| b.local == "Choice") {
            if let Some(prefixes) = understood(&choice.requires, context) {
                return (Resolution::Choice(prefixes), choice.content.clone());
            }
        }
    }
    match fallback {
        Some(fallback) => (Resolution::Fallback, fallback.content.clone()),
        None => (Resolution::Empty, Vec::new()),
    }
}

/// The report record for a resolved block.
///
/// §10.7 step 5 wants a `LossRecord` per resolved block, and this gives one for
/// **every** outcome, benign included — a report that shows only removals cannot
/// answer "what did you do with my `mc:AlternateContent`", and a resolution is a
/// decision somebody made. The severity is what separates them, and the pairing is
/// the whole point of [`Resolution`]: a `Choice` taken is `Info`, a fallback taken
/// is `Ignorable`, and a block that resolved to nothing is `Lossy` because a node
/// is gone.
pub(crate) fn record(
    resolution: &Resolution,
    policy: McePolicy,
    location: &SourceLocation,
) -> LossRecord {
    let (severity, reason) = match resolution {
        Resolution::Choice(prefixes) => (
            Severity::Info,
            format!(
                "an mc:AlternateContent block resolved to the first mc:Choice; every prefix it \
                 requires ({}) is one this project handles",
                prefixes.join(", ")
            ),
        ),
        Resolution::Fallback => (
            Severity::Ignorable,
            "an mc:AlternateContent block resolved to its mc:Fallback: no mc:Choice asked for a \
             namespace this project handles"
                .to_owned(),
        ),
        Resolution::Empty => (
            Severity::Lossy,
            "an mc:AlternateContent block resolved to nothing: no mc:Choice asked for a \
             namespace this project handles and there is no mc:Fallback, so its content is \
             removed"
                .to_owned(),
        ),
        Resolution::Reported => (
            Severity::Ignorable,
            format!(
                "an mc:AlternateContent block was left as it was: McePolicy::{policy:?} reports \
                 MCE instead of resolving it"
            ),
        ),
    };
    LossRecord {
        transform_id: "T6.mce",
        feature_id: "mc:AlternateContent".to_owned(),
        reason,
        severity,
        locations: vec![location.clone()],
    }
}

/// Queues a resolved branch for the event loop to process.
///
/// **On the queue, not straight into the writer**, and that is the whole point of
/// this function's existence: the branch is ordinary markup T1–T5 have not seen
/// yet, and it has to meet the same dispatch as the rest of the part.
/// Places resolved branch content at the **front** of the event queue so the
/// main loop processes it before the markup that followed the
/// `mc:AlternateContent` in the source.
///
/// Appending would defer the Choice until after surrounding end tags were
/// already written — `SoftUni` `wp:positionV` wrapped in `Requires="wp14"`
/// then landed after `</wp:anchor></w:drawing>`, and the floating frame lost
/// its page offset.
pub(crate) fn queue(content: Vec<Event<'static>>, buffered: &mut VecDeque<Event<'static>>) {
    for event in content.into_iter().rev() {
        buffered.push_front(event);
    }
}
