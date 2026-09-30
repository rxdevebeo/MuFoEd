//! The viewer's HTML, CSS and JavaScript.
//!
//! Kept in one module as a set of `const`s rather than as asset files: the
//! binary stays self-contained, and the markup is close enough to the routing
//! that keeping them together is easier to follow than splitting them.
//!
//! Pages are served as SVG, not rasterized. The renderer already emits vector
//! output, so there is nothing to convert, and the browser scales a page to any
//! zoom level without resampling — which matters when the point of the viewer
//! is to judge whether a glyph landed in the right place.

/// The document shell: menu, header, and the scrolling page column.
#[must_use]
pub(crate) fn index_html() -> &'static str {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>StrictLib viewer</title>
<style>#css#</style>
</head>
<body>
<header id="bar">
  <button id="menu-button" type="button" aria-expanded="false" aria-controls="menu">&#9776; Documents</button>
  <div id="title">
    <strong id="doc-name">no document open</strong>
    <span id="doc-meta"></span>
  </div>
  <div id="tools">
    <label for="zoom">zoom</label>
    <input id="zoom" type="range" min="25" max="200" step="5" value="55">
    <output id="zoom-value">55%</output>
    <label class="check"><input id="gaps" type="checkbox" checked> page gaps</label>
  </div>
</header>
<nav id="menu" hidden>
  <input id="filter" type="search" placeholder="filter documents&hellip;" autocomplete="off">
  <ul id="menu-list"></ul>
  <p id="menu-empty" hidden>no .docx files here</p>
</nav>
<main id="pages"></main>
<footer id="status">choose a document from the menu</footer>
<script>#script#</script>
</body>
</html>
"#
}

/// The stylesheet.
#[must_use]
/// The stylesheet.
///
/// Long by nature: it is one stylesheet, and splitting it to satisfy a line
/// count would scatter selectors that only make sense together.
#[allow(clippy::too_many_lines)]
pub(crate) fn css() -> &'static str {
    r#"
:root {
  --bg: #14161a;
  --panel: #1c1f26;
  --line: #2c313b;
  --text: #e6e8ec;
  --muted: #949aa6;
  --accent: #6aa9ff;
  --page: #ffffff;
}
* { box-sizing: border-box; }
html, body { height: 100%; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--text);
  font: 14px/1.45 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  display: grid;
  grid-template-rows: auto 1fr auto;
  grid-template-areas: "bar" "main" "status";
}

/* ---- header ---- */
#bar {
  grid-area: bar;
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 8px 14px;
  background: var(--panel);
  border-bottom: 1px solid var(--line);
  position: sticky;
  top: 0;
  z-index: 3;
}
#menu-button {
  font: inherit;
  color: var(--text);
  background: #262b34;
  border: 1px solid var(--line);
  border-radius: 6px;
  padding: 6px 12px;
  cursor: pointer;
  white-space: nowrap;
}
#menu-button:hover { background: #2f3540; }
#menu-button[aria-expanded="true"] { background: var(--accent); color: #0d1117; }
#title { flex: 1; min-width: 0; display: flex; flex-direction: column; }
#doc-name {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
#doc-meta { color: var(--muted); font-size: 12px; }
#doc-meta .bad { color: #ff9d7a; }
#doc-meta > span + span::before { content: " \00b7 "; }
#tools { display: flex; align-items: center; gap: 8px; color: var(--muted); }
#tools label { font-size: 12px; }
#zoom { width: 140px; accent-color: var(--accent); }
#zoom-value { font-variant-numeric: tabular-nums; min-width: 44px; }
#tools .check { display: flex; align-items: center; gap: 5px; }

/* ---- menu ---- */
#menu {
  position: fixed;
  top: 47px;
  left: 0;
  bottom: 0;
  width: 340px;
  max-width: 84vw;
  background: var(--panel);
  border-right: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  z-index: 2;
  box-shadow: 8px 0 24px rgb(0 0 0 / 35%);
}
#menu[hidden] { display: none; }
#filter {
  margin: 10px;
  padding: 8px 10px;
  font: inherit;
  color: var(--text);
  background: #10131a;
  border: 1px solid var(--line);
  border-radius: 6px;
}
#menu-list {
  list-style: none;
  margin: 0;
  padding: 0 0 20px;
  overflow-y: auto;
  flex: 1;
}
#menu-list li { border-top: 1px solid var(--line); }
#menu-list li:first-child { border-top: 0; }
#menu-list button {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  width: 100%;
  text-align: left;
  font: inherit;
  color: var(--text);
  background: none;
  border: 0;
  padding: 9px 12px;
  cursor: pointer;
}
#menu-list button:hover { background: #262b34; }
#menu-list li[aria-current="true"] button {
  background: #223049;
  box-shadow: inset 3px 0 0 var(--accent);
}
#menu-list .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
#menu-list .size { color: var(--muted); font-size: 12px; font-variant-numeric: tabular-nums; }
#menu-empty { color: var(--muted); padding: 12px; margin: 0; }

/* ---- pages ---- */
#pages {
  grid-area: main;
  overflow: auto;
  padding: 24px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 24px;
}
body.gaps-off #pages { gap: 0; }
.page {
  position: relative;
  background: var(--page);
  box-shadow: 0 2px 6px rgb(0 0 0 / 45%), 0 12px 32px rgb(0 0 0 / 30%);
  flex: none;
}
body.gaps-off .page { box-shadow: none; border-bottom: 1px solid var(--line); }
.page svg { display: block; width: 100%; height: auto; }
.page .number {
  position: absolute;
  top: 6px;
  right: 8px;
  color: #9aa0aa;
  font-size: 11px;
  background: rgb(255 255 255 / 85%);
  border-radius: 3px;
  padding: 1px 5px;
  pointer-events: none;
}
#empty { color: var(--muted); margin: auto; text-align: center; padding: 40px; }
#error {
  color: #ff9d7a;
  background: #2a1d1a;
  border: 1px solid #59352a;
  border-radius: 6px;
  padding: 10px 14px;
  margin: 10px auto;
  max-width: 760px;
  white-space: pre-wrap;
}

/* ---- status bar ---- */
#status {
  grid-area: status;
  padding: 5px 14px;
  background: var(--panel);
  border-top: 1px solid var(--line);
  color: var(--muted);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
"#
}

/// The client script: menu, navigation, zoom, and page-by-page rendering.
///
/// Long for the same reason as [`css`]: it is one program, and the handlers
/// it wires up read in order.
#[allow(clippy::too_many_lines)]
#[must_use]
pub(crate) fn script() -> &'static str {
    "
// Everything comes from the local server; nothing is fetched from a network.
const LIST_URL = '/api/documents';
const VIEW_URL = '/api/document?name=';

const $ = (id) => document.getElementById(id);
const menu = $('menu');
const list = $('menu-list');
const pages = $('pages');
const status = $('status');
const docName = $('doc-name');
const docMeta = $('doc-meta');
const menuButton = $('menu-button');
const filter = $('filter');
const zoom = $('zoom');
const zoomValue = $('zoom-value');

let documents = [];
let current = null;

// Zoom is a fraction of the page's natural CSS size. The renderer emits
// width/height in px at scale 1 and the SVG scales with its element, so
// zooming neither re-renders nor re-fetches.
function applyZoom() {
  const factor = Number(zoom.value) / 100;
  for (const page of pages.querySelectorAll('.page')) {
    page.style.width = (Number(page.dataset.width) * factor) + 'px';
  }
  zoomValue.textContent = zoom.value + '%';
}

function metaBits(view) {
  const bits = [];
  const add = (text, bad) => {
    const span = document.createElement('span');
    if (bad) span.className = 'bad';
    span.textContent = text;
    bits.push(span);
  };
  add(view.conformance, false);
  if (view.pages.length) {
    add(view.pages.length + (view.pages.length === 1 ? ' page' : ' pages'), false);
  }
  if (view.summary) {
    const s = view.summary;
    add('supported ' + s.supported, false);
    add('partial ' + s.partial, false);
    if (s.unsupported) add('unsupported ' + s.unsupported, true);
    if (s.error) add('error ' + s.error, true);
    if (s.ignored) add('ignored ' + s.ignored, false);
  }
  return bits;
}

function showMessage(text, id) {
  pages.replaceChildren();
  const div = document.createElement('div');
  div.id = id;
  div.textContent = text;
  pages.append(div);
}

function renderView(view) {
  docName.textContent = view.name;
  docMeta.replaceChildren(...metaBits(view));
  pages.replaceChildren();

  if (view.note) {
    showMessage(view.name + ' — ' + view.note, 'error');
    status.textContent = 'could not render ' + view.name;
    return;
  }
  if (!view.pages.length) {
    showMessage('this document rendered no pages', 'empty');
    status.textContent = view.name + ' — no pages';
    return;
  }

  for (const page of view.pages) {
    const holder = document.createElement('div');
    holder.className = 'page';
    holder.dataset.width = page.width;
    // Inline SVG rather than <img>: it scales without a re-fetch, and the
    // text layer stays selectable, which matters for reading a render.
    holder.innerHTML = page.svg;
    const label = document.createElement('span');
    label.className = 'number';
    label.textContent = page.number;
    holder.append(label);
    pages.append(holder);
  }
  applyZoom();
  pages.scrollTop = 0;
  const n = view.pages.length;
  status.textContent = n + (n === 1 ? ' page' : ' pages') + ' · ' + view.name;
}

async function open(name) {
  current = name;
  for (const li of list.children) {
    li.setAttribute('aria-current', String(li.dataset.name === name));
  }
  status.textContent = 'rendering ' + name + '…';
  try {
    const response = await fetch(VIEW_URL + encodeURIComponent(name));
    renderView(await response.json());
  } catch (error) {
    showMessage(String(error), 'error');
  }
  closeMenu();
}

function buildMenu(filterText) {
  list.replaceChildren();
  const needle = filterText.trim().toLowerCase();
  const shown = documents.filter((doc) => doc.name.toLowerCase().includes(needle));
  for (const doc of shown) {
    const li = document.createElement('li');
    li.dataset.name = doc.name;
    if (doc.name === current) li.setAttribute('aria-current', 'true');
    const button = document.createElement('button');
    button.type = 'button';
    const name = document.createElement('span');
    name.className = 'name';
    name.textContent = doc.name;
    const size = document.createElement('span');
    size.className = 'size';
    size.textContent = Math.max(1, Math.round(doc.size / 1024)) + ' kB';
    button.append(name, size);
    button.addEventListener('click', () => open(doc.name));
    li.append(button);
    list.append(li);
  }
  $('menu-empty').hidden = shown.length > 0;
}

function openMenu() {
  menu.hidden = false;
  menuButton.setAttribute('aria-expanded', 'true');
  filter.focus();
  filter.select();
}

function closeMenu() {
  menu.hidden = true;
  menuButton.setAttribute('aria-expanded', 'false');
}

menuButton.addEventListener('click', () => {
  if (menu.hidden) openMenu(); else closeMenu();
});
filter.addEventListener('input', () => buildMenu(filter.value));
zoom.addEventListener('input', applyZoom);
$('gaps').addEventListener('change', (event) => {
  document.body.classList.toggle('gaps-off', !event.target.checked);
});
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') closeMenu();
});
// A click outside the menu closes it, as a drawer should.
document.addEventListener('click', (event) => {
  if (menu.hidden) return;
  if (menu.contains(event.target) || menuButton.contains(event.target)) return;
  closeMenu();
});

(async function start() {
  try {
    documents = await (await fetch(LIST_URL)).json();
  } catch (error) {
    showMessage('cannot reach the viewer server: ' + error, 'error');
    return;
  }
  buildMenu('');
  showMessage(
    documents.length
      ? 'choose a document from the menu'
      : 'no .docx files in this directory',
    'empty'
  );
  status.textContent = documents.length + ' document(s)';
  // A corpus with one document has nothing to choose between.
  if (documents.length === 1) open(documents[0].name);
})();
"
}

/// Fills the `#css#` and `#script#` placeholders in the shell.
#[must_use]
pub(crate) fn page() -> String {
    index_html()
        .replace("#css#", css())
        .replace("#script#", script())
}

#[cfg(test)]
mod tests {
    use super::{css, index_html, page, script};

    #[test]
    fn the_shell_has_both_placeholders_and_they_are_filled() {
        let shell = index_html();
        assert!(shell.contains("#css#"));
        assert!(shell.contains("#script#"));
        let rendered = page();
        assert!(!rendered.contains("#css#"));
        assert!(!rendered.contains("#script#"));
        assert!(rendered.contains("StrictLib viewer"));
    }

    #[test]
    fn the_menu_and_the_page_column_exist() {
        let rendered = page();
        for id in ["menu", "menu-list", "filter", "pages", "status", "zoom"] {
            assert!(
                rendered.contains(&format!("id=\"{id}\"")),
                "the shell is missing #{id}"
            );
        }
    }

    #[test]
    fn pages_scroll_and_can_have_their_gaps_switched_off() {
        let styles = css();
        assert!(
            styles.contains("overflow: auto"),
            "the page column must scroll"
        );
        assert!(styles.contains("body.gaps-off"), "gaps must be switchable");
        assert!(styles.contains(".page"), "pages must be delimited");
        assert!(styles.contains("box-shadow"), "page edges must be visible");
    }

    #[test]
    fn the_script_only_talks_to_the_local_server() {
        let code = script();
        // A local viewer must not reach out to any other origin.
        assert!(!code.contains("http://"), "no plaintext origin");
        assert!(!code.contains("https://"), "no external origin");
        assert!(code.contains("'/api/documents'"));
    }

    #[test]
    fn zooming_reuses_the_loaded_pages() {
        let code = script();
        assert!(code.contains("applyZoom"));
        // Zoom must resize the elements, not re-request anything.
        assert!(code.contains("page.style.width"));
    }

    #[test]
    fn a_document_that_failed_to_open_says_so() {
        let code = script();
        assert!(code.contains("view.note"), "a failed render must be shown");
        assert!(code.contains("could not render") || code.contains("could not"));
    }
}
