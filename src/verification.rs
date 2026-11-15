use serde_json::{Value, json};
use uuid::Uuid;

use crate::contracts::{EpistemicStatus, VerificationResult};

const FLOAT_TOLERANCE: f64 = 1e-4;
const EXACT_FLOAT_TOLERANCE: f64 = 1e-9;

pub fn rfc3339_timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Unified entrypoint for verifying tool calculation outputs.
///
/// Dispatches to deterministic independent checkers based on tool name.
/// Returns a tuple of (VerificationResult, EpistemicStatus).
pub fn verify_tool(
    tool_name: &str,
    args: &Value,
    worker_result: &Result<Value, String>,
) -> (VerificationResult, EpistemicStatus) {
    let now = rfc3339_timestamp();
    let id = Uuid::new_v4().to_string();
    let subject_id = tool_name.to_string();

    let output = match worker_result {
        Ok(val) => val,
        Err(err_msg) => {
            let res = VerificationResult {
                id,
                subject_id: subject_id.clone(),
                status: EpistemicStatus::Uncertain,
                method: "Tool execution failed; independent verification not possible".into(),
                details: format!("Scientific worker execution error: {err_msg}"),
                independently_checked: false,
                checker: None,
                original_calculation: None,
                independently_calculated: None,
                absolute_error: None,
                relative_error: None,
                comparison_tolerance: None,
                explanation: Some(format!("Execution failed: {err_msg}")),
                verified_at: Some(now),
            };
            return (res, EpistemicStatus::Uncertain);
        }
    };

    match tool_name {
        "projectile" => verify_projectile(id, subject_id, args, output, now),
        "arithmetic" => verify_arithmetic(id, subject_id, args, output, now),
        "statistics" => verify_statistics(id, subject_id, args, output, now),
        "power_law" => verify_power_law(id, subject_id, args, output, now),
        _ => verify_unsupported(id, subject_id, tool_name, output, now),
    }
}

/// Priority 1: Independent Newtonian projectile motion checker in Rust.
///
/// Formulas for ideal projectile under uniform gravity:
/// R = v^2 * sin(2θ) / g
/// T = 2v * sin(θ) / g
/// H = v^2 * sin²(θ) / (2g)
pub fn verify_projectile(
    id: String,
    subject_id: String,
    args: &Value,
    output: &Value,
    timestamp: String,
) -> (VerificationResult, EpistemicStatus) {
    let checker = "rust_independent_projectile_checker".to_string();
    let method = "Closed-form Newtonian projectile dynamics independent verification".to_string();

    // Validate inputs
    let speed = args.get("speed").and_then(Value::as_f64);
    let deg = args.get("angle_degrees").and_then(Value::as_f64);
    let g = args
        .get("gravity")
        .and_then(Value::as_f64)
        .unwrap_or(9.80665);

    let (v, theta_deg, gravity) = match (speed, deg, g) {
        (Some(v), Some(d), grav)
            if v.is_finite() && d.is_finite() && grav.is_finite() && v >= 0.0 && grav > 0.0 =>
        {
            (v, d, grav)
        }
        _ => {
            let res = VerificationResult {
                id,
                subject_id,
                status: EpistemicStatus::Contradicted,
                method,
                details: "Invalid or non-finite projectile parameters (speed must be >= 0, gravity > 0)".into(),
                independently_checked: true,
                checker: Some(checker),
                original_calculation: Some(output.clone()),
                independently_calculated: None,
                absolute_error: None,
                relative_error: None,
                comparison_tolerance: Some(FLOAT_TOLERANCE),
                explanation: Some("Input validation failed: negative velocity, non-positive gravity, or non-finite numeric input.".into()),
                verified_at: Some(timestamp),
            };
            return (res, EpistemicStatus::Contradicted);
        }
    };

    // Independent Rust derivation
    let rad = theta_deg.to_radians();
    let sin_theta = rad.sin();
    let sin_2theta = (2.0 * rad).sin();

    // Boundary conditions:
    // When theta = 0: R = 0, T = 0, H = 0.
    // When theta = 90 deg: R = 0, T = 2v/g, H = v^2 / (2g).
    let expected_range =
        if (theta_deg % 180.0).abs() < 1e-9 || ((theta_deg - 90.0) % 180.0).abs() < 1e-9 {
            0.0
        } else {
            v * v * sin_2theta / gravity
        };
    let expected_time = if (theta_deg % 180.0).abs() < 1e-9 {
        0.0
    } else {
        2.0 * v * sin_theta / gravity
    };
    let expected_height = if (theta_deg % 180.0).abs() < 1e-9 {
        0.0
    } else {
        v * v * sin_theta.powi(2) / (2.0 * gravity)
    };

    let expected_json = json!({
        "range_m": expected_range,
        "flight_time_s": expected_time,
        "max_height_m": expected_height,
        "assumptions": ["vacuum", "level launch and landing", "constant gravity"]
    });

    // Extract actual output
    let actual_range = output.get("range_m").and_then(Value::as_f64);
    let actual_time = output.get("flight_time_s").and_then(Value::as_f64);
    let actual_height = output.get("max_height_m").and_then(Value::as_f64);

    let (act_r, act_t, act_h) = match (actual_range, actual_time, actual_height) {
        (Some(r), Some(t), Some(h)) if r.is_finite() && t.is_finite() && h.is_finite() => (r, t, h),
        _ => {
            let res = VerificationResult {
                id,
                subject_id,
                status: EpistemicStatus::Contradicted,
                method,
                details: "Worker output missing required fields or produced non-finite values"
                    .into(),
                independently_checked: true,
                checker: Some(checker),
                original_calculation: Some(output.clone()),
                independently_calculated: Some(expected_json),
                absolute_error: None,
                relative_error: None,
                comparison_tolerance: Some(FLOAT_TOLERANCE),
                explanation: Some(
                    "Missing range_m, flight_time_s, or max_height_m in worker output.".into(),
                ),
                verified_at: Some(timestamp),
            };
            return (res, EpistemicStatus::Contradicted);
        }
    };

    let err_r = (act_r - expected_range).abs();
    let err_t = (act_t - expected_time).abs();
    let err_h = (act_h - expected_height).abs();
    let max_abs_error = err_r.max(err_t).max(err_h);

    let rel_r = err_r / (expected_range.abs().max(1.0));
    let rel_t = err_t / (expected_time.abs().max(1.0));
    let rel_h = err_h / (expected_height.abs().max(1.0));
    let max_rel_error = rel_r.max(rel_t).max(rel_h);

    let passed = max_abs_error <= FLOAT_TOLERANCE || max_rel_error <= FLOAT_TOLERANCE;

    let status = if passed {
        EpistemicStatus::Verified
    } else {
        EpistemicStatus::Contradicted
    };

    let details = if passed {
        format!(
            "Independently verified in Rust: R={expected_range:.4}m, T={expected_time:.4}s, H={expected_height:.4}m (max error: {max_abs_error:.2e} <= {FLOAT_TOLERANCE:.0e})"
        )
    } else {
        format!(
            "Contradiction detected: Expected R={expected_range:.4}m, T={expected_time:.4}s, H={expected_height:.4}m, but worker returned R={act_r:.4}m, T={act_t:.4}s, H={act_h:.4}m"
        )
    };

    let explanation = Some(format!(
        "Independent derivation of Newtonian ballistics verified that range, time of flight, and apex height match closed-form formulas within tolerance {FLOAT_TOLERANCE:.0e}."
    ));

    let res = VerificationResult {
        id,
        subject_id,
        status: status.clone(),
        method,
        details,
        independently_checked: true,
        checker: Some(checker),
        original_calculation: Some(output.clone()),
        independently_calculated: Some(expected_json),
        absolute_error: Some(max_abs_error),
        relative_error: Some(max_rel_error),
        comparison_tolerance: Some(FLOAT_TOLERANCE),
        explanation,
        verified_at: Some(timestamp),
    };

    (res, status)
}

/// Priority 2: Independent Arithmetic verification in Rust.
pub fn verify_arithmetic(
    id: String,
    subject_id: String,
    args: &Value,
    output: &Value,
    timestamp: String,
) -> (VerificationResult, EpistemicStatus) {
    let checker = "rust_independent_arithmetic_checker".to_string();
    let method = "Exact independent IEEE-754 arithmetic derivation in Rust".to_string();

    let a_opt = args.get("a").and_then(Value::as_f64);
    let b_opt = args.get("b").and_then(Value::as_f64);
    let op = args.get("op").and_then(Value::as_str).unwrap_or("add");

    let (a, b) = match (a_opt, b_opt) {
        (Some(a), Some(b)) if a.is_finite() && b.is_finite() => (a, b),
        _ => {
            let res = VerificationResult {
                id,
                subject_id,
                status: EpistemicStatus::Contradicted,
                method,
                details: "Arithmetic operands are missing or non-finite".into(),
                independently_checked: true,
                checker: Some(checker),
                original_calculation: Some(output.clone()),
                independently_calculated: None,
                absolute_error: None,
                relative_error: None,
                comparison_tolerance: Some(EXACT_FLOAT_TOLERANCE),
                explanation: Some("Non-finite operand detected.".into()),
                verified_at: Some(timestamp),
            };
            return (res, EpistemicStatus::Contradicted);
        }
    };

    let expected = match op {
        "add" => Some(a + b),
        "subtract" => Some(a - b),
        "multiply" => Some(a * b),
        "divide" => {
            if b == 0.0 {
                None // Division by zero
            } else {
                Some(a / b)
            }
        }
        _ => None,
    };

    let expected_val = match expected {
        Some(v) if v.is_finite() => v,
        _ => {
            let is_div_zero = op == "divide" && b == 0.0;
            let status = if is_div_zero {
                EpistemicStatus::Contradicted
            } else {
                EpistemicStatus::Uncertain
            };
            let res = VerificationResult {
                id,
                subject_id,
                status: status.clone(),
                method,
                details: if is_div_zero {
                    "Division by zero is undefined".into()
                } else {
                    format!("Unsupported arithmetic operation: {op}")
                },
                independently_checked: true,
                checker: Some(checker),
                original_calculation: Some(output.clone()),
                independently_calculated: None,
                absolute_error: None,
                relative_error: None,
                comparison_tolerance: Some(EXACT_FLOAT_TOLERANCE),
                explanation: Some("Operation evaluated to undefined or non-finite result.".into()),
                verified_at: Some(timestamp),
            };
            return (res, status);
        }
    };

    let actual_val = output.get("value").and_then(Value::as_f64);
    let act = match actual_val {
        Some(v) if v.is_finite() => v,
        _ => {
            let res = VerificationResult {
                id,
                subject_id,
                status: EpistemicStatus::Contradicted,
                method,
                details: "Worker output missing numeric 'value' or returned non-finite value"
                    .into(),
                independently_checked: true,
                checker: Some(checker),
                original_calculation: Some(output.clone()),
                independently_calculated: Some(json!({"value": expected_val})),
                absolute_error: None,
                relative_error: None,
                comparison_tolerance: Some(EXACT_FLOAT_TOLERANCE),
                explanation: Some("Missing numeric value in worker output.".into()),
                verified_at: Some(timestamp),
            };
            return (res, EpistemicStatus::Contradicted);
        }
    };

    let abs_error = (act - expected_val).abs();
    let rel_error = abs_error / (expected_val.abs().max(1.0));
    let passed = abs_error <= EXACT_FLOAT_TOLERANCE || rel_error <= EXACT_FLOAT_TOLERANCE;

    let status = if passed {
        EpistemicStatus::Verified
    } else {
        EpistemicStatus::Contradicted
    };

    let details = if passed {
        format!(
            "Independently verified arithmetic {a} {op} {b} = {expected_val} (diff: {abs_error:.2e})"
        )
    } else {
        format!("Contradiction: expected {expected_val}, but worker computed {act}")
    };

    let res = VerificationResult {
        id,
        subject_id,
        status: status.clone(),
        method,
        details,
        independently_checked: true,
        checker: Some(checker),
        original_calculation: Some(output.clone()),
        independently_calculated: Some(json!({"value": expected_val, "operation": op})),
        absolute_error: Some(abs_error),
        relative_error: Some(rel_error),
        comparison_tolerance: Some(EXACT_FLOAT_TOLERANCE),
        explanation: Some(format!(
            "Deterministic recalculation of {op} yielded {expected_val}."
        )),
        verified_at: Some(timestamp),
    };

    (res, status)
}

/// Priority 3: Independent Statistics verification in Rust.
pub fn verify_statistics(
    id: String,
    subject_id: String,
    args: &Value,
    output: &Value,
    timestamp: String,
) -> (VerificationResult, EpistemicStatus) {
    let checker = "rust_independent_statistics_checker".to_string();
    let method =
        "Independent two-pass statistical derivation with Bessel correction in Rust".to_string();

    let values_arr = args.get("values").and_then(Value::as_array);
    let values: Vec<f64> = match values_arr {
        Some(arr) if arr.len() >= 2 => {
            let mut parsed = Vec::with_capacity(arr.len());
            let mut valid = true;
            for item in arr {
                if let Some(x) = item.as_f64() {
                    if x.is_finite() {
                        parsed.push(x);
                    } else {
                        valid = false;
                        break;
                    }
                } else {
                    valid = false;
                    break;
                }
            }
            if valid && parsed.len() >= 2 {
                parsed
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };

    if values.len() < 2 {
        let res = VerificationResult {
            id,
            subject_id,
            status: EpistemicStatus::Contradicted,
            method,
            details: "Statistics require at least two finite numeric values".into(),
            independently_checked: true,
            checker: Some(checker),
            original_calculation: Some(output.clone()),
            independently_calculated: None,
            absolute_error: None,
            relative_error: None,
            comparison_tolerance: Some(EXACT_FLOAT_TOLERANCE),
            explanation: Some("Input array contained fewer than two finite numbers.".into()),
            verified_at: Some(timestamp),
        };
        return (res, EpistemicStatus::Contradicted);
    }

    let n = values.len() as f64;
    let sum: f64 = values.iter().sum();
    let mean = sum / n;
    let var_sum: f64 = values.iter().map(|x| (x - mean).powi(2)).sum();
    let sample_stdev = (var_sum / (n - 1.0)).sqrt();
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;
    for &x in &values {
        if x < min_val {
            min_val = x;
        }
        if x > max_val {
            max_val = x;
        }
    }

    let expected_json = json!({
        "count": values.len(),
        "mean": mean,
        "sample_stdev": sample_stdev,
        "min": min_val,
        "max": max_val,
    });

    let act_count = output.get("count").and_then(Value::as_u64);
    let act_mean = output.get("mean").and_then(Value::as_f64);
    let act_stdev = output.get("sample_stdev").and_then(Value::as_f64);
    let act_min = output.get("min").and_then(Value::as_f64);
    let act_max = output.get("max").and_then(Value::as_f64);

    let (c, m, s, mn, mx) = match (act_count, act_mean, act_stdev, act_min, act_max) {
        (Some(c), Some(m), Some(s), Some(mn), Some(mx))
            if m.is_finite() && s.is_finite() && mn.is_finite() && mx.is_finite() =>
        {
            (c as usize, m, s, mn, mx)
        }
        _ => {
            let res = VerificationResult {
                id,
                subject_id,
                status: EpistemicStatus::Contradicted,
                method,
                details: "Worker statistics output is malformed or non-finite".into(),
                independently_checked: true,
                checker: Some(checker),
                original_calculation: Some(output.clone()),
                independently_calculated: Some(expected_json),
                absolute_error: None,
                relative_error: None,
                comparison_tolerance: Some(EXACT_FLOAT_TOLERANCE),
                explanation: Some("Missing or non-finite statistics output fields.".into()),
                verified_at: Some(timestamp),
            };
            return (res, EpistemicStatus::Contradicted);
        }
    };

    let count_match = c == values.len();
    let err_mean = (m - mean).abs();
    let err_stdev = (s - sample_stdev).abs();
    let err_min = (mn - min_val).abs();
    let err_max = (mx - max_val).abs();
    let max_abs_error = err_mean.max(err_stdev).max(err_min).max(err_max);

    let passed = count_match && max_abs_error <= EXACT_FLOAT_TOLERANCE;

    let status = if passed {
        EpistemicStatus::Verified
    } else {
        EpistemicStatus::Contradicted
    };

    let details = if passed {
        format!(
            "Independently verified descriptive statistics: N={}, mean={mean:.4}, sample_stdev={sample_stdev:.4}, min={min_val}, max={max_val} (diff: {max_abs_error:.2e})",
            values.len()
        )
    } else {
        format!(
            "Statistics contradiction: expected mean={mean}, stdev={sample_stdev}, min={min_val}, max={max_val}; worker returned mean={m}, stdev={s}, min={mn}, max={mx}"
        )
    };

    let res = VerificationResult {
        id,
        subject_id,
        status: status.clone(),
        method,
        details,
        independently_checked: true,
        checker: Some(checker),
        original_calculation: Some(output.clone()),
        independently_calculated: Some(expected_json),
        absolute_error: Some(max_abs_error),
        relative_error: Some(max_abs_error / (mean.abs().max(1.0))),
        comparison_tolerance: Some(EXACT_FLOAT_TOLERANCE),
        explanation: Some(
            "Independently verified population count, sample mean, sample standard deviation with Bessel correction (N-1), and extrema.".into(),
        ),
        verified_at: Some(timestamp),
    };

    (res, status)
}

/// Priority 4: Independent consistency check for Power-Law fitting.
///
/// Note: Power-law regression is an empirical statistical fit (not a deterministic physical law).
/// As required by the specification, it is validated for mathematical consistency against
/// least-squares normal equations in log-log space, but retains `Calculated` status (never claimed as `Verified` ground truth).
pub fn verify_power_law(
    id: String,
    subject_id: String,
    args: &Value,
    output: &Value,
    timestamp: String,
) -> (VerificationResult, EpistemicStatus) {
    let checker = "rust_power_law_consistency_checker".to_string();
    let method = "Least-squares normal-equation consistency check in log-log space".to_string();

    let xs = args.get("x").and_then(Value::as_array);
    let ys = args.get("y").and_then(Value::as_array);

    let (x_vals, y_vals) = match (xs, ys) {
        (Some(x_arr), Some(y_arr)) if x_arr.len() == y_arr.len() && x_arr.len() >= 3 => {
            let mut xv = Vec::new();
            let mut yv = Vec::new();
            let mut ok = true;
            for (x, y) in x_arr.iter().zip(y_arr.iter()) {
                if let (Some(x_num), Some(y_num)) = (x.as_f64(), y.as_f64()) {
                    if x_num > 0.0 && y_num > 0.0 && x_num.is_finite() && y_num.is_finite() {
                        xv.push(x_num);
                        yv.push(y_num);
                    } else {
                        ok = false;
                        break;
                    }
                } else {
                    ok = false;
                    break;
                }
            }
            if ok && xv.len() >= 3 {
                (xv, yv)
            } else {
                (Vec::new(), Vec::new())
            }
        }
        _ => (Vec::new(), Vec::new()),
    };

    if x_vals.len() < 3 {
        let res = VerificationResult {
            id,
            subject_id,
            status: EpistemicStatus::Contradicted,
            method,
            details: "Power-law fit requires at least 3 paired positive finite values".into(),
            independently_checked: false,
            checker: Some(checker),
            original_calculation: Some(output.clone()),
            independently_calculated: None,
            absolute_error: None,
            relative_error: None,
            comparison_tolerance: Some(FLOAT_TOLERANCE),
            explanation: Some("Input validation failed for power-law dataset.".into()),
            verified_at: Some(timestamp),
        };
        return (res, EpistemicStatus::Contradicted);
    }

    let coef_opt = output.get("coefficient").and_then(Value::as_f64);
    let exp_opt = output.get("exponent").and_then(Value::as_f64);
    let r2_opt = output.get("r_squared_log_space").and_then(Value::as_f64);

    let (a, b, r2) = match (coef_opt, exp_opt, r2_opt) {
        (Some(a), Some(b), Some(r2))
            if a > 0.0 && a.is_finite() && b.is_finite() && r2.is_finite() =>
        {
            (a, b, r2)
        }
        _ => {
            let res = VerificationResult {
                id,
                subject_id,
                status: EpistemicStatus::Contradicted,
                method,
                details: "Power-law output missing positive coefficient, exponent, or r_squared"
                    .into(),
                independently_checked: false,
                checker: Some(checker),
                original_calculation: Some(output.clone()),
                independently_calculated: None,
                absolute_error: None,
                relative_error: None,
                comparison_tolerance: Some(FLOAT_TOLERANCE),
                explanation: Some("Missing or non-physical power-law fit parameters.".into()),
                verified_at: Some(timestamp),
            };
            return (res, EpistemicStatus::Contradicted);
        }
    };

    // Evaluate normal equations:
    // Model in log-space: Y = ln(a) + b * X
    // Residual: e_i = ln(y_i) - (ln(a) + b * ln(x_i))
    // Least squares optimality conditions:
    // 1) sum(e_i) == 0
    // 2) sum(X_i * e_i) == 0
    let ln_a = a.ln();
    let mut sum_e = 0.0;
    let mut sum_x_e = 0.0;
    for (x, y) in x_vals.iter().zip(y_vals.iter()) {
        let lx = x.ln();
        let ly = y.ln();
        let e = ly - (ln_a + b * lx);
        sum_e += e;
        sum_x_e += lx * e;
    }

    let res_norm = sum_e.abs().max(sum_x_e.abs()) / (x_vals.len() as f64);
    let r2_valid = (0.0..=1.0001).contains(&r2);

    let passed_consistency = res_norm <= FLOAT_TOLERANCE && r2_valid;

    let (status, independently_checked) = if passed_consistency {
        // As per specification: Empirical fits retain Calculated status, NOT Verified.
        (EpistemicStatus::Calculated, false)
    } else {
        (EpistemicStatus::Contradicted, true)
    };

    let details = if passed_consistency {
        format!(
            "Consistency check passed: Normal equation residuals < {FLOAT_TOLERANCE:.0e} (R²={r2:.4}, coef={a:.4}, exponent={b:.4}). Retained as Calculated (empirical fit, not ground-truth verified)."
        )
    } else {
        format!(
            "Consistency check failed: Parameters violate least-squares normal equations (residual={res_norm:.2e}, R²={r2})"
        )
    };

    let res = VerificationResult {
        id,
        subject_id,
        status: status.clone(),
        method,
        details,
        independently_checked,
        checker: Some(checker),
        original_calculation: Some(output.clone()),
        independently_calculated: Some(json!({
            "normal_equations_residual": res_norm,
            "consistency_verified": passed_consistency
        })),
        absolute_error: Some(res_norm),
        relative_error: None,
        comparison_tolerance: Some(FLOAT_TOLERANCE),
        explanation: Some(
            "Empirical power-law regression evaluated against least-squares orthogonality criteria. Because empirical regression is a statistical modeling artifact and not an absolute physical proof, it is honestly classified as Calculated rather than Verified.".into(),
        ),
        verified_at: Some(timestamp),
    };

    (res, status)
}

/// Fallback for unverified or unsupported tool types.
fn verify_unsupported(
    id: String,
    subject_id: String,
    tool_name: &str,
    output: &Value,
    timestamp: String,
) -> (VerificationResult, EpistemicStatus) {
    let res = VerificationResult {
        id,
        subject_id,
        status: EpistemicStatus::Calculated,
        method: format!("No independent deterministic checker configured for '{tool_name}'"),
        details: "Result computed by isolated Python worker; retains calculated status without independent proof.".into(),
        independently_checked: false,
        checker: None,
        original_calculation: Some(output.clone()),
        independently_calculated: None,
        absolute_error: None,
        relative_error: None,
        comparison_tolerance: None,
        explanation: Some("Deterministic verification not configured for this tool type.".into()),
        verified_at: Some(timestamp),
    };
    (res, EpistemicStatus::Calculated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_projectile_verification_different_angles_and_velocities() {
        let now = rfc3339_timestamp();
        // 45 degrees, 20 m/s, standard g
        let v = 20.0;
        let deg = 45.0;
        let g = 9.80665;
        let rad = deg * std::f64::consts::PI / 180.0;
        let expected_range = v * v * (2.0 * rad).sin() / g;
        let expected_time = 2.0 * v * rad.sin() / g;
        let expected_height = v * v * rad.sin().powi(2) / (2.0 * g);

        let output = json!({
            "range_m": expected_range,
            "flight_time_s": expected_time,
            "max_height_m": expected_height
        });

        let (res, status) = verify_projectile(
            "test-1".into(),
            "projectile".into(),
            &json!({"speed": v, "angle_degrees": deg, "gravity": g}),
            &output,
            now.clone(),
        );

        assert_eq!(status, EpistemicStatus::Verified);
        assert!(res.independently_checked);
        assert!(res.absolute_error.unwrap() <= FLOAT_TOLERANCE);
        assert_eq!(
            res.checker.as_deref(),
            Some("rust_independent_projectile_checker")
        );
    }

    #[test]
    fn test_projectile_verification_boundary_zero_and_90_degrees() {
        let now = rfc3339_timestamp();
        // Zero launch angle
        let output_zero = json!({
            "range_m": 0.0,
            "flight_time_s": 0.0,
            "max_height_m": 0.0
        });
        let (_, status_zero) = verify_projectile(
            "test-0".into(),
            "projectile".into(),
            &json!({"speed": 15.0, "angle_degrees": 0.0, "gravity": 9.8}),
            &output_zero,
            now.clone(),
        );
        assert_eq!(status_zero, EpistemicStatus::Verified);

        // 90 degrees launch angle (straight up: range = 0, apex height = v^2 / 2g)
        let v = 10.0;
        let g = 9.80665;
        let output_90 = json!({
            "range_m": 0.0,
            "flight_time_s": 2.0 * v / g,
            "max_height_m": v * v / (2.0 * g)
        });
        let (_, status_90) = verify_projectile(
            "test-90".into(),
            "projectile".into(),
            &json!({"speed": v, "angle_degrees": 90.0, "gravity": g}),
            &output_90,
            now,
        );
        assert_eq!(status_90, EpistemicStatus::Verified);
    }

    #[test]
    fn test_projectile_rejects_invalid_inputs() {
        let now = rfc3339_timestamp();
        let dummy = json!({"range_m": 10.0, "flight_time_s": 1.0, "max_height_m": 2.0});

        // Negative velocity
        let (_, s1) = verify_projectile(
            "bad-1".into(),
            "projectile".into(),
            &json!({"speed": -5.0, "angle_degrees": 45.0}),
            &dummy,
            now.clone(),
        );
        assert_eq!(s1, EpistemicStatus::Contradicted);

        // Negative or zero gravity
        let (_, s2) = verify_projectile(
            "bad-2".into(),
            "projectile".into(),
            &json!({"speed": 20.0, "angle_degrees": 45.0, "gravity": 0.0}),
            &dummy,
            now.clone(),
        );
        assert_eq!(s2, EpistemicStatus::Contradicted);

        // Non-finite
        let (_, s3) = verify_projectile(
            "bad-3".into(),
            "projectile".into(),
            &json!({"speed": f64::NAN, "angle_degrees": 45.0}),
            &dummy,
            now,
        );
        assert_eq!(s3, EpistemicStatus::Contradicted);
    }

    #[test]
    fn test_projectile_detects_corrupted_output() {
        let now = rfc3339_timestamp();
        // Correct range for 20 m/s @ 35 deg is ~38.33 m. Provide corrupted 999.0 m.
        let corrupted = json!({
            "range_m": 999.0,
            "flight_time_s": 2.34,
            "max_height_m": 6.71
        });
        let (res, status) = verify_projectile(
            "corrupt-1".into(),
            "projectile".into(),
            &json!({"speed": 20.0, "angle_degrees": 35.0, "gravity": 9.80665}),
            &corrupted,
            now,
        );
        assert_eq!(status, EpistemicStatus::Contradicted);
        assert!(res.details.contains("Contradiction detected"));
    }

    #[test]
    fn test_arithmetic_verification() {
        let now = rfc3339_timestamp();
        // Add
        let (res, s1) = verify_arithmetic(
            "a1".into(),
            "arithmetic".into(),
            &json!({"a": 12.5, "b": 7.5, "op": "add"}),
            &json!({"value": 20.0}),
            now.clone(),
        );
        assert_eq!(s1, EpistemicStatus::Verified);
        assert!(res.absolute_error.unwrap() <= EXACT_FLOAT_TOLERANCE);

        // Divide
        let (_, s2) = verify_arithmetic(
            "a2".into(),
            "arithmetic".into(),
            &json!({"a": 10.0, "b": 4.0, "op": "divide"}),
            &json!({"value": 2.5}),
            now.clone(),
        );
        assert_eq!(s2, EpistemicStatus::Verified);

        // Division by zero
        let (_, s3) = verify_arithmetic(
            "a3".into(),
            "arithmetic".into(),
            &json!({"a": 10.0, "b": 0.0, "op": "divide"}),
            &json!({"value": 0.0}),
            now.clone(),
        );
        assert_eq!(s3, EpistemicStatus::Contradicted);

        // Arithmetic contradiction
        let (_, s4) = verify_arithmetic(
            "a4".into(),
            "arithmetic".into(),
            &json!({"a": 2.0, "b": 2.0, "op": "add"}),
            &json!({"value": 5.0}),
            now,
        );
        assert_eq!(s4, EpistemicStatus::Contradicted);
    }

    #[test]
    fn test_statistics_verification() {
        let now = rfc3339_timestamp();
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        // Mean = 30, stdev = sqrt(250) = 15.811388300841896, min = 10, max = 50, count = 5
        let output = json!({
            "count": 5,
            "mean": 30.0,
            "sample_stdev": 15.811388300841896,
            "min": 10.0,
            "max": 50.0
        });
        let (res, status) = verify_statistics(
            "s1".into(),
            "statistics".into(),
            &json!({"values": values}),
            &output,
            now.clone(),
        );
        assert_eq!(status, EpistemicStatus::Verified);
        assert!(res.absolute_error.unwrap() <= EXACT_FLOAT_TOLERANCE);

        // Corrupted stdev
        let bad_output = json!({
            "count": 5,
            "mean": 30.0,
            "sample_stdev": 12.0,
            "min": 10.0,
            "max": 50.0
        });
        let (_, bad_status) = verify_statistics(
            "s2".into(),
            "statistics".into(),
            &json!({"values": [10.0, 20.0, 30.0, 40.0, 50.0]}),
            &bad_output,
            now,
        );
        assert_eq!(bad_status, EpistemicStatus::Contradicted);
    }

    #[test]
    fn test_power_law_retains_calculated_when_consistent_and_contradicts_when_invalid() {
        let now = rfc3339_timestamp();
        // y = 2 * x^2 => ln(y) = ln(2) + 2*ln(x). Perfect fit: coef=2, exp=2, r2=1.0
        let xs = vec![1.0, 2.0, 4.0];
        let ys = vec![2.0, 8.0, 32.0];
        let valid_fit = json!({
            "coefficient": 2.0,
            "exponent": 2.0,
            "r_squared_log_space": 1.0
        });

        let (res, status) = verify_power_law(
            "pl1".into(),
            "power_law".into(),
            &json!({"x": xs, "y": ys}),
            &valid_fit,
            now.clone(),
        );

        // Crucial requirement: Empirical regression retains Calculated, NOT Verified!
        assert_eq!(status, EpistemicStatus::Calculated);
        assert!(!res.independently_checked);
        assert!(res.details.contains("Retained as Calculated"));

        // Corrupted fit parameters
        let invalid_fit = json!({
            "coefficient": -5.0, // Non-physical
            "exponent": 2.0,
            "r_squared_log_space": 1.0
        });
        let (_, bad_status) = verify_power_law(
            "pl2".into(),
            "power_law".into(),
            &json!({"x": [1.0, 2.0, 4.0], "y": [2.0, 8.0, 32.0]}),
            &invalid_fit,
            now,
        );
        assert_eq!(bad_status, EpistemicStatus::Contradicted);
    }
}
