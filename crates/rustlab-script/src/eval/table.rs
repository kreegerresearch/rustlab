//! Named-column tables.
//!
//! A table is a row-aligned list of columns that may differ in type
//! (numeric, text, or logical). This is the MATLAB subset that fits
//! rustlab's values: construction, dot access, two-subscript selection,
//! one-variable content indexing, size helpers, and plain / HTML display.
//!
//! Deliberately absent: row names, `T.Properties` writes, `T(rows, vars) =`,
//! multi-variable `T{rows, vars}` (a comma-separated list), timetables,
//! joins, `sortrows`, `summary`, `addvars` / `removevars` / `renamevars`,
//! nested tables, categoricals, datetimes, units, and `readtable` /
//! `writetable` / `table2array`. rustlab has no logical arrays, so a 0/1
//! numeric vector stays numeric; only a scalar `true` / `false` is a
//! logical column (one row). A rustlab vector is one column. A matrix
//! becomes one column per matrix column.

use super::value::Value;
use num_complex::Complex;
use rustlab_core::{CMatrix, C64};
use std::collections::HashSet;
use std::fmt;

/// Sentinel wrapped around the HTML form of a table when a notebook is
/// capturing output. REPL display does not emit it. Non-HTML notebook
/// renderers strip the span and keep the plain text.
pub const TABLE_HTML_BEGIN: &str = "\u{E000}rl-table\u{E000}";
pub const TABLE_HTML_END: &str = "\u{E000}/rl-table\u{E000}";

/// One piece of captured output: plain text, or the HTML between the
/// table sentinels (not yet checked).
pub enum OutputPiece<'a> {
    Text(&'a str),
    Table(&'a str),
}

/// Split captured output into plain text and table-HTML spans.
pub fn split_table_output(text: &str) -> Vec<OutputPiece<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(TABLE_HTML_BEGIN) {
        let (before, after) = rest.split_at(i);
        if !before.is_empty() {
            out.push(OutputPiece::Text(before));
        }
        rest = &after[TABLE_HTML_BEGIN.len()..];
        if let Some(j) = rest.find(TABLE_HTML_END) {
            out.push(OutputPiece::Table(&rest[..j]));
            rest = &rest[j + TABLE_HTML_END.len()..];
        } else {
            if !rest.is_empty() {
                out.push(OutputPiece::Text(rest));
            }
            return out;
        }
    }
    if !rest.is_empty() {
        out.push(OutputPiece::Text(rest));
    }
    out
}

/// Drop table-HTML spans, leaving the plain-text display.
pub fn strip_table_html(text: &str) -> String {
    let mut out = String::new();
    for piece in split_table_output(text) {
        if let OutputPiece::Text(t) = piece {
            out.push_str(t);
        }
    }
    out
}

/// True when `html` is a table this module would emit: only the tags
/// [`Table::html`] writes, so a forged sentinel cannot inject a script.
pub fn is_safe_table_html(html: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "<table class=\"rl-table\">",
        "</table>",
        "<thead>",
        "</thead>",
        "<tbody>",
        "</tbody>",
        "<tr>",
        "</tr>",
        "<th>",
        "</th>",
        "<td>",
        "</td>",
        "<td class=\"rl-num\">",
    ];
    let mut rest = html.trim();
    if !rest.starts_with("<table class=\"rl-table\">") || !rest.ends_with("</table>") {
        return false;
    }
    while let Some(start) = rest.find('<') {
        let Some(rel_end) = rest[start..].find('>') else {
            return false;
        };
        let tag = &rest[start..start + rel_end + 1];
        if !ALLOWED.contains(&tag) {
            return false;
        }
        rest = &rest[start + rel_end + 1..];
    }
    true
}

/// One column. Every column in a table has the same length.
#[derive(Debug, Clone)]
pub enum TableColumn {
    Number(Vec<C64>),
    Logical(Vec<bool>),
    Text(Vec<String>),
}

impl TableColumn {
    fn len(&self) -> usize {
        match self {
            TableColumn::Number(v) => v.len(),
            TableColumn::Logical(v) => v.len(),
            TableColumn::Text(v) => v.len(),
        }
    }

    fn take_rows(&self, rows: &[usize]) -> TableColumn {
        match self {
            TableColumn::Number(v) => {
                TableColumn::Number(rows.iter().map(|&r| v[r]).collect())
            }
            TableColumn::Logical(v) => {
                TableColumn::Logical(rows.iter().map(|&r| v[r]).collect())
            }
            TableColumn::Text(v) => {
                TableColumn::Text(rows.iter().map(|&r| v[r].clone()).collect())
            }
        }
    }

    /// Whole column as a script value. A length-1 logical column is a
    /// `Bool`. A longer logical column is a 0/1 vector — rustlab has no
    /// logical array to put it in.
    pub fn to_value(&self) -> Value {
        match self {
            TableColumn::Number(v) => Value::Vector(ndarray::Array1::from_vec(v.clone())),
            TableColumn::Text(v) => Value::StringArray(v.clone()),
            TableColumn::Logical(v) if v.len() == 1 => Value::Bool(v[0]),
            TableColumn::Logical(v) => Value::Vector(ndarray::Array1::from_iter(
                v.iter().map(|&b| Complex::new(if b { 1.0 } else { 0.0 }, 0.0)),
            )),
        }
    }

    /// Rows of this one column, as contents rather than a sub-table.
    fn contents_at(&self, rows: &[usize]) -> Value {
        match self {
            TableColumn::Number(v) => {
                if rows.len() == 1 {
                    let c = v[rows[0]];
                    if c.im.abs() < 1e-12 {
                        Value::Scalar(c.re)
                    } else {
                        Value::Complex(c)
                    }
                } else {
                    Value::Vector(ndarray::Array1::from_iter(rows.iter().map(|&r| v[r])))
                }
            }
            TableColumn::Text(v) => {
                if rows.len() == 1 {
                    Value::Str(v[rows[0]].clone())
                } else {
                    Value::StringArray(rows.iter().map(|&r| v[r].clone()).collect())
                }
            }
            TableColumn::Logical(v) => {
                if rows.len() == 1 {
                    Value::Bool(v[rows[0]])
                } else {
                    Value::Vector(ndarray::Array1::from_iter(rows.iter().map(|&r| {
                        Complex::new(if v[r] { 1.0 } else { 0.0 }, 0.0)
                    })))
                }
            }
        }
    }
}

/// A table: `names[i]` labels `columns[i]`, and every column has the same length.
#[derive(Debug, Clone)]
pub struct Table {
    pub names: Vec<String>,
    pub columns: Vec<TableColumn>,
}

/// One `table(...)` argument after it has been evaluated.
pub struct TableInput {
    /// `Some` when the argument was a bare variable. Expressions get `VarN`.
    pub name: Option<String>,
    pub value: Value,
}

impl Table {
    pub fn height(&self) -> usize {
        self.columns.first().map(TableColumn::len).unwrap_or(0)
    }

    pub fn width(&self) -> usize {
        self.columns.len()
    }

    pub fn column(&self, name: &str) -> Result<Value, String> {
        let i = self
            .names
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| format!("table has no variable '{name}'"))?;
        Ok(self.columns[i].to_value())
    }

    pub fn properties(&self) -> Value {
        let mut fields = std::collections::HashMap::new();
        fields.insert(
            "VariableNames".to_string(),
            Value::StringArray(self.names.clone()),
        );
        Value::Struct(fields)
    }

    pub fn head(&self, n: usize) -> Table {
        let n = n.min(self.height());
        self.slice_rows(&(0..n).collect::<Vec<_>>())
    }

    pub fn tail(&self, n: usize) -> Table {
        let h = self.height();
        let n = n.min(h);
        self.slice_rows(&(h - n..h).collect::<Vec<_>>())
    }

    fn slice_rows(&self, rows: &[usize]) -> Table {
        Table {
            names: self.names.clone(),
            columns: self.columns.iter().map(|c| c.take_rows(rows)).collect(),
        }
    }

    /// `T(rows, vars)` → a sub-table. Exactly two subscripts.
    pub fn select(&self, indices: &[Value]) -> Result<Value, String> {
        let (rows, vars) = self.row_col_positions(indices, "T(rows, vars)")?;
        if vars.is_empty() {
            return Err("table subscript selects no variables".to_string());
        }
        let mut names = Vec::with_capacity(vars.len());
        let mut columns = Vec::with_capacity(vars.len());
        let mut seen = HashSet::new();
        for v in vars {
            let name = self.names[v].clone();
            if !seen.insert(name.clone()) {
                return Err(format!("table subscript selects '{name}' more than once"));
            }
            names.push(name);
            columns.push(self.columns[v].take_rows(&rows));
        }
        Ok(Value::Table(Table { names, columns }))
    }

    /// `T{rows, vars}` → the contents of exactly one variable.
    pub fn contents(&self, indices: &[Value]) -> Result<Value, String> {
        let (rows, vars) = self.row_col_positions(indices, "T{rows, vars}")?;
        if vars.len() != 1 {
            return Err(
                "T{rows, vars} returns one variable. Selecting several would be a \
                 comma-separated list, which rustlab does not support. Use \
                 T(rows, vars) for a sub-table."
                    .to_string(),
            );
        }
        Ok(self.columns[vars[0]].contents_at(&rows))
    }

    fn row_col_positions(
        &self,
        indices: &[Value],
        what: &str,
    ) -> Result<(Vec<usize>, Vec<usize>), String> {
        if indices.len() != 2 {
            return Err(format!(
                "{what} requires two subscripts (rows, variables), got {}",
                indices.len()
            ));
        }
        let indices: Vec<Value> = indices.iter().cloned().map(widen_index).collect();
        let rows = Value::resolve_index_dim_public(&indices[0], self.height())?;
        let vars = self.var_positions(&indices[1])?;
        Ok((rows, vars))
    }

    fn var_positions(&self, idx: &Value) -> Result<Vec<usize>, String> {
        match idx {
            Value::Str(s) => {
                let i = self
                    .names
                    .iter()
                    .position(|n| n == s)
                    .ok_or_else(|| format!("table has no variable '{s}'"))?;
                Ok(vec![i])
            }
            Value::StringArray(ss) => {
                let mut out = Vec::with_capacity(ss.len());
                for s in ss {
                    let i = self
                        .names
                        .iter()
                        .position(|n| n == s)
                        .ok_or_else(|| format!("table has no variable '{s}'"))?;
                    out.push(i);
                }
                Ok(out)
            }
            other => Value::resolve_index_dim_public(other, self.width()),
        }
    }

    /// Replace or append a column. The new column's length must match the
    /// table height. An empty table (no columns) takes the new length.
    pub fn assign_column(&mut self, name: &str, value: Value) -> Result<(), String> {
        if name == "Properties" {
            return Err(
                "table: Properties is read-only. Pass 'VariableNames' to table(...) to name columns."
                    .to_string(),
            );
        }
        check_var_name(name)?;
        let col = column_from_assignment(value)?;
        if self.width() > 0 && col.len() != self.height() {
            return Err(format!(
                "table: column '{name}' has {} rows, expected {}",
                col.len(),
                self.height()
            ));
        }
        if let Some(i) = self.names.iter().position(|n| n == name) {
            self.columns[i] = col;
        } else {
            self.names.push(name.to_string());
            self.columns.push(col);
        }
        Ok(())
    }

    fn ascii(&self) -> String {
        if self.width() == 0 {
            return "0×0 empty table".to_string();
        }
        let cells: Vec<Vec<String>> = (0..self.height())
            .map(|r| self.columns.iter().map(|c| cell_text(c, r).0).collect())
            .collect();
        let aligns: Vec<bool> = self.columns.iter().map(cell_align_right).collect();
        let mut widths: Vec<usize> = self
            .names
            .iter()
            .map(|n| n.chars().count())
            .collect();
        for row in &cells {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
        let mut lines = Vec::new();
        lines.push(format_row(&self.names, &widths, &vec![false; widths.len()]));
        let bars: Vec<String> = widths.iter().map(|&w| "_".repeat(w.max(1))).collect();
        lines.push(format_row(&bars, &widths, &vec![false; widths.len()]));
        lines.push(String::new());
        if self.height() == 0 {
            lines.push(format!("0×{} table", self.width()));
        } else {
            for row in &cells {
                lines.push(format_row(row, &widths, &aligns));
            }
        }
        lines.join("\n")
    }

    /// HTML table. Cell text is escaped. Tags stay inside [`is_safe_table_html`].
    pub fn html(&self) -> String {
        if self.width() == 0 {
            return "<table class=\"rl-table\"><tbody><tr><td>0×0 empty table</td></tr></tbody></table>"
                .to_string();
        }
        let mut html = String::from("<table class=\"rl-table\"><thead><tr>");
        for name in &self.names {
            html.push_str("<th>");
            html.push_str(&escape_html(name));
            html.push_str("</th>");
        }
        html.push_str("</tr></thead><tbody>");
        for r in 0..self.height() {
            html.push_str("<tr>");
            for col in &self.columns {
                let (text, numeric) = cell_text(col, r);
                if numeric {
                    html.push_str("<td class=\"rl-num\">");
                } else {
                    html.push_str("<td>");
                }
                html.push_str(&escape_html(&text));
                html.push_str("</td>");
            }
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table>");
        html
    }
}

impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ascii())?;
        if rustlab_plot::plot_context() == rustlab_plot::PlotContext::Notebook {
            write!(
                f,
                "\n{TABLE_HTML_BEGIN}{}{TABLE_HTML_END}",
                self.html()
            )?;
        }
        Ok(())
    }
}

/// Build a table from evaluated arguments. `var_names`, when set, replaces
/// every generated name and must have one entry per column.
pub fn build_table(inputs: Vec<TableInput>, var_names: Option<Vec<String>>) -> Result<Table, String> {
    let mut named: Vec<(String, TableColumn)> = Vec::new();
    let mut height: Option<usize> = None;
    let mut next_var = 1usize;
    for input in inputs {
        let pieces = expand_value(input.value)?;
        let multi = pieces.len() > 1;
        for (k, col) in pieces.into_iter().enumerate() {
            let name = match (&input.name, multi) {
                (Some(stem), true) => format!("{stem}{}", k + 1),
                (Some(stem), false) => stem.clone(),
                (None, _) => {
                    let n = format!("Var{next_var}");
                    next_var += 1;
                    n
                }
            };
            let n = col.len();
            if let Some(h) = height {
                if h != n {
                    return Err(format!("table: '{name}' has {n} rows, expected {h}"));
                }
            } else {
                height = Some(n);
            }
            named.push((name, col));
        }
    }
    if let Some(override_names) = var_names {
        if override_names.len() != named.len() {
            return Err(format!(
                "table: VariableNames has {} names for {} variables",
                override_names.len(),
                named.len()
            ));
        }
        for (i, name) in override_names.into_iter().enumerate() {
            named[i].0 = name;
        }
    }
    let mut seen = HashSet::new();
    for (name, _) in &named {
        check_var_name(name)?;
        if !seen.insert(name.clone()) {
            return Err(format!("table: duplicate variable name '{name}'"));
        }
    }
    let (names, columns) = named.into_iter().unzip();
    Ok(Table { names, columns })
}

pub fn is_varnames(s: &str) -> bool {
    s.eq_ignore_ascii_case("VariableNames")
}

pub fn names_from_value(v: Value) -> Result<Vec<String>, String> {
    match v {
        Value::StringArray(s) => Ok(s),
        Value::Str(s) => Ok(vec![s]),
        other => Err(format!(
            "table: VariableNames must be a string or a string array, got {}",
            other.type_name()
        )),
    }
}

fn check_var_name(name: &str) -> Result<(), String> {
    if name == "Properties" {
        return Err(
            "table: 'Properties' is reserved (it is the metadata field, not a column)"
                .to_string(),
        );
    }
    let mut chars = name.chars();
    let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        return Err(format!(
            "table: '{name}' is not a valid variable name (letter, then letters, digits, or _)"
        ));
    }
    Ok(())
}

fn widen_index(v: Value) -> Value {
    match v {
        Value::Int { .. } | Value::IntArray { .. } => Value::widen_int(v),
        other => other,
    }
}

/// Construction: a matrix becomes one numeric column per matrix column.
/// A vector, scalar, string, string array, or logical scalar is one column.
fn expand_value(v: Value) -> Result<Vec<TableColumn>, String> {
    match v {
        Value::Matrix(m) => matrix_columns(&m),
        Value::IntArray { data, rows, cols, .. } => {
            if cols == 0 {
                return Err(
                    "table: a matrix with no columns cannot be a table variable".to_string(),
                );
            }
            let mut out = Vec::with_capacity(cols);
            for c in 0..cols {
                let mut col = Vec::with_capacity(rows);
                for r in 0..rows {
                    col.push(Complex::new(data[r * cols + c] as f64, 0.0));
                }
                out.push(TableColumn::Number(col));
            }
            Ok(out)
        }
        Value::Bool(b) => Ok(vec![TableColumn::Logical(vec![b])]),
        Value::Str(s) => Ok(vec![TableColumn::Text(vec![s])]),
        Value::StringArray(s) => Ok(vec![TableColumn::Text(s)]),
        Value::Vector(v) => Ok(vec![TableColumn::Number(v.iter().copied().collect())]),
        Value::Scalar(n) => Ok(vec![TableColumn::Number(vec![Complex::new(n, 0.0)])]),
        Value::Complex(c) => Ok(vec![TableColumn::Number(vec![c])]),
        Value::Int { data, .. } => {
            Ok(vec![TableColumn::Number(vec![Complex::new(data as f64, 0.0)])])
        }
        other => Err(format!(
            "table: cannot make a column from a {}",
            other.type_name()
        )),
    }
}

fn matrix_columns(m: &CMatrix) -> Result<Vec<TableColumn>, String> {
    if m.ncols() == 0 {
        return Err("table: a matrix with no columns cannot be a table variable".to_string());
    }
    let mut cols = Vec::with_capacity(m.ncols());
    for c in 0..m.ncols() {
        let data: Vec<C64> = (0..m.nrows()).map(|r| m[[r, c]]).collect();
        cols.push(TableColumn::Number(data));
    }
    Ok(cols)
}

/// Dot assignment stores one column. A multi-column matrix is rejected
/// rather than silently split into several variables.
fn column_from_assignment(v: Value) -> Result<TableColumn, String> {
    match v {
        Value::Bool(b) => Ok(TableColumn::Logical(vec![b])),
        Value::Str(s) => Ok(TableColumn::Text(vec![s])),
        Value::StringArray(s) => Ok(TableColumn::Text(s)),
        Value::Matrix(m) if m.nrows() > 1 && m.ncols() > 1 => Err(
            "table: column assignment expects a vector or a single column, not a multi-column matrix"
                .to_string(),
        ),
        Value::IntArray { rows, cols, .. } if rows > 1 && cols > 1 => Err(
            "table: column assignment expects a vector or a single column, not a multi-column matrix"
                .to_string(),
        ),
        other => {
            let vec = other.to_cvector().map_err(|_| {
                format!(
                    "table: cannot assign a {} as a column",
                    other.type_name()
                )
            })?;
            Ok(TableColumn::Number(vec.iter().copied().collect()))
        }
    }
}

fn cell_align_right(col: &TableColumn) -> bool {
    matches!(col, TableColumn::Number(_))
}

/// `(text, numeric)`. Numeric cells are right-aligned and use `td.rl-num`.
fn cell_text(col: &TableColumn, row: usize) -> (String, bool) {
    match col {
        TableColumn::Number(v) => (format_number(v[row]), true),
        TableColumn::Logical(v) => {
            (if v[row] { "true" } else { "false" }.to_string(), false)
        }
        TableColumn::Text(v) => (sanitize_sentinel(&v[row]), false),
    }
}

fn format_number(c: C64) -> String {
    if c.im.abs() < 1e-12 {
        sanitize_sentinel(&format!("{}", c.re))
    } else if c.im >= 0.0 {
        sanitize_sentinel(&format!("{}+{}j", c.re, c.im))
    } else {
        sanitize_sentinel(&format!("{}{}j", c.re, c.im))
    }
}

/// The HTML sentinel uses U+E000. A cell that contains it would split the
/// captured output, so the visible text substitutes a replacement character.
fn sanitize_sentinel(s: &str) -> String {
    s.replace('\u{E000}', "\u{FFFD}")
}

fn format_row(cells: &[String], widths: &[usize], right: &[bool]) -> String {
    let mut line = String::from("  ");
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            line.push_str("  ");
        }
        let w = widths[i];
        let n = cell.chars().count();
        let pad = w.saturating_sub(n);
        if right[i] {
            line.push_str(&" ".repeat(pad));
            line.push_str(cell);
        } else {
            line.push_str(cell);
            line.push_str(&" ".repeat(pad));
        }
    }
    line
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\u{E000}' => out.push_str("&#57344;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escapes_cells_and_stays_on_the_allowlist() {
        let t = build_table(
            vec![TableInput {
                name: Some("name".into()),
                value: Value::StringArray(vec!["<b>".into(), "a&b".into()]),
            }],
            None,
        )
        .unwrap();
        let html = t.html();
        assert!(html.contains("&lt;b&gt;"), "{html}");
        assert!(html.contains("a&amp;b"), "{html}");
        assert!(!html.contains("<b>"), "{html}");
        assert!(is_safe_table_html(&html), "{html}");
        assert!(!is_safe_table_html("<table class=\"rl-table\"><script></script></table>"));
    }
}
