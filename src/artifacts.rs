use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::domain::{ImpactAnalysis, ImplementationFuture};

pub fn export_markdown(
    output_dir: &Path,
    session_id: &str,
    change_request: &str,
    repo_name: &str,
    analysis: &ImpactAnalysis,
    selected_future_index: usize,
) -> Result<PathBuf> {
    let _ = session_id;
    fs::create_dir_all(output_dir)?;
    let report_path = output_dir.join("branch-to-the-future-report.md");
    fs::write(
        &report_path,
        render_markdown(change_request, repo_name, analysis, selected_future_index),
    )?;
    Ok(report_path)
}

pub fn render_markdown(
    change_request: &str,
    repo_name: &str,
    analysis: &ImpactAnalysis,
    selected_future_index: usize,
) -> String {
    let selected = analysis
        .futures
        .get(selected_future_index)
        .or_else(|| analysis.futures.first());
    let mut out = String::new();
    out.push_str("# Branch to the Future Report\n\n");
    out.push_str("## Change Request\n\n");
    out.push_str(change_request);
    out.push_str("\n\n## Repo Summary\n\n");
    out.push_str(&format!(
        "- Repo: {repo_name}\n- Summary: {}\n",
        analysis.summary
    ));
    out.push_str("\n## Impact Table\n\n");
    if analysis.impact_path.is_empty() {
        out.push_str("_No impact files returned._\n");
    } else {
        out.push_str("<table><thead><tr><th>File</th><th>Score</th><th>Confidence</th><th>Risk</th></tr></thead><tbody>\n");
        for file in &analysis.impact_path {
            out.push_str(&format!(
                "<tr><td><code>{}</code></td><td>{}/10</td><td>{}%</td><td>{}</td></tr>\n",
                file.path,
                crate::app::impact_score_out_of_10(file.impact_score),
                file.confidence,
                file.risk
            ));
        }
        out.push_str("</tbody></table>\n");
    }
    out.push_str("\n## Impact Path\n\n");
    if analysis.impact_path.is_empty() {
        out.push_str("_No impact files returned._\n");
    } else {
        for (index, file) in analysis.impact_path.iter().enumerate() {
            out.push_str(&format!(
                "{}. `{}` ({}/10, confidence {}%, risk {})\n   - Reason: {}\n   - Change: {}\n",
                index + 1,
                file.path,
                crate::app::impact_score_out_of_10(file.impact_score),
                file.confidence,
                file.risk,
                file.reason,
                file.change_needed
            ));
        }
    }
    out.push_str("\n## Affected Files\n\n");
    for file in &analysis.impact_path {
        out.push_str(&format!("- `{}`\n", file.path));
    }
    out.push_str("\n## Risk Summary\n\n");
    list(&mut out, &analysis.risk_summary);
    out.push_str("\n## Architecture Stages\n\n");
    out.push_str("### Current\n\n");
    architecture_stages(&mut out, &analysis.current_architecture);
    out.push_str("\n### Proposed\n\n");
    if let Some(selected) = selected {
        architecture_stages(&mut out, &selected.architecture);
    } else {
        out.push_str("_No future selected._\n");
    }
    out.push_str("\n## Evidence\n\n");
    if analysis.impact_path.is_empty() {
        out.push_str("_No impacted files returned._\n");
    } else {
        for file in &analysis.impact_path {
            out.push_str(&format!(
                "### `{}`\n\n- Reason: {}\n- Change: {}\n- Confidence: {}%\n\n",
                file.path, file.reason, file.change_needed, file.confidence
            ));
        }
    }
    out.push_str("\n## Branch to the Future\n\n");
    for future in &analysis.futures {
        future_section(&mut out, future);
    }
    out.push_str("\n## Recommended Path\n\n");
    out.push_str(&analysis.recommended_future);
    out.push('\n');
    if let Some(selected) = selected {
        out.push_str(&format!("\nSelected export path: {}\n", selected.name));
    }
    out.push_str("\n## Test Plan\n\n");
    if let Some(selected) = selected {
        list(&mut out, &selected.test_plan);
    } else {
        list(&mut out, &analysis.tests_to_add);
    }
    out.push_str("\n## Patch Plan\n\n");
    if let Some(selected) = selected {
        list(&mut out, &selected.patch_plan);
    } else {
        out.push_str("_No future selected._\n");
    }
    out.push_str("\n## Patch Skeleton\n\n");
    out.push_str(&patch_skeleton(selected));
    out.push_str("\n## Architecture Scaffold\n\n");
    out.push_str("Architecture scaffold is rendered in the TUI with terminal-native layout.\n");
    out.push_str("\n## JSON Appendix\n\n```json\n");
    match serde_json::to_string_pretty(analysis) {
        Ok(json) => out.push_str(&json),
        Err(error) => out.push_str(&format!("{{\"error\":\"{error}\"}}")),
    }
    out.push_str("\n```\n");
    out
}

pub fn render_notion_markdown(
    change_request: &str,
    repo_name: &str,
    analysis: &ImpactAnalysis,
    selected_future_index: usize,
) -> String {
    let selected = analysis
        .futures
        .get(selected_future_index)
        .or_else(|| analysis.futures.first());
    let mut out = String::new();
    out.push_str("# Branch to the Future Report\n\n");
    out.push_str("## Change Request\n\n");
    out.push_str(change_request);
    out.push_str("\n\n## Repo Summary\n\n");
    out.push_str(&format!(
        "- Repo: {repo_name}\n- Summary: {}\n",
        analysis.summary
    ));
    out.push_str("\n## Impact Path\n\n");
    if analysis.impact_path.is_empty() {
        out.push_str("_No impact files returned._\n");
    } else {
        for (index, file) in analysis.impact_path.iter().enumerate() {
            out.push_str(&format!(
                "{}. `{}`\n   - Score: {}/10\n   - Confidence: {}%\n   - Risk: {}\n   - Reason: {}\n   - Change: {}\n",
                index + 1,
                file.path,
                crate::app::impact_score_out_of_10(file.impact_score),
                file.confidence,
                file.risk,
                file.reason,
                file.change_needed
            ));
        }
    }
    out.push_str("\n## Risk Summary\n\n");
    list(&mut out, &analysis.risk_summary);
    out.push_str("\n## Architecture Stages\n\n");
    out.push_str("### Current\n\n");
    architecture_stages(&mut out, &analysis.current_architecture);
    out.push_str("\n### Proposed\n\n");
    if let Some(selected) = selected {
        architecture_stages(&mut out, &selected.architecture);
    } else {
        out.push_str("_No future selected._\n");
    }
    out.push_str("\n## Evidence\n\n");
    if analysis.impact_path.is_empty() {
        out.push_str("_No impacted files returned._\n");
    } else {
        for file in &analysis.impact_path {
            out.push_str(&format!(
                "### `{}`\n\n- Reason: {}\n- Change: {}\n- Confidence: {}%\n\n",
                file.path, file.reason, file.change_needed, file.confidence
            ));
        }
    }
    out.push_str("\n## Branch to the Future\n\n");
    for future in &analysis.futures {
        future_section(&mut out, future);
    }
    out.push_str("\n## Recommended Path\n\n");
    out.push_str(&analysis.recommended_future);
    out.push('\n');
    if let Some(selected) = selected {
        out.push_str(&format!("\nSelected export path: {}\n", selected.name));
    }
    out.push_str("\n## Test Plan\n\n");
    if let Some(selected) = selected {
        list(&mut out, &selected.test_plan);
    } else {
        list(&mut out, &analysis.tests_to_add);
    }
    out.push_str("\n## Patch Plan\n\n");
    if let Some(selected) = selected {
        list(&mut out, &selected.patch_plan);
    } else {
        out.push_str("_No future selected._\n");
    }
    out
}

fn future_section(out: &mut String, future: &ImplementationFuture) {
    out.push_str(&format!(
        "### {}\n\n- Complexity: {}\n- Risk: {}\n- Description: {}\n",
        future.name, future.complexity, future.risk, future.description
    ));
    out.push_str("- Affected files:\n");
    for file in &future.affected_files {
        out.push_str(&format!("  - `{file}`\n"));
    }
    out.push_str("- Benefits:\n");
    for item in &future.benefits {
        out.push_str(&format!("  - {item}\n"));
    }
    out.push_str("- Drawbacks:\n");
    for item in &future.drawbacks {
        out.push_str(&format!("  - {item}\n"));
    }
    out.push('\n');
}

fn list(out: &mut String, values: &[String]) {
    if values.is_empty() {
        out.push_str("_None returned._\n");
    } else {
        for value in values {
            out.push_str(&format!("- {value}\n"));
        }
    }
}

fn architecture_stages(out: &mut String, stages: &[crate::domain::ArchitectureStage]) {
    if stages.is_empty() {
        out.push_str("_None returned._\n");
        return;
    }
    for stage in stages {
        out.push_str(&format!("#### {}\n\n", stage.label));
        if !stage.responsibilities.is_empty() {
            out.push_str("- Responsibilities:\n");
            for item in &stage.responsibilities {
                out.push_str(&format!("  - {item}\n"));
            }
        }
        if !stage.files.is_empty() {
            out.push_str("- Files:\n");
            for file in &stage.files {
                out.push_str(&format!("  - `{file}`\n"));
            }
        }
        out.push('\n');
    }
}

fn patch_skeleton(future: Option<&ImplementationFuture>) -> String {
    let Some(future) = future else {
        return "_No future selected._\n".to_string();
    };
    let mut out = String::new();
    for file in &future.affected_files {
        out.push_str(&format!("### `{file}`\n\n"));
        if future.patch_plan.is_empty() {
            out.push_str("- Inspect and update for selected future.\n\n");
        } else {
            for step in &future.patch_plan {
                out.push_str(&format!("- {step}\n"));
            }
            out.push('\n');
        }
    }
    out
}
