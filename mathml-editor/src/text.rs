//! The words the widget shows. English and Russian are built in; a host with
//! its own catalogs replaces any field.

/// Interface strings: tooltips, the source button, the window title and the heading.
///
/// [`Self::for_code`] picks Russian or English. Every field is public, so a
/// host such as `MuFoEd` can write its own translation over the built-in one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    /// Window title.
    pub title: String,
    /// Heading above the formula.
    pub heading: String,
    /// Tooltip for undo.
    pub undo: String,
    /// Tooltip for redo.
    pub redo: String,
    /// Tooltip for a superscript.
    pub superscript: String,
    /// Tooltip for a subscript.
    pub subscript: String,
    /// Tooltip for a fraction.
    pub fraction: String,
    /// Tooltip for a square root.
    pub square_root: String,
    /// Tooltip for an nth root.
    pub nth_root: String,
    /// Tooltip for a sum.
    pub sum: String,
    /// Tooltip for an integral.
    pub integral: String,
    /// Tooltip for a product.
    pub product: String,
    /// Tooltip for the bracket menu.
    pub parentheses: String,
    /// Tooltip for the matrix menu.
    pub matrix: String,
    /// Tooltip for a mark above an expression.
    pub overline: String,
    /// Tooltip for a limit.
    pub limit: String,
    /// Tooltip for sine.
    pub sine: String,
    /// Tooltip for cosine.
    pub cosine: String,
    /// Tooltip for the symbol menu.
    pub symbols: String,
    /// Shown in the matrix menu until a cell is hovered.
    pub pick_cells: String,
    /// Applies the edited `MathML` source.
    pub apply: String,
}

impl Text {
    /// English strings.
    #[must_use]
    pub fn english() -> Self {
        Self {
            title: "MathML editor".to_owned(),
            heading: "Formula".to_owned(),
            undo: "Undo".to_owned(),
            redo: "Redo".to_owned(),
            superscript: "Superscript (^)".to_owned(),
            subscript: "Subscript (_)".to_owned(),
            fraction: "Fraction".to_owned(),
            square_root: "Square root".to_owned(),
            nth_root: "nth root".to_owned(),
            sum: "Sum".to_owned(),
            integral: "Integral".to_owned(),
            product: "Product".to_owned(),
            parentheses: "Brackets".to_owned(),
            matrix: "Matrix".to_owned(),
            overline: "Above".to_owned(),
            limit: "Limit".to_owned(),
            sine: "Sine".to_owned(),
            cosine: "Cosine".to_owned(),
            symbols: "Letters and signs".to_owned(),
            pick_cells: "Hover a cell".to_owned(),
            apply: "Apply".to_owned(),
        }
    }

    /// Russian strings.
    #[must_use]
    pub fn russian() -> Self {
        Self {
            title: "Редактор MathML".to_owned(),
            heading: "Формула".to_owned(),
            undo: "Отменить".to_owned(),
            redo: "Повторить".to_owned(),
            superscript: "Верхний индекс (^)".to_owned(),
            subscript: "Нижний индекс (_)".to_owned(),
            fraction: "Дробь".to_owned(),
            square_root: "Квадратный корень".to_owned(),
            nth_root: "Корень n-й степени".to_owned(),
            sum: "Сумма".to_owned(),
            integral: "Интеграл".to_owned(),
            product: "Произведение".to_owned(),
            parentheses: "Скобки".to_owned(),
            matrix: "Матрица".to_owned(),
            overline: "Знак сверху".to_owned(),
            limit: "Предел".to_owned(),
            sine: "Синус".to_owned(),
            cosine: "Косинус".to_owned(),
            symbols: "Буквы и знаки".to_owned(),
            pick_cells: "Наведите на клетки".to_owned(),
            apply: "Применить".to_owned(),
        }
    }

    /// Russian when `code` starts with `ru`, English otherwise.
    ///
    /// `ru`, `ru-RU` and `RU_ru` all select Russian. Any other code, including
    /// an empty one, selects English.
    #[must_use]
    pub fn for_code(code: &str) -> Self {
        let code = code.split(['-', '_']).next().unwrap_or(code);
        if code.eq_ignore_ascii_case("ru") {
            Self::russian()
        } else {
            Self::english()
        }
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::english()
    }
}

#[cfg(test)]
mod tests {
    use super::Text;

    #[test]
    fn a_russian_code_selects_russian() {
        assert_eq!(Text::for_code("ru-RU").title, Text::russian().title);
        assert_eq!(Text::for_code("en").heading, "Formula");
        assert_ne!(Text::english().undo, Text::russian().undo);
    }
}
