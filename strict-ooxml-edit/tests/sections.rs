//! K04 (sections): section properties and section breaks of the body.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{
    Address, Container, Edit, EditError, EditLimits, Editor, Invariant, Story, Unsupported,
};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{
    Block, Document, PageOrientation, PageSize, SectionProperties, Twips,
};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// Three paragraphs and a table in one portrait section.
fn document() -> Document {
    let xml = "<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"><w:body>\
               <w:p><w:r><w:t>one</w:t></w:r></w:p>\
               <w:p><w:r><w:t>two</w:t></w:r></w:p>\
               <w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>\
               <w:p><w:r><w:t>three</w:t></w:r></w:p>\
               <w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>\
               </w:body></w:document>";
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.as_bytes().to_vec())
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
    parse_document(&package, &ParseOptions::default()).expect("document")
}

fn landscape(base: &SectionProperties) -> SectionProperties {
    let mut properties = base.clone();
    properties.page_size = Some(PageSize {
        width: Some(Twips(15840)),
        height: Some(Twips(12240)),
        orientation: Some(PageOrientation::Landscape),
        code: None,
    });
    properties
}

fn width(properties: &SectionProperties) -> Option<i32> {
    properties
        .page_size
        .as_ref()
        .and_then(|size| size.width)
        .map(Twips::value)
}

/// The section properties each body paragraph carries as a boundary.
fn boundaries(document: &Document) -> Vec<Option<i32>> {
    document
        .body
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.props.section.as_ref().and_then(width)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_break_splits_the_section_and_each_half_takes_its_own_page() {
    let mut d = document();
    let original = d.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let portrait = e.document().sections[0].properties.clone();
    let change = e
        .transact(
            0,
            &[
                Edit::InsertSectionBreak {
                    at: Address::body(1),
                },
                Edit::SectionProperties {
                    at: Address::body(3),
                    properties: Box::new(landscape(&portrait)),
                },
            ],
        )
        .expect("break and landscape");
    let after = e.document();
    assert_eq!(after.sections.len(), 2);
    assert_eq!(width(&after.sections[0].properties), Some(12240));
    assert_eq!(width(&after.sections[1].properties), Some(15840));
    assert_eq!(
        boundaries(after),
        [None, Some(12240), None],
        "the second paragraph ends the first section"
    );
    assert!(e.validate().is_ok());
    e.undo(change.revision).expect("undo");
    assert_eq!(e.document().sections, original.sections);
    assert_eq!(e.document().body, original.body);
}

#[test]
fn setting_the_first_section_updates_its_boundary_too() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let portrait = e.document().sections[0].properties.clone();
    e.transact(
        0,
        &[
            Edit::InsertSectionBreak {
                at: Address::body(0),
            },
            Edit::SectionProperties {
                at: Address::body(0),
                properties: Box::new(landscape(&portrait)),
            },
        ],
    )
    .expect("landscape first section");
    let after = e.document();
    assert_eq!(width(&after.sections[0].properties), Some(15840));
    assert_eq!(width(&after.sections[1].properties), Some(12240));
    assert_eq!(boundaries(after), [Some(15840), None, None]);
    assert!(e.validate().is_ok());
}

#[test]
fn removing_a_break_keeps_the_properties_asked_for() {
    for (keep_after, expected) in [(false, 15840), (true, 12240)] {
        let mut d = document();
        let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
        let portrait = e.document().sections[0].properties.clone();
        e.transact(
            0,
            &[
                Edit::InsertSectionBreak {
                    at: Address::body(1),
                },
                Edit::SectionProperties {
                    at: Address::body(0),
                    properties: Box::new(landscape(&portrait)),
                },
            ],
        )
        .expect("two sections");
        let revision = e.revision();
        e.transact(
            revision,
            &[Edit::RemoveSectionBreak {
                at: Address::body(1),
                keep_after,
            }],
        )
        .expect("remove");
        let after = e.document();
        assert_eq!(after.sections.len(), 1);
        assert_eq!(width(&after.sections[0].properties), Some(expected));
        assert_eq!(boundaries(after), [None, None, None]);
        assert!(e.validate().is_ok());
    }
}

#[test]
fn section_commands_refuse_what_is_not_a_body_boundary() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    // No break to remove; the last section has no boundary paragraph.
    assert_eq!(
        e.transact(
            0,
            &[Edit::RemoveSectionBreak {
                at: Address::body(0),
                keep_after: false,
            }],
        ),
        Err(EditError::InvalidModel(Invariant::SectionBoundary))
    );
    // A paragraph in a table cannot end a section.
    let in_cell = Address {
        story: Story::Body,
        containers: vec![Container::Cell {
            table: 2,
            row: 0,
            cell: 0,
        }],
        block: 0,
    };
    assert_eq!(
        e.transact(0, &[Edit::InsertSectionBreak { at: in_cell }]),
        Err(EditError::UnsupportedContent(Unsupported::Target))
    );
    // The table itself is not a paragraph.
    assert_eq!(
        e.transact(
            0,
            &[Edit::InsertSectionBreak {
                at: Address::body(2),
            }],
        ),
        Err(EditError::InvalidParagraph)
    );
    // A break where there already is one.
    e.transact(
        0,
        &[Edit::InsertSectionBreak {
            at: Address::body(0),
        }],
    )
    .expect("break");
    let revision = e.revision();
    assert_eq!(
        e.transact(
            revision,
            &[Edit::InsertSectionBreak {
                at: Address::body(0),
            }],
        ),
        Err(EditError::InvalidModel(Invariant::SectionBoundary))
    );
}
