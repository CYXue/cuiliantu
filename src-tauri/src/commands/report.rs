//! CSV processing report writer.
//!
//! Split out of the former `image_commands.rs` monolith.

use std::io::Write;
use std::path::Path;

use crate::image::processor::ProcessingResult;

/// Write a processing report as CSV.
///
/// One row per processed file with the columns a user actually wants to audit:
/// source, destination, sizes, reduction, success/skip/target flags and any
/// error. Fields containing commas, quotes or newlines are quoted (RFC 4180).
///
/// Text fields that could be interpreted as spreadsheet formulas are
/// neutralized: a cell starting with `=`, `+`, `-`, `@`, TAB or CR makes
/// Excel / LibreOffice treat the rest as a formula or macro invocation. A file
/// named `=HYPERLINK(...)` must not become an executable cell when the report
/// is opened, so such fields get the standard single-quote prefix.
pub fn write_report_csv(results: &[ProcessingResult], path: &Path) -> Result<(), String> {
    let mut file =
        std::fs::File::create(path).map_err(|e| format!("Failed to create report: {}", e))?;
    writeln!(
        file,
        "original_path,output_path,original_size,output_size,reduction_percent,success,skipped,within_target,error"
    )
    .map_err(|e| e.to_string())?;

    for r in results {
        let escape = |s: &str| -> String {
            // Neutralize formula injection first, then apply RFC 4180 quoting
            // to whatever the sanitized text turned out to be.
            const FORMULA_PREFIXES: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];
            let sanitized = if s
                .chars()
                .next()
                .is_some_and(|c| FORMULA_PREFIXES.contains(&c))
            {
                format!("'{s}")
            } else {
                s.to_string()
            };
            if sanitized.contains(',') || sanitized.contains('"') || sanitized.contains('\n') {
                format!("\"{}\"", sanitized.replace('"', "\"\""))
            } else {
                sanitized
            }
        };
        let error = r.error.as_deref().unwrap_or("");
        writeln!(
            file,
            "{},{},{},{},{:.2},{},{},{},{}",
            escape(&r.original_path),
            escape(&r.output_path),
            r.original_size,
            r.output_size,
            r.reduction_percent,
            r.success,
            r.skipped,
            r.within_target,
            escape(error),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a unique scratch directory for one test.
    fn scratch_dir(name: &str) -> std::path::PathBuf {
        static TEST_DIR_COUNTER: std::sync::atomic::AtomicU32 =
            std::sync::atomic::AtomicU32::new(0);
        let id = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("clt-{}-{}-{}", name, std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("failed to create scratch dir");
        dir
    }

    #[test]
    fn write_report_csv_produces_escaped_rows() {
        let dir = scratch_dir("report-csv");
        let path = dir.join("report.csv");
        let results = vec![
            ProcessingResult {
                original_path: "a.png".into(),
                output_path: "a.webp".into(),
                original_size: 1000,
                output_size: 500,
                reduction_percent: 50.0,
                success: true,
                error: None,
                skipped: false,
                within_target: true,
            },
            ProcessingResult {
                original_path: "b.png".into(),
                output_path: String::new(),
                original_size: 200,
                output_size: 200,
                reduction_percent: 0.0,
                success: true,
                error: Some("oops, comma".into()),
                skipped: true,
                within_target: true,
            },
        ];
        write_report_csv(&results, &path).expect("write csv");

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 rows");
        assert!(lines[1].starts_with("a.png,"));
        // A comma inside the error field must be quoted (RFC 4180).
        assert!(lines[2].contains("\"oops, comma\""), "got: {}", lines[2]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn csv_formula_prefixes_are_neutralized() {
        // Every spreadsheet-dangerous prefix must come out carrying the
        // neutralizing single quote, so opening the report in Excel or
        // LibreOffice cannot execute a crafted filename as a formula.
        let dir = scratch_dir("report-csv-formula");
        let path = dir.join("report.csv");
        let mk = |original: &str, output: &str, error: &str| ProcessingResult {
            original_path: original.into(),
            output_path: output.into(),
            original_size: 10,
            output_size: 10,
            reduction_percent: 0.0,
            success: true,
            error: Some(error.into()),
            skipped: false,
            within_target: true,
        };
        let results = vec![
            mk(
                "=HYPERLINK(\"http://evil\",\"click\")",
                "+cmd|' /C calc",
                "@import",
            ),
            mk("-dash.png", "\tstart", "\rCR"),
        ];
        write_report_csv(&results, &path).expect("write csv");

        let content = std::fs::read_to_string(&path).unwrap();
        for neutralized in [
            "'=HYPERLINK",
            "'+cmd",
            "'@import",
            "'-dash.png",
            "'\tstart",
            "'\rCR",
        ] {
            assert!(
                content.contains(neutralized),
                "must contain {neutralized:?}; got: {content}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
