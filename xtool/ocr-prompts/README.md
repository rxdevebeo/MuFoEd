# Prompts for `hostq` `ocr-probe`

`prompt_file` of an `ocr-probe` job: the `layout_probe` example asks the model
this instead of its built-in layout prompt. Compare variants on the same pages
to see what a prompt changes.

- `layout-lang.txt` - the built-in layout prompt plus a `language` tag per
  block: does the model tell the languages of a page apart by itself?
- `layout-sr.txt` - the same plus a hint that the text is Serbian in both
  scripts, with the letters it must keep: does a stated language keep the
  diacritics (`Đorđe`, `nemački`) the plain prompt lost?
