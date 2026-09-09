use colored::*;
use graphox_core::Config;
use graphox_core::config::Severity;
use std::path::Path;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, Uri};

/// The path a related location points at, displayed the same way as the primary
/// path of the diagnostic it belongs to.
///
/// Both go through `Config::relativize`, so both are relative to the same root.
/// Relativizing this one against the working directory instead made the two
/// disagree whenever `check` ran from a subdirectory, and the shorter of them
/// was not a path anyone could follow from what was printed beside it.
fn related_path(config: &Config, uri: &Uri) -> String {
    let Some(path) = graphox_core::utils::uri_to_path(uri) else {
        return uri.as_str().to_string();
    };
    config.relativize(&path).display().to_string()
}

/// Extra locations a finding covers, folded into the message for reporters
/// whose format carries one location per diagnostic.
fn related_summary(config: &Config, diagnostic: &Diagnostic) -> String {
    let related = diagnostic
        .related_information
        .as_deref()
        .unwrap_or_default();
    if related.is_empty() {
        return String::new();
    }
    let places: Vec<String> = related
        .iter()
        .map(|r| {
            format!(
                "{}:{}",
                related_path(config, &r.location.uri),
                r.location.range.start.line + 1
            )
        })
        .collect();
    format!(" Also at: {}.", places.join(", "))
}

pub trait Reporter: Send + Sync {
    fn report_project_start(&self, project_name: &str);
    fn report_diagnostic(&self, path: &Path, diagnostic: &Diagnostic, verbose: bool);
    fn report_duplicate_operation(
        &self,
        op_name: &str,
        project_name: &str,
        paths: &[&Path],
        severity: Severity,
    );
    fn report_error(&self, message: &str);
    /// `below_threshold` counts diagnostics that were reported but sat under
    /// `--fail-on`, so a clean exit does not have to claim a clean run.
    fn report_success(&self, verbose: bool, below_threshold: usize);
    fn report_failure(&self);
}

pub struct DefaultReporter {
    config: Config,
}

impl DefaultReporter {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Reporter for DefaultReporter {
    fn report_project_start(&self, project_name: &str) {
        println!("Checking project: {}", project_name.blue());
    }

    fn report_diagnostic(&self, path: &Path, diagnostic: &Diagnostic, verbose: bool) {
        // Anything a rule was configured to report is shown. A hint is not
        // something anyone asked for by name, so it stays behind --verbose.
        let is_shown = matches!(
            diagnostic.severity,
            Some(DiagnosticSeverity::ERROR)
                | Some(DiagnosticSeverity::WARNING)
                | Some(DiagnosticSeverity::INFORMATION)
        );
        // Only a problem belongs on stderr; advice belongs with the rest of the
        // output.
        let is_issue = matches!(
            diagnostic.severity,
            Some(DiagnosticSeverity::ERROR) | Some(DiagnosticSeverity::WARNING)
        );

        if is_shown || verbose {
            let (severity_label, colored_msg) = match diagnostic.severity {
                Some(DiagnosticSeverity::ERROR) => ("Error".red(), diagnostic.message.red()),
                Some(DiagnosticSeverity::WARNING) => {
                    ("Warning".yellow(), diagnostic.message.yellow())
                }
                Some(DiagnosticSeverity::INFORMATION) => {
                    ("Info".bright_black(), diagnostic.message.bright_black())
                }
                Some(DiagnosticSeverity::HINT) => {
                    ("Hint".bright_black(), diagnostic.message.bright_black())
                }
                _ => ("Diagnostic".normal(), diagnostic.message.normal()),
            };

            let mut rendered = format!(
                "File: {}\n  [{}:{}] {}: {}",
                path.display().to_string().blue(),
                (diagnostic.range.start.line + 1).to_string().bright_black(),
                (diagnostic.range.start.character + 1)
                    .to_string()
                    .bright_black(),
                severity_label,
                colored_msg
            );

            // A finding that covers several places lists them, so the reader
            // does not have to run a search to find the rest.
            for related in diagnostic.related_information.iter().flatten() {
                rendered.push_str(&format!(
                    "\n    {} [{}:{}] {}",
                    related_path(&self.config, &related.location.uri).bright_black(),
                    (related.location.range.start.line + 1)
                        .to_string()
                        .bright_black(),
                    (related.location.range.start.character + 1)
                        .to_string()
                        .bright_black(),
                    related.message.bright_black()
                ));
            }
            if is_issue {
                eprintln!("{rendered}");
            } else {
                println!("{rendered}");
            }
        }
    }

    fn report_duplicate_operation(
        &self,
        op_name: &str,
        project_name: &str,
        paths: &[&Path],
        severity: Severity,
    ) {
        let label = match severity {
            Severity::Error => "Error:".red(),
            Severity::Warning => "Warning:".yellow(),
            Severity::Info => "Info:".bright_black(),
        };
        eprintln!(
            "\n{} Duplicate operation name '{}' in project {}:",
            label,
            op_name.yellow(),
            project_name.blue()
        );
        for path in paths {
            eprintln!("  - {}", path.display().to_string().blue());
        }
    }

    fn report_error(&self, message: &str) {
        eprintln!("{}", message.red());
    }

    fn report_success(&self, verbose: bool, below_threshold: usize) {
        if verbose {
            println!("\n{}", "Scan complete.".bright_black());
        } else if below_threshold > 0 {
            let noun = if below_threshold == 1 {
                "issue"
            } else {
                "issues"
            };
            println!(
                "{}",
                format!("{below_threshold} {noun} reported, none above the failure threshold.")
                    .yellow()
            );
        } else {
            println!("{}", "No issues found.".green());
        }
    }

    fn report_failure(&self) {
        eprintln!("\n{}", "Check failed.".red());
    }
}

pub struct GitHubReporter {
    config: Config,
}

impl GitHubReporter {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Reporter for GitHubReporter {
    fn report_project_start(&self, _project_name: &str) {
        // GitHub annotations don't really need a project start message,
        // but we can log it to stderr or as an info message if we want.
    }

    fn report_diagnostic(&self, path: &Path, diagnostic: &Diagnostic, _verbose: bool) {
        let severity = match diagnostic.severity {
            Some(DiagnosticSeverity::ERROR) => "error",
            Some(DiagnosticSeverity::WARNING) => "warning",
            _ => "notice", // For info/hint
        };

        let file = path.to_string_lossy();
        let line = diagnostic.range.start.line + 1;
        let col = diagnostic.range.start.character + 1;
        // An annotation points at one place, so any others are named in the
        // body rather than dropped.
        let message = format!(
            "{}{}",
            diagnostic.message,
            related_summary(&self.config, diagnostic)
        )
        .replace('\n', "%0A");

        println!(
            "::{} file={},line={},col={}::{}",
            severity, file, line, col, message
        );
    }

    fn report_duplicate_operation(
        &self,
        op_name: &str,
        project_name: &str,
        paths: &[&Path],
        severity: Severity,
    ) {
        let level = match severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "notice",
        };
        for path in paths {
            let file = path.to_string_lossy();
            println!(
                "::{} file={}::Duplicate operation name '{}' in project {}",
                level, file, op_name, project_name
            );
        }
    }

    fn report_error(&self, message: &str) {
        println!("::error::{}", message.replace('\n', "%0A"));
    }

    fn report_success(&self, _verbose: bool, _below_threshold: usize) {
        // No special output for success in GitHub reporter
    }

    fn report_failure(&self) {
        // GitHub Actions will see the non-zero exit code
    }
}

pub struct TscReporter {
    config: Config,
}

impl TscReporter {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Reporter for TscReporter {
    fn report_project_start(&self, _project_name: &str) {
        // No project start message for tsc format
    }

    fn report_diagnostic(&self, path: &Path, diagnostic: &Diagnostic, _verbose: bool) {
        let severity = match diagnostic.severity {
            Some(DiagnosticSeverity::ERROR) => "error",
            Some(DiagnosticSeverity::WARNING) => "warning",
            Some(DiagnosticSeverity::INFORMATION) => "info",
            _ => "suggestion",
        };

        let file = path.to_string_lossy();
        let line = diagnostic.range.start.line + 1;
        let col = diagnostic.range.start.character + 1;
        let message = format!(
            "{}{}",
            diagnostic.message,
            related_summary(&self.config, diagnostic)
        );

        println!("{}({},{}): {}: {}", file, line, col, severity, message);
    }

    fn report_duplicate_operation(
        &self,
        op_name: &str,
        project_name: &str,
        paths: &[&Path],
        severity: Severity,
    ) {
        let level = match severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        for path in paths {
            let file = path.to_string_lossy();
            println!(
                "{}: {}: Duplicate operation name '{}' in project {}",
                file, level, op_name, project_name
            );
        }
    }

    fn report_error(&self, message: &str) {
        eprintln!("error: {}", message);
    }

    fn report_success(&self, _verbose: bool, _below_threshold: usize) {
        // No special output for success
    }

    fn report_failure(&self) {
        // Exit code handles failure
    }
}
