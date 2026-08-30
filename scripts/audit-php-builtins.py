#!/usr/bin/env python3
"""Compare PHP and RPHP builtin names and callable signatures on demand."""

from __future__ import annotations

import argparse
import csv
import json
import os
import resource
import shlex
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
SYMFONY_BASE_EXTENSIONS = {
    "Core",
    "SPL",
    "ctype",
    "date",
    "filter",
    "hash",
    "json",
    "pcre",
    "random",
    "standard",
}


def parse_args() -> argparse.Namespace:
    script = Path(__file__).resolve()
    repository = script.parent.parent
    parser = argparse.ArgumentParser(
        description=(
            "Inventory reference-PHP and RPHP builtins, compare argument "
            "contracts, and optionally prioritize functions found in vendor PHP."
        )
    )
    parser.add_argument(
        "--reference-php",
        default="php",
        help="reference PHP command (default: php)",
    )
    parser.add_argument(
        "--rphp",
        default=str(repository / "target" / "debug" / "rphp"),
        help="RPHP executable (default: target/debug/rphp)",
    )
    parser.add_argument(
        "--output-dir",
        default=str(repository / "target" / "builtin-audit"),
        help="generated report directory (default: target/builtin-audit)",
    )
    parser.add_argument(
        "--vendor",
        action="append",
        default=[],
        help="vendor/package directory to scan for static function calls; repeatable",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=300,
        help="seconds allowed for each runtime probe (default: 300)",
    )
    parser.add_argument(
        "--stack-kb",
        type=int,
        default=65520,
        help="soft stack limit for probe subprocesses (default: 65520)",
    )
    parser.add_argument(
        "--fail-on",
        choices=["none", "vendor", "all"],
        default="none",
        help="optional CI failure policy (default: none)",
    )
    return parser.parse_args()


def lower(value: str) -> str:
    return value.casefold()


def executable_label(command: list[str]) -> str:
    return Path(command[0]).name + (" " + " ".join(command[1:]) if len(command) > 1 else "")


def probe_preexec(stack_kb: int):
    def configure() -> None:
        desired = stack_kb * 1024
        soft, hard = resource.getrlimit(resource.RLIMIT_STACK)
        if hard != resource.RLIM_INFINITY:
            desired = min(desired, hard)
        if soft < desired:
            resource.setrlimit(resource.RLIMIT_STACK, (desired, hard))

    return configure


def run_probe(
    command: list[str],
    probe: Path,
    environment: dict[str, str],
    timeout: int,
    stack_kb: int,
) -> dict[str, Any]:
    completed = subprocess.run(
        [*command, str(probe)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
        timeout=timeout,
        check=False,
        preexec_fn=probe_preexec(stack_kb),
    )
    if completed.returncode != 0:
        stderr = completed.stderr.decode("utf-8", errors="replace")
        raise RuntimeError(
            f"{executable_label(command)} probe exited {completed.returncode}:\n{stderr}"
        )
    if completed.stderr:
        stderr = completed.stderr.decode("utf-8", errors="replace")
        raise RuntimeError(f"{executable_label(command)} probe wrote stderr:\n{stderr}")
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        preview = completed.stdout[:1000].decode("utf-8", errors="replace")
        raise RuntimeError(
            f"{executable_label(command)} returned invalid inventory JSON: {error}\n{preview}"
        ) from error


def entry_index(entries: list[dict[str, Any]], key: str) -> dict[str, dict[str, Any]]:
    return {lower(str(entry[key])): entry for entry in entries}


def compare_callable(
    reference: dict[str, Any], candidate: dict[str, Any]
) -> tuple[list[str], list[str]]:
    shape: list[str] = []
    metadata: list[str] = []
    scalar_shape_fields = [
        "required_parameters",
        "total_parameters",
        "variadic",
        "returns_reference",
    ]
    for field in scalar_shape_fields:
        if reference.get(field) != candidate.get(field):
            shape.append(
                f"{field}:{reference.get(field)!r}->{candidate.get(field)!r}"
            )

    reference_parameters = reference.get("parameters", [])
    candidate_parameters = candidate.get("parameters", [])
    for position in range(min(len(reference_parameters), len(candidate_parameters))):
        expected = reference_parameters[position]
        actual = candidate_parameters[position]
        for field in ["name", "required", "optional", "variadic", "by_reference"]:
            if expected.get(field) != actual.get(field):
                shape.append(
                    f"param[{position}].{field}:{expected.get(field)!r}->{actual.get(field)!r}"
                )
        expected_type = expected.get("type")
        actual_type = actual.get("type")
        if expected_type != actual_type:
            metadata.append(
                f"param[{position}].type:{expected_type!r}->{actual_type!r}"
            )
        if expected.get("allows_null") != actual.get("allows_null"):
            metadata.append(
                "param[{}].allows_null:{!r}->{!r}".format(
                    position,
                    expected.get("allows_null"),
                    actual.get("allows_null"),
                )
            )
        expected_default = expected.get("default", {})
        actual_default = actual.get("default", {})
        if expected_default.get("available") != actual_default.get("available"):
            metadata.append(
                "param[{}].default.available:{!r}->{!r}".format(
                    position,
                    expected_default.get("available"),
                    actual_default.get("available"),
                )
            )
        elif expected_default.get("available"):
            for field in ["value_type", "value_export"]:
                if expected_default.get(field) != actual_default.get(field):
                    metadata.append(
                        "param[{}].default.{}:{!r}->{!r}".format(
                            position,
                            field,
                            expected_default.get(field),
                            actual_default.get(field),
                        )
                    )
            if expected_default.get("constant") != actual_default.get("constant"):
                metadata.append(
                    "param[{}].default.constant:{!r}->{!r}".format(
                        position,
                        expected_default.get("constant"),
                        actual_default.get("constant"),
                    )
                )

    for field in ["return_type", "deprecated"]:
        if reference.get(field) != candidate.get(field):
            metadata.append(f"{field}:{reference.get(field)!r}->{candidate.get(field)!r}")
    return shape, metadata


def format_parameter(parameter: dict[str, Any]) -> str:
    pieces: list[str] = []
    if parameter.get("type"):
        pieces.append(str(parameter["type"]))
    name = "$" + str(parameter.get("name", "?"))
    if parameter.get("by_reference"):
        name = "&" + name
    if parameter.get("variadic"):
        name = "..." + name
    pieces.append(name)
    default = parameter.get("default", {})
    if default.get("available"):
        value = default.get("constant") or default.get("value_export") or "?"
        pieces.append("= " + str(value))
    elif parameter.get("optional") and not parameter.get("variadic"):
        pieces.append("= ?")
    return " ".join(pieces)


def format_signature(entry: dict[str, Any] | None) -> str:
    if entry is None:
        return "-"
    parameters = ", ".join(format_parameter(item) for item in entry.get("parameters", []))
    result = f"({parameters})"
    if entry.get("return_type"):
        result += ": " + str(entry["return_type"])
    return result


def function_rows(
    reference: dict[str, Any], candidate: dict[str, Any]
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    reference_index = entry_index(reference["functions"], "name")
    candidate_index = entry_index(candidate["functions"], "name")
    rows: list[dict[str, Any]] = []
    counts = Counter()
    extension_counts: dict[str, Counter] = defaultdict(Counter)

    for key in sorted(reference_index):
        expected = reference_index[key]
        actual = candidate_index.get(key)
        extension = expected.get("extension") or "Core"
        if actual is None:
            status = "missing"
            shape_issues = ["function is not registered"]
            metadata_issues: list[str] = []
        else:
            shape_issues, metadata_issues = compare_callable(expected, actual)
            if shape_issues:
                status = "call_shape_mismatch"
            elif metadata_issues:
                status = "metadata_mismatch"
            else:
                status = "exact"
        counts[status] += 1
        counts["reference"] += 1
        if actual is not None:
            counts["present"] += 1
        extension_counts[extension]["reference"] += 1
        extension_counts[extension][status] += 1
        if actual is not None:
            extension_counts[extension]["present"] += 1
        rows.append(
            {
                "name": expected["name"],
                "extension": extension,
                "status": status,
                "reference_signature": format_signature(expected),
                "rphp_signature": format_signature(actual),
                "shape_issues": shape_issues,
                "metadata_issues": metadata_issues,
            }
        )

    extras = []
    for key in sorted(set(candidate_index) - set(reference_index)):
        entry = candidate_index[key]
        extras.append(
            {
                "name": entry["name"],
                "extension": None,
                "status": "rphp_extra",
                "reference_signature": "-",
                "rphp_signature": format_signature(entry),
                "shape_issues": [],
                "metadata_issues": [],
            }
        )
    counts["rphp_extra"] = len(extras)
    counts["call_shape_compatible"] = counts["exact"] + counts["metadata_mismatch"]
    for extension_count in extension_counts.values():
        extension_count["call_shape_compatible"] = (
            extension_count["exact"] + extension_count["metadata_mismatch"]
        )
    rows.extend(extras)
    rows.sort(key=lambda item: lower(item["name"]))

    by_extension = {
        extension: dict(extension_counts[extension])
        for extension in sorted(extension_counts, key=lower)
    }
    symfony_base = Counter()
    for row in rows:
        if row["extension"] in SYMFONY_BASE_EXTENSIONS:
            symfony_base["reference"] += 1
            symfony_base[row["status"]] += 1
            if row["status"] != "missing":
                symfony_base["present"] += 1
    symfony_base["call_shape_compatible"] = (
        symfony_base["exact"] + symfony_base["metadata_mismatch"]
    )
    return rows, {
        "counts": dict(counts),
        "by_extension": by_extension,
        "symfony_base_extensions": sorted(SYMFONY_BASE_EXTENSIONS, key=lower),
        "symfony_base_counts": dict(symfony_base),
    }


def type_rows(
    reference: dict[str, Any], candidate: dict[str, Any]
) -> tuple[list[dict[str, Any]], dict[str, int]]:
    reference_index = entry_index(reference["types"], "name")
    candidate_index = entry_index(candidate["types"], "name")
    rows = []
    counts = Counter()
    for key in sorted(reference_index):
        expected = reference_index[key]
        actual = candidate_index.get(key)
        if actual is None:
            status = "missing"
        elif expected.get("kind") != actual.get("kind"):
            status = "kind_mismatch"
        else:
            status = "present"
        counts[status] += 1
        counts["reference"] += 1
        rows.append(
            {
                "name": expected["name"],
                "extension": expected.get("extension") or "Core",
                "kind": expected.get("kind"),
                "status": status,
                "rphp_kind": actual.get("kind") if actual else None,
            }
        )
    for key in sorted(set(candidate_index) - set(reference_index)):
        actual = candidate_index[key]
        counts["rphp_extra"] += 1
        rows.append(
            {
                "name": actual["name"],
                "extension": None,
                "kind": None,
                "status": "rphp_extra",
                "rphp_kind": actual.get("kind"),
            }
        )
    rows.sort(key=lambda item: lower(item["name"]))
    return rows, dict(counts)


def method_rows(
    reference: dict[str, Any], candidate: dict[str, Any]
) -> tuple[list[dict[str, Any]], dict[str, int]]:
    reference_index = entry_index(reference["declared_methods"], "full_name")
    candidate_index = entry_index(candidate["probed_methods"], "full_name")
    rows = []
    counts = Counter()
    for key in sorted(reference_index):
        expected = reference_index[key]
        actual = candidate_index.get(key)
        if expected.get("probeable_by_name") is False:
            status = "reference_unprobeable"
            shape_issues = ["native ReflectionMethod cannot reconstruct this declared method"]
            metadata_issues = []
            actual = None
        elif actual is None:
            status = "missing_or_unreflectable"
            shape_issues = ["method is absent from ReflectionMethod"]
            metadata_issues: list[str] = []
        else:
            shape_issues, metadata_issues = compare_callable(expected, actual)
            for field in ["static", "public", "protected", "private", "abstract", "final"]:
                if expected.get(field) != actual.get(field):
                    shape_issues.append(
                        f"{field}:{expected.get(field)!r}->{actual.get(field)!r}"
                    )
            if shape_issues:
                status = "call_shape_mismatch"
            elif metadata_issues:
                status = "metadata_mismatch"
            else:
                status = "exact"
        counts[status] += 1
        counts["reference"] += 1
        if actual is not None:
            counts["present"] += 1
        rows.append(
            {
                "full_name": expected["full_name"],
                "class": expected["class"],
                "name": expected["name"],
                "status": status,
                "reference_signature": format_signature(expected),
                "rphp_signature": format_signature(actual),
                "shape_issues": shape_issues,
                "metadata_issues": metadata_issues,
            }
        )
    rows.sort(key=lambda item: lower(item["full_name"]))
    return rows, dict(counts)


def vendor_rows(
    reference: dict[str, Any], function_rows_list: list[dict[str, Any]]
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    function_index = {lower(row["name"]): row for row in function_rows_list}
    rows = []
    counts = Counter()
    for call in reference.get("vendor_scan", {}).get("calls", []):
        function = function_index.get(lower(call["name"]))
        if function is None or function["status"] == "rphp_extra":
            continue
        row = {
            "name": function["name"],
            "extension": function["extension"],
            "status": function["status"],
            "occurrences": call["occurrences"],
            "file_count": len(call["files"]),
            "files": call["files"],
            "reference_signature": function["reference_signature"],
            "rphp_signature": function["rphp_signature"],
            "shape_issues": function["shape_issues"],
            "metadata_issues": function["metadata_issues"],
        }
        rows.append(row)
        counts["unique_builtin_functions"] += 1
        counts[function["status"]] += 1
        counts["occurrences"] += call["occurrences"]
        if function["status"] != "missing":
            counts["present"] += 1
    rows.sort(key=lambda item: (-item["occurrences"], lower(item["name"])))
    counts["call_shape_compatible"] = counts["exact"] + counts["metadata_mismatch"]
    return rows, {
        "files_scanned": reference.get("vendor_scan", {}).get("files_scanned", 0),
        **dict(counts),
    }


def write_csv(path: Path, fieldnames: list[str], rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            rendered = dict(row)
            for field in ["shape_issues", "metadata_issues", "files"]:
                if isinstance(rendered.get(field), list):
                    rendered[field] = "; ".join(str(value) for value in rendered[field])
            writer.writerow(rendered)


def markdown_table(headers: list[str], rows: list[list[Any]]) -> list[str]:
    def cell(value: Any) -> str:
        return str(value).replace("|", "\\|").replace("\n", " ")

    lines = ["| " + " | ".join(headers) + " |"]
    lines.append("| " + " | ".join("---" for _ in headers) + " |")
    lines.extend("| " + " | ".join(cell(value) for value in row) + " |" for row in rows)
    return lines


def write_report(
    path: Path,
    summary: dict[str, Any],
    function_rows_list: list[dict[str, Any]],
    type_rows_list: list[dict[str, Any]],
    method_rows_list: list[dict[str, Any]],
    vendor_rows_list: list[dict[str, Any]],
) -> None:
    functions = summary["functions"]
    lines = [
        "# PHP builtin compatibility audit",
        "",
        f"Schema: {SCHEMA_VERSION}",
        "Reference: PHP {} ({})".format(
            summary["reference"]["php_version"], summary["reference"]["php_sapi"]
        ),
        "Candidate: RPHP {} ({})".format(
            summary["candidate"]["php_version"], summary["candidate"]["php_sapi"]
        ),
        f"Repository commit: `{summary['repository_commit']}`",
        "",
        "This report inventories registered names and Reflection-visible callable contracts. "
        "It does not prove function semantics, diagnostics, side effects, resources "
        "or edge behavior.",
        "Call-shape compatibility covers arity, required/optional arguments, variadics, names and "
        "by-reference mode. Type/default/return/deprecation differences remain visible "
        "as Reflection metadata mismatches but do not by themselves fail the invocation gate.",
        "",
        "## Global functions",
        "",
    ]
    counts = functions["counts"]
    lines.extend(
        markdown_table(
            ["Metric", "Count"],
            [
                ["Reference functions", counts.get("reference", 0)],
                ["RPHP-present reference names", counts.get("present", 0)],
                ["Missing names", counts.get("missing", 0)],
                ["Call-shape mismatches", counts.get("call_shape_mismatch", 0)],
                ["Call-shape compatible names", counts.get("call_shape_compatible", 0)],
                ["Reflection metadata mismatches", counts.get("metadata_mismatch", 0)],
                ["Exact reflected signatures", counts.get("exact", 0)],
                ["RPHP-only names", counts.get("rphp_extra", 0)],
            ],
        )
    )
    lines.extend(["", "### By reference extension", ""])
    extension_rows = []
    for extension, extension_count in functions["by_extension"].items():
        extension_rows.append(
            [
                extension,
                extension_count.get("reference", 0),
                extension_count.get("present", 0),
                extension_count.get("missing", 0),
                extension_count.get("call_shape_mismatch", 0),
                extension_count.get("metadata_mismatch", 0),
                extension_count.get("exact", 0),
            ]
        )
    lines.extend(
        markdown_table(
            ["Extension", "Reference", "Present", "Missing", "Shape", "Metadata", "Exact"],
            extension_rows,
        )
    )

    lines.extend(["", "### Symfony-base extension set", ""])
    base = functions["symfony_base_counts"]
    lines.append(
        ", ".join(functions["symfony_base_extensions"])
        + ". This is a prioritization set, not a Symfony platform-requirement claim."
    )
    lines.append("")
    lines.extend(
        markdown_table(
            [
                "Reference",
                "Present",
                "Missing",
                "Shape mismatches",
                "Shape compatible",
                "Metadata mismatches",
                "Exact",
            ],
            [[
                base.get("reference", 0),
                base.get("present", 0),
                base.get("missing", 0),
                base.get("call_shape_mismatch", 0),
                base.get("call_shape_compatible", 0),
                base.get("metadata_mismatch", 0),
                base.get("exact", 0),
            ]],
        )
    )

    lines.extend(["", "## Internal types and methods", ""])
    type_counts = summary["types"]
    method_counts = summary["methods"]
    lines.extend(
        markdown_table(
            [
                "Surface",
                "Reference",
                "Present",
                "RPHP missing/unreflectable",
                "Reference-unprobeable",
                "Shape",
                "Metadata",
                "Exact",
            ],
            [
                [
                    "Types",
                    type_counts.get("reference", 0),
                    type_counts.get("present", 0),
                    type_counts.get("missing", 0),
                    0,
                    type_counts.get("kind_mismatch", 0),
                    "-",
                    "-",
                ],
                [
                    "Declared methods",
                    method_counts.get("reference", 0),
                    method_counts.get("present", 0),
                    method_counts.get("missing_or_unreflectable", 0),
                    method_counts.get("reference_unprobeable", 0),
                    method_counts.get("call_shape_mismatch", 0),
                    method_counts.get("metadata_mismatch", 0),
                    method_counts.get("exact", 0),
                ],
            ],
        )
    )
    lines.append("")
    lines.append(
        "RPHP method availability is probed from the reference method list. Engine-only protocols "
        "that are callable but not exposed through ReflectionMethod remain classified as "
        "missing/unreflectable; methods that native PHP itself cannot reconstruct by name are "
        "reported separately."
    )

    lines.extend(["", "## Vendor static function surface", ""])
    vendor = summary["vendor"]
    if vendor.get("files_scanned", 0) == 0:
        lines.append("No vendor directory was supplied.")
    else:
        lines.extend(
            markdown_table(
                [
                    "Files",
                    "Unique builtins",
                    "Present",
                    "Missing",
                    "Shape",
                    "Shape compatible",
                    "Metadata",
                    "Exact",
                ],
                [[
                    vendor.get("files_scanned", 0),
                    vendor.get("unique_builtin_functions", 0),
                    vendor.get("present", 0),
                    vendor.get("missing", 0),
                    vendor.get("call_shape_mismatch", 0),
                    vendor.get("call_shape_compatible", 0),
                    vendor.get("metadata_mismatch", 0),
                    vendor.get("exact", 0),
                ]],
            )
        )
        blockers = [row for row in vendor_rows_list if row["status"] != "exact"][:40]
        lines.extend(["", "### Highest-frequency non-exact vendor functions", ""])
        lines.extend(
            markdown_table(
                ["Function", "Extension", "Status", "Calls", "Files", "Reference", "RPHP"],
                [[
                    row["name"],
                    row["extension"],
                    row["status"],
                    row["occurrences"],
                    row["file_count"],
                    row["reference_signature"],
                    row["rphp_signature"],
                ] for row in blockers],
            )
        )

    lines.extend(["", "## Generated files", ""])
    lines.extend(
        [
            "- `summary.json`: machine-readable counts and provenance",
            "- `functions.csv`: every reference function and its RPHP status/signatures",
            "- `types.csv`: every internal type name",
            "- `methods.csv`: every reference-declared internal method probe",
            "- `vendor-functions.csv`: static vendor builtin call surface",
            "- `reference-inventory.json` and `rphp-inventory.json`: raw probe snapshots",
            "",
        ]
    )
    path.write_text("\n".join(lines), encoding="utf-8")


def git_commit(repository: Path) -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repository,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
        text=True,
    )
    return completed.stdout.strip() if completed.returncode == 0 else "unknown"


def main() -> int:
    args = parse_args()
    if args.timeout <= 0:
        raise RuntimeError("--timeout must be positive")
    if args.stack_kb <= 0:
        raise RuntimeError("--stack-kb must be positive")
    script = Path(__file__).resolve()
    repository = script.parent.parent
    probe = script.parent / "builtin-inventory-probe.php"
    output = Path(args.output_dir).resolve()
    output.mkdir(parents=True, exist_ok=True)

    reference_command = shlex.split(args.reference_php)
    rphp_command = shlex.split(args.rphp)
    if not reference_command or not rphp_command:
        raise RuntimeError("runtime command cannot be empty")

    base_environment = os.environ.copy()
    base_environment.update({"LC_ALL": "C", "LANG": "C"})
    reference_environment = base_environment.copy()
    vendor_paths = []
    seen_vendor_paths = set()
    for raw_path in args.vendor:
        resolved = Path(raw_path).resolve()
        if not resolved.is_dir():
            raise RuntimeError(f"--vendor is not a directory: {raw_path}")
        normalized = str(resolved)
        if normalized not in seen_vendor_paths:
            vendor_paths.append(normalized)
            seen_vendor_paths.add(normalized)
    if vendor_paths:
        reference_environment["RPHP_BUILTIN_AUDIT_VENDOR_PATHS"] = os.pathsep.join(vendor_paths)

    reference = run_probe(
        reference_command,
        probe,
        reference_environment,
        args.timeout,
        args.stack_kb,
    )
    if reference.get("schema_version") != SCHEMA_VERSION:
        raise RuntimeError("reference probe schema does not match the audit driver")
    method_names = sorted(
        {
            entry["full_name"]
            for entry in reference["declared_methods"]
            if entry.get("probeable_by_name") is not False
        },
        key=lower,
    )
    method_file = output / "reference-methods.txt"
    method_file.write_text("\n".join(method_names) + "\n", encoding="utf-8")

    rphp_environment = base_environment.copy()
    rphp_environment["RPHP_BUILTIN_AUDIT_METHODS_FILE"] = str(method_file)
    candidate = run_probe(
        rphp_command,
        probe,
        rphp_environment,
        args.timeout,
        args.stack_kb,
    )
    if candidate.get("schema_version") != SCHEMA_VERSION:
        raise RuntimeError("RPHP probe schema does not match the audit driver")

    function_rows_list, function_summary = function_rows(reference, candidate)
    type_rows_list, type_summary = type_rows(reference, candidate)
    method_rows_list, method_summary = method_rows(reference, candidate)
    vendor_rows_list, vendor_summary = vendor_rows(reference, function_rows_list)

    summary = {
        "schema_version": SCHEMA_VERSION,
        "repository_commit": git_commit(repository),
        "reference": {
            "command": executable_label(reference_command),
            "php_version": reference.get("php_version"),
            "php_version_id": reference.get("php_version_id"),
            "php_sapi": reference.get("php_sapi"),
            "probe_errors": len(reference.get("function_errors", [])),
        },
        "candidate": {
            "command": executable_label(rphp_command),
            "php_version": candidate.get("php_version"),
            "php_version_id": candidate.get("php_version_id"),
            "php_sapi": candidate.get("php_sapi"),
            "probe_errors": len(candidate.get("function_errors", [])),
        },
        "functions": function_summary,
        "types": type_summary,
        "methods": method_summary,
        "vendor": vendor_summary,
        "limitations": [
            "Presence and Reflection-visible signatures do not prove behavior.",
            "Vendor function scanning is static and does not resolve dynamic calls "
            "or namespace shadowing.",
            "RPHP engine-only methods absent from ReflectionMethod are reported as "
            "missing/unreflectable.",
            "The reference surface depends on the extensions loaded by the selected PHP binary.",
        ],
    }

    (output / "reference-inventory.json").write_text(
        json.dumps(reference, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (output / "rphp-inventory.json").write_text(
        json.dumps(candidate, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (output / "summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    write_csv(
        output / "functions.csv",
        [
            "name",
            "extension",
            "status",
            "reference_signature",
            "rphp_signature",
            "shape_issues",
            "metadata_issues",
        ],
        function_rows_list,
    )
    write_csv(
        output / "types.csv",
        ["name", "extension", "kind", "status", "rphp_kind"],
        type_rows_list,
    )
    write_csv(
        output / "methods.csv",
        [
            "full_name",
            "class",
            "name",
            "status",
            "reference_signature",
            "rphp_signature",
            "shape_issues",
            "metadata_issues",
        ],
        method_rows_list,
    )
    write_csv(
        output / "vendor-functions.csv",
        [
            "name",
            "extension",
            "status",
            "occurrences",
            "file_count",
            "reference_signature",
            "rphp_signature",
            "shape_issues",
            "metadata_issues",
            "files",
        ],
        vendor_rows_list,
    )
    write_report(
        output / "report.md",
        summary,
        function_rows_list,
        type_rows_list,
        method_rows_list,
        vendor_rows_list,
    )

    counts = function_summary["counts"]
    print(f"report: {output / 'report.md'}")
    print(
        "functions: reference={reference} present={present} missing={missing} "
        "shape_mismatch={shape} metadata_mismatch={metadata} exact={exact}".format(
            reference=counts.get("reference", 0),
            present=counts.get("present", 0),
            missing=counts.get("missing", 0),
            shape=counts.get("call_shape_mismatch", 0),
            metadata=counts.get("metadata_mismatch", 0),
            exact=counts.get("exact", 0),
        )
    )
    if vendor_summary.get("files_scanned", 0):
        print(
            "vendor: files={files} unique_builtins={unique} missing={missing} "
            "shape_mismatch={shape}".format(
                files=vendor_summary.get("files_scanned", 0),
                unique=vendor_summary.get("unique_builtin_functions", 0),
                missing=vendor_summary.get("missing", 0),
                shape=vendor_summary.get("call_shape_mismatch", 0),
            )
        )

    if args.fail_on == "all" and (
        counts.get("missing", 0) or counts.get("call_shape_mismatch", 0)
    ):
        return 1
    if args.fail_on == "vendor" and (
        vendor_summary.get("missing", 0)
        or vendor_summary.get("call_shape_mismatch", 0)
    ):
        return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
