import type { CheckResult, Role } from "./api";

/** Positive phrasing stays scoped to the searched collection and calibration quality. */
export function assessmentCopy(check: CheckResult, role: Role | null = null) {
  if (check.search.hits.length === 0) {
    return { label: "Nothing to compare", message: "This selection has no indexed items to assess." };
  }
  if (check.calibration.stale) {
    return { label: "Reassessment needed", message: "Recalibrate this collection before relying on the assessment." };
  }
  const scope = role ? `the ${role} items in this collection` : "this collection";
  switch (check.verdict) {
    case "open": return { label: "Worth exploring", message: `Your idea appears distinct in ${scope}.` };
    case "near": return { label: "Related ideas", message: `Related items exist in ${scope}; compare their approaches.` };
    case "exists": return { label: "Close match", message: `A similar idea appears in ${scope}.` };
  }
}
