use colored::*;
use std::path::Path;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity};

pub trait Reporter: Send + Sync {
    fn report_project_start(&self, project_name: &str);
    fn report_diagnostic(&self, path: &Path, diagnostic: &Diagnostic, verbose: bool);
    fn report_duplicate_operation(&self, op_name: &str, project_name: &str, paths: &[&Path]);
    fn report_error(&self, message: &str);
    fn report_success(&self, verbose: bool);
    fn report_failure(&self);
}

pub struct DefaultReporter;

impl Reporter for DefaultReporter {
    fn report_project_start(&self, project_name: &str) {
        println!("Checking project: {}", project_name.blue());
    }

    fn report_diagnostic(&self, path: &Path, diagnostic: &Diagnostic, verbose: bool) {
        let is_issue = matches!(
            diagnostic.severity,
            Some(DiagnosticSeverity::ERROR) | Some(DiagnosticSeverity::WARNING)
        );

        if is_issue || verbose {
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

            let rendered = format!(
                "File: {}\n  [{}:{}] {}: {}",
                path.display().to_string().blue(),
                (diagnostic.range.start.line + 1).to_string().bright_black(),
                (diagnostic.range.start.character + 1)
                    .to_string()
                    .bright_black(),
                severity_label,
                colored_msg
            );
            if is_issue {
                eprintln!("{rendered}");
            } else {
                println!("{rendered}");
            }
        }
    }

    fn report_duplicate_operation(&self, op_name: &str, project_name: &str, paths: &[&Path]) {
        eprintln!(
            "\n{} Duplicate operation name '{}' in project {}:",
            "Error:".red(),
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

    fn report_success(&self, verbose: bool) {
        if verbose {
            println!("\n{}", "Scan complete.".bright_black());
        } else {
            println!("{}", "No issues found.".green());
        }
    }

    fn report_failure(&self) {
        eprintln!("\n{}", "Check failed.".red());
    }
}

pub struct GitHubReporter;

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
        let message = diagnostic.message.replace('\n', "%0A");

        println!(
            "::{} file={},line={},col={}::{}",
            severity, file, line, col, message
        );
    }

    fn report_duplicate_operation(&self, op_name: &str, project_name: &str, paths: &[&Path]) {
        for path in paths {
            let file = path.to_string_lossy();
            println!(
                "::error file={}::Duplicate operation name '{}' in project {}",
                file, op_name, project_name
            );
        }
    }

    fn report_error(&self, message: &str) {
        println!("::error::{}", message.replace('\n', "%0A"));
    }

    fn report_success(&self, _verbose: bool) {
        // No special output for success in GitHub reporter
    }

    fn report_failure(&self) {
        // GitHub Actions will see the non-zero exit code
    }
}

pub struct TscReporter;

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
        let message = &diagnostic.message;

        println!("{}({},{}): {}: {}", file, line, col, severity, message);
    }

    fn report_duplicate_operation(&self, op_name: &str, project_name: &str, paths: &[&Path]) {
        for path in paths {
            let file = path.to_string_lossy();
            println!(
                "{}: error: Duplicate operation name '{}' in project {}",
                file, op_name, project_name
            );
        }
    }

    fn report_error(&self, message: &str) {
        eprintln!("error: {}", message);
    }

    fn report_success(&self, _verbose: bool) {
        // No special output for success
    }

    fn report_failure(&self) {
        // Exit code handles failure
    }
}
