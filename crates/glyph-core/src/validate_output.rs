//! Phase 6b: validate-output structural checks.
//!
//! Validates that a compiled `.md` file structurally matches its `.ir.json`
//! counterpart. Implements the 26 `G::expand::*` diagnostic IDs from
//! `design/expand.md` §4.1.
//!
//! This module operates on external files (not the compiler's internal IR),
//! using `serde_json::Value` to parse the IR JSON.

use serde_json::Value;

/// A single validation violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub id: String,
    pub message: String,
}

impl Violation {
    fn new(id: &str, message: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            message: message.into(),
        }
    }
}

/// Result of validate-output: a list of violations (empty = pass).
pub fn validate_output(ir_json: &str, md: &str) -> Vec<Violation> {
    let ir: Value = match serde_json::from_str(ir_json) {
        Ok(v) => v,
        Err(e) => {
            return vec![Violation::new(
                "G::expand::malformed-markdown",
                format!("failed to parse IR JSON: {}", e),
            )];
        }
    };

    let skill = match ir.get("skill") {
        Some(s) => s,
        None => {
            return vec![Violation::new(
                "G::expand::malformed-markdown",
                "IR JSON has no `skill` field",
            )];
        }
    };

    let mut violations = Vec::new();

    // Strip leading YAML frontmatter if present (Emit adds it; the agent's
    // reshaped .md may carry it from the original compiled output).
    let md = strip_leading_frontmatter(md);
    let md = md.as_str();

    // Check frontmatter (fires if frontmatter still remains after stripping,
    // meaning Step 2 injected its own frontmatter block in the body)
    check_frontmatter(md, &mut violations);

    // Check malformed markdown
    check_malformed_markdown(md, &mut violations);

    // Parse markdown structure
    let md_struct = parse_md_structure(md);

    // Section shape checks
    check_section_shape(&md_struct, skill, &mut violations);

    // Context count
    check_context_count(&md_struct, skill, &mut violations);

    // Step count and order
    check_step_count(&md_struct, skill, &mut violations);

    // Substep count (branches)
    check_substep_count(&md_struct, skill, &mut violations);

    // Constraint count
    check_constraint_count(&md_struct, skill, &mut violations);

    // Parameter checks
    check_params(&md_struct, skill, &mut violations);

    // Parameter reference integrity
    check_param_refs(&md_struct, skill, md, &mut violations);

    // Unresolved local refs
    check_unresolved_local_refs(skill, md, &mut violations);

    // Modifier leakage
    check_modifier_leaked(skill, md, &mut violations);

    // Content shape (sentence limits)
    check_content_shape(&md_struct, &mut violations);

    // Procedure checks
    check_procedures(&md_struct, skill, &mut violations);

    // Description-driven branch validation
    check_applies_descriptions(skill, md, &mut violations);

    violations
}

// ---------------------------------------------------------------------------
// Markdown structure parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct MdStructure {
    h2_sections: Vec<H2Section>,
}

#[derive(Debug)]
struct H2Section {
    name: String,
    h3_sections: Vec<H3Section>,
    /// Raw content lines (between H2 heading and first H3, or between H3s)
    content_lines: Vec<String>,
}

#[derive(Debug)]
struct H3Section {
    name: String,
    items: Vec<ListItem>,
}

#[derive(Debug)]
struct ListItem {
    text: String,
    sub_items: Vec<SubItem>,
}

#[derive(Debug)]
struct SubItem {
    text: String,
}

fn parse_md_structure(md: &str) -> MdStructure {
    let mut structure = MdStructure::default();
    let mut current_h2: Option<H2Section> = None;
    let mut current_h3: Option<H3Section> = None;
    let mut current_item: Option<ListItem> = None;

    for line in md.lines() {
        if line.starts_with("## ") {
            // Flush current state
            if let Some(item) = current_item.take() {
                if let Some(ref mut h3) = current_h3 {
                    h3.items.push(item);
                }
            }
            if let Some(h3) = current_h3.take() {
                if let Some(ref mut h2) = current_h2 {
                    h2.h3_sections.push(h3);
                }
            }
            if let Some(h2) = current_h2.take() {
                structure.h2_sections.push(h2);
            }
            let name = line.trim_start_matches("## ").trim().to_string();
            current_h2 = Some(H2Section {
                name,
                h3_sections: Vec::new(),
                content_lines: Vec::new(),
            });
        } else if line.starts_with("### ") {
            // Flush current item and H3
            if let Some(item) = current_item.take() {
                if let Some(ref mut h3) = current_h3 {
                    h3.items.push(item);
                }
            }
            if let Some(h3) = current_h3.take() {
                if let Some(ref mut h2) = current_h2 {
                    h2.h3_sections.push(h3);
                }
            }
            let name = line.trim_start_matches("### ").trim().to_string();
            current_h3 = Some(H3Section {
                name: name.clone(),
                items: Vec::new(),
            });
        } else if let Some(ref mut _h2) = current_h2 {
            if current_h3.is_some() {
                // Inside an H3 section - check for list items
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Numbered list item: "1. ...", "2. ...", etc.
                if is_numbered_item(trimmed) {
                    if let Some(item) = current_item.take() {
                        if let Some(ref mut h3) = current_h3 {
                            h3.items.push(item);
                        }
                    }
                    let text = strip_number_prefix(trimmed);
                    current_item = Some(ListItem {
                        text,
                        sub_items: Vec::new(),
                    });
                } else if is_bulleted_item(trimmed) {
                    if let Some(item) = current_item.take() {
                        if let Some(ref mut h3) = current_h3 {
                            h3.items.push(item);
                        }
                    }
                    let text = strip_bullet_prefix(trimmed);
                    current_item = Some(ListItem {
                        text,
                        sub_items: Vec::new(),
                    });
                } else if is_lettered_subitem(trimmed) {
                    // Lettered sub-item: "a. ...", "b. ...", etc.
                    let text = strip_letter_prefix(trimmed);
                    if let Some(ref mut item) = current_item {
                        item.sub_items.push(SubItem { text });
                    }
                } else if let Some(ref mut item) = current_item {
                    // Continuation line
                    item.text.push(' ');
                    item.text.push_str(trimmed);
                } else {
                    // Content line inside H3 but not a list item
                    // (e.g., procedure preamble)
                }
            } else {
                // Content between H2 and first H3
                if let Some(ref mut h2) = current_h2 {
                    h2.content_lines.push(line.to_string());
                }
            }
        }
    }

    // Flush remaining
    if let Some(item) = current_item.take() {
        if let Some(ref mut h3) = current_h3 {
            h3.items.push(item);
        }
    }
    if let Some(h3) = current_h3.take() {
        if let Some(ref mut h2) = current_h2 {
            h2.h3_sections.push(h3);
        }
    }
    if let Some(h2) = current_h2.take() {
        structure.h2_sections.push(h2);
    }

    structure
}

fn is_numbered_item(s: &str) -> bool {
    let mut chars = s.chars();
    // Must start with digit(s)
    let first = chars.next();
    if !first.map_or(false, |c| c.is_ascii_digit()) {
        return false;
    }
    for c in chars {
        if c == '.' {
            return true;
        }
        if !c.is_ascii_digit() {
            return false;
        }
    }
    false
}

fn strip_number_prefix(s: &str) -> String {
    if let Some(pos) = s.find(". ") {
        s[pos + 2..].to_string()
    } else {
        s.to_string()
    }
}

fn is_bulleted_item(s: &str) -> bool {
    s.starts_with("- ") || s.starts_with("* ")
}

fn strip_bullet_prefix(s: &str) -> String {
    if s.starts_with("- ") {
        s[2..].to_string()
    } else if s.starts_with("* ") {
        s[2..].to_string()
    } else {
        s.to_string()
    }
}

fn is_lettered_subitem(s: &str) -> bool {
    // Lettered sub-items within numbered lists: "a. text", "b. text", etc.
    // They're indented in practice, so check the trimmed form.
    let bytes = s.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    bytes[0].is_ascii_lowercase() && bytes[1] == b'.' && bytes[2] == b' '
}

fn strip_letter_prefix(s: &str) -> String {
    if s.len() >= 3 {
        s[3..].to_string()
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Strip leading YAML frontmatter
// ---------------------------------------------------------------------------

fn strip_leading_frontmatter(md: &str) -> String {
    let trimmed = md.trim_start();
    if !trimmed.starts_with("---") {
        return md.to_string();
    }
    // Find closing ---
    let after_opening = &trimmed[3..];
    if let Some(close_pos) = after_opening.find("\n---") {
        let after_close = &after_opening[close_pos + 4..];
        // Skip any trailing newline after the closing ---
        let after_close = after_close.strip_prefix('\n').unwrap_or(after_close);
        return after_close.to_string();
    }
    // No closing --- found, return as-is
    md.to_string()
}

// ---------------------------------------------------------------------------
// Check: frontmatter-returned
// ---------------------------------------------------------------------------

fn check_frontmatter(md: &str, violations: &mut Vec<Violation>) {
    let trimmed = md.trim_start();
    if trimmed.starts_with("---") {
        violations.push(Violation::new(
            "G::expand::frontmatter-returned",
            "output contains YAML frontmatter (frontmatter is assembled by Emit, not Step 2)",
        ));
    }
}

// ---------------------------------------------------------------------------
// Check: malformed-markdown
// ---------------------------------------------------------------------------

fn check_malformed_markdown(md: &str, violations: &mut Vec<Violation>) {
    // Basic structural check: must have at least one heading
    let has_heading = md.lines().any(|l| l.starts_with('#'));
    if !has_heading {
        violations.push(Violation::new(
            "G::expand::malformed-markdown",
            "output does not contain any Markdown headings",
        ));
    }
}

// ---------------------------------------------------------------------------
// Check: section shape (extra-h2, missing-instructions, extra-h3)
// ---------------------------------------------------------------------------

fn check_section_shape(md_struct: &MdStructure, skill: &Value, violations: &mut Vec<Violation>) {
    let has_params = skill
        .get("params")
        .and_then(|p| p.as_array())
        .map_or(false, |a| !a.is_empty());

    let mut found_instructions = false;
    let mut found_parameters = false;

    for h2 in &md_struct.h2_sections {
        if h2.name == "Instructions" {
            found_instructions = true;
            // Check H3 sections
            for h3 in &h2.h3_sections {
                let valid = h3.name == "Context"
                    || h3.name == "Steps"
                    || h3.name == "Constraints"
                    || h3.name.starts_with("Procedure: ");
                if !valid {
                    violations.push(Violation::new(
                        "G::expand::extra-h3",
                        format!(
                            "unexpected H3 section `### {}` under `## Instructions`",
                            h3.name
                        ),
                    ));
                }
            }
        } else if h2.name == "Parameters" {
            found_parameters = true;
        } else {
            violations.push(Violation::new(
                "G::expand::extra-h2",
                format!("unexpected H2 section `## {}`", h2.name),
            ));
        }
    }

    if !found_instructions {
        violations.push(Violation::new(
            "G::expand::missing-instructions",
            "`## Instructions` section not found",
        ));
    }

    // params-section-missing / params-section-spurious
    if has_params && !found_parameters {
        violations.push(Violation::new(
            "G::expand::params-section-missing",
            "skill has parameters but `## Parameters` section is absent",
        ));
    }
    if !has_params && found_parameters {
        violations.push(Violation::new(
            "G::expand::params-section-spurious",
            "skill has no parameters but `## Parameters` section is present",
        ));
    }
}

// ---------------------------------------------------------------------------
// Check: context-count-mismatch
// ---------------------------------------------------------------------------

fn check_context_count(md_struct: &MdStructure, skill: &Value, violations: &mut Vec<Violation>) {
    let ir_context_count = skill
        .get("context")
        .and_then(|c| c.as_array())
        .map_or(0, |a| a.len());

    let md_context_count = find_h3_items(md_struct, "Context");

    if ir_context_count != md_context_count {
        violations.push(Violation::new(
            "G::expand::context-count-mismatch",
            format!(
                "IR has {} context entries but `### Context` has {} items",
                ir_context_count, md_context_count
            ),
        ));
    }
}

// ---------------------------------------------------------------------------
// Check: step-count-mismatch, step-order-mismatch
// ---------------------------------------------------------------------------

fn check_step_count(md_struct: &MdStructure, skill: &Value, violations: &mut Vec<Violation>) {
    let flow = match skill.get("flow").and_then(|f| f.as_array()) {
        Some(f) => f,
        None => return,
    };

    let expected = compute_expected_step_count(flow);
    let md_step_count = find_h3_items(md_struct, "Steps");

    if expected != md_step_count {
        violations.push(Violation::new(
            "G::expand::step-count-mismatch",
            format!(
                "expected {} top-level steps but `### Steps` has {} items",
                expected, md_step_count
            ),
        ));
    }

    // step-order-mismatch: we check that the ordering of content in Steps matches
    // the IR flow order. This is a structural check - we verify by looking at
    // whether IR step-projecting nodes appear in the same relative order.
    // For now, if the counts match, we trust the order (a more detailed check
    // would compare content).
    check_step_order(md_struct, flow, violations);
}

fn compute_expected_step_count(flow: &[Value]) -> usize {
    let mut count = 0;
    let mut has_trailing_return = false;

    for (i, node) in flow.iter().enumerate() {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        match kind {
            "call" | "inline_instruction" | "instruction_ref" => {
                let role = node.get("role").and_then(|r| r.as_str()).unwrap_or("");
                if role == "step" {
                    count += 1;
                }
            }
            "branch" => {
                count += 1; // Each branch = 1 top-level step
            }
            "return" => {
                // Return folds into the last step — check if it's the last node
                if i == flow.len() - 1 {
                    has_trailing_return = true;
                }
            }
            "constraint" => {
                // Constraints don't count as steps
            }
            _ => {}
        }
    }

    // Return folds into the last step, so doesn't add a new step.
    // But if the last node before return was a step-projecting node,
    // the return folds into it (already counted).
    // If return is standalone and not last, it would be a separate item,
    // but per spec, return always folds.
    let _ = has_trailing_return;
    count
}

fn check_step_order(
    md_struct: &MdStructure,
    flow: &[Value],
    violations: &mut Vec<Violation>,
) {
    // Extract step-projecting node targets/texts from IR in order
    let mut ir_step_keys: Vec<String> = Vec::new();
    for node in flow {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        match kind {
            "call" => {
                let role = node.get("role").and_then(|r| r.as_str()).unwrap_or("");
                if role == "step" {
                    let target = node
                        .get("target")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    ir_step_keys.push(target.to_string());
                }
            }
            "inline_instruction" => {
                let role = node.get("role").and_then(|r| r.as_str()).unwrap_or("");
                if role == "step" {
                    let text = node.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    // Use first few words as key
                    let key: String = text.split_whitespace().take(5).collect::<Vec<_>>().join(" ");
                    ir_step_keys.push(key);
                }
            }
            "instruction_ref" => {
                let role = node.get("role").and_then(|r| r.as_str()).unwrap_or("");
                if role == "step" {
                    let name = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    ir_step_keys.push(name.to_string());
                }
            }
            "branch" => {
                let cond = node
                    .get("condition")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                ir_step_keys.push(format!("branch:{}", cond));
            }
            _ => {}
        }
    }

    // Get md step texts
    let steps_section = find_instructions_h3(md_struct, "Steps");
    if let Some(section) = steps_section {
        if section.items.len() == ir_step_keys.len() {
            // Check if items contain the expected content (partial match)
            // For step-order-mismatch, we check if each IR step's content appears
            // in the corresponding md step. This is approximate but catches
            // obvious reorderings.
            for (i, (ir_key, md_item)) in
                ir_step_keys.iter().zip(section.items.iter()).enumerate()
            {
                if ir_key.starts_with("branch:") {
                    continue; // Branch steps are harder to match; skip for now
                }
                // Check if the IR key words appear somewhere in the md item
                let ir_words: Vec<&str> = ir_key.split_whitespace().collect();
                let md_lower = md_item.text.to_lowercase();
                let mut found = false;
                for word in &ir_words {
                    if md_lower.contains(&word.to_lowercase()) {
                        found = true;
                        break;
                    }
                }
                if !found && !ir_key.is_empty() {
                    violations.push(Violation::new(
                        "G::expand::step-order-mismatch",
                        format!(
                            "step {} does not match IR flow order (expected content related to `{}`)",
                            i + 1,
                            ir_key
                        ),
                    ));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Check: substep-count-mismatch
// ---------------------------------------------------------------------------

fn check_substep_count(md_struct: &MdStructure, skill: &Value, violations: &mut Vec<Violation>) {
    let flow = match skill.get("flow").and_then(|f| f.as_array()) {
        Some(f) => f,
        None => return,
    };

    let steps_section = match find_instructions_h3(md_struct, "Steps") {
        Some(s) => s,
        None => return,
    };

    // Find Branch nodes in the flow and their corresponding md steps
    let mut step_idx = 0;
    for node in flow {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        match kind {
            "call" | "inline_instruction" | "instruction_ref" => {
                let role = node.get("role").and_then(|r| r.as_str()).unwrap_or("step");
                if role == "step" {
                    step_idx += 1;
                }
            }
            "branch" => {
                if step_idx < steps_section.items.len() {
                    let md_item = &steps_section.items[step_idx];
                    // Count sub-items per arm
                    check_branch_substeps(node, md_item, violations);
                }
                step_idx += 1;
            }
            "return" => {} // folds, doesn't increment
            "constraint" => {} // doesn't count as step
            _ => {}
        }
    }
}

fn check_branch_substeps(branch: &Value, md_item: &ListItem, violations: &mut Vec<Violation>) {
    // Count expected substeps per arm from IR
    let then_body = branch.get("then_body").and_then(|b| b.as_array());
    let elif_branches = branch.get("elif_branches").and_then(|b| b.as_array());
    let else_body = branch.get("else_body").and_then(|b| b.as_array());

    let mut expected_total = 0;
    if let Some(body) = then_body {
        expected_total += count_step_projecting_nodes(body);
    }
    if let Some(elifs) = elif_branches {
        for elif in elifs {
            if let Some(body) = elif.get("body").and_then(|b| b.as_array()) {
                expected_total += count_step_projecting_nodes(body);
            }
        }
    }
    if let Some(body) = else_body {
        expected_total += count_step_projecting_nodes(body);
    }

    let actual = md_item.sub_items.len();
    if expected_total != actual && expected_total > 0 {
        violations.push(Violation::new(
            "G::expand::substep-count-mismatch",
            format!(
                "branch has {} expected sub-steps but found {} lettered sub-items",
                expected_total, actual
            ),
        ));
    }
}

fn count_step_projecting_nodes(body: &[Value]) -> usize {
    body.iter()
        .filter(|node| {
            let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            match kind {
                "call" | "inline_instruction" | "instruction_ref" => {
                    let role = node.get("role").and_then(|r| r.as_str()).unwrap_or("");
                    role == "step"
                }
                "branch" => true, // nested branch counts as 1
                _ => false,
            }
        })
        .count()
}

// ---------------------------------------------------------------------------
// Check: constraint-count-mismatch
// ---------------------------------------------------------------------------

fn check_constraint_count(
    md_struct: &MdStructure,
    skill: &Value,
    violations: &mut Vec<Violation>,
) {
    let ir_constraint_count = skill
        .get("constraints")
        .and_then(|c| c.as_array())
        .map_or(0, |a| a.len());

    let md_constraint_count = find_h3_items(md_struct, "Constraints");

    if ir_constraint_count != md_constraint_count {
        violations.push(Violation::new(
            "G::expand::constraint-count-mismatch",
            format!(
                "IR has {} constraints but `### Constraints` has {} items",
                ir_constraint_count, md_constraint_count
            ),
        ));
    }
}

// ---------------------------------------------------------------------------
// Check: params-section-mismatch
// ---------------------------------------------------------------------------

fn check_params(md_struct: &MdStructure, skill: &Value, violations: &mut Vec<Violation>) {
    let ir_params = match skill.get("params").and_then(|p| p.as_array()) {
        Some(p) => p,
        None => return,
    };

    if ir_params.is_empty() {
        return;
    }

    // Find ## Parameters section
    let params_section = md_struct
        .h2_sections
        .iter()
        .find(|h2| h2.name == "Parameters");

    if let Some(section) = params_section {
        // Count bulleted items directly under ## Parameters (no H3)
        // The items are in content_lines as "- **name**: ..."
        let bullet_count = section
            .content_lines
            .iter()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("- ") || t.starts_with("* ")
            })
            .count();

        if bullet_count != ir_params.len() {
            violations.push(Violation::new(
                "G::expand::params-section-mismatch",
                format!(
                    "IR has {} parameters but `## Parameters` has {} items",
                    ir_params.len(),
                    bullet_count
                ),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Check: invented-param-ref, dropped-param-ref
// ---------------------------------------------------------------------------

fn check_param_refs(
    _md_struct: &MdStructure,
    skill: &Value,
    md: &str,
    violations: &mut Vec<Violation>,
) {
    let ir_params = skill
        .get("params")
        .and_then(|p| p.as_array())
        .unwrap_or(&Vec::new())
        .clone();

    let param_names: Vec<String> = ir_params
        .iter()
        .filter_map(|p| p.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();

    // Collect all local_ref names to exclude from invented-param-ref check
    let local_ref_names = collect_all_local_ref_names(skill);

    // Find all {name} references in the md body (excluding ## Parameters section)
    let md_refs = find_curly_refs(md);

    // invented-param-ref: {name} in md not matching any declared param
    // (and not a local_ref, which is checked separately)
    for ref_name in &md_refs {
        if !param_names.contains(ref_name) && !local_ref_names.contains(ref_name) {
            violations.push(Violation::new(
                "G::expand::invented-param-ref",
                format!(
                    "`{{{}}}` reference does not match any declared parameter",
                    ref_name
                ),
            ));
        }
    }

    // dropped-param-ref: param ref from IR's resolved text not found in md
    let ir_param_refs = collect_param_refs_from_ir(skill, &param_names);
    for param_ref in &ir_param_refs {
        let token = format!("{{{}}}", param_ref);
        if !md.contains(&token) {
            violations.push(Violation::new(
                "G::expand::dropped-param-ref",
                format!(
                    "parameter reference `{{{}}}` from IR was dropped in output",
                    param_ref
                ),
            ));
        }
    }
}

fn collect_all_local_ref_names(skill: &Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(flow) = skill.get("flow").and_then(|f| f.as_array()) {
        collect_local_refs_from_flow(flow, &mut names);
    }
    names
}

fn collect_local_refs_from_flow(flow: &[Value], names: &mut Vec<String>) {
    for node in flow {
        if let Some(local_refs) = node.get("local_refs").and_then(|l| l.as_array()) {
            for lr in local_refs {
                if let Some(name) = lr.get("name").and_then(|n| n.as_str()) {
                    names.push(name.to_string());
                }
            }
        }
        // Recurse into branch bodies
        if let Some(then_body) = node.get("then_body").and_then(|b| b.as_array()) {
            collect_local_refs_from_flow(then_body, names);
        }
        if let Some(elifs) = node.get("elif_branches").and_then(|b| b.as_array()) {
            for elif in elifs {
                if let Some(body) = elif.get("body").and_then(|b| b.as_array()) {
                    collect_local_refs_from_flow(body, names);
                }
            }
        }
        if let Some(else_body) = node.get("else_body").and_then(|b| b.as_array()) {
            collect_local_refs_from_flow(else_body, names);
        }
    }
}

fn find_curly_refs(md: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let bytes = md.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'}' && bytes[end] != b'\n' {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'}' {
                let name = &md[start..end];
                // Only consider simple identifiers (no spaces, no special chars)
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_')
                {
                    if !refs.contains(&name.to_string()) {
                        refs.push(name.to_string());
                    }
                }
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    refs
}

fn collect_param_refs_from_ir(skill: &Value, param_names: &[String]) -> Vec<String> {
    let mut refs = Vec::new();
    if let Some(flow) = skill.get("flow").and_then(|f| f.as_array()) {
        collect_param_refs_from_flow(flow, param_names, &mut refs);
    }
    // Also check constraints
    if let Some(constraints) = skill.get("constraints").and_then(|c| c.as_array()) {
        for c in constraints {
            if let Some(text) = c.get("text").and_then(|t| t.as_str()) {
                for name in find_curly_refs_in_str(text) {
                    if param_names.contains(&name) && !refs.contains(&name) {
                        refs.push(name);
                    }
                }
            }
        }
    }
    refs
}

fn collect_param_refs_from_flow(flow: &[Value], param_names: &[String], refs: &mut Vec<String>) {
    for node in flow {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        match kind {
            "call" => {
                if let Some(text) = node.get("resolved_body_text").and_then(|t| t.as_str()) {
                    for name in find_curly_refs_in_str(text) {
                        if param_names.contains(&name) && !refs.contains(&name) {
                            refs.push(name);
                        }
                    }
                }
            }
            "inline_instruction" => {
                if let Some(text) = node.get("text").and_then(|t| t.as_str()) {
                    for name in find_curly_refs_in_str(text) {
                        if param_names.contains(&name) && !refs.contains(&name) {
                            refs.push(name);
                        }
                    }
                }
            }
            "instruction_ref" => {
                if let Some(text) = node.get("resolved_text").and_then(|t| t.as_str()) {
                    for name in find_curly_refs_in_str(text) {
                        if param_names.contains(&name) && !refs.contains(&name) {
                            refs.push(name);
                        }
                    }
                }
            }
            "branch" => {
                if let Some(body) = node.get("then_body").and_then(|b| b.as_array()) {
                    collect_param_refs_from_flow(body, param_names, refs);
                }
                if let Some(elifs) = node.get("elif_branches").and_then(|b| b.as_array()) {
                    for elif in elifs {
                        if let Some(body) = elif.get("body").and_then(|b| b.as_array()) {
                            collect_param_refs_from_flow(body, param_names, refs);
                        }
                    }
                }
                if let Some(body) = node.get("else_body").and_then(|b| b.as_array()) {
                    collect_param_refs_from_flow(body, param_names, refs);
                }
            }
            _ => {}
        }
    }
}

fn find_curly_refs_in_str(s: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'}' {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'}' {
                let name = &s[start..end];
                if !name.is_empty()
                    && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                {
                    refs.push(name.to_string());
                }
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    refs
}

// ---------------------------------------------------------------------------
// Check: unresolved-local-ref
// ---------------------------------------------------------------------------

fn check_unresolved_local_refs(skill: &Value, md: &str, violations: &mut Vec<Violation>) {
    let local_ref_names = collect_all_local_ref_names(skill);
    for name in &local_ref_names {
        let token = format!("{{{}}}", name);
        if md.contains(&token) {
            violations.push(Violation::new(
                "G::expand::unresolved-local-ref",
                format!(
                    "local_ref `{{{}}}` survived as a literal token in the output",
                    name
                ),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Check: modifier-leaked
// ---------------------------------------------------------------------------

fn check_modifier_leaked(skill: &Value, md: &str, violations: &mut Vec<Violation>) {
    if let Some(flow) = skill.get("flow").and_then(|f| f.as_array()) {
        check_modifier_leaked_in_flow(flow, md, violations);
    }
}

fn check_modifier_leaked_in_flow(flow: &[Value], md: &str, violations: &mut Vec<Violation>) {
    for node in flow {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind == "call" {
            if let Some(modifier) = node.get("site_modifier").and_then(|m| m.as_str()) {
                if md.contains(modifier) {
                    violations.push(Violation::new(
                        "G::expand::modifier-leaked",
                        format!(
                            "`with` modifier `{}` appears verbatim in output",
                            modifier
                        ),
                    ));
                }
            }
        }
        // Recurse into branches
        if kind == "branch" {
            if let Some(body) = node.get("then_body").and_then(|b| b.as_array()) {
                check_modifier_leaked_in_flow(body, md, violations);
            }
            if let Some(elifs) = node.get("elif_branches").and_then(|b| b.as_array()) {
                for elif in elifs {
                    if let Some(body) = elif.get("body").and_then(|b| b.as_array()) {
                        check_modifier_leaked_in_flow(body, md, violations);
                    }
                }
            }
            if let Some(body) = node.get("else_body").and_then(|b| b.as_array()) {
                check_modifier_leaked_in_flow(body, md, violations);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Check: content shape (step-too-long, constraint-multi-sentence)
// ---------------------------------------------------------------------------

fn check_content_shape(md_struct: &MdStructure, violations: &mut Vec<Violation>) {
    // Check steps
    if let Some(steps) = find_instructions_h3(md_struct, "Steps") {
        for (i, item) in steps.items.iter().enumerate() {
            if item.sub_items.is_empty() {
                // Non-conditional step
                let sentences = count_sentences(&item.text);
                if sentences > 3 {
                    violations.push(Violation::new(
                        "G::expand::step-too-long",
                        format!(
                            "step {} has {} sentences (max 3)",
                            i + 1,
                            sentences
                        ),
                    ));
                }
            } else {
                // Conditional step — check each sub-item
                for (j, sub) in item.sub_items.iter().enumerate() {
                    let sentences = count_sentences(&sub.text);
                    if sentences > 3 {
                        violations.push(Violation::new(
                            "G::expand::step-too-long",
                            format!(
                                "step {} sub-step {} has {} sentences (max 3)",
                                i + 1,
                                (b'a' + j as u8) as char,
                                sentences
                            ),
                        ));
                    }
                }
            }
        }
    }

    // Check constraints
    if let Some(constraints) = find_instructions_h3(md_struct, "Constraints") {
        for (i, item) in constraints.items.iter().enumerate() {
            let sentences = count_sentences(&item.text);
            if sentences > 1 {
                violations.push(Violation::new(
                    "G::expand::constraint-multi-sentence",
                    format!(
                        "constraint {} has {} sentences (max 1)",
                        i + 1,
                        sentences
                    ),
                ));
            }
        }
    }
}

/// Count sentences per the spec: strip backtick code spans, then count
/// `.`, `!`, `?` followed by whitespace or end-of-string.
fn count_sentences(text: &str) -> usize {
    // Step 1: strip backtick code spans
    let stripped = strip_code_spans(text);

    // Step 2: count sentence boundaries
    let bytes = stripped.as_bytes();
    let mut count = 0;
    for i in 0..bytes.len() {
        if bytes[i] == b'.' || bytes[i] == b'!' || bytes[i] == b'?' {
            // Followed by whitespace or end-of-string
            if i + 1 >= bytes.len() || bytes[i + 1].is_ascii_whitespace() {
                count += 1;
            }
        }
    }

    // If there's text but no sentence boundary found, it's still one sentence
    if count == 0 && !stripped.trim().is_empty() {
        count = 1;
    }

    count
}

fn strip_code_spans(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            // Find matching backtick
            let mut end = i + 1;
            while end < bytes.len() && bytes[end] != b'`' {
                end += 1;
            }
            if end < bytes.len() {
                i = end + 1; // Skip past closing backtick
            } else {
                result.push(bytes[i] as char);
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Check: procedure checks
// ---------------------------------------------------------------------------

fn check_procedures(md_struct: &MdStructure, skill: &Value, violations: &mut Vec<Violation>) {
    let flow = match skill.get("flow").and_then(|f| f.as_array()) {
        Some(f) => f,
        None => return,
    };

    // Collect expected procedures from IR (calls with same_file_procedure projection)
    let mut expected_procedures: Vec<String> = Vec::new();
    collect_procedure_calls(flow, &mut expected_procedures);
    // Deduplicate preserving order
    let mut unique_procedures: Vec<String> = Vec::new();
    for p in &expected_procedures {
        if !unique_procedures.contains(p) {
            unique_procedures.push(p.clone());
        }
    }

    // Find actual procedure sections in md
    let instructions = md_struct
        .h2_sections
        .iter()
        .find(|h2| h2.name == "Instructions");

    let actual_procedures: Vec<&H3Section> = instructions
        .map(|h2| {
            h2.h3_sections
                .iter()
                .filter(|h3| h3.name.starts_with("Procedure: "))
                .collect()
        })
        .unwrap_or_default();

    let actual_names: Vec<String> = actual_procedures
        .iter()
        .map(|h3| h3.name.strip_prefix("Procedure: ").unwrap_or("").to_string())
        .collect();

    // procedure-count-mismatch
    if unique_procedures.len() != actual_procedures.len() {
        violations.push(Violation::new(
            "G::expand::procedure-count-mismatch",
            format!(
                "expected {} procedure sections but found {}",
                unique_procedures.len(),
                actual_procedures.len()
            ),
        ));
    }

    // procedure-name-mismatch
    for actual_name in &actual_names {
        let kebab = actual_name.to_string();
        let matching = unique_procedures.iter().any(|p| to_kebab(p) == kebab);
        if !matching {
            violations.push(Violation::new(
                "G::expand::procedure-name-mismatch",
                format!(
                    "procedure section `### Procedure: {}` does not match any callee",
                    actual_name
                ),
            ));
        }
    }

    // procedure-duplicate
    let mut seen_names: Vec<String> = Vec::new();
    for name in &actual_names {
        if seen_names.contains(name) {
            violations.push(Violation::new(
                "G::expand::procedure-duplicate",
                format!("duplicate procedure section `### Procedure: {}`", name),
            ));
        } else {
            seen_names.push(name.clone());
        }
    }

    // procedure-step-count-mismatch: check item count against callee flow
    for proc_section in &actual_procedures {
        let proc_name = proc_section
            .name
            .strip_prefix("Procedure: ")
            .unwrap_or("");
        // Find the corresponding call in IR to get callee_flow
        if let Some(callee_flow_count) =
            find_callee_flow_count(flow, proc_name)
        {
            if proc_section.items.len() != callee_flow_count {
                violations.push(Violation::new(
                    "G::expand::procedure-step-count-mismatch",
                    format!(
                        "procedure `{}` has {} items but callee flow has {} nodes",
                        proc_name,
                        proc_section.items.len(),
                        callee_flow_count
                    ),
                ));
            }
        }
    }

    // procedure-ref-missing: check that Steps referencing same_file_procedure
    // calls mention the procedure name
    if let Some(steps) = find_instructions_h3(md_struct, "Steps") {
        for proc_name in &unique_procedures {
            let kebab = to_kebab(proc_name);
            let referenced = steps
                .items
                .iter()
                .any(|item| item.text.contains(&kebab));
            if !referenced {
                violations.push(Violation::new(
                    "G::expand::procedure-ref-missing",
                    format!(
                        "no step references procedure `{}`",
                        kebab
                    ),
                ));
            }
        }
    }

    // procedure-ref-dangling: check that step prose referencing a procedure
    // has a matching section
    if let Some(steps) = find_instructions_h3(md_struct, "Steps") {
        for item in &steps.items {
            // Look for "procedure" references in step text
            // (actual_names are checked below for dangling refs)
            // Check if step references a procedure name that doesn't have a section
            for proc_name in &unique_procedures {
                let kebab = to_kebab(proc_name);
                if item.text.contains(&kebab) && !actual_names.contains(&kebab) {
                    violations.push(Violation::new(
                        "G::expand::procedure-ref-dangling",
                        format!(
                            "step references procedure `{}` but no matching `### Procedure: {}` section exists",
                            kebab, kebab
                        ),
                    ));
                }
            }
        }
    }

    // procedure-order: check that procedure sections are ordered by first reference
    if actual_names.len() >= 2 {
        if let Some(steps) = find_instructions_h3(md_struct, "Steps") {
            let mut first_ref_order: Vec<String> = Vec::new();
            for item in &steps.items {
                for name in &actual_names {
                    if item.text.contains(name) && !first_ref_order.contains(name) {
                        first_ref_order.push(name.clone());
                    }
                }
            }
            // Check that actual_names follows first_ref_order
            let mut ordered = true;
            let mut ref_idx = 0;
            for actual in &actual_names {
                if ref_idx < first_ref_order.len() && &first_ref_order[ref_idx] == actual {
                    ref_idx += 1;
                } else {
                    ordered = false;
                    break;
                }
            }
            if !ordered {
                violations.push(Violation::new(
                    "G::expand::procedure-order",
                    "procedure sections are not ordered by first reference from `### Steps`",
                ));
            }
        }
    }
}

fn collect_procedure_calls(flow: &[Value], procedures: &mut Vec<String>) {
    for node in flow {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind == "call" {
            let mode = node
                .get("projection_mode")
                .and_then(|m| m.as_str())
                .unwrap_or("inline");
            if mode == "same_file_procedure" {
                if let Some(target) = node.get("target").and_then(|t| t.as_str()) {
                    procedures.push(target.to_string());
                }
            }
        }
        // Recurse into branches
        if kind == "branch" {
            if let Some(body) = node.get("then_body").and_then(|b| b.as_array()) {
                collect_procedure_calls(body, procedures);
            }
            if let Some(elifs) = node.get("elif_branches").and_then(|b| b.as_array()) {
                for elif in elifs {
                    if let Some(body) = elif.get("body").and_then(|b| b.as_array()) {
                        collect_procedure_calls(body, procedures);
                    }
                }
            }
            if let Some(body) = node.get("else_body").and_then(|b| b.as_array()) {
                collect_procedure_calls(body, procedures);
            }
        }
    }
}

fn find_callee_flow_count(flow: &[Value], proc_name: &str) -> Option<usize> {
    for node in flow {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind == "call" {
            let target = node.get("target").and_then(|t| t.as_str()).unwrap_or("");
            let mode = node
                .get("projection_mode")
                .and_then(|m| m.as_str())
                .unwrap_or("inline");
            if mode == "same_file_procedure" && to_kebab(target) == proc_name {
                if let Some(callee_flow) = node.get("callee_flow").and_then(|f| f.as_array()) {
                    return Some(callee_flow.len());
                }
            }
        }
        // Recurse into branches
        if kind == "branch" {
            if let Some(body) = node.get("then_body").and_then(|b| b.as_array()) {
                if let Some(count) = find_callee_flow_count(body, proc_name) {
                    return Some(count);
                }
            }
            if let Some(elifs) = node.get("elif_branches").and_then(|b| b.as_array()) {
                for elif in elifs {
                    if let Some(body) = elif.get("body").and_then(|b| b.as_array()) {
                        if let Some(count) = find_callee_flow_count(body, proc_name) {
                            return Some(count);
                        }
                    }
                }
            }
            if let Some(body) = node.get("else_body").and_then(|b| b.as_array()) {
                if let Some(count) = find_callee_flow_count(body, proc_name) {
                    return Some(count);
                }
            }
        }
    }
    None
}

fn to_kebab(name: &str) -> String {
    name.replace('_', "-")
}

// ---------------------------------------------------------------------------
// Check: description-shape-missing (description-driven branch projection)
// ---------------------------------------------------------------------------

fn check_applies_descriptions(skill: &Value, md: &str, violations: &mut Vec<Violation>) {
    if let Some(flow) = skill.get("flow").and_then(|f| f.as_array()) {
        check_applies_descriptions_in_flow(flow, md, violations);
    }
}

fn check_applies_descriptions_in_flow(flow: &[Value], md: &str, violations: &mut Vec<Violation>) {
    for node in flow {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind == "branch" {
            // Check if this branch has applies_descriptions
            if let Some(desc_map) = node.get("applies_descriptions").and_then(|d| d.as_object()) {
                // When applies_descriptions is populated, the compiled output
                // must use description-keyed prose, not raw condition expressions.
                // Verify that none of the block names followed by `.applies()`
                // survive literally in the markdown.
                for block_name in desc_map.keys() {
                    let raw_condition = format!("{}.applies()", block_name);
                    if md.contains(&raw_condition) {
                        violations.push(Violation::new(
                            "G::expand::description-shape-missing",
                            format!(
                                "raw condition `{}` survives in output; \
                                 description-driven branch should use resolved description prose",
                                raw_condition
                            ),
                        ));
                    }
                }
            }
            // Recurse into branch bodies
            if let Some(body) = node.get("then_body").and_then(|b| b.as_array()) {
                check_applies_descriptions_in_flow(body, md, violations);
            }
            if let Some(elifs) = node.get("elif_branches").and_then(|b| b.as_array()) {
                for elif in elifs {
                    if let Some(body) = elif.get("body").and_then(|b| b.as_array()) {
                        check_applies_descriptions_in_flow(body, md, violations);
                    }
                }
            }
            if let Some(body) = node.get("else_body").and_then(|b| b.as_array()) {
                check_applies_descriptions_in_flow(body, md, violations);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_h3_items(md_struct: &MdStructure, h3_name: &str) -> usize {
    let section = find_instructions_h3(md_struct, h3_name);
    section.map_or(0, |s| s.items.len())
}

fn find_instructions_h3<'a>(md_struct: &'a MdStructure, h3_name: &str) -> Option<&'a H3Section> {
    md_struct
        .h2_sections
        .iter()
        .find(|h2| h2.name == "Instructions")
        .and_then(|h2| h2.h3_sections.iter().find(|h3| h3.name == h3_name))
}

/// Serialize violations to JSON (for --format json output).
pub fn violations_to_json(violations: &[Violation]) -> String {
    let values: Vec<serde_json::Value> = violations
        .iter()
        .map(|v| {
            serde_json::json!({
                "id": v.id,
                "classification": "error",
                "message": v.message,
            })
        })
        .collect();
    values
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render violations in pretty format to a string.
pub fn violations_to_pretty(violations: &[Violation]) -> String {
    violations
        .iter()
        .map(|v| format!("error[{}]: {}", v.id, v.message))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: minimal valid IR JSON
    fn minimal_ir() -> String {
        serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "inline_instruction",
                        "text": "Do something.",
                        "role": "step"
                    }
                ]
            }
        })
        .to_string()
    }

    /// Helper: minimal valid MD
    fn minimal_md() -> String {
        "## Instructions\n\n### Steps\n\n1. Do something.\n".to_string()
    }

    #[test]
    fn clean_pass() {
        let violations = validate_output(&minimal_ir(), &minimal_md());
        assert!(
            violations.is_empty(),
            "expected clean pass but got: {:?}",
            violations
        );
    }

    // --- frontmatter-returned ---
    #[test]
    fn frontmatter_returned() {
        // Step 2 injected a second frontmatter block in the body
        let md = "---\nname: test\n---\n---\nextra: junk\n---\n## Instructions\n\n### Steps\n\n1. Do something.\n";
        let violations = validate_output(&minimal_ir(), md);
        assert!(violations.iter().any(|v| v.id == "G::expand::frontmatter-returned"));
    }

    #[test]
    fn legitimate_frontmatter_stripped() {
        // Legitimate Emit-produced frontmatter should be stripped and not flagged
        let md = "---\nname: test_skill\ndescription: A test skill.\n---\n## Instructions\n\n### Steps\n\n1. Do something.\n";
        let violations = validate_output(&minimal_ir(), md);
        assert!(!violations.iter().any(|v| v.id == "G::expand::frontmatter-returned"),
            "legitimate frontmatter should not be flagged: {:?}", violations);
    }

    // --- malformed-markdown ---
    #[test]
    fn malformed_markdown() {
        let md = "just some text with no headings";
        let violations = validate_output(&minimal_ir(), md);
        assert!(violations.iter().any(|v| v.id == "G::expand::malformed-markdown"));
    }

    // --- extra-h2 ---
    #[test]
    fn extra_h2() {
        let md = "## Instructions\n\n### Steps\n\n1. Do something.\n\n## Extra Section\n\nSome content.\n";
        let violations = validate_output(&minimal_ir(), md);
        assert!(violations.iter().any(|v| v.id == "G::expand::extra-h2"));
    }

    // --- missing-instructions ---
    #[test]
    fn missing_instructions() {
        let md = "## Something Else\n\n### Steps\n\n1. Do something.\n";
        let violations = validate_output(&minimal_ir(), md);
        assert!(violations.iter().any(|v| v.id == "G::expand::missing-instructions"));
    }

    // --- extra-h3 ---
    #[test]
    fn extra_h3() {
        let md = "## Instructions\n\n### Steps\n\n1. Do something.\n\n### Notes\n\nSome notes.\n";
        let violations = validate_output(&minimal_ir(), md);
        assert!(violations.iter().any(|v| v.id == "G::expand::extra-h3"));
    }

    #[test]
    fn extra_h3_accepts_valid_h3s() {
        // All valid H3s: Context, Steps, Constraints, Procedure: <name>
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [
                    { "node_id": "n1", "kind": "context", "text": "Some context." }
                ],
                "constraints": [
                    { "node_id": "n2", "kind": "constraint", "text": "A constraint.", "strength": "soft", "polarity": "require" }
                ],
                "flow": [
                    {
                        "node_id": "n3",
                        "kind": "call",
                        "target": "review_code",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Review the code (follow the review-code procedure below).",
                        "local_refs": [],
                        "projection_mode": "same_file_procedure",
                        "callee_flow": [
                            { "node_id": "n4", "kind": "inline_instruction", "text": "Scan for issues.", "role": "step" },
                            { "node_id": "n5", "kind": "inline_instruction", "text": "Report findings.", "role": "step" }
                        ],
                        "callee_context": [],
                        "callee_constraints": [],
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        let md = "\
## Instructions

### Context

- Some context.

### Steps

1. Review the code (follow the review-code procedure below).

### Constraints

- A constraint.

### Procedure: review-code

1. Scan for issues.
2. Report findings.
";
        let violations = validate_output(&ir, &md);
        let extra_h3 = violations.iter().filter(|v| v.id == "G::expand::extra-h3").collect::<Vec<_>>();
        assert!(extra_h3.is_empty(), "should not flag valid H3s: {:?}", extra_h3);
    }

    // --- step-count-mismatch ---
    #[test]
    fn step_count_mismatch() {
        let md = "## Instructions\n\n### Steps\n\n1. Do something.\n2. Extra step.\n";
        let violations = validate_output(&minimal_ir(), md);
        assert!(violations.iter().any(|v| v.id == "G::expand::step-count-mismatch"));
    }

    // --- substep-count-mismatch ---
    #[test]
    fn substep_count_mismatch() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "branch",
                        "condition": "has_tests",
                        "then_body": [
                            { "node_id": "n2", "kind": "inline_instruction", "text": "Run tests.", "role": "step" },
                            { "node_id": "n3", "kind": "inline_instruction", "text": "Check coverage.", "role": "step" }
                        ],
                        "elif_branches": [],
                        "else_body": [
                            { "node_id": "n4", "kind": "inline_instruction", "text": "Skip tests.", "role": "step" }
                        ],
                        "applies_descriptions": null
                    }
                ]
            }
        }).to_string();

        // Only 1 sub-item instead of 3 (2 then + 1 else)
        let md = "## Instructions\n\n### Steps\n\n1. If has tests:\n   a. Run tests.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::substep-count-mismatch"));
    }

    // --- constraint-count-mismatch ---
    #[test]
    fn constraint_count_mismatch() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [
                    { "node_id": "n2", "kind": "constraint", "text": "First constraint.", "strength": "soft", "polarity": "require" },
                    { "node_id": "n3", "kind": "constraint", "text": "Second constraint.", "strength": "soft", "polarity": "avoid" }
                ],
                "flow": [
                    { "node_id": "n1", "kind": "inline_instruction", "text": "Do something.", "role": "step" }
                ]
            }
        }).to_string();

        // Only 1 constraint instead of 2
        let md = "## Instructions\n\n### Steps\n\n1. Do something.\n\n### Constraints\n\n- First constraint.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::constraint-count-mismatch"));
    }

    // --- context-count-mismatch ---
    #[test]
    fn context_count_mismatch() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [
                    { "node_id": "n2", "kind": "context", "text": "Context A." },
                    { "node_id": "n3", "kind": "context", "text": "Context B." }
                ],
                "constraints": [],
                "flow": [
                    { "node_id": "n1", "kind": "inline_instruction", "text": "Do something.", "role": "step" }
                ]
            }
        }).to_string();

        // Only 1 context instead of 2
        let md = "## Instructions\n\n### Context\n\n- Context A.\n\n### Steps\n\n1. Do something.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::context-count-mismatch"));
    }

    // --- step-order-mismatch ---
    #[test]
    fn step_order_mismatch() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    { "node_id": "n1", "kind": "call", "target": "analyze", "args": {}, "output": null, "return_type": null, "effects": [], "site_modifier": null, "role": "step", "scoped_constraints": [], "resolved_body_text": "Analyze the code.", "local_refs": [], "projection_mode": "inline", "callee_flow": null, "callee_context": null, "callee_constraints": null, "procedure_path": null },
                    { "node_id": "n2", "kind": "call", "target": "fix", "args": {}, "output": null, "return_type": null, "effects": [], "site_modifier": null, "role": "step", "scoped_constraints": [], "resolved_body_text": "Fix the issue.", "local_refs": [], "projection_mode": "inline", "callee_flow": null, "callee_context": null, "callee_constraints": null, "procedure_path": null }
                ]
            }
        }).to_string();

        // Reversed order
        let md = "## Instructions\n\n### Steps\n\n1. Fix the issue.\n2. Analyze the code.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::step-order-mismatch"), "violations: {:?}", violations);
    }

    // --- invented-param-ref ---
    #[test]
    fn invented_param_ref() {
        let md = "## Instructions\n\n### Steps\n\n1. Do something in {unknown_param}.\n";
        let violations = validate_output(&minimal_ir(), md);
        assert!(violations.iter().any(|v| v.id == "G::expand::invented-param-ref"));
    }

    // --- dropped-param-ref ---
    #[test]
    fn dropped_param_ref() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [
                    { "node_id": "n1", "kind": "param", "name": "scope", "default": { "kind": "string", "value": "." } }
                ],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n2",
                        "kind": "call",
                        "target": "inspect",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Inspect {scope} for issues.",
                        "local_refs": [],
                        "projection_mode": "inline",
                        "callee_flow": null,
                        "callee_context": null,
                        "callee_constraints": null,
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        // MD doesn't use {scope}
        let md = "## Parameters\n- **scope**: Area to focus on (default: \".\")\n\n## Instructions\n\n### Steps\n\n1. Inspect the area for issues.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::dropped-param-ref"), "violations: {:?}", violations);
    }

    // --- unresolved-local-ref ---
    #[test]
    fn unresolved_local_ref() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "call",
                        "target": "analyze",
                        "args": {},
                        "output": "diagnosis",
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Analyze the code.",
                        "local_refs": [],
                        "projection_mode": "inline",
                        "callee_flow": null,
                        "callee_context": null,
                        "callee_constraints": null,
                        "procedure_path": null
                    },
                    {
                        "node_id": "n2",
                        "kind": "call",
                        "target": "fix",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Fix based on {diagnosis}.",
                        "local_refs": [
                            { "name": "diagnosis", "node_id": "n1" }
                        ],
                        "projection_mode": "inline",
                        "callee_flow": null,
                        "callee_context": null,
                        "callee_constraints": null,
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        // MD still has {diagnosis} as a literal token
        let md = "## Instructions\n\n### Steps\n\n1. Analyze the code.\n2. Fix based on {diagnosis}.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::unresolved-local-ref"), "violations: {:?}", violations);
    }

    // --- modifier-leaked ---
    #[test]
    fn modifier_leaked() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "call",
                        "target": "inspect",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": "focus on auth boundaries",
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Inspect the code.",
                        "local_refs": [],
                        "projection_mode": "inline",
                        "callee_flow": null,
                        "callee_context": null,
                        "callee_constraints": null,
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        // MD contains the modifier verbatim
        let md = "## Instructions\n\n### Steps\n\n1. Inspect the code. focus on auth boundaries.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::modifier-leaked"), "violations: {:?}", violations);
    }

    // --- params-section-mismatch ---
    #[test]
    fn params_section_mismatch() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [
                    { "node_id": "n1", "kind": "param", "name": "scope" },
                    { "node_id": "n2", "kind": "param", "name": "depth" }
                ],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    { "node_id": "n3", "kind": "inline_instruction", "text": "Do something.", "role": "step" }
                ]
            }
        }).to_string();

        // Only 1 param listed instead of 2
        let md = "## Parameters\n- **scope**: The scope\n\n## Instructions\n\n### Steps\n\n1. Do something.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::params-section-mismatch"), "violations: {:?}", violations);
    }

    // --- params-section-missing ---
    #[test]
    fn params_section_missing() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [
                    { "node_id": "n1", "kind": "param", "name": "scope" }
                ],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    { "node_id": "n2", "kind": "inline_instruction", "text": "Do something.", "role": "step" }
                ]
            }
        }).to_string();

        // No ## Parameters section
        let md = "## Instructions\n\n### Steps\n\n1. Do something.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::params-section-missing"));
    }

    // --- params-section-spurious ---
    #[test]
    fn params_section_spurious() {
        let md = "## Parameters\n- **scope**: something\n\n## Instructions\n\n### Steps\n\n1. Do something.\n";
        let violations = validate_output(&minimal_ir(), md);
        assert!(violations.iter().any(|v| v.id == "G::expand::params-section-spurious"));
    }

    // --- step-too-long ---
    #[test]
    fn step_too_long() {
        let md = "## Instructions\n\n### Steps\n\n1. First sentence. Second sentence. Third sentence. Fourth sentence.\n";
        let violations = validate_output(&minimal_ir(), md);
        assert!(violations.iter().any(|v| v.id == "G::expand::step-too-long"), "violations: {:?}", violations);
    }

    // --- constraint-multi-sentence ---
    #[test]
    fn constraint_multi_sentence() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [
                    { "node_id": "n2", "kind": "constraint", "text": "Don't do bad things.", "strength": "soft", "polarity": "avoid" }
                ],
                "flow": [
                    { "node_id": "n1", "kind": "inline_instruction", "text": "Do something.", "role": "step" }
                ]
            }
        }).to_string();

        let md = "## Instructions\n\n### Steps\n\n1. Do something.\n\n### Constraints\n\n- Don't do bad things. Also don't do other bad things.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::constraint-multi-sentence"), "violations: {:?}", violations);
    }

    // --- procedure-count-mismatch ---
    #[test]
    fn procedure_count_mismatch() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "call",
                        "target": "review_code",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Review the code (follow the review-code procedure below).",
                        "local_refs": [],
                        "projection_mode": "same_file_procedure",
                        "callee_flow": [
                            { "node_id": "n2", "kind": "inline_instruction", "text": "Scan for issues.", "role": "step" }
                        ],
                        "callee_context": [],
                        "callee_constraints": [],
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        // No procedure section
        let md = "## Instructions\n\n### Steps\n\n1. Review the code (follow the review-code procedure below).\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::procedure-count-mismatch"), "violations: {:?}", violations);
    }

    // --- procedure-name-mismatch ---
    #[test]
    fn procedure_name_mismatch() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "call",
                        "target": "review_code",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Review the code (follow the wrong-name procedure below).",
                        "local_refs": [],
                        "projection_mode": "same_file_procedure",
                        "callee_flow": [
                            { "node_id": "n2", "kind": "inline_instruction", "text": "Scan.", "role": "step" }
                        ],
                        "callee_context": [],
                        "callee_constraints": [],
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        let md = "## Instructions\n\n### Steps\n\n1. Review the code (follow the wrong-name procedure below).\n\n### Procedure: wrong-name\n\n1. Scan.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::procedure-name-mismatch"), "violations: {:?}", violations);
    }

    // --- procedure-step-count-mismatch ---
    #[test]
    fn procedure_step_count_mismatch() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "call",
                        "target": "review_code",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Review the code (follow the review-code procedure below).",
                        "local_refs": [],
                        "projection_mode": "same_file_procedure",
                        "callee_flow": [
                            { "node_id": "n2", "kind": "inline_instruction", "text": "Scan.", "role": "step" },
                            { "node_id": "n3", "kind": "inline_instruction", "text": "Report.", "role": "step" }
                        ],
                        "callee_context": [],
                        "callee_constraints": [],
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        // Procedure section has 1 item instead of 2
        let md = "## Instructions\n\n### Steps\n\n1. Review the code (follow the review-code procedure below).\n\n### Procedure: review-code\n\n1. Scan.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::procedure-step-count-mismatch"), "violations: {:?}", violations);
    }

    // --- procedure-ref-missing ---
    #[test]
    fn procedure_ref_missing() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "call",
                        "target": "review_code",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Review the code.",
                        "local_refs": [],
                        "projection_mode": "same_file_procedure",
                        "callee_flow": [
                            { "node_id": "n2", "kind": "inline_instruction", "text": "Scan.", "role": "step" }
                        ],
                        "callee_context": [],
                        "callee_constraints": [],
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        // Step doesn't mention the procedure name
        let md = "## Instructions\n\n### Steps\n\n1. Review the code.\n\n### Procedure: review-code\n\n1. Scan.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::procedure-ref-missing"), "violations: {:?}", violations);
    }

    // --- procedure-ref-dangling ---
    #[test]
    fn procedure_ref_dangling() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "call",
                        "target": "review_code",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Review the code (follow the review-code procedure below).",
                        "local_refs": [],
                        "projection_mode": "same_file_procedure",
                        "callee_flow": [
                            { "node_id": "n2", "kind": "inline_instruction", "text": "Scan.", "role": "step" }
                        ],
                        "callee_context": [],
                        "callee_constraints": [],
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        // Step references review-code but no procedure section exists
        let md = "## Instructions\n\n### Steps\n\n1. Review the code (follow the review-code procedure below).\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::procedure-ref-dangling"), "violations: {:?}", violations);
    }

    // --- procedure-duplicate ---
    #[test]
    fn procedure_duplicate() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    { "node_id": "n1", "kind": "inline_instruction", "text": "Do something.", "role": "step" }
                ]
            }
        }).to_string();

        let md = "## Instructions\n\n### Steps\n\n1. Do something.\n\n### Procedure: review-code\n\n1. Scan.\n\n### Procedure: review-code\n\n1. Report.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::procedure-duplicate"), "violations: {:?}", violations);
    }

    // --- procedure-order ---
    #[test]
    fn procedure_order() {
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "call",
                        "target": "step_a",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Do step-a procedure.",
                        "local_refs": [],
                        "projection_mode": "same_file_procedure",
                        "callee_flow": [
                            { "node_id": "n2", "kind": "inline_instruction", "text": "A1.", "role": "step" }
                        ],
                        "callee_context": [],
                        "callee_constraints": [],
                        "procedure_path": null
                    },
                    {
                        "node_id": "n3",
                        "kind": "call",
                        "target": "step_b",
                        "args": {},
                        "output": null,
                        "return_type": null,
                        "effects": [],
                        "site_modifier": null,
                        "role": "step",
                        "scoped_constraints": [],
                        "resolved_body_text": "Do step-b procedure.",
                        "local_refs": [],
                        "projection_mode": "same_file_procedure",
                        "callee_flow": [
                            { "node_id": "n4", "kind": "inline_instruction", "text": "B1.", "role": "step" }
                        ],
                        "callee_context": [],
                        "callee_constraints": [],
                        "procedure_path": null
                    }
                ]
            }
        }).to_string();

        // Wrong order: step-b before step-a
        let md = "## Instructions\n\n### Steps\n\n1. Do step-a procedure.\n2. Do step-b procedure.\n\n### Procedure: step-b\n\n1. B1.\n\n### Procedure: step-a\n\n1. A1.\n";
        let violations = validate_output(&ir, md);
        assert!(violations.iter().any(|v| v.id == "G::expand::procedure-order"), "violations: {:?}", violations);
    }

    // --- sentence counting ---
    #[test]
    fn sentence_counting() {
        assert_eq!(count_sentences("One sentence."), 1);
        assert_eq!(count_sentences("First. Second."), 2);
        assert_eq!(count_sentences("First. Second. Third."), 3);
        assert_eq!(count_sentences("First. Second. Third. Fourth."), 4);
        // Code spans stripped
        assert_eq!(count_sentences("Use `file.txt` here."), 1);
        // No trailing period
        assert_eq!(count_sentences("Just text"), 1);
    }

    // --- format output ---
    #[test]
    fn json_output_format() {
        let violations = vec![Violation::new("G::expand::extra-h2", "bad section")];
        let json = violations_to_json(&violations);
        assert!(json.contains("G::expand::extra-h2"));
        assert!(json.contains("bad section"));
    }

    #[test]
    fn pretty_output_format() {
        let violations = vec![Violation::new("G::expand::extra-h2", "bad section")];
        let pretty = violations_to_pretty(&violations);
        assert!(pretty.contains("error[G::expand::extra-h2]: bad section"));
    }

    // --- description-driven branch validation ---
    #[test]
    fn description_driven_branch_rejects_missing_description_shape() {
        // A pure-applies branch (all conditions are .applies() calls) should project
        // using description-keyed shape. This test verifies that validates passes
        // when the branch is correctly rendered.
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "branch",
                        "condition": "fork_with_plan.applies()",
                        "then_body": [
                            { "node_id": "n2", "kind": "inline_instruction", "text": "Fork with plan.", "role": "step" }
                        ],
                        "elif_branches": [
                            {
                                "node_id": "n3",
                                "kind": "elif_branch",
                                "condition": "fork_with_summary.applies()",
                                "body": [
                                    { "node_id": "n4", "kind": "inline_instruction", "text": "Fork with summary.", "role": "step" }
                                ]
                            }
                        ],
                        "else_body": null,
                        "applies_descriptions": {
                            "fork_with_plan": "Fork a terminal with a plan.",
                            "fork_with_summary": "Fork a terminal with a summary."
                        }
                    }
                ]
            }
        }).to_string();

        // Valid rendering with sub-steps
        let md = "## Instructions\n\n### Steps\n\n1. Decide which approach applies:\n   a. Fork with plan.\n   b. Fork with summary.\n";
        let violations = validate_output(&ir, md);
        // Should pass without branch-specific errors
        let branch_errors: Vec<_> = violations.iter()
            .filter(|v| v.id.contains("substep") || v.id.contains("step-count"))
            .collect();
        assert!(branch_errors.is_empty(), "unexpected branch errors: {:?}", branch_errors);
    }

    #[test]
    fn description_driven_branch_rejects_raw_applies_condition() {
        // When a Branch has applies_descriptions, the compiled output must NOT
        // contain the raw `.applies()` condition expressions. If they survive,
        // it means the description-keyed rendering failed.
        let ir = serde_json::json!({
            "ir_version": 1,
            "compiler": "glyph 0.1.0",
            "source_file": "test.glyph.md",
            "skill": {
                "node_id": "n0",
                "kind": "skill",
                "name": "test_skill",
                "description": "A test skill.",
                "params": [],
                "effects": [],
                "context": [],
                "constraints": [],
                "flow": [
                    {
                        "node_id": "n1",
                        "kind": "branch",
                        "condition": "fork_with_plan.applies()",
                        "then_body": [
                            { "node_id": "n2", "kind": "inline_instruction", "text": "Fork with plan.", "role": "step" }
                        ],
                        "elif_branches": [
                            {
                                "node_id": "n3",
                                "kind": "elif_branch",
                                "condition": "fork_with_summary.applies()",
                                "body": [
                                    { "node_id": "n4", "kind": "inline_instruction", "text": "Fork with summary.", "role": "step" }
                                ]
                            }
                        ],
                        "else_body": null,
                        "applies_descriptions": {
                            "fork_with_plan": "Fork a terminal with a plan.",
                            "fork_with_summary": "Fork a terminal with a summary."
                        }
                    }
                ]
            }
        }).to_string();

        // BAD rendering: uses raw condition expressions instead of descriptions
        let md = "## Instructions\n\n### Steps\n\n1. If fork_with_plan.applies():\n   a. Fork with plan.\n   b. Fork with summary.\n";
        let violations = validate_output(&ir, md);
        assert!(
            violations.iter().any(|v| v.id == "G::expand::description-shape-missing"),
            "should reject raw .applies() condition in step prose; got: {:?}",
            violations
        );
    }
}
