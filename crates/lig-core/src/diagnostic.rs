//! Diagnostics with spans, machine-applicable suggestions, and miette
//! rendering (D-7, §12.2: NFR-5 — `file:line:column`, offending text, reason,
//! suggestion, per-code URL).

use crate::span::Span;
use miette::{Diagnostic, GraphicalReportHandler, LabeledSpan, NamedSource, SourceCode};

/// Diagnostic severity: the admission ladder's Deny vs Lint tiers (§6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Hard error — fail-closed, zero output (BR-1).
    Error,
    /// Lowered but flagged (`LIG-W###`); CI can promote via `--deny=warn`.
    Warning,
}

/// A Ligature diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LigDiagnostic {
    /// Severity tier.
    pub severity: Severity,
    /// Stable code, e.g. `LIG-E001`.
    pub code: String,
    /// One-line message.
    pub message: String,
    /// Optional concrete suggestion (rendered as `help:`).
    pub help: Option<String>,
    /// Byte range in `source`.
    pub span: Span,
}

impl LigDiagnostic {
    /// A Deny-tier diagnostic.
    #[must_use]
    pub fn error(code: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            code: code.into(),
            message: message.into(),
            help: None,
            span,
        }
    }

    /// A Lint-tier diagnostic.
    #[must_use]
    pub fn warning(code: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.into(),
            message: message.into(),
            help: None,
            span,
        }
    }

    /// Attach a concrete suggestion.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// The catalog URL for this code (§12.2).
    #[must_use]
    pub fn url(&self) -> String {
        crate::error_code::url(&self.code)
    }
}

impl core::fmt::Display for LigDiagnostic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LigDiagnostic {}

impl Diagnostic for LigDiagnostic {
    fn code<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        Some(Box::new(&self.code))
    }

    fn url<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        Some(Box::new(crate::error_code::url(&self.code)))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        self.help
            .as_ref()
            .map(|h| Box::new(h) as Box<dyn core::fmt::Display>)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(core::iter::once(LabeledSpan::at(
            self.span.start as usize..self.span.end as usize,
            &self.message,
        ))))
    }
}

/// A diagnostic paired with its source, for graphical rendering.
struct WithSource<'a> {
    diag: &'a LigDiagnostic,
    src: &'a dyn SourceCode,
}

impl core::fmt::Debug for WithSource<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WithSource")
            .field("diag", self.diag)
            .finish_non_exhaustive()
    }
}

impl core::fmt::Display for WithSource<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self.diag, f)
    }
}

impl std::error::Error for WithSource<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl Diagnostic for WithSource<'_> {
    fn code<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        self.diag.code()
    }

    fn url<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        Diagnostic::url(self.diag)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        self.diag.help()
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        self.diag.labels()
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(self.src)
    }
}

/// Render a diagnostic as miette's graphical report (code frame ± context with
/// a caret). This is the NFR-5 fixture format.
///
/// # Panics
/// Never — rendering failures degrade to a plain message line.
#[must_use]
pub fn render(source_name: &str, source: &str, diag: &LigDiagnostic) -> String {
    let src: NamedSource<String> = NamedSource::new(source_name, source.to_string());
    render_with_source(&src, diag)
}

/// Render against explicit source text (convenience overload of [`render`]).
#[must_use]
pub fn render_with_source(source: &dyn SourceCode, diag: &LigDiagnostic) -> String {
    let handler = GraphicalReportHandler::new();
    let mut out = String::new();
    let wrapped = WithSource { diag, src: source };
    if handler.render_report(&mut out, &wrapped).is_err() {
        out = format!("{}: {}", diag.code, diag.message);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{LigDiagnostic, Severity, render, render_with_source};
    use crate::span::Span;

    const SRC: &str =
        "export function App() {\n  return <ul>{items.filter(i => i.active)}</ul>;\n}\n";

    #[test]
    fn error_diagnostic_carries_code_url_and_help() {
        let d = LigDiagnostic::error(
            "LIG-E114",
            "function call with callback not admitted in view expressions",
            Span::new(31, 57),
        )
        .with_help("move filtering into a memo in the model section");
        assert_eq!(d.severity, Severity::Error);
        assert!(d.url().ends_with("/e/LIG-E114"));
    }

    #[test]
    fn render_contains_code_frame_caret_and_url() {
        let d = LigDiagnostic::error(
            "LIG-E114",
            "function call with callback not admitted in view expressions",
            Span::new(31, 57),
        )
        .with_help("move filtering into a memo in the model section, then read it here");
        let rendered = render("src/todos.tsx", SRC, &d);
        assert!(rendered.contains("LIG-E114"), "code in header:\n{rendered}");
        assert!(rendered.contains("src/todos.tsx"), "file name:\n{rendered}");
        assert!(
            rendered.contains("ligature.dev/e/LIG-E114"),
            "url footer:\n{rendered}"
        );
        assert!(
            rendered.contains("items.filter"),
            "offending text:\n{rendered}"
        );
        insta::assert_snapshot!("e114_code_frame", rendered);
    }

    #[test]
    fn warning_renders_with_badge() {
        let d =
            LigDiagnostic::warning("LIG-W101", "dynamic spread on hot element", Span::new(0, 6));
        let src = miette::NamedSource::new("src/todos.tsx", SRC.to_string());
        let rendered = render_with_source(&src, &d);
        assert!(rendered.contains("LIG-W101"));
        assert!(rendered.contains("dynamic spread on hot element"));
    }
}
