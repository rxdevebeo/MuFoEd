# LaTeX corpus 023 regression fragments

The two XML fragments are the Strict OMML output of importing CC0_TEX case
`part2/projects/023_2408.03243/mainMB06Aug24.tex` (density/pressure array near
lines 492-495, perturbation equation array near lines 613-617).
They are wrapped by `common::document` in `tests/math.rs`. The tests are
self-contained and require neither the original corpus nor a TeX installation.

These regressions check row baselines/extents and scalar paint multiplicity;
the XML is not an assertion of complete TeX-import fidelity.

`latex023-clearance.xml` contains 34 imported paragraphs from the same source
that contain an equation array or matrix. It exercises nested fractions,
multiple rows and display-line wrapping against the painted-ink clearance
diagnostic. It is also self-contained; it is not a complete document or a
reference rendering from TeX.
